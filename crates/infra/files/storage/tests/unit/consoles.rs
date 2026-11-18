//! The consoles' list: written and read back the same, a missing file read as none, a
//! broken one an error never written over.

use super::*;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("consoles-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

#[test]
fn consoles_are_written_and_read_back() {
    let dir = temp("round");
    let store = TomlConsoles::new(&dir);
    assert_eq!(store.load(), Ok(Vec::new()), "no file: none");
    let source = DataSourceId::new();
    let plain = ConsoleSpec::new(source, "console");
    let mut full = ConsoleSpec::new(source, "console_1");
    full.database = Some("shop".into());
    full.schema = Some("public".into());
    full.tx = TxMode::Manual;
    full.isolation = Some(Isolation::RepeatableRead);
    full.mode = RunMode::Script;
    full.last_run_ms = Some(543);
    full.params = vec!["id".into(), "email".into()];
    let consoles = vec![plain, full];
    let saved = store.save(&consoles);
    let loaded = store.load();
    let text = std::fs::read_to_string(store.file()).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(saved.is_ok(), "{saved:?}");
    assert_eq!(loaded, Ok(consoles));
    assert!(text.contains(&format!("file = \"consoles/{source}/console_1.sql\"")), "{text}");
}

#[test]
fn a_broken_list_is_an_error_left_as_it_is() {
    let dir = temp("broken");
    let store = TomlConsoles::new(&dir);
    let _ = std::fs::create_dir_all(store.file().parent().unwrap());
    std::fs::write(store.file(), "[[console]]\nname = 3\n").unwrap();
    let loaded = store.load();
    let still = std::fs::read_to_string(store.file()).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(loaded.is_err(), "{loaded:?}");
    assert_eq!(still, "[[console]]\nname = 3\n");
}

/// The project's SQL files attached to a source (39n): their own list beside the consoles',
/// each known by its path in the project (always written with `/`), named as its file.
#[test]
fn attached_files_are_written_and_read_back() {
    let dir = temp("attached");
    let store = TomlConsoles::sql_files(&dir);
    assert_eq!(store.load(), Ok(Vec::new()), "no file: none");
    assert_eq!(
        store.file(),
        dir.join(kernel::brand::PROJECT_DIR).join(database::console::SQL_FILES)
    );
    let source = DataSourceId::new();
    let mut report =
        ConsoleSpec::attached(source, ["db", "reports", "monthly.sql"].iter().collect());
    report.schema = Some("public".into());
    report.tx = TxMode::Manual;
    report.mode = RunMode::Script;
    let top = ConsoleSpec::attached(source, PathBuf::from("schema.sql"));
    let files = vec![report, top];
    let saved = store.save(&files);
    let loaded = store.load();
    let text = std::fs::read_to_string(store.file()).unwrap_or_default();
    // The consoles' list is another file, untouched.
    let consoles = TomlConsoles::new(&dir).load();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(saved.is_ok(), "{saved:?}");
    assert_eq!(loaded, Ok(files));
    assert!(text.contains("[[file]]"), "{text}");
    assert!(text.contains("file = \"db/reports/monthly.sql\""), "{text}");
    assert_eq!(consoles, Ok(Vec::new()));
}

#[test]
fn an_attached_file_without_its_path_is_an_error_left_as_it_is() {
    let dir = temp("attached-broken");
    let store = TomlConsoles::sql_files(&dir);
    let _ = std::fs::create_dir_all(store.file().parent().unwrap());
    let written =
        format!("[[file]]\nid = \"{}\"\nsource = \"{}\"\n", ConsoleId::new(), DataSourceId::new());
    std::fs::write(store.file(), &written).unwrap();
    let loaded = store.load();
    let still = std::fs::read_to_string(store.file()).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(loaded.is_err(), "{loaded:?}");
    assert_eq!(still, written);
}
