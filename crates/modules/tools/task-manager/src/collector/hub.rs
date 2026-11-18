//! One collector per window, however many Task Manager views it has (a tab and its mirror
//! in a split): the hub of a window's [`ResourceSource`]. Each view holds a
//! [`Subscription`] with whether it's shown and what it looks at (this project or every
//! open one). The hub's thread (`task-manager`) looks once a second while at least one
//! view is shown, sleeps (a condition variable, no polling) while they're all hidden, and
//! ends once the last view is gone. Two views looking at different scopes are served by
//! one look ([`ResourceSource::snapshot_both`]). The source is held only while a view
//! is shown (weakly otherwise): the window stops filling in its parts as soon as nothing
//! shows them.
//!
//! A look reaches the views as a [`Tick`]: its number and the frame it found (one `Arc`
//! for every view of the scope), left in the view's [`Latest`]: only the newest is kept,
//! so a view that doesn't take them (a screen's widget reads only the history; a window
//! that isn't drawn) holds one look, never one a second. The values over time stay here, in each scope's
//! [`History`] (rings), read by a view only when it draws a graph
//! ([`Subscription::history`]). How often it looks and how long the history is come from
//! the settings ([`configure`]).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};
use std::time::{Duration, Instant};

use workbench_model::{ResourceSnapshot, ResourceSource};

use super::frame::Tick;
use super::history::History;
use super::sample::{EVERY, FrameLabels, ProcessRates, Rates, Sampler, add_programs, frame};

/// How often the hubs look (milliseconds) and how long their history is (seconds): the
/// settings' (`[tasks] interval`, `[tasks] history`).
static INTERVAL_MS: AtomicU64 = AtomicU64::new(EVERY.as_millis() as u64);
static HISTORY_SECS: AtomicU64 = AtomicU64::new(60);

/// The hubs look every `interval` and keep `history` of values (the settings changed): from
/// their next look.
pub fn configure(interval: Duration, history: Duration) {
    INTERVAL_MS.store(interval.as_millis().max(1) as u64, Ordering::Relaxed);
    HISTORY_SECS.store(history.as_secs().max(1), Ordering::Relaxed);
}

/// How often the hubs look now.
pub fn interval() -> Duration {
    Duration::from_millis(INTERVAL_MS.load(Ordering::Relaxed))
}

/// How long a history is now.
pub fn history_window() -> Duration {
    Duration::from_secs(HISTORY_SECS.load(Ordering::Relaxed))
}

/// What wakes the app when a look is ready (the view's [`ui_components::ViewerPaint`]'s).
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// Every window's hub, held weakly: one goes when its last view and its thread are gone.
static HUBS: Mutex<Vec<Weak<Hub>>> = Mutex::new(Vec::new());

/// The scopes, by index: this window's project, every open project.
const THIS: usize = 0;
const ALL: usize = 1;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A view as the hub knows it.
struct Subscriber {
    id: u64,
    shown: bool,
    all: bool,
    tables: Latest,
    wake: Option<Waker>,
}

struct State {
    subscribers: Vec<Subscriber>,
    source: Weak<dyn ResourceSource>,
    /// The source, held while a view is shown.
    held: Option<Arc<dyn ResourceSource>>,
    /// Its thread is running.
    thread: bool,
    /// Its last view is gone: it serves nobody again (a new view makes a new hub).
    ended: bool,
    /// Look now (a view changed its scope), not at the next second.
    kick: bool,
    /// Each scope's last look: a view shown (or switched) gets it at once.
    last: [Option<Tick>; 2],
    /// Each scope's values over time (its views read them when they draw).
    histories: [Arc<Mutex<History>>; 2],
    next_id: u64,
    /// How many looks it took.
    looks: u64,
    /// Its own interval (the tests'), else the settings'.
    every: Option<Duration>,
}

impl State {
    fn scope_of(all: bool) -> usize {
        if all { ALL } else { THIS }
    }

    /// The source held while a view is shown, let go otherwise.
    fn hold(&mut self) {
        let shown = self.subscribers.iter().any(|s| s.shown);
        if !shown {
            self.held = None;
        } else if self.held.is_none() {
            self.held = self.source.upgrade();
        }
    }
}

/// A window's collector.
pub struct Hub {
    state: Mutex<State>,
    signal: Condvar,
    /// The rows of the threads no part claims and of the memory nobody reported.
    labels: FrameLabels,
}

