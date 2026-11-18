use super::*;

fn rebase() -> Operation {
    Operation { kind: OperationKind::Rebase, branch: Some("feature".into()), step: Some((3, 7)) }
}

#[test]
fn what_is_in_progress_is_said_with_its_branch_and_step() {
    assert_eq!(operation_text(&rebase()), format!("{} · feature · 3/7", t("git.op_rebase")));
    let merge = Operation { kind: OperationKind::Merge, branch: None, step: None };
    assert_eq!(operation_text(&merge), t("git.op_merge"));
}

#[test]
fn each_outcome_is_told_as_it_went() {
    let kind = |step, result| outcome(step, "Rebase", &result).kind;
    use OperationStep::{Abort, Continue};
    assert_eq!(kind(Continue, Ok(Proceeded::Finished)), NotificationKind::Success);
    assert_eq!(kind(Abort, Ok(Proceeded::Finished)), NotificationKind::Success);
    // Stopped again, or conflicts left: a warning leading to them.
    for result in [Ok(Proceeded::Stopped), Err(ProceedError::Unresolved(vec!["a.php".into()]))] {
        let told = outcome(Continue, "Rebase", &result);
        assert_eq!(told.kind, NotificationKind::Warning);
        assert_eq!(told.actions[0].command, crate::SHOW_CONFLICTS);
    }
    let failed = Err(ProceedError::Vcs(version_control::VcsError::Failed("nope".into())));
    assert_eq!(outcome(Continue, "Rebase", &failed).title, "nope");
    assert_eq!(kind(Continue, Err(ProceedError::NotAllowed)), NotificationKind::Info);
}

#[test]
fn only_one_step_runs_at_a_time_per_repository() {
    let root = std::path::PathBuf::from("/p/one-at-a-time");
    let first = Running::start(root.clone()).expect("free");
    assert!(Running::start(root.clone()).is_none(), "busy");
    assert!(Running::start("/p/another".into()).is_some(), "another repository runs meanwhile");
    drop(first);
    assert!(Running::start(root).is_some());
}
