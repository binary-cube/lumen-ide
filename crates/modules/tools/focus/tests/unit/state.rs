use super::*;

#[test]
fn off_at_start_then_toggled() {
    assert!(!is_on());
    assert!(toggle() && is_on());
    assert!(!toggle() && !is_on());
    set(true);
    assert!(is_on());
}
