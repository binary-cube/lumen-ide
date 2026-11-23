use super::*;

/// A change makes a new generation (each view told again); the same options again don't.
#[test]
fn a_change_is_a_new_generation() {
    let _turn = TEST_TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let before = generation();
    let options = current();
    set(options);
    assert_eq!(generation(), before, "the same: nothing to tell");
    set(TextOptions { word_wrap: !options.word_wrap });
    assert!(generation() > before);
    assert_eq!(current().word_wrap, !options.word_wrap);
    set(options);
}
