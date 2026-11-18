//! The code actions at the cursor from the built-in engine (a PHP project without language
//! servers): the class imported, the unused import removed, the refactorings listed, the
//! bulb once the cursor rests; and the document links.

use super::*;

const SHOP: &str = "<?php\nnamespace App;\n\nuse App\\Lib\\Unused;\n\nclass Shop\n{\n    public function cart()\n    {\n        return new Cart(); // https://example.com/docs\n    }\n}\n";
const CART: &str = "<?php\nnamespace App\\Lib;\n\nclass Cart {}\n";
const UNUSED: &str = "<?php\nnamespace App\\Lib;\n\nclass Unused {}\n";

fn opened(name: &str) -> (Workbench, TextSystem, PathBuf) {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    let dir = std::env::temp_dir().join(format!("actions-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    std::fs::write(dir.join("Shop.php"), SHOP).unwrap();
    std::fs::write(dir.join("lib/Cart.php"), CART).unwrap();
    std::fs::write(dir.join("lib/Unused.php"), UNUSED).unwrap();
    let dir = crate::workbench::canonical(&dir);
    wb.open_project(&dir).unwrap();
    let started = std::time::Instant::now();
    while wb.data.symbols.read().unwrap().type_by_fqn("App\\Lib\\Cart").is_none() {
        assert!(started.elapsed() < std::time::Duration::from_secs(20), "not indexed");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    wb.open_file(&dir.join("Shop.php")).unwrap();
    wb.focus = Focus::Editor;
    (wb, TextSystem::new(), dir)
}

fn cursor(wb: &mut Workbench, needle: &str) -> usize {
    let at = SHOP.find(needle).unwrap() + 1;
    document::commands::click(&mut wb.tabs[wb.active].doc, at, false, false);
    at
}

fn frame(wb: &mut Workbench, text: &mut TextSystem) {
    wb.poll_symbols();
    wb.poll_servers();
    wb.paint(text, &mut Scene::default());
}

#[test]
fn a_class_written_without_its_import_is_imported() {
    let (mut wb, mut text, _) = opened("import");
    let at = cursor(&mut wb, "Cart()");
    let fixes = wb.built_in_fixes(at);
    assert_eq!(fixes.len(), 1);
    assert_eq!(fixes[0].title, i18n::tr!("actions.import_class", name = "App\\Lib\\Cart"));
    // Listed with the refactorings there (Rename…), the fix first; chosen, it's done.
    assert!(wb.show_context_actions(&mut text));
    let ChoicePurpose::Actions(rows) = &wb.choice_purpose else { panic!("the actions' list") };
    assert!(matches!(rows[0], context_actions::ContextAction::Edits(_)));
    assert!(
        rows.iter().any(|r| matches!(r, context_actions::ContextAction::Command(_))),
        "a refactoring"
    );
    wb.choice_picked(0);
    frame(&mut wb, &mut text);
    let now = wb.tabs[wb.active].doc.buffer().text();
    assert!(now.contains("use App\\Lib\\Unused;\nuse App\\Lib\\Cart;"), "{now}");
    // Imported: nothing more to fix there.
    let at = now.find("Cart()").unwrap() + 1;
    assert!(wb.built_in_fixes(at).is_empty());
}

#[test]
fn an_unused_import_is_removed_and_the_bulb_shows_once_the_cursor_rests() {
    let (mut wb, mut text, _) = opened("unused");
    let started = std::time::Instant::now();
    while !wb.tabs[wb.active].diagnostics.unused.iter().any(|u| u.kind == "import") {
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "no unused import found");
        frame(&mut wb, &mut text);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let at = cursor(&mut wb, "Unused;");
    frame(&mut wb, &mut text);
    assert!(wb.bulb().is_none(), "not while the cursor just moved");
    std::thread::sleep(context_actions::BULB_DELAY);
    frame(&mut wb, &mut text);
    assert_eq!(wb.bulb(), Some(ui_workbench::Bulb { line: 3, fix: true }));
    let fixes = wb.built_in_fixes(at);
    assert_eq!(fixes[0].title, i18n::tr!("actions.remove_import", name = "Unused"));
    wb.data.actions.chosen = Some(fixes[0].action.clone());
    frame(&mut wb, &mut text);
    assert!(!wb.tabs[wb.active].doc.buffer().text().contains("Unused"));
    // One undo takes it back.
    wb.tabs[wb.active].doc.undo();
    assert!(wb.tabs[wb.active].doc.buffer().text().contains("use App\\Lib\\Unused;"));
}

/// The cursor moved, then another kind of tab came to the front before the bulb was worked
/// out: nothing stays due — a wait left due asked for a frame at every frame (the app at
/// 100% of a core, idle).
#[test]
fn a_tab_of_another_kind_in_front_leaves_no_wait_due() {
    let (mut wb, mut text, _) = opened("other-tab");
    cursor(&mut wb, "Cart()");
    frame(&mut wb, &mut text);
    assert!(wb.data.actions.due.is_some(), "the bulb waits for the cursor to rest");
    assert_eq!(wb.run_command(task_manager::OPEN), Some(true));
    std::thread::sleep(context_actions::BULB_DELAY);
    frame(&mut wb, &mut text);
    let now = std::time::Instant::now();
    assert!(
        wb.served_deadline().is_none_or(|at| at > now),
        "nothing due once the editor isn't in front"
    );
}

#[test]
fn quoted_paths_and_urls_are_links() {
    let (mut wb, mut text, dir) = opened("links");
    let source = "<?php\nrequire 'lib/Cart.php';\n// https://example.com/docs\n";
    let len = wb.tabs[wb.active].doc.buffer().len();
    assert!(workbench_model::EditorHost::edit_active(&mut wb, 0..len, source));
    let at = source.find("Cart.php").unwrap();
    let link = wb.document_link_at(at).unwrap();
    assert_eq!(&source[link.range.clone()], "lib/Cart.php");
    let file =
        code_intelligence::links::LinkTarget::File { path: dir.join("lib/Cart.php"), line: None };
    assert_eq!(link.target, file);
    let url = wb.document_link_at(source.find("example").unwrap()).unwrap();
    assert_eq!(
        url.target,
        code_intelligence::links::LinkTarget::Url("https://example.com/docs".into())
    );
    assert!(wb.follow_link(&mut text, url.target));
    assert!(
        matches!(wb.take_request(), Some(MouseResult::OpenUrl(u)) if u == "https://example.com/docs")
    );
    assert!(wb.follow_link(&mut text, file));
    assert_eq!(wb.tabs[wb.active].doc.path(), Some(dir.join("lib/Cart.php").as_path()));
    assert!(wb.document_link_at(2).is_none());
}

#[test]
fn what_the_project_says_is_wrong_is_marked_once_it_is_indexed() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    let dir = std::env::temp_dir().join(format!("actions-problems-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let probe = "<?php\nnamespace App;\n\nclass Probe\n{\n    /** @deprecated */\n    public function old(int $a): void {}\n\n    public function run(): void\n    {\n        $this->old();\n        new Missing();\n    }\n}\n";
    std::fs::write(dir.join("Probe.php"), probe).unwrap();
    let dir = crate::workbench::canonical(&dir);
    wb.open_project(&dir).unwrap();
    let started = std::time::Instant::now();
    while wb.indexing() || wb.data.symbols.read().unwrap().type_by_fqn("App\\Probe").is_none() {
        assert!(started.elapsed() < std::time::Duration::from_secs(20), "not indexed");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    wb.open_file(&dir.join("Probe.php")).unwrap();
    let mut text = TextSystem::new();
    let started = std::time::Instant::now();
    while wb.tabs[wb.active].diagnostics.problems.len() < 2 {
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "no problems found");
        frame(&mut wb, &mut text);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let tab = &wb.tabs[wb.active];
    let kinds: Vec<&str> = tab.diagnostics.problems.iter().map(|p| p.kind).collect();
    assert_eq!(kinds, ["too_few_arguments", "undefined_class"]);
    assert!(tab.diagnostics.deprecated.iter().any(|d| d.kind == "method" && d.name == "old"));
    frame(&mut wb, &mut text);
    let marks = &wb.tabs[wb.active].diagnostics.marks;
    let expected = i18n::tr!("problem.too_few_arguments", expected = 1, given = 0);
    assert!(marks.iter().any(|m| m.message == expected && m.warning), "{marks:?}");
    assert!(marks.iter().any(|m| m.message == i18n::t("problem.undefined_class")));
}

/// Another file's class changes on disk (as the watcher says: `EntriesChanged` on the
/// project's bus): the index takes it in, says so (`IndexUpdated`), and the open file is
/// checked again against it — its argument-count and unknown-method warnings follow,
/// without an edit of its own (plan 55c).
#[test]
fn another_files_change_checks_the_open_file_again() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    let dir = std::env::temp_dir().join(format!("actions-recheck-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let lib = "<?php\nnamespace App;\n\nclass Lib\n{\n    public function go(int $a): void {}\n\n    public function walk(): void {}\n}\n";
    let probe = "<?php\nnamespace App;\n\nfunction probe(): void\n{\n    $lib = new Lib();\n    $lib->go();\n    $lib->walk();\n}\n";
    std::fs::write(dir.join("Lib.php"), lib).unwrap();
    std::fs::write(dir.join("Probe.php"), probe).unwrap();
    let dir = crate::workbench::canonical(&dir);
    wb.open_project(&dir).unwrap();
    let started = std::time::Instant::now();
    while wb.indexing() || wb.data.symbols.read().unwrap().type_by_fqn("App\\Lib").is_none() {
        assert!(started.elapsed() < std::time::Duration::from_secs(20), "not indexed");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    wb.open_file(&dir.join("Probe.php")).unwrap();
    let mut text = TextSystem::new();
    let kinds = |wb: &Workbench| -> Vec<&'static str> {
        wb.tabs[wb.active].diagnostics.problems.iter().map(|p| p.kind).collect()
    };
    let wait_for = |wb: &mut Workbench, text: &mut TextSystem, want: &[&str]| {
        let started = std::time::Instant::now();
        while kinds(wb) != want {
            assert!(started.elapsed() < std::time::Duration::from_secs(10), "{:?}", kinds(wb));
            // As at the app's wake: the index looked at, then the frame.
            wb.poll_indexer();
            frame(wb, text);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    };
    wait_for(&mut wb, &mut text, &["too_few_arguments"]);
    let version = wb.tabs[wb.active].doc.version();
    // `go` takes nothing now, `walk` is gone.
    let changed = "<?php\nnamespace App;\n\nclass Lib\n{\n    public function go(): void {}\n}\n";
    std::fs::write(dir.join("Lib.php"), changed).unwrap();
    let events = wb.data.events.clone();
    events.publish(&workspace::events::EntriesChanged { paths: vec![dir.join("Lib.php")] });
    wait_for(&mut wb, &mut text, &["undefined_method"]);
    assert_eq!(wb.tabs[wb.active].doc.version(), version, "no edit of its own");
    let _ = std::fs::remove_dir_all(&dir);
}
