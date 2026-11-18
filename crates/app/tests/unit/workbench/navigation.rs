//! "Go to Implementation", "Go to Type Declaration" and "Type Hierarchy" from the built-in
//! engine (the index), in a PHP project without language servers.

use super::*;

const SHAPE: &str =
    "<?php\nnamespace App;\n\ninterface Shape\n{\n    public function area(): float;\n}\n";
const CIRCLE: &str = "<?php\nnamespace App;\n\nclass Circle implements Shape\n{\n    public function area(): float { return 1.0; }\n}\n";
const SQUARE: &str = "<?php\nnamespace App;\n\nclass Square implements Shape\n{\n    public function area(): float { return 2.0; }\n}\n";
const BIG: &str = "<?php\nnamespace App;\n\nclass Big extends Square\n{\n}\n";
const SHOP: &str = "<?php\nnamespace App;\n\nclass Shop\n{\n    private Circle $circle;\n\n    public function make(): ?Square\n    {\n        $big = new Big();\n        $this->circle->area();\n        $c = new Circle();\n        $c->area();\n        return $this->make();\n    }\n}\n";

/// The files above in a project, indexed, `open` opened.
fn opened(name: &str, open: &str) -> (Workbench, TextSystem, PathBuf) {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    let dir = std::env::temp_dir().join(format!("navigation-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let files = [
        ("Shape.php", SHAPE),
        ("Circle.php", CIRCLE),
        ("Square.php", SQUARE),
        ("Big.php", BIG),
        ("Shop.php", SHOP),
    ];
    for (file, text) in files {
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
    (wb, TextSystem::new(), dir)
}

fn cursor(wb: &mut Workbench, text: &str, needle: &str) {
    let at = text.find(needle).unwrap() + 1;
    document::commands::click(&mut wb.tabs[wb.active].doc, at, false, false);
}

fn active_file(wb: &Workbench) -> String {
    let path = wb.tabs[wb.active].doc.path().unwrap();
    path.file_name().unwrap().to_string_lossy().into_owned()
}

#[test]
fn an_interface_lists_what_implements_it_down_the_chain() {
    let (mut wb, mut text, _) = opened("interface", "Shape.php");
    cursor(&mut wb, SHAPE, "Shape\n");
    let (subject, found) =
        wb.built_in_implementations(wb.tabs[wb.active].doc.selections().primary().head).unwrap();
    assert_eq!(subject, "Shape");
    let names: Vec<&str> = found.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Circle", "Square", "Big"]);
    // Several: listed beside the cursor; the one chosen opens.
    assert!(wb.go_to_related(&mut text, Related::Implementations));
    assert!(wb.choice_menu.is_open());
    assert!(matches!(&wb.choice_purpose, ChoicePurpose::Navigate(rows) if rows.len() == 3));
    wb.choice_picked(1);
    wb.paint(&mut text, &mut Scene::default());
    assert_eq!(active_file(&wb), "Square.php");
}

#[test]
fn a_method_lists_its_overrides_and_one_is_jumped_to() {
    let (mut wb, mut text, _) = opened("method", "Shape.php");
    cursor(&mut wb, SHAPE, "area");
    let head = wb.tabs[wb.active].doc.selections().primary().head;
    let (_, found) = wb.built_in_implementations(head).unwrap();
    let owners: Vec<_> = found.iter().map(|p| p.container.clone().unwrap()).collect();
    assert_eq!(owners, ["App\\Circle", "App\\Square"]);
    // From a call on a `Circle`: its own `area` has no overrides — said, nothing opened.
    let (mut wb, mut text2, _) = opened("method-call", "Shop.php");
    cursor(&mut wb, SHOP, "area();\n        return");
    let head = wb.tabs[wb.active].doc.selections().primary().head;
    assert_eq!(wb.built_in_implementations(head).map(|(_, f)| f.len()), Some(0));
    assert!(!wb.go_to_related(&mut text2, Related::Implementations));
    // `Square` has one subclass: jumped to at once.
    cursor(&mut wb, SHOP, "Square");
    assert!(wb.go_to_related(&mut text, Related::Implementations));
    assert_eq!(active_file(&wb), "Big.php");
}

#[test]
fn the_type_of_a_variable_a_property_and_a_call() {
    let (mut wb, mut text, _) = opened("types", "Shop.php");
    let place = |wb: &mut Workbench, needle: &str| {
        cursor(wb, SHOP, needle);
        let head = wb.tabs[wb.active].doc.selections().primary().head;
        wb.built_in_type_definition(head).map(|p| p.name)
    };
    assert_eq!(place(&mut wb, "$big = ").as_deref(), Some("Big"), "a variable: its `new`");
    assert_eq!(place(&mut wb, "circle->area").as_deref(), Some("Circle"), "a property's type");
    assert_eq!(place(&mut wb, "make();").as_deref(), Some("Square"), "what a method returns");
    assert_eq!(place(&mut wb, "Shop\n").as_deref(), Some("Shop"), "a type is its own");
    assert_eq!(place(&mut wb, "return"), None);
    cursor(&mut wb, SHOP, "$big = ");
    assert!(wb.go_to_related(&mut text, Related::TypeDefinition));
    assert_eq!(active_file(&wb), "Big.php");
}

#[test]
fn the_hierarchy_lists_up_and_down_with_the_diagram_last() {
    let (mut wb, mut text, _) = opened("hierarchy", "Square.php");
    cursor(&mut wb, SQUARE, "Square");
    let h = wb.built_in_hierarchy().unwrap();
    let rows: Vec<(i8, usize, &str)> =
        h.rows().into_iter().map(|(s, d, p)| (s, d, p.name.as_str())).collect();
    assert_eq!(rows, [(-1, 1, "Shape"), (0, 0, "Square"), (1, 1, "Big")]);
    assert!(wb.go_to_related(&mut text, Related::Hierarchy));
    let ChoicePurpose::Navigate(targets) = &wb.choice_purpose else { panic!("a navigation list") };
    assert_eq!(targets.len(), 4, "three types and Show Diagram");
    assert_eq!(targets[3], navigation::NavRow::Command(workbench_model::SHOW_TYPE_DIAGRAM));
}
