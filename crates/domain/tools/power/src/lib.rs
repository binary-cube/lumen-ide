//! Keeping the computer awake while work runs (an AI working in a terminal, transfers, a
//! long indexing): the port a system answers ([`SleepBlocker`]: a hold on its idle sleep,
//! let go when the [`SleepHold`] is dropped — and by the system when the process ends), and
//! the policy around it ([`policy`]): when a hold is wanted, and what the screen does while
//! it's kept awake and nobody touches anything (dimmed, something moving so that nothing
//! burns in).

pub mod policy;

pub use policy::{Mode, ScreenCare, wants_hold};

/// What a hold keeps awake: the computer always; `screen`: its screen too (it doesn't turn
/// off nor lock while held — the user's choice, off by default).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Keep {
    pub screen: bool,
}

/// Why the system wouldn't keep awake.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PowerError {
    /// The system has no way to (Linux without systemd): said once, nothing held.
    #[error("not supported here: {0}")]
    Unsupported(String),
    /// The system refused (what it said).
    #[error("{0}")]
    Failed(String),
}

/// A hold on the system's idle sleep: the computer doesn't sleep while it lives (the screen
/// may still turn off or lock, unless [`Keep::screen`]). Let go when dropped.
pub trait SleepHold: Send + std::fmt::Debug {}

/// The system's way to keep the computer awake (macOS, Windows, Linux each their own).
pub trait SleepBlocker: Send + Sync + std::fmt::Debug {
    /// A hold on what `keep` says, `reason` shown where the system lists who keeps it awake
    /// (`pmset -g assertions`, `powercfg /requests`, `systemd-inhibit --list`).
    fn hold(&self, reason: &str, keep: Keep) -> Result<Box<dyn SleepHold>, PowerError>;
}
