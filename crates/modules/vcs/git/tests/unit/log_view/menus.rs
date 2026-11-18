use super::*;

use crate::fake::{FakeRepository, commit, entry};
use ui_components::{AppRequest, ViewMenuEntry};

fn file(path: &str) -> version_control::FileChange {
    version_control::FileChange { path: path.into(), old_path: None, state: FileState::Modified }
}

/// A file's history (`src/b.php`) of two commits, the newest one changing three files.
fn history() -> LogView {
    let repo: Arc<dyn Repository> = Arc::new(FakeRepository {
        log: vec![entry("m", &["a"]), entry("a", &[])],
        details: vec![CommitDetails {
            commit: commit("m", "fix the cart"),
            email: String::new(),
            message: "fix the cart\n\nand its totals".into(),
            parents: vec!["a".into()],
            files: vec![file("src/a.php"), file("src/b.php"), file("src/c.php")],
        }],
        texts: vec![
            (("a".into(), "src/b.php".into()), "old".into()),
            (("m".into(), "src/b.php".into()), "new".into()),
        ],
        ..Default::default()
    });
    let mut view = LogView::default();
    view.set_data(&LogData { repo, path: None, commit: None, queries: None, reference: None });
    settle(&mut view);
    view.path = Some("src/b.php".into());
    view.select(0);
    settle(&mut view);
    view
}

