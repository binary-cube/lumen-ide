//! A source's query history: appended block by block (the file valid TOML after each),
//! read back with its duplicates moved up, compacted atomically, a broken file kept aside,
//! no secret and no parameter's value on disk.

use super::*;
use database::history::{QueryHistory, Write};
use database::sql::{Flavor, ParamStyles};

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("query-history-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn entry(source: DataSourceId, text: &str, at: i64) -> HistoryEntry {
    HistoryEntry::new(source, text, Flavor::Sqlite, &ParamStyles::of(Flavor::Sqlite), at)
}

#[test]
fn runs_are_appended_and_read_back() {
    let dir = temp("append");
    let store = TomlQueryHistory::new(&dir);
    let source = DataSourceId::new();
    assert_eq!(store.load(source), Ok(Loaded::default()), "no file: none");
    let mut full = entry(source, "SELECT *\nFROM orders\nWHERE id = :id;", 1_793_440_823_698);
    full.console = Some(ConsoleId::new());
    full.database = Some("shop".into());
    full.schema = Some("main".into());
    full.duration_ms = Some(28);
    full.rows = Some(9);
    let mut failed = entry(source, "SELEC 1", 2);
    failed.outcome = Outcome::Failed;
    store.append(source, &full).unwrap();
    let file = store.file(source);
    assert!(file.starts_with(dir.join(kernel::brand::PROJECT_DIR).join("database/history")));
    let first = std::fs::read_to_string(&file).unwrap();
    assert!(first.parse::<toml::Table>().is_ok(), "valid after one block");
    store.append(source, &failed).unwrap();
    let both = std::fs::read_to_string(&file).unwrap();
    // Appended: what was there is left as it was.
    assert!(both.starts_with(&first), "{both}");
    assert!(both.parse::<toml::Table>().is_ok(), "valid after two: {both}");
    assert_eq!(both.matches("[[entry]]").count(), 2);
    let loaded = store.load(source).unwrap();
    assert_eq!(loaded.entries, [full, failed]);
    assert_eq!(loaded.broken, None);
}

#[test]
fn a_file_without_its_last_line_ended_still_takes_a_block() {
    let dir = temp("unended");
    let store = TomlQueryHistory::new(&dir);
    let source = DataSourceId::new();
    let a = entry(source, "SELECT a", 1);
    store.append(source, &a).unwrap();
    let file = store.file(source);
    let text = std::fs::read_to_string(&file).unwrap();
    std::fs::write(&file, text.trim_end()).unwrap();
    let b = entry(source, "SELECT b", 2);
    store.append(source, &b).unwrap();
    assert_eq!(store.load(source).unwrap().entries, [a, b]);
}

#[test]
fn the_same_statement_again_is_moved_up_when_read_and_the_file_compacted() {
    let dir = temp("compact");
    let store = TomlQueryHistory::new(&dir);
    let source = DataSourceId::new();
    let limit = 10;
    let mut history = QueryHistory::default();
    let mut rewrites = 0;
    for i in 0..40 {
        let text = if i % 3 == 0 { "SELECT 1".to_owned() } else { format!("SELECT {i}") };
        match history.record(entry(source, &text, i), limit) {
            Write::Append(e) => store.append(source, &e).unwrap(),
            Write::Compact(all) => {
                rewrites += 1;
                store.rewrite(source, &all).unwrap();
            }
        }
        // What's read back is what's kept in memory.
        let read = QueryHistory::from_blocks(store.load(source).unwrap().entries, limit);
        assert_eq!(read.entries(), history.entries(), "after run {i}");
    }
    assert!(rewrites >= 1);
    let blocks = std::fs::read_to_string(store.file(source)).unwrap().matches("[[entry]]").count();
    assert!(blocks <= limit + 16, "{blocks} blocks");
    // Deleted down to nothing: the file goes.
    store.rewrite(source, &[]).unwrap();
    assert!(!store.file(source).exists());
    store.rewrite(source, &[]).unwrap();
}

#[test]
fn a_broken_file_is_kept_aside_and_what_can_be_read_is_kept() {
    let dir = temp("broken");
    let store = TomlQueryHistory::new(&dir);
    let source = DataSourceId::new();
    let a = entry(source, "SELECT a", 1);
    let b = entry(source, "SELECT b", 2);
    store.append(source, &a).unwrap();
    store.append(source, &b).unwrap();
    let file = store.file(source);
    // A block cut halfway (the app stopped while writing it).
    let mut text = std::fs::read_to_string(&file).unwrap();
    text.push_str("\n[[entry]]\nid = \"7c1e5a2b-9d3f-4e6a-8b0c-2f4d6e8a0b1c\"\ntext = \"SELECT c");
    std::fs::write(&file, &text).unwrap();
    let loaded = store.load(source).unwrap();
    assert_eq!(loaded.entries, [a.clone(), b.clone()]);
    let aside = loaded.broken.expect("kept aside");
    assert_eq!(aside, file.with_file_name(format!("{source}.toml.broken")));
    assert_eq!(std::fs::read_to_string(&aside).unwrap(), text, "never written over");
    // The good blocks written again: read as they are now.
    assert_eq!(store.load(source).unwrap(), Loaded { entries: vec![a, b], broken: None });
    // Broken again: kept beside the first, not over it.
    std::fs::write(&file, "not = [toml").unwrap();
    let loaded = store.load(source).unwrap();
    assert!(loaded.entries.is_empty());
    assert_eq!(loaded.broken, Some(file.with_file_name(format!("{source}.toml.broken-2"))));
    assert_eq!(std::fs::read_to_string(&aside).unwrap(), text);
    assert!(!file.exists(), "nothing readable: no file until the next run");
}

#[test]
fn no_secret_and_no_parameter_value_reach_the_disk() {
    let dir = temp("secrets");
    let store = TomlQueryHistory::new(&dir);
    let source = DataSourceId::new();
    let pg = Flavor::Postgres { version: Some(170_000) };
    let e =
        HistoryEntry::new(source, "ALTER ROLE app PASSWORD 'hunter2'", pg, &ParamStyles::of(pg), 1);
    store.append(source, &e).unwrap();
    // A statement with a parameter keeps it as written (its value was never given here).
    let p = entry(source, "SELECT * FROM users WHERE email = :email", 2);
    store.append(source, &p).unwrap();
    let text = std::fs::read_to_string(store.file(source)).unwrap();
    assert!(!text.contains("hunter2"), "{text}");
    assert!(text.contains(":email"));
    assert!(text.contains("params = [\":email\"]"), "{text}");
}

#[test]
fn an_entry_without_its_id_or_text_is_left_out_not_the_file() {
    let dir = temp("partial");
    let store = TomlQueryHistory::new(&dir);
    let source = DataSourceId::new();
    let file = store.file(source);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(
        &file,
        "[[entry]]\ntext = \"SELECT 1\"\n\n[[entry]]\nid = \"7c1e5a2b-9d3f-4e6a-8b0c-2f4d6e8a0b1c\"\ntext = \"SELECT 2\"\noutcome = \"cancelled\"\n",
    )
    .unwrap();
    let loaded = store.load(source).unwrap();
    assert_eq!(loaded.entries.len(), 1);
    assert_eq!(loaded.entries[0].text, "SELECT 2");
    assert_eq!(loaded.entries[0].outcome, Outcome::Cancelled);
    assert_eq!(loaded.broken, None);
}
