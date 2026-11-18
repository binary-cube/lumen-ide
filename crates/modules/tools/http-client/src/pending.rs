//! What a view prepares on a worker (never on the UI thread): started, then taken by a poll
//! once it's ready. The app is woken when it is, by the view's paint waker (told once it's
//! known; a result ready before it is taken at the next poll or paint). A new start replaces
//! the one waited for: the last one wins.

use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError};

type Wake = Arc<dyn Fn() + Send + Sync>;

pub struct Pending<T> {
    waiting: Option<Receiver<T>>,
    wake: Arc<Mutex<Option<Wake>>>,
}

impl<T> Default for Pending<T> {
    fn default() -> Self {
        Self { waiting: None, wake: Arc::default() }
    }
}

impl<T> std::fmt::Debug for Pending<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pending").field("busy", &self.waiting.is_some()).finish_non_exhaustive()
    }
}

impl<T: Send + 'static> Pending<T> {
    /// How the app is woken when a result is ready (the view's paint has it).
    pub fn set_waker(&self, waker: &Option<Wake>) {
        let Some(waker) = waker else { return };
        let mut wake = self.wake.lock().unwrap_or_else(PoisonError::into_inner);
        if wake.is_none() {
            *wake = Some(Arc::clone(waker));
        }
    }

    /// `job` run on the worker pool named `thread`.
    pub fn start(&mut self, thread: &'static str, job: impl FnOnce() -> T + Send + 'static) {
        let (tx, rx) = std::sync::mpsc::channel();
        let wake = Arc::clone(&self.wake);
        kernel::work::run(thread, move || {
            let _ = tx.send(job());
            let wake = wake.lock().unwrap_or_else(PoisonError::into_inner).clone();
            if let Some(wake) = wake {
                wake();
            }
        });
        self.waiting = Some(rx);
    }

    /// A job is going (its result not taken yet).
    pub fn is_busy(&self) -> bool {
        self.waiting.is_some()
    }

    /// The result, once it's ready (once).
    pub fn take(&mut self) -> Option<T> {
        let rx = self.waiting.as_ref()?;
        match rx.try_recv() {
            Ok(value) => {
                self.waiting = None;
                Some(value)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.waiting = None;
                None
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/pending.rs"]
mod tests;
