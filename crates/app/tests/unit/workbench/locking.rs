//! Locked tabs: every edit refused (typing, paste, undo, a module's, a refactoring touching
//! them), the user told once a while; unlocked, all works again. The padlock in the status
//! bar and the command only where a tab can be locked; files read-only on disk open locked
//! and stay so; the lock comes back with the session.

use super::*;

/// A project in a scratch folder with `files` written, laid out.
fn project(name: &str, files: &[(&str, &[u8])]) -> (Workbench, PathBuf) {
    let dir = std::env::temp_dir().join(format!("locking-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (file, text) in files {
        std::fs::write(dir.join(file), text).unwrap();
    }
    let dir = crate::workbench::canonical(&dir);
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    wb.open_project(&dir).unwrap();
    (wb, dir)
}

fn text_of(wb: &Workbench) -> String {
    wb.tabs[wb.active].doc.buffer().to_string()
}

/// The notifications shown, by title.
fn told(wb: &Workbench) -> Vec<String> {
    wb.data.notifications.shown().map(|t| t.notification.title.clone()).collect()
}

fn set_read_only(path: &Path, read_only: bool) {
    let mut permissions = std::fs::metadata(path).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(read_only);
    std::fs::set_permissions(path, permissions).unwrap();
}

#[test]
fn a_locked_tab_refuses_every_edit_and_says_so_once() {
    let (mut wb, dir) = project("edits", &[("a.txt", b"abc\n")]);
    wb.open_file(&dir.join("a.txt")).unwrap();
    let mut text = TextSystem::new();
    wb.type_text(&mut text, "x");
    assert_eq!(text_of(&wb), "xabc\n", "unlocked: typed");
    assert_eq!(wb.active_lock(), Some(LockState::Writable));
    assert!(wb.toggle_active_lock());
    assert_eq!(wb.active_lock(), Some(LockState::Locked));
    assert!(!wb.report_refused_edits(), "nothing refused yet");
    // Typing, pasting, deleting, undoing, a module's edit: all refused.
    wb.type_text(&mut text, "y");
    wb.paste(&mut text, "pasted");
    wb.edit_key(&mut text, EditKey::Backspace { word: false });
    wb.edit(&mut text, |d| {
        d.undo();
    });
    assert!(!workbench_model::EditorHost::edit_active(&mut wb, 0..1, "z"));
    assert!(workbench_model::EditorHost::refuse_if_locked(&mut wb), "a formatter's run");
    assert_eq!(text_of(&wb), "xabc\n");
    assert_eq!(wb.selected_text(), None, "nothing selected: not the line a copy takes");
    // The cursor still moves, the text can still be copied.
    wb.edit(&mut text, document::commands::select_all);
    assert_eq!(wb.copy().as_deref(), Some("xabc\n"));
    // Told once (not on every key).
    assert!(wb.report_refused_edits());
    assert_eq!(told(&wb), [i18n::t("lock.refused")]);
    wb.type_text(&mut text, "y");
    assert!(!wb.report_refused_edits(), "said a moment ago");
    // Unlocked: edits again.
    assert!(wb.toggle_active_lock());
    wb.edit(&mut text, |d| {
        d.undo();
    });
    assert_eq!(text_of(&wb), "abc\n", "undo works again");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A refactoring (a rename's edits) touching a locked file changes nothing, and says which;
/// unlocked, it applies.
#[test]
fn a_refactoring_touching_a_locked_file_is_refused() {
    let (mut wb, dir) =
        project("refactor", &[("a.php", b"<?php\nfoo();\n"), ("b.php", b"<?php\nfoo();\n")]);
    let (a, b) = (dir.join("a.php"), dir.join("b.php"));
    wb.open_file(&a).unwrap();
    assert!(wb.toggle_active_lock());
    let occurrence = refactor::Occurrence {
        range: 6..9,
        original: "foo".into(),
        replacement: "bar".into(),
        certainty: refactor::Certainty::Certain,
        chosen: true,
        line: 1,
        line_text: "foo();".into(),
        in_line: 0..3,
    };
    let plan = refactor::Plan {
        files: [&a, &b]
            .iter()
            .map(|p| refactor::FilePlan {
                path: (*p).clone(),
                occurrences: vec![occurrence.clone()],
            })
            .collect(),
        renames: Vec::new(),
    };
    let refused = wb.apply_refactoring(&plan).unwrap_err();
    assert!(refused.contains("a.php"), "{refused}");
    assert_eq!(std::fs::read_to_string(&b).unwrap(), "<?php\nfoo();\n", "nothing changed");
    assert_eq!(text_of(&wb), "<?php\nfoo();\n");
    assert!(wb.toggle_active_lock());
    assert_eq!(wb.apply_refactoring(&plan), Ok(2));
    assert_eq!(text_of(&wb), "<?php\nbar();\n");
    // Its undo waits too while a file it changed is locked.
    assert!(wb.toggle_active_lock());
    assert_eq!(wb.undo_refactoring(), None);
    assert_eq!(std::fs::read_to_string(&b).unwrap(), "<?php\nbar();\n");
    assert!(wb.toggle_active_lock());
    assert_eq!(wb.undo_refactoring(), Some(0));
    assert_eq!(std::fs::read_to_string(&b).unwrap(), "<?php\nfoo();\n");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The command and the padlock's click only for a tab that can be locked (a text's, not a
/// binary file's notice; else the padlock faint, doing nothing); the click is the command,
/// and toggles.
#[test]
fn the_lock_is_offered_only_where_it_applies() {
    let (mut wb, dir) =
        project("where", &[("a.txt", b"abc\n"), ("b.bin", b"\0\0\x01\x02binary\0")]);
    let toggle = crate::commands::TOGGLE_READ_ONLY;
    let mut text = TextSystem::new();
    let mut scene = Scene::default();
    // Nothing open: the padlock still in its corner (nothing moves when a file opens), faint,
    // doing nothing, its tooltip naming what it's for.
    wb.paint(&mut text, &mut scene);
    assert!(wb.status.action_rect(toggle.0).is_none());
    let bar = wb.status.bounds();
    let tip = (0..60)
        .map(|dx| Point::new(bar.max_x() - dx as f32, bar.min_y() + bar.size.height / 2.0))
        .find_map(|p| wb.status.tooltip_at(p));
    assert_eq!(tip.map(|(_, t)| t), Some(crate::commands::label(toggle)), "the padlock, faint");
    wb.open_file(&dir.join("b.bin")).unwrap();
    assert_eq!(wb.active_lock(), None, "a binary file's notice can't be locked");
    let context = wb.menu_context();
    assert_eq!(
        wb.row_state(&context, toggle.0),
        RowState::Disabled(Some(command_model::Requires::Lockable))
    );
    wb.paint(&mut text, &mut scene);
    assert!(wb.status.action_rect(toggle.0).is_none(), "a padlock doing nothing");
    wb.open_file(&dir.join("a.txt")).unwrap();
    let context = wb.menu_context();
    assert_eq!(wb.row_state(&context, toggle.0), RowState::Enabled);
    wb.paint(&mut text, &mut scene);
    let rect = wb.status.action_rect(toggle.0).expect("the padlock in the status bar");
    assert_eq!(wb.lock_tooltip(), Some(i18n::t("lock.make_read_only")));
    let center = Point::new(rect.min_x() + rect.size.width / 2.0, rect.min_y() + 4.0);
    match wb.mouse_down(&mut text, center, false, false, false, 1) {
        MouseResult::Command(id) => assert_eq!(id, toggle.0),
        other => panic!("the padlock runs the command: {other:?}"),
    }
    assert!(wb.toggle_active_lock());
    wb.paint(&mut text, &mut scene);
    assert_eq!(wb.lock_tooltip(), Some(i18n::t("lock.make_writable")));
    assert!(wb.status.action_rect(toggle.0).is_some(), "closed now, still there");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A file read-only on disk opens locked and isn't unlocked (the user told why) until it
/// can be written again; a second tab of a locked file is locked too.
#[test]
fn files_read_only_on_disk_open_locked() {
    let (mut wb, dir) = project("disk", &[("ro.txt", b"abc\n")]);
    let file = dir.join("ro.txt");
    set_read_only(&file, true);
    // Root writes anything (a container's tests): nothing to check there.
    if std::fs::OpenOptions::new().append(true).open(&file).is_ok() {
        set_read_only(&file, false);
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    wb.open_file(&file).unwrap();
    assert_eq!(wb.active_lock(), Some(LockState::ReadOnlyOnDisk));
    assert_eq!(wb.lock_tooltip(), Some(i18n::t("lock.read_only_on_disk")));
    assert!(!wb.toggle_active_lock(), "can't be unlocked");
    assert_eq!(wb.active_lock(), Some(LockState::ReadOnlyOnDisk));
    assert_eq!(told(&wb), [i18n::tr!("lock.on_disk_title", name = "ro.txt")]);
    let mut text = TextSystem::new();
    wb.type_text(&mut text, "x");
    assert_eq!(text_of(&wb), "abc\n");
    // Split into another group: locked there too.
    wb.split_tab(wb.active, Split::Right, false);
    assert!(wb.tabs[wb.active].locked(), "the other tab of the file");
    // Writable again (cleared outside): it unlocks.
    set_read_only(&file, false);
    assert!(wb.toggle_active_lock());
    assert_eq!(wb.active_lock(), Some(LockState::Writable));
    wb.type_text(&mut text, "x");
    assert_eq!(text_of(&wb), "xabc\n");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The lock the user chose comes back with the session (by the file's path).
#[test]
fn the_lock_comes_back_with_the_session() {
    let (mut wb, dir) = project("session", &[("a.txt", b"a\n"), ("b.txt", b"b\n")]);
    let support = Arc::clone(&wb.support);
    wb.open_file(&dir.join("a.txt")).unwrap();
    assert!(wb.toggle_active_lock());
    wb.open_file(&dir.join("b.txt")).unwrap();
    let mut text = TextSystem::new();
    wb.paint(&mut text, &mut Scene::default());
    wb.freeze_session();
    drop(wb);
    let mut wb = Workbench::new(support, Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    wb.open_project(&dir).unwrap();
    assert!(wb.restore_session());
    let locks: Vec<(String, Option<LockState>)> =
        (0..wb.tabs.len()).map(|i| (wb.tabs[i].title(), wb.tab_lock(i))).collect();
    assert_eq!(
        locks,
        [
            ("a.txt".to_owned(), Some(LockState::Locked)),
            ("b.txt".to_owned(), Some(LockState::Writable))
        ]
    );
    let _ = std::fs::remove_dir_all(&dir);
}
