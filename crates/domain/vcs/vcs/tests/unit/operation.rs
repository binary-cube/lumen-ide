use super::*;

#[test]
fn a_rebase_says_its_branch_and_step() {
    let op = Operation::rebase(Some("refs/heads/feature/login\n"), Some("3\n"), Some("7"));
    assert_eq!(op.branch.as_deref(), Some("feature/login"));
    assert_eq!(op.step, Some((3, 7)));
    let op = Operation::rebase(Some("detached HEAD"), None, Some("7"));
    assert_eq!((op.branch, op.step), (None, None), "nothing said: nothing shown");
    assert_eq!(Operation::rebase(None, Some("1"), Some("0")).step, None);
}
