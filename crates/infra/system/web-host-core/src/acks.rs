//! A frame's buffer held until its app copied it: the engine's paint callback waits here
//! (on the host's main thread), the app's `FrameDone` reaches it straight from the client's
//! reader thread — never through the main thread, which is the one waiting.
//!
//! A page whose app didn't copy a frame in time is behind: its next frames aren't waited
//! for until it copies one again (one slow app never slows the others' pages, which share
//! the main thread).

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::engine::PageKey;

/// A page's copies.
#[derive(Debug, Clone, Copy, Default)]
struct Copies {
    /// The latest frame copied.
    latest: u64,
    /// A frame wasn't copied in time: not waited for until one is.
    behind: bool,
}

/// What a copy coming for a page that was behind calls (the host's main loop woken: it
/// paints the page again at once).
type Wake = Arc<dyn Fn() + Send + Sync>;

/// The frames copied, by open page.
#[derive(Clone, Default)]
pub struct Acks {
    shared: Arc<(Mutex<HashMap<PageKey, Copies>>, Condvar)>,
    /// `None`: nobody is woken.
    wake: Option<Wake>,
}

impl std::fmt::Debug for Acks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Acks").field("pages", &self.lock().len()).finish_non_exhaustive()
    }
}

impl Acks {
    /// Acks that call `wake` when a page that was behind gets a copy again.
    pub(crate) fn waking(wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self { shared: Arc::default(), wake: Some(Arc::new(wake)) }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<PageKey, Copies>> {
        self.shared.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A page opened (only open pages' copies are kept).
    pub(crate) fn open(&self, page: PageKey) {
        self.lock().insert(page, Copies::default());
    }

    /// Frame `frame` of `page` copied (a page not open: nothing kept); a page that was behind
    /// caught up wakes the main loop.
    pub(crate) fn done(&self, page: PageKey, frame: u64) {
        let mut copied = self.lock();
        let Some(copies) = copied.get_mut(&page) else { return };
        copies.latest = copies.latest.max(frame);
        let was_behind = std::mem::replace(&mut copies.behind, false);
        drop(copied);
        self.shared.1.notify_all();
        // Woken after the lock is let go.
        if was_behind && let Some(wake) = &self.wake {
            wake();
        }
    }

    /// Waits until `page`'s frame `frame` is copied (`false`: not within `wait`, or the page
    /// is behind — its app is slow or gone; the frame's buffer goes back anyway, the next
    /// frame shows the page).
    pub fn wait(&self, page: PageKey, frame: u64, wait: Duration) -> bool {
        let deadline = Instant::now() + wait;
        let mut guard = self.lock();
        loop {
            let Some(copies) = guard.get_mut(&page) else { return false };
            if copies.latest >= frame {
                return true;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if copies.behind || left.is_zero() {
                copies.behind = true;
                return false;
            }
            guard =
                self.shared.1.wait_timeout(guard, left).unwrap_or_else(PoisonError::into_inner).0;
        }
    }

    /// Whether `page`'s app is behind: a frame of it wasn't copied in time, and none has been
    /// since (a page not open: not behind). Its next frames aren't waited for; the engine
    /// hands them over sparingly until it catches up (a full queue would block its paint).
    pub fn behind(&self, page: PageKey) -> bool {
        self.lock().get(&page).is_some_and(|copies| copies.behind)
    }

    /// A page gone: its entry let go.
    pub(crate) fn forget(&self, page: PageKey) {
        self.lock().remove(&page);
    }
}

#[cfg(test)]
#[path = "../tests/unit/acks.rs"]
mod tests;
