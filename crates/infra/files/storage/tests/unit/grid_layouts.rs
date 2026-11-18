//! The grid layouts' file: written and read back the same (widths, order, hidden, sort,
//! filter, page size, schema), a missing or broken file read as none.

use super::*;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("grid-layouts-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

#[test]
fn layouts_are_written_and_read_back() {
    let dir = temp("round");
    let store = TomlGridLayouts::new(&dir);
    assert!(store.load().is_empty(), "no file: none");
    let full = GridLayout {
        widths: vec![("id".into(), 64.0), ("na\"me".into(), 120.0)],
        order: vec!["na\"me".into(), "id".into()],
        hidden: vec!["notes".into()],
        sort: vec![Order { column: "id".into(), descending: true }],
        filter: Some("id > 3 AND \"na\"\"me\" LIKE '%a%'".into()),
        page: Some(Some(100)),
    };
    let layouts = vec![
        (
            LayoutKey { source: "data/a.sqlite".into(), table: TableRef::in_schema("main", "t") },
            full,
        ),
        (
            LayoutKey { source: "b.csv".into(), table: TableRef::new("b") },
            GridLayout { page: Some(None), ..GridLayout::default() },
        ),
    ];
    let saved = store.save(&layouts);
    let loaded = store.load();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(saved.is_ok(), "{saved:?}");
    assert_eq!(loaded, layouts);
}

#[test]
fn a_broken_file_is_read_as_none() {
    let dir = temp("broken");
    let store = TomlGridLayouts::new(&dir);
    let _ = std::fs::create_dir_all(dir.join(kernel::brand::PROJECT_DIR));
    let _ = std::fs::write(store.file(), "[[grid]\nsource = ");
    let loaded = store.load();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(loaded.is_empty());
}
