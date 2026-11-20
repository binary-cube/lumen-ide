//! What a module offers a profile recording of its own (plan 91), beyond the probes: its
//! richer figures (a language server's requests waiting, its slowest answer; the HTTP
//! client's runs going), read by the recorder without knowing whose they are.
//!
//! - **The probes stay for what's small and often** (`kernel::probe`: a frame, a poll, a
//!   byte counted where it happens). A source is for what a module knows as a whole.
//! - **A contribution, registered by the app** ([`register`]); the recorder lists
//!   [`sources`] and never names a module.
//! - **Told when a recording starts and ends** ([`ProfileSource::started`] /
//!   [`ProfileSource::stopped`]): what a source keeps only while it's recorded begins and is
//!   let go there; nothing runs while nothing records.
//! - **Read once a second** ([`ProfileSource::read`]) on the recorder's thread: from memory
//!   only — never the disk, never a lock held long, never a call out.
//! - **Anonymous by its type**: an owner and what's read are `&'static str` of our code, a
//!   value a number — never a path, a name, a URL or a user's text
//!   (`.claude/rules/privacy.md`).

use std::sync::{Arc, PoisonError, RwLock};

/// What a reading is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReadingKind {
    /// How many happened since the last read (requests sent, entries parsed).
    Count,
    /// Bytes moved since the last read.
    Bytes,
    /// Time spent since the last read (microseconds).
    Micros,
    /// What it holds now (requests waiting, entries cached).
    Level,
}

impl ReadingKind {
    /// Its unit's name in a recording.
    pub fn unit(self) -> &'static str {
        match self {
            ReadingKind::Count | ReadingKind::Level => "count",
            ReadingKind::Bytes => "bytes",
            ReadingKind::Micros => "us",
        }
    }
}

/// One figure a source gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    /// What it is (`requests`, `waiting`, `answer-time`).
    pub what: &'static str,
    pub kind: ReadingKind,
    pub value: u64,
}

impl Reading {
    pub fn new(what: &'static str, kind: ReadingKind, value: u64) -> Self {
        Self { what, kind, value }
    }
}

/// A module's figures for a profile recording (contributed: `Arc<dyn ProfileSource>`).
pub trait ProfileSource: Send + Sync + std::fmt::Debug {
    /// Whose they are in the recording: the module's id (ours, static).
    fn owner(&self) -> &'static str;

    /// What it has now, or did since the last read (see [`ReadingKind`]): from memory only.
    fn read(&self) -> Vec<Reading>;

    /// A recording begins: what it keeps only while recorded begins (nothing by default).
    fn started(&self) {}

    /// The recording ended: what it kept for it is let go (nothing by default).
    fn stopped(&self) {}
}

static SOURCES: RwLock<Vec<Arc<dyn ProfileSource>>> = RwLock::new(Vec::new());

/// Registers a source (the app, at startup, from the modules' contributions); one with the
/// same owner is replaced.
pub fn register(source: Arc<dyn ProfileSource>) {
    let mut sources = SOURCES.write().unwrap_or_else(PoisonError::into_inner);
    sources.retain(|s| s.owner() != source.owner());
    sources.push(source);
}

/// Removes `owner`'s source (tests).
pub fn unregister(owner: &str) {
    SOURCES.write().unwrap_or_else(PoisonError::into_inner).retain(|s| s.owner() != owner);
}

/// The sources registered.
pub fn sources() -> Vec<Arc<dyn ProfileSource>> {
    SOURCES.read().unwrap_or_else(PoisonError::into_inner).clone()
}

#[cfg(test)]
#[path = "../tests/unit/profiling.rs"]
mod tests;
