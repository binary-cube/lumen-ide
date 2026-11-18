use ui_components::data_grid::GridColumn;

use super::*;

const MOOD: &str = r#"
protocol = { min = 1, max = 1 }
command = "mood-driver"

[driver]
id = "test-mood"
name = "Mood"
[driver.capabilities]
file_based = true
[driver.files]
extensions = ["mood"]
header = "MOOD"

[[driver.value_editors]]
id = "test-mood-editor"
types = ["mood"]
format = { choice = ["happy", "sad"] }
priority = 5
"#;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("external-drivers-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn column(type_name: &str) -> GridColumn {
    GridColumn {
        name: "c".into(),
        type_name: type_name.into(),
        class: CellClass::Text,
        key: false,
        nullable: true,
        editable: true,
        computed: false,
        generated: false,
    }
}

#[test]
fn installed_drivers_are_listed_marked_external_with_their_files_and_value_editors() {
    let dir = temp("install");
    std::fs::create_dir_all(dir.join("mood")).unwrap();
    std::fs::write(dir.join("mood").join(manifest::FILE), MOOD).unwrap();
    // One taking an id the app already has: left out.
    database::drivers::register(database::DriverFactory::new(
        "test-taken",
        "Taken",
        database::Kind::Relational,
        None,
        || unreachable!("never made in this test"),
    ));
    std::fs::create_dir_all(dir.join("taken")).unwrap();
    std::fs::write(dir.join("taken").join(manifest::FILE), MOOD.replace("test-mood", "test-taken"))
        .unwrap();
    // One broken: said, skipped.
    std::fs::create_dir_all(dir.join("broken")).unwrap();
    std::fs::write(dir.join("broken").join(manifest::FILE), "nonsense").unwrap();

    assert_eq!(install(&dir), vec!["test-mood".to_owned()]);
    let mood = database::drivers::find("test-mood").expect("listed");
    assert!(mood.external);
    assert!(!database::drivers::find("test-taken").unwrap().external, "the app's stays");
    let files = database::drivers::for_file(Path::new("/d/x.MOOD"), |_| Some(b"MOOD!".to_vec()));
    assert_eq!(files.map(|f| f.driver.into_owned()), Some("test-mood".to_owned()));
    let editor = value_editor::editor_for(&column("mood"));
    assert_eq!(editor.id(), "test-mood-editor");
    assert!(editor.parse("angry", &column("mood")).is_err(), "one of its values only");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_value_editor_is_the_grids_spec() {
    let spec = value_editor(&dto::ValueEditor {
        id: "x".into(),
        classes: vec![dto::TypeClass::Json],
        types: vec!["JSONB".into()],
        format: dto::ValueFormat::Json,
        large: true,
        priority: 2,
    });
    assert_eq!(
        spec,
        ValueEditorSpec::new("x", ValueFormat::Json)
            .for_classes(&[CellClass::Json])
            .for_types(&["jsonb"])
            .in_large_editor()
            .priority(2)
    );
}

#[test]
fn the_module_starts_nothing_when_activated() {
    assert_eq!(DatabaseExternalModule.manifest().id.as_str(), MODULE_ID);
    assert!(drivers_dir().is_none_or(|d| d.ends_with(DRIVERS_DIR)));
}
