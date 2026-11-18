use super::*;
use render_api::Scene;

/// A workbench with the modules and the syntax worker, `source` open as `name` (its `|`
/// the cursor), parsed at its version.
fn opened(name: &str, source: &str) -> (Workbench, TextSystem, std::path::PathBuf) {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let support = Arc::new(support);
    let mut wb = Workbench::new(Arc::clone(&support), Ports::local());
    wb.forget_projects();
    wb.resize(kernel::PhysicalSize::new(1200, 800), 1.0);
    let worker =
        syntax_ts::SyntaxWorker::spawn(Arc::clone(&support.syntax), Arc::new(|| {})).unwrap();
    wb.set_worker(worker);
    let dir = std::env::temp_dir().join(format!("indent-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(name);
    let at = source.find('|').unwrap();
    std::fs::write(&file, source.replace('|', "")).unwrap();
    wb.open_file(&file).unwrap();
    let mut text = TextSystem::new();
    let tab = &mut wb.tabs[wb.active];
    tab.doc.set_selections(document::Selections::single(document::Selection::cursor(at)));
    settle(&mut wb, &mut text);
    (wb, text, dir)
}

/// Until the tree is parsed at the document's version.
fn settle(wb: &mut Workbench, text: &mut TextSystem) {
    let mut scene = Scene::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        wb.paint(text, &mut scene);
        wb.poll_syntax();
        let tab = &wb.tabs[wb.active];
        if tab.syntax.snapshot().is_some_and(|s| s.version() == tab.doc.version()) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("not parsed");
}

/// The text after Enter at the `|` of `source` in a file named `name`.
fn enter(name: &str, source: &str) -> String {
    let (mut wb, mut text, dir) = opened(name, source);
    wb.edit_key(&mut text, EditKey::Enter { shift: false });
    let out = wb.tabs[wb.active].doc.buffer().to_string();
    let _ = std::fs::remove_dir_all(dir);
    out
}

#[test]
fn enter_indents_by_the_tree_in_every_language() {
    // PHP: a method's body between its braces; a call's arguments.
    assert_eq!(
        enter("a.php", "<?php\nclass A\n{\n    public function f()\n    {|}\n}\n"),
        "<?php\nclass A\n{\n    public function f()\n    {\n        \n    }\n}\n"
    );
    assert_eq!(
        enter("b.php", "<?php\n$x = foo(\n    1,|\n);\n"),
        "<?php\n$x = foo(\n    1,\n    \n);\n"
    );
    // JavaScript / TypeScript: a nested block, an interface.
    assert_eq!(
        enter("a.js", "function a() {\n    if (x) {|}\n}\n"),
        "function a() {\n    if (x) {\n        \n    }\n}\n"
    );
    assert_eq!(enter("a.ts", "interface A {|}\n"), "interface A {\n    \n}\n");
    // CSS: a rule's block.
    assert_eq!(enter("a.css", ".a {|}\n"), ".a {\n    \n}\n");
    // HTML: between a tag's open and close, the content in, the close at the tag's level.
    assert_eq!(
        enter("a.html", "<ul>\n    <li>|</li>\n</ul>\n"),
        "<ul>\n    <li>\n        \n    </li>\n</ul>\n"
    );
    // JSON: an array inside an object.
    assert_eq!(enter("a.json", "{\n    \"a\": [|]\n}\n"), "{\n    \"a\": [\n        \n    ]\n}\n");
    // YAML: after `key:`, the value one level in.
    let yaml = enter("a.yaml", "root:|\n");
    assert!(yaml.starts_with("root:\n ") && yaml.lines().nth(1).unwrap().len() >= 2, "{yaml:?}");
}

/// `</` typed first on a line: the line goes back to its element's indentation.
#[test]
fn a_closing_tag_goes_back_to_its_element() {
    let (mut wb, mut text, dir) = opened("b.html", "<ul>\n    <li>a</li>\n        |");
    wb.type_text(&mut text, "<");
    settle(&mut wb, &mut text);
    wb.type_text(&mut text, "/");
    let out = wb.tabs[wb.active].doc.buffer().to_string();
    let _ = std::fs::remove_dir_all(dir);
    let last = out.lines().last().unwrap();
    assert!(last.starts_with("</"), "{out:?}");
}

/// `<?` typed in a PHP file's HTML: `<?php` and a new line (the cursor on it); one Undo
/// leaves `<?`; `=` right after gives `<?=`. Not in a file of another language.
#[test]
fn an_open_tag_typed_in_a_php_file_becomes_php() {
    let typed = |name: &str, keys: &[&str]| {
        let (mut wb, mut text, dir) = opened(name, "<div>\n    |\n</div>\n");
        for key in keys {
            wb.type_text(&mut text, key);
            settle(&mut wb, &mut text);
        }
        let _ = std::fs::remove_dir_all(dir);
        wb
    };
    let mut wb = typed("c.php", &["<", "?"]);
    let tab = &wb.tabs[wb.active];
    assert_eq!(tab.doc.buffer().to_string(), "<div>\n    <?php\n    \n</div>\n");
    assert_eq!(tab.doc.selections().primary().head, "<div>\n    <?php\n    ".len());
    let mut text = TextSystem::new();
    wb.edit(&mut text, |doc| {
        doc.undo();
    });
    assert_eq!(wb.tabs[wb.active].doc.buffer().to_string(), "<div>\n    <?\n</div>\n");
    let wb = typed("d.php", &["<", "?", "="]);
    assert_eq!(wb.tabs[wb.active].doc.buffer().to_string(), "<div>\n    <?=\n</div>\n");
    let wb = typed("e.html", &["<", "?"]);
    assert_eq!(wb.tabs[wb.active].doc.buffer().to_string(), "<div>\n    <?\n</div>\n");
}
