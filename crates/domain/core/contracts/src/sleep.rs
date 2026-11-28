//! Views that can sleep (a heavy view left alone: an agent's session, a diagram's page), asked
//! and followed by modules that don't know them: put to sleep or woken by their key (their
//! tab's), how each stands, and its changes on the project's bus. Whoever runs the sleeping
//! answers; nobody answering, nothing sleeps.

use kernel::{DomainEvent, Request};

pub use sleep::{CanSleep, Veto};

/// How a view that can sleep stands now, as last told: its kind, what it holds (`None`: not
/// measured, or asleep), whether what it runs runs now, whether it sleeps, whether it can be
/// put to sleep by hand now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SleepState {
    pub kind: &'static str,
    pub memory: Option<u64>,
    pub running: bool,
    pub asleep: bool,
    pub can_sleep: CanSleep,
}

/// Put the view known by `key` to sleep now (the user's order: whatever its idle time).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PutToSleep {
    pub key: String,
}

impl Request for PutToSleep {
    /// `false`: no such view, or it can't sleep now (told to the user, with why).
    type Answer = bool;
}

/// Wake the view known by `key`, its tab brought to the front.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WakeUp {
    pub key: String,
}

impl Request for WakeUp {
    /// `false`: no such view, it isn't asleep, or its tab can't be shown.
    type Answer = bool;
}

/// How the view known by `key` stands now (read from memory only). Asking also says someone
/// shows what it holds: its memory is measured for a while.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SleepStateOf {
    pub key: String,
}

impl Request for SleepStateOf {
    /// `None`: no such view (its tab closed).
    type Answer = Option<SleepState>;
}

/// The view known by `key` stands otherwise now, or is gone (`None`: its tab closed):
/// published on its project's bus (a view of no project: on every open project's), once per
/// change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SleepStateChanged {
    pub key: String,
    pub state: Option<SleepState>,
}

impl DomainEvent for SleepStateChanged {}
