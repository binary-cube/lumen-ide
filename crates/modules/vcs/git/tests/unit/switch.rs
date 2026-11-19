use super::*;
use crate::fake::FakeRepository;
use version_control::{RefLabel, SyncState};

fn label(name: &str, kind: RefKind) -> RefLabel {
    RefLabel { name: name.into(), kind }
}

#[test]
fn a_switch_nothing_stands_in_the_way_of_runs_at_once() {
    // `a.php` is modified, `b.php` untracked; neither differs on `feature`.
    let repo = FakeRepository::default();
    let found = find(&repo, &Wanted::Branch("feature".into()));
    assert!(matches!(found, Found::Switched { carried: 2, .. }));
    let switched = repo.switched.lock().unwrap().clone();
    assert_eq!(switched, [(SwitchTarget::Branch("feature".into()), false)]);
}

#[test]
fn files_in_the_way_are_named_and_nothing_switches() {
    let repo = FakeRepository {
        differs: vec!["a.php".into()],
        present: vec!["b.php".into()],
        ..Default::default()
    };
    let Found::Blocked { files, label, .. } = find(&repo, &Wanted::Branch("feature".into())) else {
        panic!("blocked");
    };
    assert_eq!(label, "feature");
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["a.php", "b.php"]);
    assert_eq!(files[1].state, FileState::Untracked);
    assert!(repo.switched.lock().unwrap().is_empty());
}

#[test]
fn the_current_branch_and_an_operation_in_progress_switch_nothing() {
    let repo = FakeRepository::default();
    assert!(matches!(find(&repo, &Wanted::Branch("main".into())), Found::Already(_)));
    let busy = FakeRepository {
        operation: Some(Operation {
            kind: version_control::OperationKind::Merge,
            branch: None,
            step: None,
        }),
        ..Default::default()
    };
    assert!(matches!(find(&busy, &Wanted::Branch("feature".into())), Found::Busy(_)));
}

#[test]
fn a_remote_branch_goes_through_its_local_one_or_a_new_one_tracking_it() {
    let repo = FakeRepository {
        refs: vec![
            label("main", RefKind::Branch),
            label("work", RefKind::Branch),
            label("origin/feature", RefKind::Remote),
            label("origin/new", RefKind::Remote),
            label("origin/main", RefKind::Remote),
        ],
        branch_states: vec![SyncState {
            branch: "work".into(),
            upstream: Some("origin/feature".into()),
            ahead: 0,
            behind: 0,
        }],
        ..Default::default()
    };
    find(&repo, &Wanted::Remote("origin/feature".into()));
    find(&repo, &Wanted::Remote("origin/new".into()));
    let switched = repo.switched.lock().unwrap().clone();
    assert_eq!(switched[0].0, SwitchTarget::Branch("work".into()));
    assert_eq!(
        switched[1].0,
        SwitchTarget::NewBranch { name: "new".into(), start: "origin/new".into(), track: true }
    );
    // `main` is a local branch tracking nothing: a name is asked.
    assert!(matches!(find(&repo, &Wanted::Remote("origin/main".into())), Found::NameTaken(_)));
}

#[test]
fn a_tag_or_a_commit_is_checked_out_detached() {
    let repo = FakeRepository::default();
    find(&repo, &Wanted::Tag("v1".into()));
    find(&repo, &Wanted::Commit { id: "abc123".into(), short: "abc".into() });
    let switched = repo.switched.lock().unwrap().clone();
    assert_eq!(switched[0].0, SwitchTarget::Detached("v1".into()));
    assert_eq!(switched[1].0, SwitchTarget::Detached("abc123".into()));
}

#[test]
fn the_work_put_aside_is_offered_back_after_the_switch() {
    let note = switched("feature", 0, true);
    assert_eq!(note.actions.len(), 1);
    assert_eq!(note.actions[0].command, crate::POP_SWITCH_STASH);
    assert!(switched("feature", 0, false).actions.is_empty());
    assert!(switched("feature", 3, false).detail.is_some());
    let root = std::path::Path::new("/repo-switch-test");
    remember_put_aside(root, "s1".into());
    assert_eq!(take_put_aside(root).as_deref(), Some("s1"));
    assert_eq!(take_put_aside(root), None, "brought back once");
}
