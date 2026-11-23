use super::*;

/// A large document's query typed in the find bar is searched on a worker: typing doesn't
/// wait, the matches and the selection come at the poll after it's back.
#[test]
fn a_large_document_is_searched_off_the_ui_thread() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = Arc::clone(&woken);
    wb.set_waker(Arc::new(move || {
        count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }));
    let dir = std::env::temp_dir().join(format!("find-bg-{:x}", kernel::id::new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("big.txt");
    let line = "some text here, a needle there\n";
    std::fs::write(&file, line.repeat(ON_WORKER / line.len() + 10)).unwrap();
    wb.open_file(&file).unwrap();
    let mut text = TextSystem::new();
    assert!(wb.open_find(&mut text, false));
    assert!(wb.type_text(&mut text, "needle"));
    let tab = &mut wb.tabs[wb.active];
    assert!(tab.find.is_searching(), "out on the worker");
    assert!(tab.find.matches(&tab.doc).is_empty(), "nothing searched here");
    let started = std::time::Instant::now();
    while !wb.poll_find(&mut text) && started.elapsed() < std::time::Duration::from_secs(10) {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let tab = &mut wb.tabs[wb.active];
    let lines = tab.doc.buffer().line_count() - 1;
    assert_eq!(tab.find.matches(&tab.doc).len(), lines);
    assert_eq!(tab.doc.selections().primary(), document::Selection::new(18, 24), "selected");
    assert!(woken.load(std::sync::atomic::Ordering::SeqCst) > 0, "the loop woken");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A screen reader moving the caret in the field with the keyboard (the find bar's): its
/// selection set, aligned to characters.
#[test]
fn a_screen_reader_sets_the_focused_field_s_selection() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    let mut text = TextSystem::new();
    let dir = std::env::temp_dir().join(format!("reader-select-{:x}", kernel::id::new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("a.txt");
    std::fs::write(&file, "text").unwrap();
    wb.open_file(&file).unwrap();
    assert!(wb.open_find(&mut text, false));
    assert!(wb.type_text(&mut text, "ăbc"));
    assert!(wb.select_in_field(2..3, "ăbc"));
    assert_eq!(wb.selected_text().as_deref(), Some("b"));
    assert!(wb.select_in_field(1..2, "ăbc"), "from inside ă: aligned to its start");
    assert_eq!(wb.selected_text().as_deref(), Some("ă"));
    // Meant for another field (its text another): this one's selection stays.
    assert!(!wb.select_in_field(0..1, "a setting's value"));
    assert_eq!(wb.selected_text().as_deref(), Some("ă"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A screen reader's selection in the code editor (no field with the keyboard): its places
/// among the lines told, turned into the document's bytes.
#[test]
fn a_screen_reader_sets_the_editor_s_selection() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    let mut text = TextSystem::new();
    let dir = std::env::temp_dir().join(format!("reader-editor-{:x}", kernel::id::new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("a.txt");
    std::fs::write(&file, "first\nsecond ăb\n").unwrap();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    wb.open_file(&file).unwrap();
    wb.paint(&mut text, &mut render_api::Scene::default());
    assert!(wb.select_in_editor((1, 7), (1, 9)));
    assert_eq!(wb.selected_text().as_deref(), Some("ăb"));
    let _ = std::fs::remove_dir_all(&dir);
}