impl std::fmt::Debug for Hub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Hub")
    }
}

/// What a hub is doing (for the tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HubInfo {
    pub views: usize,
    pub shown: usize,
    /// Its thread runs (looking, or asleep while every view is hidden).
    pub thread: bool,
    /// It holds its source (a view is shown).
    pub holds: bool,
    pub looks: u64,
}

impl Hub {
    /// `source`'s hub (made if it has none) and a view of it, hidden until
    /// [`Subscription::set_shown`]: looking at every open project when `all`.
    pub fn subscribe(
        source: &Arc<dyn ResourceSource>,
        labels: FrameLabels,
        all: bool,
    ) -> Subscription {
        Self::subscribe_with(source, labels, all, None)
    }

    /// The same, a hub made here looking every `every` (whatever the settings say).
    pub fn subscribe_every(
        source: &Arc<dyn ResourceSource>,
        labels: FrameLabels,
        all: bool,
        every: Duration,
    ) -> Subscription {
        Self::subscribe_with(source, labels, all, Some(every))
    }

    fn subscribe_with(
        source: &Arc<dyn ResourceSource>,
        labels: FrameLabels,
        all: bool,
        every: Option<Duration>,
    ) -> Subscription {
        let tables = Latest::default();
        let mut tx = Some(tables.clone());
        let mut hubs = lock(&HUBS);
        hubs.retain(|h| h.strong_count() > 0);
        // Joined under its lock: a hub whose last view just went serves nobody again.
        let mut join = |hub: &Arc<Hub>| {
            let mut state = lock(&hub.state);
            if state.ended {
                return None;
            }
            state.next_id += 1;
            let id = state.next_id;
            let tables = tx.take()?;
            state.subscribers.push(Subscriber { id, shown: false, all, tables, wake: None });
            Some(id)
        };
        let found = hubs
            .iter()
            .filter_map(Weak::upgrade)
            .filter(|h| h.is_of(source))
            .find_map(|h| join(&h).map(|id| (h, id)));
        let (hub, id) = match found {
            Some(found) => found,
            None => {
                let hub = Arc::new(Hub {
                    state: Mutex::new(State {
                        subscribers: Vec::new(),
                        source: Arc::downgrade(source),
                        held: None,
                        thread: false,
                        ended: false,
                        kick: false,
                        last: [None, None],
                        histories: std::array::from_fn(|_| {
                            let every = every.unwrap_or_else(interval);
                            Arc::new(Mutex::new(History::new(history_window(), every)))
                        }),
                        next_id: 0,
                        looks: 0,
                        every,
                    }),
                    signal: Condvar::new(),
                    labels,
                });
                hubs.push(Arc::downgrade(&hub));
                let id = join(&hub).unwrap_or_default();
                (hub, id)
            }
        };
        drop(hubs);
        Subscription { hub, id, tables }
    }

    /// What `source`'s hub is doing (`None`: it has none).
    pub fn info(source: &Arc<dyn ResourceSource>) -> Option<HubInfo> {
        let hubs: Vec<Arc<Hub>> = lock(&HUBS).iter().filter_map(Weak::upgrade).collect();
        let mut of: Vec<Arc<Hub>> = hubs.into_iter().filter(|h| h.is_of(source)).collect();
        // The one serving its views first (one whose last view went may still be ending).
        of.sort_by_key(|h| lock(&h.state).ended);
        let hub = of.into_iter().next()?;
        let state = lock(&hub.state);
        Some(HubInfo {
            views: state.subscribers.len(),
            shown: state.subscribers.iter().filter(|s| s.shown).count(),
            thread: state.thread,
            holds: state.held.is_some(),
            looks: state.looks,
        })
    }

    fn is_of(&self, source: &Arc<dyn ResourceSource>) -> bool {
        let state = lock(&self.state);
        state.source.strong_count() > 0
            && std::ptr::addr_eq(state.source.as_ptr(), Arc::as_ptr(source))
    }

