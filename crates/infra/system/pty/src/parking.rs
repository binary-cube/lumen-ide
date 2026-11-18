//! A terminal's history parked while the terminal isn't shown: after [`PARK_AFTER`] without
//! being drawn, the rows above its screen leave the emulator's grid for a compact form
//! ([`Parked`]: a busy terminal's ~20 MB of cells come to well under one), and come back,
//! the same, before anything reads them again.
//!
//! - **What goes**: only the history (its rows moved out under the emulator's lock, packed
//!   outside it); the screen, the cursor, the modes stay. Not while a full-screen program
//!   has its own screen (the history is then the emulator's other grid, out of reach).
//! - **Output meanwhile**: goes into the grid as always; the parked rows are older. Parked
//!   again every [`PARK_AGAIN`] while it isn't drawn, they join the others.
//! - **Coming back**: drawn again, the rows are put back on a worker (the screen is the
//!   grid's own meanwhile, the history's length counts them); scrolled, resized, cleared, or
//!   drawn while it was scrolled back, at once. Above the grid's history, as many as the
//!   history's limit leaves room for (the oldest let go), the scroll position kept.
//! - **Erased meanwhile**: a program erasing the history (`ESC[3J` let through, a full reset
//!   `ESC c`) erases the parked rows too: the reader counts the erases it hands the emulator
//!   ([`Parking::erased`]); rows parked before the last one are let go.
//!
//! One thread looks at every session's last draw (asleep until the next one is due; gone
//! when there are none).

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError, TryLockError, Weak};
use std::time::{Duration, Instant};

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::{Dimensions, Grid, Row, Scroll};
use alacritty_terminal::index::Line;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::{Term, TermMode};

use crate::parked::Parked;

/// How long a terminal goes without being drawn before its history is parked.
pub(crate) const PARK_AFTER: Duration = Duration::from_secs(120);

/// How often a parked terminal's new history is parked too (a program still writing in a
/// tab not shown: its grid fills again).
pub(crate) const PARK_AGAIN: Duration = Duration::from_secs(15);

/// The fewest history rows worth parking.
const PARK_LEAST: usize = 256;

/// The history's rows taken out of `grid`, oldest first: its history emptied, the rows it
/// keeps made ahead let go. Each row is moved, not copied.
pub(crate) fn take_history(grid: &mut Grid<Cell>) -> Vec<Row<Cell>> {
    let history = grid.history_size();
    let rows = (1..=history).rev().map(|i| std::mem::take(&mut grid[Line(-(i as i32))])).collect();
    grid.clear_history();
    // The rows emptied above go with those made ahead (a row left empty there would be
    // taken up again by the next line of output).
    grid.truncate();
    rows
}

/// `rows` (oldest first, as wide as `grid`) put back above `grid`'s history, as many as its
/// limit (`limit`, the emulator's own) leaves room for: the oldest let go. `scrolled`: how
/// far back the view was when they left (0: at the bottom), kept above the rows added since.
pub(crate) fn put_back(
    grid: &mut Grid<Cell>,
    mut rows: Vec<Row<Cell>>,
    limit: usize,
    scrolled: usize,
) {
    // The limit is the emulator's (set from the same options): told again so the room
    // below is exactly the grid's.
    grid.update_history(limit);
    let live = grid.history_size();
    let room = limit.saturating_sub(live);
    if rows.len() > room {
        rows.drain(..rows.len() - room);
    }
    if rows.is_empty() {
        return;
    }
    let added = rows.len();
    let lines = grid.screen_lines();
    let offset = if scrolled > 0 { scrolled + live } else { grid.display_offset() };
    // Every row taken out (history and screen); the grid then grows by `added` rows (made
    // ahead, or new), and each row is put where it belongs.
    let total = live + lines;
    let mut all = rows;
    all.reserve(total);
    for i in (lines as i32 - total as i32)..lines as i32 {
        all.push(std::mem::take(&mut grid[Line(i)]));
    }
    grid.scroll_up(&(Line(0)..Line(lines as i32)), added);
    let top = lines as i32 - all.len() as i32;
    for (i, row) in all.into_iter().enumerate() {
        grid[Line(top + i as i32)] = row;
    }
    let now = grid.display_offset() as i32;
    grid.scroll_display(Scroll::Delta(offset.min(grid.history_size()) as i32 - now));
}