fn settle(view: &mut LogView) {
    // Until nothing is being read any more (up to 15 s with every test running).
    for _ in 0..3000 {
        if !view.poll() && !busy(view) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn ids(rows: &[ViewMenuEntry]) -> Vec<String> {
    rows.iter()
        .filter_map(|r| match r {
            ViewMenuEntry::Item { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_commits_menu_toggles_its_files_copies_and_compares() {
    let mut view = history();
    let rows = view.commit_menu();
    assert_eq!(
        ids(&rows),
        [
            ALL_FILES,
            COPY_HASH,
            COPY_MESSAGE,
            COMPARE_PREVIOUS,
            COMPARE_WORKING,
            NEW_BRANCH_HERE,
            NEW_TAG_HERE
        ]
    );
    // Every file of the commit, then only this one again.
    assert_eq!(view.shown_paths().len(), 1);
    assert!(view.command(ALL_FILES));
    assert_eq!(view.shown_paths().len(), 3);
    assert!(view.commit_menu().iter().any(
        |r| matches!(r, ViewMenuEntry::Item { label, .. } if *label == t("git.only_this_file"))
    ));
    assert!(view.command(ALL_FILES));
    assert_eq!(view.shown_paths().len(), 1);
    // The hash, the whole message.
    assert!(view.command(COPY_HASH));
    assert!(matches!(
        view.take_action(),
        Some(ViewerAction::Request(AppRequest::CopyText(id))) if id == "m"
    ));
    assert!(view.command(COPY_MESSAGE));
    assert!(matches!(
        view.take_action(),
        Some(ViewerAction::Request(AppRequest::CopyText(m))) if m == "fix the cart\n\nand its totals"
    ));
    // The history's file compared with its revision before.
    assert!(view.command(COMPARE_PREVIOUS));
    let Some(ViewerAction::CompareBeside(input, title)) = prepared(&mut view) else {
        panic!("a comparison");
    };
    assert_eq!((input.left.1.as_str(), input.right.1.as_str()), ("old", "new"));
    assert_eq!(title, "b.php · m");
    // With the working copy: the commit's version on the left, read-only.
    assert!(view.command(COMPARE_WORKING));
    let Some(ViewerAction::CompareBeside(input, _)) = prepared(&mut view) else {
        panic!("a comparison with the working copy");
    };
    assert_eq!(input.left.1, "new");
    assert_eq!(input.read_only, [true, false]);
    assert!(!view.command("nothing.like.it"));
}

#[test]
fn the_logs_commit_menu_has_only_what_concerns_every_file() {
    let mut view = history();
    view.path = None;
    assert_eq!(ids(&view.commit_menu()), [COPY_HASH, COPY_MESSAGE, NEW_BRANCH_HERE, NEW_TAG_HERE]);
}

#[test]
fn a_files_menu_opens_compares_copies_and_shows_it() {
    let mut view = history();
    // Every file of the commit: the second one's.
    assert!(view.command(ALL_FILES));
    let rows = view.file_menu(1);
    assert_eq!(
        ids(&rows),
        [
            "file.open:1",
            "file.diff:1",
            "file.copy-path:1",
            "file.show-in-project:1",
            "file.reveal:1"
        ]
    );
    assert!(view.command("file.open:1"));
    assert!(matches!(
        view.take_action(),
        Some(ViewerAction::OpenFile { path, .. }) if path == std::path::Path::new("/p/src/b.php")
    ));
    assert!(view.command("file.diff:1"));
    assert!(matches!(prepared(&mut view), Some(ViewerAction::CompareBeside(..))));
    assert!(view.command("file.copy-path:1"));
    assert!(matches!(
        view.take_action(),
        Some(ViewerAction::Request(AppRequest::CopyText(p))) if p == "src/b.php"
    ));
    assert!(view.command("file.show-in-project:1"));
    assert!(matches!(view.take_action(), Some(ViewerAction::Request(AppRequest::ShowFolder(_)))));
    assert!(view.command("file.reveal:1"));
    assert!(matches!(view.take_action(), Some(ViewerAction::Request(AppRequest::Reveal(_)))));
}

/// A commit's and a branch's menus ask for a branch or a tag from there: the dialog's
/// command, the place it starts from left for it.
#[test]
fn branches_and_tags_are_asked_from_the_menus() {
    let mut view = history();
    view.path = None;
    assert!(view.command(NEW_BRANCH_HERE));
    assert!(matches!(view.take_action(), Some(ViewerAction::WithHost(_))));
    view.branch_rows = vec![crate::branches::BranchRow {
        depth: 1,
        item: BranchItem::Ref {
            name: "topic".into(),
            label: "topic".into(),
            kind: RefKind::Branch,
        },
    }];
    let rows = ids(&view.branch_menu(0).unwrap());
    assert_eq!(rows, ["branch.new-branch:0", "branch.rename:0", "branch.new-tag:0"]);
    assert!(view.command("branch.rename:0"));
    assert!(matches!(view.take_action(), Some(ViewerAction::WithHost(_))));
}

#[test]
fn a_local_branch_pushes_to_what_it_tracks_and_to_each_remote() {
    let mut view = history();
    view.branch_rows = vec![crate::branches::BranchRow {
        depth: 0,
        item: BranchItem::Ref {
            name: "topic".into(),
            label: "topic".into(),
            kind: RefKind::Branch,
        },
    }];
    assert_eq!(view.branch_menu(0).unwrap().len(), 3, "without remotes, no pushes");
    view.remotes = vec!["fork".into(), "origin".into()];
    view.branch_states = vec![version_control::SyncState {
        branch: "topic".into(),
        upstream: Some("origin/topic".into()),
        ahead: 0,
        behind: 0,
    }];
    let rows = view.branch_menu(0).unwrap();
    assert!(matches!(&rows[4], ViewMenuEntry::Item { id, .. } if id == "branch.push:0"));
    let ViewMenuEntry::Submenu { entries, .. } = &rows[5] else { panic!("the remotes") };
    assert_eq!(ids(entries), ["branch.push-to:0:0", "branch.push-to:0:1"]);
    assert!(view.command("branch.push-to:0:1"));
    assert!(matches!(view.take_action(), Some(ViewerAction::WithHost(_))));
}

/// What the view prepared in the background (a comparison being read).
fn prepared(view: &mut LogView) -> Option<ViewerAction> {
    let started = std::time::Instant::now();
    while view.opening.is_busy() && started.elapsed() < std::time::Duration::from_secs(10) {
        view.poll();
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    view.take_action()
}

/// Something is still being read in the background (the log, a commit's details, the lists).
fn busy(view: &LogView) -> bool {
    ui_components::Viewer::is_loading(view)
}
