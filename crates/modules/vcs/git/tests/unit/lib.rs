use super::*;

#[test]
fn contributes_the_provider_and_the_git_menu() {
    let mut registry = extensions::ModuleRegistry::default();
    registry.register(Box::new(GitModule));
    let (mut contributions, reports) = registry.activate(&Default::default());
    assert!(reports.iter().all(|r| r.state == extensions::ModuleState::Active));
    assert_eq!(contributions.take::<Arc<dyn VcsProvider>>().len(), 1);
    let commands = contributions.take::<CommandContribution>();
    let ids: Vec<&str> = commands.iter().map(|c| c.spec.id.0).collect();
    assert_eq!(
        ids,
        [
            COMMIT.0,
            PULL.0,
            PUSH.0,
            PUSH_TO.0,
            FETCH.0,
            SHOW_LOG.0,
            SHOW_CHANGES.0,
            REVERT_CHANGE.0,
            CHANGE_ACTIONS.0,
            ANNOTATED_COMMIT.0,
            FILE_HISTORY.0,
            ROLLBACK.0,
            COMPARE_HEAD.0,
            COMPARE_REVISION.0,
            COMPARE_BRANCH.0,
            ANNOTATE.0,
            NEW_BRANCH.0,
            RENAME_BRANCH.0,
            NEW_TAG.0,
            SHOW_CONFLICTS.0,
            SWITCH_BRANCH.0,
            STASH.0,
            POP_SWITCH_STASH.0,
            CONTINUE_OPERATION.0,
            SKIP_OPERATION.0,
            ABORT_OPERATION.0,
            COMMIT_NOW.0,
            RELOAD_LOG.0
        ]
    );
    // Reloading works in the log, on its key.
    let reload = commands.last().unwrap();
    assert_eq!(reload.handler.when(), command_model::When::View(log_view::VIEW));
    assert_eq!(reload.spec.shortcuts, ["r"]);
    // The commit view, the log, the changes, the conflicts, the dialogs (branches, push,
    // switch, stash) and the branch list; the service following the changes.
    assert_eq!(contributions.take::<Arc<dyn ViewerProvider>>().len(), 9);
    // The top bar's branch: its list.
    let label = contributions.take::<workbench_model::TopBarLabelAction>();
    assert_eq!(label.len(), 1);
    assert_eq!(label[0].command, SWITCH_BRANCH);
    assert_eq!(contributions.take::<Arc<dyn ProjectService>>().len(), 1);
    // The Git menu: the repository's operations everywhere, a file's where there's one.
    let menus = contributions.take::<MenuContribution>();
    assert_eq!(menus.len(), 1);
    let rows = menus[0].rows_in(MenuPlace::TreeFolder, &|_| true);
    assert!(rows.contains(&MenuRow::Command(PULL, EVERYWHERE)));
    assert!(!rows.iter().any(|r| matches!(r, MenuRow::Command(id, _) if *id == FILE_HISTORY)));
    assert!(commands.iter().all(|c| c.spec.menu.is_some()
        || [RELOAD_LOG, COMMIT_NOW, POP_SWITCH_STASH].contains(&c.spec.id)));
    // The button's menu: the log first; the changes aren't in it (the Commit tab shows
    // them), only in the command palette.
    let strip = menus[0].rows_in(MenuPlace::ToolStrip, &|_| true);
    assert_eq!(strip[0], MenuRow::Command(SHOW_LOG, &[MenuPlace::ToolStrip]));
    assert!(!strip.contains(&MenuRow::Command(SHOW_CHANGES, &[MenuPlace::ToolStrip])));
}

#[test]
fn opens_this_repository_and_nothing_elsewhere() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    if let Some(repo) = GitProvider.open(here) {
        assert!(
            here.starts_with(repo.root()) || here.canonicalize().unwrap().starts_with(repo.root())
        );
    }
    assert!(GitProvider.open(&std::env::temp_dir().join("no-such-dir")).is_none());
}

mod commit_view_tests {
    use ui_components::{Viewer, ViewerKey};

    use crate::commit_view::{CommitData, CommitView};
    use crate::fake::FakeRepository;

    /// Polled until nothing is waited for (at most 5 s).
    fn settle(view: &mut CommitView) {
        for _ in 0..1000 {
            if !view.poll() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn commits_what_is_staged_with_the_message() {
        let repo = std::sync::Arc::new(FakeRepository::default());
        // Only `a.php` is on the staging area.
        repo.staged.lock().unwrap().push("a.php".to_owned());
        let mut view = CommitView::default();
        view.set_data(&CommitData { repo: repo.clone(), project: None, docked: false });
        settle(&mut view);
        view.text_input("Fix the thing");
        // ↵ breaks a line; ⌘ / Ctrl+↵ (the view's `commit`) commits, the message trimmed.
        assert!(view.edit_key(ViewerKey::Enter));
        assert!(view.command("commit"));
        settle(&mut view);
        assert_eq!(
            *repo.committed.lock().unwrap(),
            [(vec!["a.php".to_owned()], "Fix the thing".to_owned())]
        );
    }
}

#[test]
fn errors_say_what_is_missing() {
    use version_control::VcsError;
    for error in [
        VcsError::ToolMissing("git".into()),
        VcsError::NoRemote,
        VcsError::Authentication("denied".into()),
        VcsError::TimedOut,
    ] {
        assert!(!error_message(&error).is_empty());
    }
    assert!(error_message(&VcsError::Authentication("denied".into())).contains("denied"));
}

/// The Task Manager can't stop a git: its part offers nothing, its owner does nothing
/// (a write cut short leaves `index.lock`).
#[test]
fn git_offers_the_task_manager_no_actions() {
    use workbench_model::{ActOutcome, ActionTarget, PartRef, ResourceReporter as _};
    // Its part is listed only while a project is open or a git runs (other tests' state):
    // asked as it's named either way.
    let parts = GitPrograms.report();
    assert!(parts.iter().all(|p| p.actions.is_empty()));
    let part = PartRef {
        owner: MANIFEST.id.as_str().to_owned(),
        name: i18n::t("tasks.git_programs"),
        project: None,
    };
    assert_eq!(
        GitPrograms.act(&part, "stop-program", ActionTarget::Process(1)),
        ActOutcome::Unsupported
    );
}