/// `rows` `from` columns wide, made `to` wide as the emulator's resize makes its history
/// (wrapped lines flowed again).
pub(crate) fn reflow(mut rows: Vec<Row<Cell>>, from: usize, to: usize) -> Vec<Row<Cell>> {
    if rows.is_empty() || from == to || from == 0 || to == 0 {
        return rows;
    }
    // The newest row's line goes on in the grid's history, not here: flowed apart, its
    // wrap set again on what it became.
    let wrapped = rows.last_mut().and_then(Row::last_mut).is_some_and(|cell| {
        let wrapped = cell.flags.contains(Flags::WRAPLINE);
        cell.flags.remove(Flags::WRAPLINE);
        wrapped
    });
    // Narrower, a row may become several.
    let most = rows.len() * (from / to + 1) + 1;
    let mut grid: Grid<Cell> = Grid::new(1, from, most);
    put_back(&mut grid, rows, most, 0);
    grid.resize(true, 1, to);
    let mut rows = take_history(&mut grid);
    if wrapped && let Some(cell) = rows.last_mut().and_then(Row::last_mut) {
        cell.flags.insert(Flags::WRAPLINE);
    }
    rows
}

/// What is kept of a parked history.
#[derive(Debug, Default)]
struct Store {
    parked: Parked,
    /// The erases counted when its rows left (older ones: let go).
    erased: u64,
    /// How far back the view was (rows from the bottom; 0: at the bottom).
    scrolled: usize,
}

/// A session's parked history and what decides when it parks.
pub(crate) struct Parking<L> {
    term: Weak<FairMutex<Term<L>>>,
    /// Locked before the emulator, never after.
    store: Mutex<Store>,
    /// Something is parked (asked at every draw, no lock): the rows and their bytes.
    parked: AtomicBool,
    rows: AtomicUsize,
    bytes: AtomicUsize,
    /// Parked while it was scrolled back: drawn, it's put back at once.
    scrolled: AtomicBool,
    /// Put back on a worker now.
    restoring: AtomicBool,
    /// When it was last drawn, and last looked at to park (ms since [`epoch`]).
    used: AtomicU64,
    checked: AtomicU64,
    /// The erases of the history handed to the emulator (counted by the reader).
    erased: Arc<AtomicU64>,
    /// The history's limit (the user's setting, as the emulator has it).
    limit: AtomicUsize,
    /// Told once the rows are back (a frame: the history's place may show).
    restored: Box<dyn Fn() + Send + Sync>,
    /// How long without being drawn before it parks (ms: [`PARK_AFTER`]; the tests' sooner).
    pub(crate) park_after: AtomicU64,
}

impl<L> std::fmt::Debug for Parking<L> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Parking").field("rows", &self.rows).finish_non_exhaustive()
    }
}

fn epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

