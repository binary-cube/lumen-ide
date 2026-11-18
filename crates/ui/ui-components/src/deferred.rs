//! A view's action prepared in the background (what reads a file or git never runs on the UI
//! thread): the view starts it, polls it, and gives the action to the app when it's ready. A new
//! start replaces the one waited for: the last click wins.

use std::sync::Arc;
use std::sync::mpsc::Receiver;

use crate::ViewerAction;

type Wake = Arc<dyn Fn() + Send + Sync>;

/// One action being prepared.
#[derive(Default)]
pub struct Deferred {
    waiting: Option<Receiver<Option<ViewerAction>>>,
    waker: Option<Wake>,
}

impl std::fmt::Debug for Deferred {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Deferred").field("busy", &self.is_busy()).finish_non_exhaustive()
    }
}

impl Deferred {
    /// How the app is woken when it's ready (the view's paint has it).
    pub fn set_waker(&mut self, waker: &Option<Wake>) {
        if waker.is_some() {
            self.waker.clone_from(waker);
        }
    }

    /// `job` run on a thread named `thread`.
    pub fn start(
        &mut self,
        thread: &'static str,
        job: impl FnOnce() -> Option<ViewerAction> + Send + 'static,
    ) {
        let (tx, rx) = std::sync::mpsc::channel();
        let waker = self.waker.clone();
        kernel::work::run(thread, move || {
            let _ = tx.send(job());
            if let Some(wake) = waker {
                wake();
            }
        });
        self.waiting = Some(rx);
    }

    pub fn is_busy(&self) -> bool {
        self.waiting.is_some()
    }

    /// The action, once ready.
    pub fn poll(&mut self) -> Option<ViewerAction> {
        let action = match self.waiting.as_ref()?.try_recv() {
            Ok(action) => action,
            Err(std::sync::mpsc::TryRecvError::Empty) => return None,
            // The job ended without an answer (it panicked): nothing to wait for.
            Err(std::sync::mpsc::TryRecvError::Disconnected) => None,
        };
        self.waiting = None;
        action
    }
}

#[cfg(test)]
#[path = "../tests/unit/deferred.rs"]
mod tests;
