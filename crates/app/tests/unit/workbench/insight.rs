//! The inlay hints and code lenses from the built-in engine (a PHP project without language
//! servers): worked out in the background for the visible lines, following the edits, never
//! per frame; the lenses' usages and implementations, a click on them.

use super::*;

const SHAPE: &str = "<?php\nnamespace App;\n\ninterface Shape\n{\n    public function area(int $scale, bool $round = false): float;\n}\n";
const SQUARE: &str = "<?php\nnamespace App;\n\nclass Square implements Shape\n{\n    public function area(int $scale, bool $round = false): float\n    {\n        return $this->area(2, true);\n    }\n}\n";
const CIRCLE: &str = "<?php\nnamespace App;\n\nclass Circle implements Shape\n{\n    public function area(int $scale, bool $round = false): float { return 1.0; }\n}\n";
const USE: &str = "<?php\nnamespace App;\n\nfunction draw(Shape $shape): void {}\n";

/// PHP answered by the built-in engine while the test runs (the servers' tests give the
/// process a server for it), beside the tests that don't choose PHP's server.
fn beside_php_servers()
-> (std::sync::RwLockReadGuard<'static, ()>, language_servers::settings::Pinned) {
    let lock =
        super::super::servers::tests::PHP.read().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut chosen = language_servers::settings::config();
    chosen.engines = vec![(
        "php".into(),
        language_servers::settings::Choice {
            engine: code_intelligence::servers::Engine::BuiltIn,
            server: None,
        },
    )];
    (lock, language_servers::settings::pin(chosen))
}

fn opened(name: &str, open: &str, extra: &[(&str, String)]) -> (Workbench, TextSystem, PathBuf) {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    let dir = std::env::temp_dir().join(format!("insight-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut files: Vec<(&str, String)> = vec![
        ("Shape.php", SHAPE.into()),
        ("Square.php", SQUARE.into()),
        ("Circle.php", CIRCLE.into()),
        ("use.php", USE.into()),
    ];
    files.extend(extra.iter().cloned());
    for (file, text) in &files {
        std::fs::write(dir.join(file), text).unwrap();
    }
    let dir = crate::workbench::canonical(&dir);
    wb.open_project(&dir).unwrap();
    let started = std::time::Instant::now();
    let indexed = |wb: &Workbench| {
        let index = wb.data.symbols.read().unwrap();
        files.iter().all(|(f, _)| index.file(&dir.join(f)).is_some())
    };
    while !indexed(&wb) {
        assert!(started.elapsed() < std::time::Duration::from_secs(20), "not indexed");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    wb.open_file(&dir.join(open)).unwrap();
    wb.focus = Focus::Editor;
    (wb, TextSystem::new(), dir)
}

fn frame(wb: &mut Workbench, text: &mut TextSystem) {
    wb.poll_symbols();
    wb.poll_servers();
    wb.poll_indexer();
    wb.paint(text, &mut Scene::default());
}

fn settle(
    wb: &mut Workbench,
    text: &mut TextSystem,
    what: &str,
    done: impl Fn(&Workbench) -> bool,
) {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        frame(wb, text);
        if done(wb) {
            return;
        }
        assert!(std::time::Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn hints(wb: &Workbench) -> Vec<(usize, String)> {
    wb.tabs[wb.active].view.inlays().iter().map(|i| (i.offset, i.label.clone())).collect()
}

#[test]
fn literal_arguments_get_their_parameters_name_and_follow_the_edits() {
    let _php = beside_php_servers();
    let (mut wb, mut text, _) = opened("hints", "Square.php", &[]);
    settle(&mut wb, &mut text, "the hints", |wb| !hints(wb).is_empty());
    let at = SQUARE.find("2, true").unwrap();
    assert_eq!(hints(&wb), [(at, "scale:".into()), (at + 3, "round:".into())]);
    // Frames without changes ask nothing again.
    let started = wb.data.insight.started;
    for _ in 0..20 {
        frame(&mut wb, &mut text);
    }
    assert_eq!(wb.data.insight.started, started, "nothing worked out per frame");
    // Typed before them: moved at once, worked out again once after the pause.
    wb.edit(&mut text, |doc| commands::click(doc, 0, false, false));
    wb.type_text(&mut text, "\n");
    frame(&mut wb, &mut text);
    assert_eq!(hints(&wb)[0].0, at + 1, "moved with the text");
    assert_eq!(wb.data.insight.started, started, "not before the pause");
    std::thread::sleep(insight::HINTS_DELAY);
    settle(&mut wb, &mut text, "worked out again", |wb| wb.data.insight.started == started + 1);
    for _ in 0..5 {
        frame(&mut wb, &mut text);
    }
    assert_eq!(wb.data.insight.started, started + 1);
}

#[test]
fn a_large_file_works_out_only_the_lines_in_view() {
    let mut big = String::from(
        "<?php\nnamespace App;\n\nfunction add(int $a, int $b): int { return $a + $b; }\n",
    );
    for i in 0..5000 {
        big.push_str(&format!("add({i}, 2);\n"));
    }
    let _php = beside_php_servers();
    let (mut wb, mut text, _) = opened("large", "big.php", &[("big.php", big.clone())]);
    let started = std::time::Instant::now();
    settle(&mut wb, &mut text, "the hints", |wb| !hints(wb).is_empty());
    let took = started.elapsed();
    let found = hints(&wb);
    // The visible rows (~40) and a margin, not the 5000 calls.
    assert!(found.len() < 400, "{} hints", found.len());
    let last = found.last().unwrap().0;
    assert!(last < big.len() / 10, "only near the top: {last}");
    assert!(took < std::time::Duration::from_secs(5), "{took:?}");
    // A frame with them costs no work: the same list given to the view.
    let before = Arc::as_ptr(wb.tabs[wb.active].view.inlays());
    frame(&mut wb, &mut text);
    assert_eq!(Arc::as_ptr(wb.tabs[wb.active].view.inlays()), before);
}

#[test]
fn lenses_count_usages_and_implementations_and_open_them() {
    let _php = beside_php_servers();
    let (mut wb, mut text, _) = opened("lenses", "Shape.php", &[]);
    settle(&mut wb, &mut text, "the lenses", |wb| !wb.tabs[wb.active].insight.lines.is_empty());
    let lines = wb.tabs[wb.active].insight.lines.clone();
    let shape = lines.iter().find(|l| l.line == 3).expect("a lens on the interface");
    // Used in Square, Circle and use.php; implemented by two classes.
    assert_eq!(
        shape.parts,
        [i18n::tr!("insight.usages", count = 3), i18n::tr!("insight.implementations", count = 2)]
    );
    let area = lines.iter().find(|l| l.line == 5).expect("a lens on the method");
    assert_eq!(area.parts.last(), Some(&i18n::tr!("insight.overrides", count = 2)));
    // Its implementations: listed.
    let index = lines.iter().position(|l| l.line == 3).unwrap();
    assert!(wb.lens_clicked(&mut text, index, 1));
    assert!(matches!(&wb.choice_purpose, ChoicePurpose::Navigate(rows) if rows.len() == 2));
}
