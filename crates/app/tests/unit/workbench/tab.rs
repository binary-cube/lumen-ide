use super::*;
use document::{Selection, Selections};
use render_api::Scene;

/// A workbench with the modules and the syntax worker, `source` open as `name`, its
/// folding regions in.
fn opened(name: &str, source: &str) -> (Workbench, TextSystem, std::path::PathBuf) {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let support = Arc::new(support);
    let mut wb = Workbench::new(Arc::clone(&support), Ports::local());
    wb.forget_projects();
    wb.resize(kernel::PhysicalSize::new(1200, 800), 1.0);
    let worker =
        syntax_ts::SyntaxWorker::spawn(Arc::clone(&support.syntax), Arc::new(|| {})).unwrap();
    wb.set_worker(worker);
    let dir = std::env::temp_dir().join(format!("fold-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(name);
    std::fs::write(&file, source).unwrap();
    wb.open_file(&file).unwrap();
    let mut text = TextSystem::new();
    let mut scene = Scene::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while wb.tabs[wb.active].folds.fold_at(1).is_none() {
        assert!(std::time::Instant::now() < deadline, "no folding regions");
        wb.paint(&mut text, &mut scene);
        wb.poll_syntax();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    (wb, text, dir)
}

const PAGE: &str = "<div>\n  <div class=\"card\">\n    <p>x</p>\n  </div>\n</div>\n";

/// `PAGE` with its card (lines 1..=3) collapsed, `selection` set.
fn folded(selection: impl Fn(&document::TextBuffer) -> Selection) -> (Workbench, TextSystem) {
    let (mut wb, text, dir) = opened("page.html", PAGE);
    let _ = std::fs::remove_dir_all(dir);
    let tab = &mut wb.tabs[wb.active];
    tab.toggle_fold(1);
    assert_eq!(tab.folds.folded_lines(), vec![(1, 3)]);
    let s = selection(tab.doc.buffer());
    tab.doc.set_selections(Selections::single(s));
    (wb, text)
}

#[test]
fn a_collapsed_region_selected_is_deleted_and_copied_whole() {
    // Its row's text selected up to the `⋯`: ⌫ takes the whole card, `</div>` included.
    let (mut wb, mut text) = folded(|b| Selection::new(b.line_start(1) + 2, b.line_end(1)));
    assert_eq!(wb.copy().as_deref(), Some("<div class=\"card\">\n    <p>x</p>\n  </div>"));
    let tab = &wb.tabs[wb.active];
    let b = tab.doc.buffer();
    let kept = Selection::new(b.line_start(1) + 2, b.line_end(1));
    assert_eq!(tab.doc.selections().primary(), kept, "copying changed no selection");
    wb.edit_key(&mut text, EditKey::Backspace { word: false });
    assert_eq!(wb.tabs[wb.active].doc.buffer().to_string(), "<div>\n  \n</div>\n");
    // The cursor on its row: the line commands take every line of it.
    let (mut wb, mut text) = folded(|b| Selection::cursor(b.line_start(1) + 4));
    assert_eq!(wb.copy().as_deref(), Some("  <div class=\"card\">\n    <p>x</p>\n  </div>\n"));
    wb.edit(&mut text, commands::delete_lines);
    assert_eq!(wb.tabs[wb.active].doc.buffer().to_string(), "<div>\n</div>\n");
    // Moving with ⇧ doesn't take it (it stays collapsed), as in any line.
    let (mut wb, mut text) = folded(|b| Selection::new(b.line_start(1), b.line_start(1) + 2));
    wb.edit_key(&mut text, EditKey::Move { motion: Motion::Right, extend: true });
    let tab = &wb.tabs[wb.active];
    assert_eq!(tab.doc.selections().primary().head, tab.doc.buffer().line_start(1) + 3);
    assert_eq!(tab.folds.folded_lines(), vec![(1, 3)], "still collapsed");
}
