use super::*;

/// While work runs the hold is taken, once it ends it's let go; always: at once; off: none.
#[test]
fn the_hold_follows_the_mode_and_the_work() {
    let keeper = crate::keeper::keeper();
    follow(Mode::WhileWorking).unwrap();
    assert!(!keeper.is_held(), "nothing runs yet");
    step(true);
    assert!(keeper.is_held(), "work runs: kept awake");
    step(true);
    assert!(keeper.is_held());
    step(false);
    assert!(!keeper.is_held(), "the work ended: it may sleep");
    follow(Mode::Always).unwrap();
    assert!(keeper.is_held());
    follow(Mode::Off).unwrap();
    assert!(!keeper.is_held());
}
