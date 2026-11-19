use super::*;
use crate::{OperationKind, Stash};

fn file(path: &str, staged: Option<FileState>, unstaged: Option<FileState>) -> StagedFile {
    StagedFile { path: path.into(), staged, unstaged }
}

#[test]
fn nothing_being_worked_on_switches_freely() {
    assert_eq!(switch_plan(None, &[], &[], &[]), SwitchPlan::Free { carried: 0 });
}

#[test]
fn changes_the_target_doesn_t_touch_come_along() {
    let dirty = [file("a.php", None, Some(FileState::Modified))];
    assert_eq!(switch_plan(None, &dirty, &[], &[]), SwitchPlan::Free { carried: 1 });
}

#[test]
fn changes_to_files_that_differ_there_are_in_the_way() {
    let dirty = [
        file("a.php", Some(FileState::Modified), None),
        file("b.php", None, Some(FileState::Modified)),
        file("new.txt", None, Some(FileState::Untracked)),
    ];
    let differs = ["a.php".to_owned()];
    assert_eq!(
        switch_plan(None, &dirty, &differs, &[]),
        SwitchPlan::Blocked { files: vec!["a.php".into()] }
    );
    // An untracked file the target has would be overwritten.
    let present = ["new.txt".to_owned()];
    assert_eq!(
        switch_plan(None, &dirty, &[], &present),
        SwitchPlan::Blocked { files: vec!["new.txt".into()] }
    );
}

#[test]
fn an_operation_in_progress_comes_first() {
    let merge = Operation { kind: OperationKind::Merge, branch: None, step: None };
    assert_eq!(switch_plan(Some(merge.clone()), &[], &[], &[]), SwitchPlan::Busy(merge));
}

#[test]
fn a_remote_branch_is_switched_to_through_its_local_one() {
    let states = [SyncState {
        branch: "work".into(),
        upstream: Some("origin/feature".into()),
        ahead: 0,
        behind: 2,
    }];
    let locals = ["main".to_owned(), "work".to_owned()];
    assert_eq!(
        remote_switch("origin/feature", &states, &locals),
        RemoteSwitch::Existing("work".into())
    );
    assert_eq!(
        remote_switch("origin/fix/login", &states, &locals),
        RemoteSwitch::New { name: "fix/login".into(), taken: false }
    );
    assert_eq!(
        remote_switch("origin/main", &states, &locals),
        RemoteSwitch::New { name: "main".into(), taken: true }
    );
}

#[test]
fn stash_options_that_don_t_go_together_are_refused() {
    let staged = StashOptions { staged_only: true, ..Default::default() };
    assert_eq!(staged.check(), Ok(()));
    let kept = StashOptions { keep_index: true, ..staged.clone() };
    assert_eq!(kept.check(), Err(StashConflict::StagedAndKept));
    let untracked = StashOptions { untracked: true, ..staged.clone() };
    assert_eq!(untracked.check(), Err(StashConflict::StagedAndUntracked));
    let paths = StashOptions { paths: vec!["a".into()], ..staged };
    assert_eq!(paths.check(), Err(StashConflict::StagedAndPaths));
    let all = StashOptions { keep_index: true, untracked: true, ..Default::default() };
    assert_eq!(all.check(), Ok(()));
}

#[test]
fn a_stash_is_found_by_its_commit_after_its_name_moved() {
    let stashes = [
        Stash { name: "stash@{0}".into(), message: "new".into(), id: "bbb".into() },
        Stash { name: "stash@{1}".into(), message: "old".into(), id: "aaa".into() },
    ];
    assert_eq!(stash_named(&stashes, "aaa").map(|s| s.name.as_str()), Some("stash@{1}"));
    assert!(stash_named(&stashes, "").is_none());
}

#[test]
fn the_branches_last_checked_out_come_from_the_reflog() {
    let reflog = "checkout: moving from feature to main\n\
        commit: fix\n\
        checkout: moving from 4b3685a1 to feature\n\
        checkout: moving from main to 4b3685a1\n\
        checkout: moving from fix/login to main\n";
    assert_eq!(recent_checkouts(reflog, Some("main"), 5), ["feature", "fix/login"]);
    assert_eq!(recent_checkouts(reflog, Some("main"), 1), ["feature"]);
    assert!(recent_checkouts("", None, 5).is_empty());
}
