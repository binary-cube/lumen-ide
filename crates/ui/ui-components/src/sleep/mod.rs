//! Views that can sleep (plan 101): a heavy view left alone (an agent's session, a diagram's
//! page) stopped by its own module, its tab kept, brought back as it was when the user acts
//! on it. This is the part every such view holds, the same for all:
//!
//! - its **slot** ([`Slot`]), known by its key in a registry: when it was last touched, how
//!   long it was quiet (its own measure, [`Quiet`]), what of its own keeps it awake (a
//!   [`Veto`]), an order to sleep or to wake waiting for its poll, what wakes its window;
//! - the **part** a view keeps ([`SleepPart`]): takes the orders at its poll, says its menu
//!   rows, its tab's moon, its card ([`card`]) and its refusals.
//!
//! What runs the sleeping (the timer looking once a minute, the requests from other modules,
//! the commands) is a module of its own, which knows no view: it reads these slots. What a
//! view does to fall asleep and to wake is its own module's. Each kind's idle time comes from
//! its module's settings ([`configure`]). Nothing here reads the disk or blocks: locks only
//! move data, announcements and wakes happen with them let go.

mod card;
mod part;

use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::{Duration, Instant};

pub use ::sleep::{CanSleep, ORDER_FRESH, SleepFacts, SleepKind, SleepTexts, SleepWhy, Veto};
pub use card::{AsleepCard, SCRIM_ALPHA, paint_card, paint_picture};
pub use contracts::sleep::{SleepState, SleepStateChanged};
pub use part::SleepPart;

/// A view's action putting it to sleep by hand (its tab's menu, the command).
pub const SLEEP_ACTION: &str = "sleep-now";
/// A view's action waking it (its tab's menu, the command).
pub const WAKE_ACTION: &str = "wake-now";
/// Asleep: its tab's icon, its card's, its menu row's.
pub const ASLEEP_ICON: &str = "moon";
/// Waking: its menu row's icon.
pub const WAKE_ICON: &str = "sun";

/// What wakes a window (its poll follows).
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// How long a view has done nothing by itself, by its own measure (a program's output, a
/// page's changes): read off the UI thread, by the timer.
pub trait Quiet: Send + Sync {
    /// `None`: not known (it isn't put to sleep).
    fn quiet_for(&self) -> Option<Duration>;
}

/// Where a view's tab is, to bring it to the front when it's woken from elsewhere: asked
/// again by its name, its tab comes forward.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SleepPlace {
    /// A tab of the editor.
    Editor { name: String, extension: &'static str },
    /// A tab of a tool window's (at the bottom).
    Panel { tool: &'static str, name: String, extension: &'static str },
}

/// An order to sleep, waiting for the view's poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Order {
    pub at: Instant,
    pub why: SleepWhy,
}

/// What a view's slot holds: written by its view (and by the orders), read by the timer and
/// by whoever asks how it stands.
pub struct SlotState {
    pub kind: &'static SleepKind,
    pub key: String,
    /// The project it was opened in (whose bus is told of it; `None`: every one's).
    pub project: Option<kernel::ProjectId>,
    pub place: SleepPlace,
    /// When the user last acted on it, or it last started (`None`: never started).
    pub touched: Option<Instant>,
    pub quiet: Option<Arc<dyn Quiet>>,
    pub veto: Option<Veto>,
    /// What it runs runs now (started, not ended, not asleep).
    pub running: bool,
    pub asleep: bool,
    pub order: Option<Order>,
    /// Asked to wake (from elsewhere), taken at its view's poll.
    pub wake: bool,
    /// Coming back (until its view says it's settled).
    pub waking: bool,
    /// What it holds, as its module last measured.
    pub memory: Option<u64>,
    /// What wakes its window (given at its view's paint): an order is taken at once.
    pub waker: Option<Waker>,
    /// How it stood when last announced.
    pub announced: Option<SleepState>,
    /// What kept it awake at the timer's last look, as last logged (`Some(None)`: nothing).
    pub told_awake: Option<Option<&'static str>>,
}

impl SlotState {
    fn new(
        kind: &'static SleepKind,
        key: String,
        project: Option<kernel::ProjectId>,
        place: SleepPlace,
    ) -> Self {
        Self {
            kind,
            key,
            project,
            place,
            touched: None,
            quiet: None,
            veto: None,
            running: false,
            asleep: false,
            order: None,
            wake: false,
            waking: false,
            memory: None,
            waker: None,
            announced: None,
            told_awake: None,
        }
    }

    /// The facts the rule reads, now.
    pub fn facts(&self) -> SleepFacts {
        SleepFacts {
            touched: self.touched,
            quiet_for: self.quiet.as_ref().and_then(|q| q.quiet_for()),
            veto: self.veto,
            asleep: self.asleep,
            ordered: self.order.is_some(),
            waking: self.wake || self.waking,
        }
    }

    /// Whether it can be put to sleep by hand now.
    pub fn can_sleep(&self) -> CanSleep {
        ::sleep::can_sleep(&self.facts())
    }