fn now_ms() -> u64 {
    epoch().elapsed().as_millis() as u64
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl<L: EventListener> Parking<L> {
    /// For `term`, whose reader counts the erases in `erased`, its history limited to
    /// `limit` rows; `restored` is told when parked rows are back.
    pub(crate) fn new(
        term: &Arc<FairMutex<Term<L>>>,
        erased: Arc<AtomicU64>,
        limit: usize,
        restored: Box<dyn Fn() + Send + Sync>,
    ) -> Self {
        let now = now_ms();
        Self {
            term: Arc::downgrade(term),
            store: Mutex::default(),
            parked: AtomicBool::new(false),
            rows: AtomicUsize::new(0),
            bytes: AtomicUsize::new(0),
            scrolled: AtomicBool::new(false),
            restoring: AtomicBool::new(false),
            used: AtomicU64::new(now),
            checked: AtomicU64::new(now),
            erased,
            limit: AtomicUsize::new(limit),
            restored,
            park_after: AtomicU64::new(PARK_AFTER.as_millis() as u64),
        }
    }

    /// The rows parked now (counted in the history's length shown).
    pub(crate) fn rows(&self) -> usize {
        if self.parked.load(Ordering::Acquire) { self.rows.load(Ordering::Acquire) } else { 0 }
    }

    /// The history's length shown with `live` rows in the grid: the parked ones counted, as
    /// many as will come back.
    pub(crate) fn history(&self, live: usize) -> usize {
        match self.rows() {
            0 => live,
            parked => (live + parked).min(self.limit.load(Ordering::Relaxed).max(live)),
        }
    }

    /// What the parked rows hold in memory (bytes).
    pub(crate) fn bytes(&self) -> usize {
        if self.parked.load(Ordering::Acquire) { self.bytes.load(Ordering::Relaxed) } else { 0 }
    }

    /// The history's limit changed (the user's setting): parked rows past it let go.
    pub(crate) fn set_limit(&self, limit: usize) {
        self.limit.store(limit, Ordering::Relaxed);
        if self.parked.load(Ordering::Acquire) {
            let mut store = lock(&self.store);
            store.parked.keep(limit);
            self.counted(&store);
        }
    }

    /// The user cleared the history (Clear Terminal): the parked rows go with it.
    pub(crate) fn forget(&self) {
        let mut store = lock(&self.store);
        store.parked = Parked::default();
        store.scrolled = 0;
        self.counted(&store);
    }

    /// Its counts as `store` holds them.
    fn counted(&self, store: &Store) {
        self.rows.store(store.parked.len(), Ordering::Release);
        self.bytes.store(store.parked.bytes(), Ordering::Relaxed);
        self.scrolled.store(store.scrolled > 0, Ordering::Release);
        self.parked.store(!store.parked.is_empty(), Ordering::Release);
    }

    /// It's being drawn: parked rows come back — on a worker, or at once (`false`: it's busy,
    /// try again at the next frame) when it was parked scrolled back (what's shown is in
    /// them).
    pub(crate) fn shown(self: &Arc<Self>, wait: bool) -> bool
    where
        L: Send + 'static,
    {
        self.used.store(now_ms(), Ordering::Relaxed);
        if !self.parked.load(Ordering::Acquire) {
            return true;
        }
        if self.scrolled.load(Ordering::Acquire) {
            return self.restore(wait);
        }
        if !self.restoring.swap(true, Ordering::AcqRel) {
            let me = Arc::clone(self);
            kernel::work::run("terminal-park", move || {
                me.restore(true);
                me.restoring.store(false, Ordering::Release);
                (me.restored)();
            });
        }
        true
    }

    /// Whether it must be put back before what's drawn is read (parked scrolled back).
    pub(crate) fn must_restore(&self) -> bool {
        self.parked.load(Ordering::Acquire) && self.scrolled.load(Ordering::Acquire)
    }

    /// The parked rows put back into the grid now (`wait`: whatever it takes; else `false`
    /// when the emulator or a park is busy). Rows parked before an erase are let go; while a
    /// full-screen program has its own screen they stay parked.
    pub(crate) fn restore(&self, wait: bool) -> bool {
        self.used.store(now_ms(), Ordering::Relaxed);
        if !self.parked.load(Ordering::Acquire) {
            return true;
        }
        let Some(term) = self.term.upgrade() else { return true };
        let mut store = if wait {
            lock(&self.store)
        } else {
            match self.store.try_lock() {
                Ok(store) => store,
                Err(TryLockError::Poisoned(p)) => p.into_inner(),
                Err(TryLockError::WouldBlock) => return false,
            }
        };
        if store.erased != self.erased.load(Ordering::Acquire) {
            store.parked = Parked::default();
        }
        if store.parked.is_empty() {
            store.scrolled = 0;
            self.counted(&store);
            return true;
        }
        // A look first: nothing unpacked for a full-screen program's screen, nor more
        // than the history's limit leaves room for.
        {
            let term = if wait {
                term.lock_unfair()
            } else {
                match term.try_lock_unfair() {
                    Some(term) => term,
                    None => return false,
                }
            };
            if term.mode().contains(TermMode::ALT_SCREEN) {
                return true;
            }
            let room =
                self.limit.load(Ordering::Relaxed).saturating_sub(term.grid().history_size());
            store.parked.keep(room);
        }
        // Unpacked outside the emulator's lock (its reader goes on).
        let runs = store.parked.take();
        let mut term = term.lock_unfair();
        if store.erased != self.erased.load(Ordering::Acquire) {
            // Erased meanwhile: those rows are gone.
        } else if term.mode().contains(TermMode::ALT_SCREEN) {
            // Its own screen meanwhile: parked again as they were.
            for (columns, rows) in &runs {
                store.parked.push(rows, *columns);
            }
            self.counted(&store);
            return true;
        } else {
            let columns = term.columns();
            let rows: Vec<Row<Cell>> =
                runs.into_iter().flat_map(|(width, rows)| reflow(rows, width, columns)).collect();
            let limit = self.limit.load(Ordering::Relaxed);
            put_back(term.grid_mut(), rows, limit, store.scrolled);
        }
        store.scrolled = 0;
        // Counted with the grid still locked: a frame sees the history's length whole.
        self.counted(&store);
        true
    }

    /// The history parked now, if it hasn't been drawn for [`PARK_AFTER`] and has enough to
    /// park. The emulator is locked fairly: never between the reader's read and its parse
    /// (an erase handed to it is parsed before).
    pub(crate) fn park(&self) {
        self.checked.store(now_ms(), Ordering::Relaxed);
        let Some(term) = self.term.upgrade() else { return };
        let mut store = lock(&self.store);
        let (rows, columns, erased, offset) = {
            let mut term = term.lock();
            if self.used.load(Ordering::Relaxed) + self.park_after.load(Ordering::Relaxed)
                > now_ms()
                || self.restoring.load(Ordering::Acquire)
            {
                return;
            }
            let erased = self.erased.load(Ordering::Acquire);
            if store.erased != erased {
                // Erased since the last park: those rows are gone.
                store.parked = Parked::default();
                store.scrolled = 0;
                store.erased = erased;
                self.counted(&store);
            }
            if term.mode().contains(TermMode::ALT_SCREEN) || term.grid().history_size() < PARK_LEAST
            {
                return;
            }
            let offset = term.grid().display_offset();
            let columns = term.columns();
            let rows = take_history(term.grid_mut());
            // Counted while still locked: a frame sees the history's length whole.
            self.rows.store(store.parked.len() + rows.len(), Ordering::Release);
            self.parked.store(true, Ordering::Release);
            (rows, columns, erased, offset)
        };
        store.scrolled = if store.parked.is_empty() {
            offset
        } else if store.scrolled > 0 {
            store.scrolled + rows.len()
        } else {
            0
        };
        store.parked.push(&rows, columns);
        store.parked.keep(self.limit.load(Ordering::Relaxed));
        store.erased = erased;
        self.counted(&store);
        drop(store);
        drop(rows);
        kernel::memory::release_free();
    }

    /// When it should be looked at next (ms since [`epoch`]); `None` while it's put back.
    /// Parked, sooner: what it wrote since goes too.
    fn due(&self) -> Option<u64> {
        if self.restoring.load(Ordering::Acquire) {
            return None;
        }
        let checked = self.checked.load(Ordering::Relaxed);
        if self.parked.load(Ordering::Acquire) {
            return Some(checked + PARK_AGAIN.as_millis() as u64);
        }
        let since = self.used.load(Ordering::Relaxed).max(checked);
        Some(since + self.park_after.load(Ordering::Relaxed))
    }
}

/// A session the parking thread looks at.
trait Parkable: Send + Sync {
    fn due(&self) -> Option<u64>;
    fn park(&self);
}

impl<L: EventListener + Send + 'static> Parkable for Parking<L> {
    fn due(&self) -> Option<u64> {
        Parking::due(self)
    }

    fn park(&self) {
        Parking::park(self);
    }
}

