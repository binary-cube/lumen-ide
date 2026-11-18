//! The Node runtime the projects' Node tools run on when the system has none in `PATH`:
//! the app's own (the language servers' Node, bundled or downloaded), told here by the app
//! once its modules are loaded — no module names another.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

/// Where the app's Node is (`None`: it has none now).
pub type NodeFinder = Arc<dyn Fn() -> Option<PathBuf> + Send + Sync>;

static NODE: RwLock<Option<NodeFinder>> = RwLock::new(None);

/// How the app's Node is found (the app's wiring; `None`: it has none).
pub fn use_node(finder: Option<NodeFinder>) {
    *NODE.write().unwrap_or_else(std::sync::PoisonError::into_inner) = finder;
}

/// The app's Node program, if it has one (asked off the UI thread: it may look at the disk).
pub fn node() -> Option<PathBuf> {
    let finder = NODE.read().unwrap_or_else(std::sync::PoisonError::into_inner).clone()?;
    finder()
}
