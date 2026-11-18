//! An update asked for by the updates' module, handed over as the app ends (plan 84f): its
//! helper started for the staged update and the app's place, to reopen this run's session
//! once the new version starts. The module stages it and asks; only the app quits.

use std::path::Path;

/// The update staged in `dir` handed to its helper (session `session` reopened after):
/// `false` if nothing could be (said in the log; the app just ends).
pub fn hand_over(dir: &Path, session: u128) -> bool {
    let Some(staged) = self_update::stage::read(dir) else {
        tracing::warn!(dir = %dir.display(), "no update staged there: nothing put in place");
        return false;
    };
    let Ok(exe) = std::env::current_exe() else {
        tracing::warn!("this program's path unknown: the update isn't put in place");
        return false;
    };
    let target = self_update::install::target(&exe);
    match self_update::install::start_helper(&staged, &target, Some(session)) {
        Ok(()) => true,
        Err(why) => {
            tracing::warn!("the update's helper didn't start: {why}");
            false
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/updating.rs"]
mod tests;