    /// Changes a view's state; the thread started when one is shown, woken.
    fn change(self: &Arc<Self>, id: u64, f: impl FnOnce(&mut Subscriber, &[Option<Tick>; 2])) {
        let mut state = lock(&self.state);
        let State { subscribers, last, .. } = &mut *state;
        if let Some(sub) = subscribers.iter_mut().find(|s| s.id == id) {
            f(sub, last);
        }
        state.hold();
        let shown = state.subscribers.iter().any(|s| s.shown);
        if shown && !state.thread && !state.ended {
            state.thread = self.start();
        }
        drop(state);
        self.signal.notify_all();
    }

    /// Its thread, looking while a view is shown.
    // Its own thread: it samples every second while a view shows (a pool's thread would be
    // held as long).
    #[allow(clippy::disallowed_methods)]
    fn start(self: &Arc<Self>) -> bool {
        let hub = Arc::clone(self);
        std::thread::Builder::new().name("task-manager".into()).spawn(move || hub.run()).is_ok()
    }

    fn run(&self) {
        let mut sampler = Sampler::default();
        let mut scopes: [Option<Scope>; 2] = [None, None];
        let histories = lock(&self.state).histories.clone();
        while let Some((source, wants)) = self.wait_for_a_view() {
            let now = Instant::now();
            let threads = sampler.sample(&process_stats::threads(), now);
            let snapshots: [Option<ResourceSnapshot>; 2] = match wants {
                [true, true] => {
                    let (this, all) = source.snapshot_both();
                    [Some(this), Some(all)]
                }
                [true, false] => [Some(source.snapshot()), None],
                [false, true] => [None, Some(source.snapshot_all())],
                [false, false] => [None, None],
            };
            // Not held while it waits (a hidden view lets it go meanwhile).
            drop(source);
            let every = self.every();
            let mut frames: [Option<super::Frame>; 2] = [None, None];
            for (scope, snapshot) in snapshots.into_iter().enumerate() {
                let Some(snapshot) = snapshot else { continue };
                let state = scopes[scope].get_or_insert_with(Scope::default);
                let frame = state.look(snapshot, &threads, now, &self.labels, scope);
                let mut history = lock(&histories[scope]);
                history.set_window(history_window(), every);
                history.record(&frame, now);
                drop(history);
                frames[scope] = Some(frame);
            }
            for wake in self.publish(frames) {
                wake();
            }
            self.rest();
        }
    }

    /// Asleep while every view is hidden; `None` once the last one is gone (or the source):
    /// the thread ends. Else the source and which scopes are looked at.
    fn wait_for_a_view(&self) -> Option<(Arc<dyn ResourceSource>, [bool; 2])> {
        let mut state = lock(&self.state);
        loop {
            if state.ended || state.subscribers.is_empty() {
                state.ended = true;
                state.thread = false;
                state.held = None;
                return None;
            }
            if state.subscribers.iter().any(|s| s.shown) {
                break;
            }
            state = self.signal.wait(state).unwrap_or_else(PoisonError::into_inner);
        }
        state.kick = false;
        let wants = [THIS, ALL].map(|scope| {
            state.subscribers.iter().any(|s| s.shown && State::scope_of(s.all) == scope)
        });
        match state.source.upgrade() {
            Some(source) => Some((source, wants)),
            None => {
                state.ended = true;
                state.thread = false;
                state.held = None;
                None
            }
        }
    }

    /// Each shown view gets its scope's look (the frame shared); kept for the next ones.
    /// The wakers of those told.
    fn publish(&self, frames: [Option<super::Frame>; 2]) -> Vec<Waker> {
        let mut state = lock(&self.state);
        state.looks += 1;
        let generation = state.looks;
        let ticks = frames.map(|f| f.map(|f| Tick { generation, now: Arc::new(f) }));
        let mut wakers = Vec::new();
        for sub in state.subscribers.iter().filter(|s| s.shown) {
            if let Some(tick) = &ticks[State::scope_of(sub.all)] {
                sub.tables.put(tick.clone());
                wakers.extend(sub.wake.clone());
            }
        }
        for (scope, tick) in ticks.into_iter().enumerate() {
            if tick.is_some() {
                state.last[scope] = tick;
            }
        }
        wakers
    }

    /// How often it looks.
    fn every(&self) -> Duration {
        lock(&self.state).every.unwrap_or_else(interval)
    }

    /// Until the next look (sooner when kicked, or when the last view goes).
    fn rest(&self) {
        let every = self.every();
        let mut state = lock(&self.state);
        let deadline = Instant::now() + every;
        while !state.kick && !state.ended && !state.subscribers.is_empty() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            state = self.signal.wait_timeout(state, left).unwrap_or_else(PoisonError::into_inner).0;
        }
    }
}