/// The sessions looked at, and whether the thread runs.
struct Watched {
    sessions: Vec<Weak<dyn Parkable>>,
    running: bool,
}

static WATCHED: Mutex<Watched> = Mutex::new(Watched { sessions: Vec::new(), running: false });
static WAKE: Condvar = Condvar::new();

/// `parking` looked at by the parking thread (started if it isn't running) for as long as
/// it lives.
pub(crate) fn watch<L: EventListener + Send + 'static>(parking: &Arc<Parking<L>>) {
    let parking: Arc<dyn Parkable> = Arc::clone(parking) as Arc<dyn Parkable>;
    let mut watched = lock(&WATCHED);
    watched.sessions.push(Arc::downgrade(&parking));
    if !watched.running && spawn_parker() {
        watched.running = true;
    }
    WAKE.notify_one();
}

/// The thread parking the sessions not drawn for a while: asleep until the next is due,
/// gone when no session is left.
#[allow(clippy::disallowed_methods)] // One for all sessions, while there are some.
fn spawn_parker() -> bool {
    let started = std::thread::Builder::new().name("terminal-park".into()).spawn(|| {
        let mut watched = lock(&WATCHED);
        loop {
            watched.sessions.retain(|s| s.strong_count() > 0);
            if watched.sessions.is_empty() {
                watched.running = false;
                return;
            }
            let now = now_ms();
            let live: Vec<Arc<dyn Parkable>> =
                watched.sessions.iter().filter_map(Weak::upgrade).collect();
            let due: Vec<&Arc<dyn Parkable>> =
                live.iter().filter(|s| s.due().is_some_and(|at| at <= now)).collect();
            if due.is_empty() {
                let next = live.iter().filter_map(|s| s.due()).min();
                // Nothing due (all coming back): looked at again after a while.
                let wait =
                    next.map_or(PARK_AFTER, |at| Duration::from_millis(at.saturating_sub(now)));
                drop(live);
                watched =
                    WAKE.wait_timeout(watched, wait).unwrap_or_else(PoisonError::into_inner).0;
                continue;
            }
            drop(watched);
            for session in due {
                session.park();
            }
            drop(live);
            watched = lock(&WATCHED);
        }
    });
    match started {
        Ok(_) => true,
        Err(e) => {
            tracing::warn!("can't start the terminals' parking thread: {e}");
            false
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/parking.rs"]
mod tests;
