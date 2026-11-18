//! The system's own way to keep the computer awake ([`power::SleepBlocker`]): an IOKit
//! assertion on macOS (`pmset -g assertions` lists it), a power request on Windows
//! (`powercfg /requests`), a systemd inhibitor on Linux (`systemd-inhibit --list`). The
//! system's idle sleep is held off, and the screen's too when asked ([`power::Keep`]). A
//! hold is let go when dropped, and by the system when the process ends (a crash leaves
//! nothing behind).

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

use std::sync::Arc;

use power::{Keep, PowerError, SleepBlocker, SleepHold};

/// This system's blocker.
pub fn system() -> Arc<dyn SleepBlocker> {
    Arc::new(System)
}

#[derive(Debug)]
struct System;

impl SleepBlocker for System {
    fn hold(&self, reason: &str, keep: Keep) -> Result<Box<dyn SleepHold>, PowerError> {
        #[cfg(target_os = "macos")]
        return macos::hold(reason, keep);
        #[cfg(windows)]
        return windows::hold(reason, keep);
        #[cfg(target_os = "linux")]
        return linux::hold(reason, keep);
        #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
        {
            let _ = (reason, keep);
            Err(PowerError::Unsupported("this system".into()))
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
