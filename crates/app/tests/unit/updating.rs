use super::*;

/// A folder with no update staged in it: nothing is started.
#[test]
fn nothing_staged_hands_nothing_over() {
    let dir = std::env::temp_dir()
        .join(format!("updating-{}", kernel::id::uuid_text(kernel::id::new_uuid())));
    assert!(!hand_over(&dir, 1));
}