/// A scope's counters between looks (rates, programs).
#[derive(Default)]
struct Scope {
    rates: Rates,
    trees: process_stats::ProcessTrees,
    programs: ProcessRates,
}

impl Scope {
    fn look(
        &mut self,
        mut snapshot: ResourceSnapshot,
        threads: &[(String, f32)],
        now: Instant,
        labels: &FrameLabels,
        scope: usize,
    ) -> super::Frame {
        let disk = snapshot.disk.or_else(process_stats::disk_io);
        let (cpu, used) = add_programs(&mut self.trees, &mut snapshot.parts);
        let trees = &mut self.trees;
        let rows = self.programs.sample(&used, |pid| (trees.name(pid), trees.invoked(pid)), now);
        let (parts, disk) = self.rates.sample(&snapshot.parts, &cpu, disk, now);
        let mut frame = frame(snapshot, threads, &parts, &rows, labels);
        frame.disk = disk;
        frame.all = scope == ALL;
        frame
    }
}

/// A view's newest look, waiting for it: a new one replaces it (a view draws the latest;
/// the ones it missed would only be thrown away).
#[derive(Debug, Clone, Default)]
pub struct Latest(Arc<Mutex<Option<Tick>>>);

impl Latest {
    fn put(&self, tick: Tick) {
        *lock(&self.0) = Some(tick);
    }

    /// The newest look not taken yet.
    pub fn take(&self) -> Option<Tick> {
        lock(&self.0).take()
    }
}

/// A view's hold on its window's hub: its newest look waits in [`Self::tables`]. Dropped (the
/// view closed), the hub forgets it; the last one gone, its thread ends.
pub struct Subscription {
    hub: Arc<Hub>,
    id: u64,
    pub tables: Latest,
}

impl std::fmt::Debug for Subscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Subscription").field("id", &self.id).finish_non_exhaustive()
    }
}

impl Subscription {
    /// The view shown (the hub looks; its scope's last table comes at once) or hidden.
    pub fn set_shown(&self, shown: bool) {
        self.hub.change(self.id, |sub, last| {
            if shown
                && !sub.shown
                && let Some(table) = &last[State::scope_of(sub.all)]
            {
                sub.tables.put(table.clone());
            }
            sub.shown = shown;
        });
    }

    /// The view looks at every open project (`true`) or this window's: looked at now.
    pub fn set_all(&self, all: bool) {
        self.hub.change(self.id, |sub, last| {
            if sub.all == all {
                return;
            }
            sub.all = all;
            if sub.shown
                && let Some(table) = &last[State::scope_of(all)]
            {
                sub.tables.put(table.clone());
            }
        });
        lock(&self.hub.state).kick = true;
        self.hub.signal.notify_all();
    }

    /// A look asked now (an action done: its effect shown without waiting the interval).
    pub fn look_now(&self) {
        lock(&self.hub.state).kick = true;
        self.hub.signal.notify_all();
    }

    /// The values over time of the scope it looks at (locked briefly, when a graph is
    /// drawn).
    pub fn history(&self) -> Arc<Mutex<History>> {
        let state = lock(&self.hub.state);
        let all = state.subscribers.iter().find(|s| s.id == self.id).is_some_and(|s| s.all);
        Arc::clone(&state.histories[State::scope_of(all)])
    }

    /// What wakes the app when its looks come.
    pub fn set_waker(&self, wake: Waker) {
        let mut state = lock(&self.hub.state);
        if let Some(sub) = state.subscribers.iter_mut().find(|s| s.id == self.id) {
            sub.wake = Some(wake);
        }
    }

    pub fn has_waker(&self) -> bool {
        let state = lock(&self.hub.state);
        state.subscribers.iter().any(|s| s.id == self.id && s.wake.is_some())
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let mut state = lock(&self.hub.state);
        state.subscribers.retain(|s| s.id != self.id);
        if state.subscribers.is_empty() {
            state.ended = true;
        }
        state.hold();
        drop(state);
        self.hub.signal.notify_all();
    }
}

#[cfg(test)]
#[path = "../../tests/unit/collector/hub.rs"]
mod tests;
