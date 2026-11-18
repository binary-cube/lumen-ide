use super::*;

use std::time::Duration;

fn rebase() -> Operation {
    Operation { kind: OperationKind::Rebase, branch: Some("feature".into()), step: Some((1, 2)) }
}

/// A list following project `id`'s changes (attached to a bus of its own), read once.
fn followed(
    repo: Arc<crate::fake::FakeRepository>,
) -> (ConflictsList, Vec<kernel::Subscription>, Arc<crate::status::Changes>) {
    let root = std::env::temp_dir();
    let events = kernel::EventBus::default();
    let id = kernel::ProjectId::new();
    let context = workbench_model::ProjectContext {
        marks: Default::default(),
        id,
        root: &root,
        storage: &root,
        events: &events,
        wake: Arc::new(|| {}),
        settings: None,
        language_of: Arc::new(|_: &std::path::Path| None),
        quality_tools: Vec::new(),
        text_formatters: Vec::new(),
        notifier: Arc::new(workbench_model::NoNotifier),
    };
    let subscriptions =
        workbench_model::ProjectService::attach(&crate::status::ChangesService, &context);
    let changes = crate::status::changes(id).unwrap();
    let mut list = ConflictsList::default();
    list.set_data(&ConflictsData { project: id, repo: repo as Arc<dyn Repository>, queries: None });
    (list, subscriptions, changes)
}

/// Waits for the next read of `changes` (started with `repo`) and the list to show it.
fn read_again(
    list: &mut ConflictsList,
    changes: &Arc<crate::status::Changes>,
    repo: &Arc<dyn Repository>,
) {
    let before = changes.files().0;
    changes.stale();
    changes.start(Arc::clone(repo));
    let started = std::time::Instant::now();
    while changes.files().0 == before && started.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(5));
    }
    list.poll();
}

#[test]
fn the_conflicted_files_are_listed_under_what_is_in_progress() {
    let fake = Arc::new(crate::fake::FakeRepository {
        conflicted: std::sync::Mutex::new(vec!["src/b.php".into(), "a.php".into()]),
        operation: Some(rebase()),
        ..Default::default()
    });
    let repo: Arc<dyn Repository> = Arc::clone(&fake) as Arc<dyn Repository>;
    let (mut list, _subscriptions, changes) = followed(Arc::clone(&fake));
    read_again(&mut list, &changes, &repo);
    // Only the conflicted ones, sorted.
    assert_eq!(list.files, ["a.php", "src/b.php"]);
    assert_eq!(list.operation, Some(rebase()));
    assert!(list.title().starts_with(&operation_text(&rebase())), "{}", list.title());

    // A side taken (in the background): the file leaves the list.
    let i = list.files.iter().position(|f| f == "a.php").unwrap();
    assert!(list.command(&format!("{TAKE_THEIRS}{i}")));
    let started = std::time::Instant::now();
    while list.running.is_some() && started.elapsed() < Duration::from_secs(5) {
        list.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(*fake.taken.lock().unwrap(), [(vec!["a.php".to_owned()], Side::Theirs)]);
    read_again(&mut list, &changes, &repo);
    assert_eq!(list.files, ["src/b.php"]);
}

#[test]
fn a_rebase_s_sides_are_named_for_what_they_are() {
    assert_eq!(side_labels(Some(&rebase())), (t("git.take_upstream"), t("git.take_replayed")));
    let merge = Operation { kind: OperationKind::Merge, branch: None, step: None };
    assert_eq!(side_labels(Some(&merge)), (t("git.take_ours"), t("git.take_theirs")));
    assert_eq!(side_labels(None), (t("git.take_ours"), t("git.take_theirs")));
}

#[test]
fn a_file_s_menu_has_its_resolutions() {
    let list = ConflictsList { files: vec!["a.php".into()], ..Default::default() };
    let ids: Vec<String> = list
        .file_menu(0)
        .into_iter()
        .filter_map(|row| match row {
            ViewMenuEntry::Item { id, .. } => Some(id),
            _ => None,
        })
        .collect();
    for prefix in [RESOLVE, TAKE_OURS, TAKE_THEIRS, MARK_RESOLVED, OPEN_FILE] {
        assert!(ids.contains(&format!("{prefix}0")), "{prefix}");
    }
}
