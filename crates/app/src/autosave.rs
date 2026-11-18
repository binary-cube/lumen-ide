//! Saving automatically (on by default, `[editor] autosave` in `settings.toml`): a file
//! is written once typing pauses, when another tab is opened and when the window loses
//! focus. Unsaved documents (no file yet) are left alone.

use std::time::Duration;

/// How long typing pauses before the files are saved.
pub const IDLE: Duration = Duration::from_secs(2);

pub fn enabled() -> bool {
    crate::config::settings().get(&crate::settings::AUTOSAVE)
}