    /// How it stands, as told to others.
    pub fn state(&self) -> SleepState {
        SleepState {
            kind: self.kind.id,
            memory: self.memory,
            running: self.running && !self.asleep,
            asleep: self.asleep,
            can_sleep: self.can_sleep(),
        }
    }
}

impl std::fmt::Debug for SlotState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SlotState")
            .field("kind", &self.kind.id)
            .field("key", &self.key)
            .field("asleep", &self.asleep)
            .finish_non_exhaustive()
    }
}

/// A view's slot (shared with the registry, weakly: gone with its view).
#[derive(Debug)]
pub struct Slot(Mutex<SlotState>);

impl Slot {
    /// Locked, a poisoned lock taken as it is.
    pub fn lock(&self) -> MutexGuard<'_, SlotState> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Its key.
    pub fn key(&self) -> String {
        self.lock().key.clone()
    }

    /// Announced as it stands now, only if that changed since last (its lock let go first).
    pub fn announce(&self) {
        let (project, event) = {
            let mut state = self.lock();
            let now = state.state();
            if state.announced == Some(now) {
                return;
            }
            state.announced = Some(now);
            (state.project, SleepStateChanged { key: state.key.clone(), state: Some(now) })
        };
        publish(project, &event);
    }

    /// Announced as it stands, changed or not: what was asked of it was refused (whoever
    /// shows it may have guessed the answer).
    pub fn announce_again(&self) {
        self.lock().announced = None;
        self.announce();
    }

    /// What of its own keeps it awake now (`None`: nothing), told when it changes.
    pub fn set_veto(&self, veto: Option<Veto>) {
        {
            let mut state = self.lock();
            if state.veto == veto {
                return;
            }
            state.veto = veto;
        }
        self.announce();
    }

    /// What it holds, as just measured (`None`: not known), told when it changes.
    pub fn set_memory(&self, memory: Option<u64>) {
        self.lock().memory = memory;
        self.announce();
    }

    /// Put to sleep by hand now (its view stops at the poll its window's wake brings).
    pub fn sleep_by_hand(&self) -> Result<(), CanSleep> {
        let waker = {
            let mut state = self.lock();
            match state.can_sleep() {
                CanSleep::Yes => {}
                other => return Err(other),
            }
            state.order = Some(Order { at: Instant::now(), why: SleepWhy::ByHand });
            state.waker.clone()
        };
        self.announce();
        if let Some(waker) = waker {
            waker();
        }
        Ok(())
    }

    /// Put to sleep after `idle` left alone, if the rule still says so now (looked at again
    /// under its lock: touched meanwhile, it stays awake). `true` if ordered.
    pub fn sleep_idle(&self, idle: Duration) -> bool {
        let waker = {
            let mut state = self.lock();
            let now = Instant::now();
            if ::sleep::awake_because(&state.facts(), idle, now).is_some() {
                return false;
            }
            state.order = Some(Order { at: now, why: SleepWhy::Idle(idle) });
            state.waker.clone()
        };
        self.announce();
        if let Some(waker) = waker {
            waker();
        }
        true
    }

    /// Woken (at its view's poll): `false` if it isn't asleep.
    pub fn wake(&self) -> bool {
        let waker = {
            let mut state = self.lock();
            if !state.asleep {
                return false;
            }
            state.wake = true;
            state.waker.clone()
        };
        self.announce();
        if let Some(waker) = waker {
            waker();
        }
        true
    }

    /// An order not taken in time let go (the timer's look): `true` if one was.
    pub fn drop_stale_order(&self, now: Instant) -> bool {
        let stale = {
            let mut state = self.lock();
            let stale = state
                .order
                .is_some_and(|o| now.saturating_duration_since(o.at) >= ::sleep::ORDER_FRESH);
            if stale {
                state.order = None;
            }
            stale
        };
        if stale {
            self.announce();
        }
        stale
    }
}

/// The live slots.
static SLOTS: Mutex<Vec<Weak<Slot>>> = Mutex::new(Vec::new());

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A new slot, registered (the timer started for it).
pub(crate) fn register(
    kind: &'static SleepKind,
    key: String,
    project: Option<kernel::ProjectId>,
    place: SleepPlace,
) -> Arc<Slot> {
    let slot = Arc::new(Slot(Mutex::new(SlotState::new(kind, key, project, place))));
    {
        let mut slots = lock(&SLOTS);
        slots.retain(|s| s.strong_count() > 0);
        slots.push(Arc::downgrade(&slot));
    }
    start_timer();
    slot
}

/// Every live slot.
pub fn slots() -> Vec<Arc<Slot>> {
    let mut slots = lock(&SLOTS);
    slots.retain(|s| s.strong_count() > 0);
    slots.iter().filter_map(Weak::upgrade).collect()
}

/// Whether some view that can sleep lives.
pub fn any() -> bool {
    lock(&SLOTS).iter().any(|s| s.strong_count() > 0)
}

