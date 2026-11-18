use super::*;

use history::HistoryStore;
use std::sync::Arc;

/// A project folder and its data folder, in the temp dir.
fn folders(name: &str) -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("local-history-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let (root, storage) = (base.join("project"), base.join("data"));
    std::fs::create_dir_all(&root).unwrap();
    (root, storage)
}

/// Saved as the app saves: a new file put in the old one's place.
fn save(file: &Path, text: &str) {
    let tmp = file.with_extension("saving.tmp");
    std::fs::write(&tmp, text).unwrap();
    std::fs::rename(&tmp, file).unwrap();
}

fn context<'a>(
    root: &'a Path,
    storage: &'a Path,
    events: &'a EventBus,
    id: kernel::ProjectId,
) -> ProjectContext<'a> {
    ProjectContext {
        marks: Default::default(),
        id,
        root,
        storage,
        events,
        wake: Arc::new(|| {}),
        settings: None,
        language_of: std::sync::Arc::new(|_: &std::path::Path| None),
        quality_tools: Vec::new(),
        text_formatters: Vec::new(),
        notifier: std::sync::Arc::new(workbench_model::NoNotifier),
    }
}

fn settle(store: &DiskHistory, path: &str, count: usize) {
    for _ in 0..400 {
        if store.revisions(path).len() >= count {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn a_save_keeps_the_version_before_and_the_one_saved() {
    let (root, storage) = folders("save");
    let file = root.join("a.php");
    std::fs::write(&file, "<?php // before").unwrap();
    let events = EventBus::new();
    let project = context(&root, &storage, &events, kernel::ProjectId::new());
    let subscriptions = HistoryService::default().attach(&project);
    events.publish(&document::events::FileWillSave { path: file.clone() });
    save(&file, "<?php // after");
    events.publish(&document::events::FileSaved { path: file });
    let store = DiskHistory::new(storage.join(crate::FOLDER));
    settle(&store, "a.php", 2);
    let revisions = store.revisions("a.php");
    assert_eq!(revisions.len(), 2);
    assert_eq!(revisions[0].kind, RevisionKind::Saved);
    assert_eq!(store.read("a.php", revisions[1].at).unwrap(), b"<?php // before");
    // Outside the project: nothing kept.
    events.publish(&document::events::FileSaved { path: "/elsewhere/x.php".into() });
    // For the Task Manager: its project's part, the versions written, the saved file read.
    let parts = Reporter.report();
    let part = parts.iter().find(|p| p.project == Some(project.id)).expect("its part");
    assert!(part.written.unwrap() > 0, "the versions written");
    assert!(part.read.unwrap() >= "<?php // after".len() as u64, "the saved file read");
    drop(subscriptions);
    let _ = std::fs::remove_dir_all(storage.parent().unwrap());
}

/// Saved again as this history last kept it: the version before is there already, nothing
/// is read; changed since by another program, it is kept.
#[test]
fn a_file_as_last_kept_isn_t_read_again_before_a_save() {
    let (root, storage) = folders("again");
    let file = root.join("a.php");
    std::fs::write(&file, "<?php // one").unwrap();
    let events = EventBus::new();
    let id = kernel::ProjectId::new();
    let project = context(&root, &storage, &events, id);
    let subscriptions = HistoryService::default().attach(&project);
    let store = DiskHistory::new(storage.join(crate::FOLDER));
    let saved = |text: &str, count: usize| {
        events.publish(&document::events::FileWillSave { path: file.clone() });
        save(&file, text);
        events.publish(&document::events::FileSaved { path: file.clone() });
        settle(&store, "a.php", count);
    };
    let read = || {
        let parts = Reporter.report();
        parts.iter().find(|p| p.project == Some(id)).and_then(|p| p.read).unwrap_or(0)
    };
    saved("<?php // two", 2);
    let before = read();
    saved("<?php // three", 3);
    assert_eq!(read() - before, "<?php // three".len() as u64, "only the saved file read");
    // Another program's change: kept before the next save.
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(&file, "<?php // theirs").unwrap();
    saved("<?php // four", 5);
    let texts: Vec<Vec<u8>> =
        store.revisions("a.php").iter().map(|r| store.read("a.php", r.at).unwrap()).collect();
    assert_eq!(texts[1], b"<?php // theirs");
    drop(subscriptions);
    let _ = std::fs::remove_dir_all(storage.parent().unwrap());
}

/// The file opened before a save, then written in place (not replaced) before it's read:
/// it may not hold the old version any more, so nothing is kept from it.
#[test]
fn a_file_written_in_place_after_it_was_opened_is_not_kept() {
    let (root, storage) = folders("in-place");
    let file = root.join("a.php");
    std::fs::write(&file, "<?php // old").unwrap();
    let keeper = Keeper {
        pool: kernel::work::Pool::new("local-history-test", 1, kernel::work::IDLE),
        store: DiskHistory::new(storage.join(crate::FOLDER)),
        usage: Arc::new(Usage {
            project: kernel::ProjectId::new(),
            name: "p".into(),
            store: DiskHistory::new(storage.join(crate::FOLDER)),
            read: AtomicU64::new(0),
            cpu: AtomicU64::new(0),
        }),
        fixed: Some(Retention::default()),
        pruned: AtomicBool::new(true),
        kept: Mutex::default(),
    };
    let opened = || {
        let stamp = stamp_of(&std::fs::metadata(&file).unwrap()).unwrap();
        (std::fs::File::open(&file).unwrap(), stamp)
    };
    // Replaced: the opened one still holds the old version.
    let (handle, stamp) = opened();
    save(&file, "<?php // new");
    keeper.keep(Job::KeepOpened { path: "a.php".into(), file: handle, stamp });
    let texts = |k: &Keeper| -> Vec<Vec<u8>> {
        k.store.revisions("a.php").iter().map(|r| k.store.read("a.php", r.at).unwrap()).collect()
    };
    assert_eq!(texts(&keeper), [b"<?php // old".to_vec()]);
    // Written in place: dropped.
    let (handle, stamp) = opened();
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(&file, "<?php // rewritten in place").unwrap();
    keeper.keep(Job::KeepOpened { path: "a.php".into(), file: handle, stamp });
    assert_eq!(texts(&keeper).len(), 1, "nothing kept from it");
    let _ = std::fs::remove_dir_all(storage.parent().unwrap());
}
