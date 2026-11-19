//! An update asked for by the updates' module (plan 85): already in the app's place (the
//! module put it there), the app started again as it ends, to reopen this run's session.
//! The module puts it in place and asks; only the app quits.

/// The app started again once this process ends, session `session` reopened: `false` if
/// it couldn't be (said in the log; the app just ends, the update in place for its next
/// start).
pub fn restart(session: u128) -> bool {
    self_update::install::restart(Some(session))
}
