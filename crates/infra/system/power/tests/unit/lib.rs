use super::*;

/// Held, then let go (on this machine, for real: macOS lists it under `pmset -g
/// assertions` while it lives). Asked for by `EDITOR_POWER_REAL=1` only: it touches the
/// system's power state.
#[test]
fn a_hold_is_taken_and_let_go() {
    if std::env::var("EDITOR_POWER_REAL").is_err() {
        return;
    }
    let hold = system().hold("editor test: kept awake", power::Keep::default()).expect("held");
    drop(hold);
    let screen = power::Keep { screen: true };
    let hold = system().hold("editor test: the screen kept on", screen).expect("held");
    drop(hold);
}
