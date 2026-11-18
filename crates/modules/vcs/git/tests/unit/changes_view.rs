use super::*;

/// Without the project's changes (not followed): nothing read, nothing shown.
#[test]
fn without_the_project_s_changes_the_list_is_empty() {
    let mut list = ChangesList::default();
    assert!(!list.poll());
    assert!(list.files.is_empty());
    assert_eq!(list.generation, 0);
}
