//! The remote context's domain events (on the project's bus): what was transferred, and what
//! runs and waits — for whoever shows it (the log, the status bar, the tree).

use kernel::DomainEvent;

use crate::application::Action;

/// A transfer ended: the file (relative), which way, the bytes, and why it failed if it did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferFinished {
    pub path: String,
    pub action: Action,
    pub bytes: u64,
    pub error: Option<String>,
}

impl DomainEvent for TransferFinished {}

/// What the project's transfers are doing now: how many run, how many files wait to upload
/// (they couldn't go: offline, an error).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TransfersChanged {
    pub running: usize,
    pub to_upload: usize,
}

impl DomainEvent for TransfersChanged {}
