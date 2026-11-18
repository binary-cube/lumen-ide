use super::*;

#[test]
fn steps_wrap_pages_stop() {
    assert_eq!(step_index(4, 1, 5), 0, "past the end: the start");
    assert_eq!(step_index(0, -1, 5), 4, "before the start: the end");
    assert_eq!(step_index(3, 8, 5), 4, "a page stops at the end");
    assert_eq!(step_index(1, -8, 5), 0);
    assert_eq!(step_index(0, 1, 0), 0, "empty list");
}
