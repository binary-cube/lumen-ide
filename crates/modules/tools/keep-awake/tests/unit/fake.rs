//! A blocker that counts its holds (live ones), or refuses.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use power::{Keep, PowerError, SleepBlocker, SleepHold};

#[derive(Debug, Default)]
pub struct Blocker {
    pub live: Arc<AtomicUsize>,
    pub refuse: AtomicBool,
    /// Whether the last hold kept the screen on too.
    pub screen: AtomicBool,
}

#[derive(Debug)]
struct Hold(Arc<AtomicUsize>);

impl SleepHold for Hold {}

impl Drop for Hold {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

impl SleepBlocker for Blocker {
    fn hold(&self, _: &str, keep: Keep) -> Result<Box<dyn SleepHold>, PowerError> {
        if self.refuse.load(Ordering::Relaxed) {
            return Err(PowerError::Unsupported("a test".into()));
        }
        self.screen.store(keep.screen, Ordering::Relaxed);
        self.live.fetch_add(1, Ordering::Relaxed);
        Ok(Box::new(Hold(Arc::clone(&self.live))))
    }
}
