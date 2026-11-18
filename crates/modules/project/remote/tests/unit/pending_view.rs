use super::*;

/// Without a session (not a remote project): nothing read, nothing shown.
#[test]
fn without_a_session_the_list_is_empty() {
    let mut list = PendingList::default();
    list.set_data(&PendingData { project: kernel::ProjectId::new(), queries: None });
    assert!(!list.poll());
    assert!(list.rows.is_empty());
    assert_eq!(list.generation, 0);
}

/// Each reason as the tree shows it: its state, where it was, why it couldn't go.
#[test]
fn rows_show_as_changed_files() {
    let file = |why: Why| changed_file(&("a.php".to_owned(), why));
    assert_eq!(file(Why::New).state, ChangeState::Added);
    assert_eq!(file(Why::Changed).state, ChangeState::Modified);
    assert_eq!(file(Why::Deleted).state, ChangeState::Deleted);
    let moved = file(Why::Renamed { from: "old.php".into() });
    assert_eq!((moved.state, moved.old_path.as_deref()), (ChangeState::Renamed, Some("old.php")));
    let failed = file(Why::Waiting("refused".into()));
    assert_eq!((failed.state, failed.note.as_deref()), (ChangeState::Failed, Some("refused")));
    assert_eq!(file(Why::Conflict).state, ChangeState::Conflicted);
}

/// The menu: Compare for one file only; `*.ext` offered when all share it.
#[test]
fn the_menu_follows_what_is_chosen() {
    let rows = |paths: &[&str]| -> Vec<(String, Why)> {
        paths.iter().map(|p| ((*p).to_owned(), Why::Changed)).collect()
    };
    let ids = |menu: Vec<ViewMenuEntry>| -> Vec<String> {
        menu.into_iter()
            .filter_map(|e| match e {
                ViewMenuEntry::Item { id, .. } => Some(id),
                _ => None,
            })
            .collect()
    };
    let one = ids(PendingList::menu_for(&rows(&["logs/a.log"]), None));
    assert!(one.iter().any(|i| i == COMPARE) && one.iter().any(|i| i == EXCLUDE_EXT));
    let many = ids(PendingList::menu_for(&rows(&["a.log", "b.LOG"]), Some("logs")));
    assert!(!many.iter().any(|i| i == COMPARE));
    assert!(many.iter().any(|i| i == EXCLUDE_EXT));
    let mixed = ids(PendingList::menu_for(&rows(&["a.log", "b.php"]), None));
    assert!(!mixed.iter().any(|i| i == EXCLUDE_EXT));
    assert_eq!(shared_extension(&["a.tar.gz", "x/b.gz"]).as_deref(), Some(".gz"));
    assert_eq!(shared_extension(&[".env"]), None, "a hidden file has no extension");
}
