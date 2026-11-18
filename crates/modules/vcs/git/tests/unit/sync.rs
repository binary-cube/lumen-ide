use super::*;

fn state(ahead: usize, behind: usize) -> SyncState {
    SyncState { branch: "main".into(), upstream: Some("origin/main".into()), ahead, behind }
}

#[test]
fn the_badge_shows_what_to_push_and_pull() {
    assert_eq!(sync_badge(&state(2, 1)), "↑2 ↓1");
    assert_eq!(sync_badge(&state(0, 3)), "↓3");
    assert_eq!(sync_badge(&state(0, 0)), "");
}

#[test]
fn outcomes_read_well() {
    let ok = |up_to_date| Ok(RemoteOutcome { up_to_date, message: "a..b main -> main".into() });
    let pushed = outcome_message(SyncOp::Push, &ok(false), Some(&state(0, 0)));
    assert!(pushed.contains("a..b"));
    let fetched = outcome_message(SyncOp::Fetch, &ok(false), Some(&state(0, 4)));
    assert!(fetched.ends_with("↓4"));
    let missing = outcome_message(SyncOp::Pull, &Err(VcsError::ToolMissing("git".into())), None);
    assert!(missing.contains("git"));
}

#[test]
fn a_missing_git_is_reported_not_a_crash() {
    let repo = crate::fake::FakeRepository {
        remote_error: Some(VcsError::ToolMissing("git".into())),
        ..Default::default()
    };
    let result = sync(&repo, SyncOp::Push);
    assert_eq!(result, Err(VcsError::ToolMissing("git".into())));
}

/// Before closing: a push running is reported (half done if cut, never stopped: git would
/// leave its lock), a fetch only interrupted; once it ends, nothing is.
#[test]
fn operations_on_remotes_are_reported_while_they_run() {
    let project = Some(kernel::ProjectId::new());
    let root = PathBuf::from("/closing/push-repo");
    let mine = |root: &PathBuf| {
        running().into_iter().filter(|w| w.detail == root.display().to_string()).collect::<Vec<_>>()
    };
    let push = Running::start(root.clone(), SyncOp::Push, project).unwrap();
    assert!(Running::start(root.clone(), SyncOp::Fetch, project).is_none(), "one at a time");
    let work = mine(&root);
    assert_eq!(work.len(), 1);
    assert_eq!(work[0].project, project);
    assert_eq!(work[0].impact, workbench_model::StopImpact::LosesWork);
    assert!(work[0].stop.is_none(), "a git isn't stopped midway");
    drop(push);
    assert!(mine(&root).is_empty());
    let fetch = Running::start(root.clone(), SyncOp::Fetch, project).unwrap();
    assert_eq!(mine(&root)[0].impact, workbench_model::StopImpact::Interrupts);
    drop(fetch);
}
