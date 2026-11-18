//! The system's hold on its sleep, one for the whole app (every window shares it): taken
//! when wanted, let go when not — and by the system when the process ends, so a crash
//! leaves nothing behind. Taking it is a quick call to the system (an IOKit assertion, a
//! Windows power request, a waiting `systemd-inhibit`), never a wait.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use power::{Keep, PowerError, SleepBlocker, SleepHold};

/// A hold taken: since when, on what.
type Held = (Box<dyn SleepHold>, Instant, Keep);

/// The hold, and since when it's held.
#[derive(Debug)]
pub struct Keeper {
    blocker: Arc<dyn SleepBlocker>,
    held: Mutex<Option<Held>>,
}

impl Keeper {
    pub fn new(blocker: Arc<dyn SleepBlocker>) -> Self {
        Self { blocker, held: Mutex::new(None) }
    }

    fn held(&self) -> std::sync::MutexGuard<'_, Option<Held>> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The hold taken on what `wanted` says, or let go (`None`). Held already on something
    /// else (the screen asked too, or no longer): the new hold taken before the old one goes
    /// (no moment without). Taking it may fail (the system refused, or has no way to):
    /// nothing new is held then.
    pub fn set(&self, wanted: Option<Keep>) -> Result<(), PowerError> {
        let mut held = self.held();
        let now = held.as_ref().map(|(_, _, keep)| *keep);
        match wanted {
            Some(keep) if now != Some(keep) => {
                let reason = kernel::brand::named("{app}: kept awake while work runs");
                let hold = self.blocker.hold(&reason, keep)?;
                let since = held.as_ref().map_or_else(Instant::now, |(_, at, _)| *at);
                *held = Some((hold, since, keep));
                tracing::info!(screen = keep.screen, "the computer is kept awake");
            }
            None if now.is_some() => {
                *held = None;
                tracing::info!("the computer may sleep again");
            }
            _ => {}
        }
        Ok(())
    }

    pub fn is_held(&self) -> bool {
        self.held().is_some()
    }

    /// Since when it's held.
    pub fn since(&self) -> Option<Instant> {
        self.held().as_ref().map(|(_, at, _)| *at)
    }
}

/// The app's keeper (the system's blocker).
#[cfg(not(test))]
pub fn keeper() -> &'static Keeper {
    static KEEPER: std::sync::OnceLock<Keeper> = std::sync::OnceLock::new();
    KEEPER.get_or_init(|| Keeper::new(power_system::system()))
}

/// Each test thread its own keeper, on a blocker that only counts (the tests never keep
/// the machine they run on awake).
#[cfg(test)]
pub fn keeper() -> &'static Keeper {
    thread_local! {
        static KEEPER: &'static Keeper =
            Box::leak(Box::new(Keeper::new(Arc::new(fake::Blocker::default()))));
    }
    KEEPER.with(|k| *k)
}

/// A blocker that counts its holds (live ones), or refuses: the tests'.
#[cfg(test)]
#[path = "../tests/unit/fake.rs"]
pub mod fake;

#[cfg(test)]
#[path = "../tests/unit/keeper.rs"]
mod tests;
