use super::*;

/// The native rows get the app's states: the ones it says don't apply greyed out, every
/// other enabled again.
#[test]
fn the_rows_follow_the_states_given() {
    let ids = ["git.commit", "file.save", "tool-window:git"];
    let disabled = vec!["git.commit".to_owned(), "tool-window:git".to_owned()];
    assert_eq!(
        states(ids.iter().copied(), &disabled),
        [("git.commit", false), ("file.save", true), ("tool-window:git", false)]
    );
    assert!(states(ids.iter().copied(), &[]).iter().all(|(_, enabled)| *enabled));
}
