//! "Extend Selection" / "Shrink Selection" on a PHP file's syntax tree, with several
//! cursors, and back.

use super::*;

const SRC: &str =
    "<?php\nfunction total(int $a, int $b): int\n{\n    return add($a, 'x') + $b;\n}\n";

/// `SRC` open in a project, its syntax tree parsed.
fn opened(name: &str) -> (Workbench, TextSystem) {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let support = Arc::new(support);
    let mut wb = Workbench::new(Arc::clone(&support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    let worker = SyntaxWorker::spawn(Arc::clone(&support.syntax), Arc::new(|| {})).unwrap();
    wb.set_worker(worker);
    let dir = std::env::temp_dir().join(format!("select-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.php"), SRC).unwrap();
    wb.open_project(&dir).unwrap();
    wb.open_file(&dir.join("a.php")).unwrap();
    let mut text = TextSystem::new();
    let mut scene = Scene::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while wb.tabs[wb.active].syntax.snapshot().is_none() {
        assert!(std::time::Instant::now() < deadline, "no syntax tree");
        wb.paint(&mut text, &mut scene);
        wb.poll_syntax();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    wb.focus = Focus::Editor;
    (wb, text)
}

fn selected(wb: &Workbench) -> Vec<String> {
    let doc = &wb.tabs[wb.active].doc;
    doc.selections().iter().map(|s| doc.buffer().slice(s.range())).collect()
}

#[test]
fn a_cursor_grows_by_the_tree_and_shrinks_back() {
    let (mut wb, mut text) = opened("grow");
    let at = SRC.find("$a, 'x'").unwrap() + 1;
    document::commands::click(&mut wb.tabs[wb.active].doc, at, false, false);
    let mut steps = Vec::new();
    for _ in 0..4 {
        assert!(wb.extend_selection(&mut text, true));
        steps.push(selected(&wb)[0].clone());
    }
    assert_eq!(steps[0], "$a", "the variable (the word with its `$`)");
    assert_eq!(steps[1], "$a, 'x'", "inside the call's parentheses");
    assert!(steps[2].starts_with('(') && steps[2].ends_with(')'), "{steps:?}");
    assert_eq!(steps[3], "add($a, 'x')");
    // Back the way it came, then nothing more to shrink.
    assert!(wb.extend_selection(&mut text, false));
    assert!(selected(&wb)[0].starts_with('('));
    for _ in 0..3 {
        wb.extend_selection(&mut text, false);
    }
    assert_eq!(selected(&wb), [""]);
    assert!(!wb.extend_selection(&mut text, false));
    // Up to the whole file, then it stays.
    while wb.extend_selection(&mut text, true) {}
    assert_eq!(selected(&wb), [SRC]);
}

#[test]
fn every_cursor_grows_on_its_own() {
    let (mut wb, mut text) = opened("carets");
    let doc = &mut wb.tabs[wb.active].doc;
    let first = SRC.find("int $a").unwrap() + 1;
    let second = SRC.find("$b;").unwrap() + 1;
    document::commands::click(doc, first, false, false);
    document::commands::toggle_cursor(doc, second);
    assert!(wb.extend_selection(&mut text, true));
    assert_eq!(selected(&wb), ["int", "$b"]);
    // A selection made otherwise forgets the steps: nothing to shrink back to.
    document::commands::click(&mut wb.tabs[wb.active].doc, 0, false, false);
    assert!(!wb.extend_selection(&mut text, false));
}

#[test]
fn outside_the_editor_the_keys_move_as_before() {
    let (mut wb, mut text) = opened("outside");
    wb.focus = Focus::Find;
    let before = wb.tabs[wb.active].doc.selections().clone();
    wb.extend_selection(&mut text, true);
    assert!(wb.tabs[wb.active].doc.selections().iter().all(|s| s.is_empty()));
    let _ = before;
}
