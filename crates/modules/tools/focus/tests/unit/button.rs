use super::*;

#[test]
fn lit_while_on() {
    crate::state::set(false);
    assert!(!FocusButton.active());
    crate::state::set(true);
    assert!(FocusButton.active());
    assert_eq!(FocusButton.toggle_command(), Some(crate::TOGGLE.0));
    crate::state::set(false);
}