/// The live slot known by `key` (a tab closed and opened again with the same key: the new).
pub fn find(key: &str) -> Option<Arc<Slot>> {
    slots().into_iter().rev().find(|s| s.lock().key == key)
}

/// How many views of `kind` sleep now.
pub fn asleep_count(kind: &str) -> usize {
    slots()
        .iter()
        .filter(|s| {
            let state = s.lock();
            state.asleep && state.kind.id == kind
        })
        .count()
}

// ---------------------------------------------------------------------------------------
// Each kind's idle time, as its module's settings say.

/// Each kind's idle time (`None`: its sleeping is off).
static IDLE: Mutex<Vec<(&'static str, Option<Duration>)>> = Mutex::new(Vec::new());

/// `kind`'s views sleep after `idle` left alone (`None`: they don't), as its settings say
/// now: told by its module at startup and at each change.
pub fn configure(kind: &SleepKind, idle: Option<Duration>) {
    {
        let mut all = lock(&IDLE);
        all.retain(|(k, _)| *k != kind.id);
        all.push((kind.id, idle));
    }
    start_timer();
}

/// `kind`'s idle time (`None`: its sleeping is off, or never told).
pub fn idle_for(kind: &str) -> Option<Duration> {
    lock(&IDLE).iter().find(|(k, _)| *k == kind).and_then(|(_, idle)| *idle)
}

/// Whether some kind's sleeping is on.
pub fn any_on() -> bool {
    lock(&IDLE).iter().any(|(_, idle)| idle.is_some())
}

// ---------------------------------------------------------------------------------------
// What runs the sleeping, told by its module: how a state is published, what starts it.

/// Publishes a view's state on its project's bus (`None`: every open project's).
pub type Announcer = Arc<dyn Fn(Option<kernel::ProjectId>, &SleepStateChanged) + Send + Sync>;

static ANNOUNCER: Mutex<Option<Announcer>> = Mutex::new(None);
static STARTER: Mutex<Option<Waker>> = Mutex::new(None);

/// What publishes the views' states (the sleeping's module, as it starts).
pub fn set_announcer(announcer: Option<Announcer>) {
    *lock(&ANNOUNCER) = announcer;
}

/// What makes sure the timer runs (called as a view registers or a kind is configured).
pub fn set_starter(starter: Option<Waker>) {
    *lock(&STARTER) = starter;
}

fn start_timer() {
    let starter = lock(&STARTER).clone();
    if let Some(start) = starter {
        start();
    }
}

fn publish(project: Option<kernel::ProjectId>, event: &SleepStateChanged) {
    let announcer = lock(&ANNOUNCER).clone();
    if let Some(announce) = announcer {
        announce(project, event);
    }
}

// ---------------------------------------------------------------------------------------
// What shows the views' memory: measured only while someone asked lately.

/// When someone last asked what the views hold.
static MEMORY_ASKED: Mutex<Option<Instant>> = Mutex::new(None);

/// Someone shows what the views hold (a panel's rows): their modules measure for a while.
pub fn want_memory() {
    *lock(&MEMORY_ASKED) = Some(Instant::now());
    start_timer();
}

/// Whether someone asked what the views hold within `within`.
pub fn memory_wanted(within: Duration) -> bool {
    lock(&MEMORY_ASKED).is_some_and(|at| at.elapsed() < within)
}

// ---------------------------------------------------------------------------------------
// The view with the keyboard: what "Put to Sleep" and "Wake Up" act on.

static FOCUSED: Mutex<Weak<Slot>> = Mutex::new(Weak::new());

/// [`FOCUSED`], given to `f` (in the tests, each thread's own: they draw in parallel).
fn with_focused<R>(f: impl FnOnce(&mut Weak<Slot>) -> R) -> R {
    #[cfg(test)]
    {
        thread_local! {
            static TEST: std::cell::RefCell<Weak<Slot>> = const { std::cell::RefCell::new(Weak::new()) };
        }
        let _ = &FOCUSED;
        TEST.with(|focused| f(&mut focused.borrow_mut()))
    }
    #[cfg(not(test))]
    {
        f(&mut lock(&FOCUSED))
    }
}

/// The slot of the view that last had the keyboard (drawn focused), if it lives.
pub fn focused() -> Option<Arc<Slot>> {
    with_focused(|f| f.upgrade())
}

/// A refusal to sleep (`can`) told to the user, in `kind`'s words.
pub fn refusal(kind: &SleepKind, can: CanSleep) -> workbench_model::Notification {
    let texts = kind.texts;
    let why = match can {
        CanSleep::NotNow(veto) => veto.refusal,
        CanSleep::Sleeping => texts.cant_sleeping,
        CanSleep::Waking => texts.cant_waking,
        CanSleep::No | CanSleep::Yes => texts.cant,
    };
    workbench_model::Notification::warning(i18n::t(why))
}

#[cfg(test)]
#[path = "../../tests/unit/sleep/mod.rs"]
mod tests;
