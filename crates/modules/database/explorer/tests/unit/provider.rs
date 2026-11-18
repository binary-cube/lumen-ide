use std::path::PathBuf;

use super::*;

fn files(text: bool) -> DataFiles {
    DataFiles::new("test-data", &["tdat"], 4, |h| h.starts_with(b"TDAT"), text)
}

#[test]
fn a_binary_data_file_replaces_its_bytes_and_a_text_one_is_a_preview() {
    assert_eq!(kind_of(&files(false), None), ViewerKind::Replace);
    assert_eq!(kind_of(&files(true), Some(TextCheck::Text)), ViewerKind::Preview);
    // Too large for the editor: the data alone.
    assert_eq!(kind_of(&files(true), Some(TextCheck::TooLarge)), ViewerKind::Replace);
}

#[test]
fn only_files_a_driver_recognizes_open_as_data() {
    database::drivers::register_files(files(false));
    let dir = std::env::temp_dir().join(format!("data-provider-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let good: PathBuf = dir.join("a.tdat");
    let other: PathBuf = dir.join("b.tdat");
    let named: PathBuf = dir.join("c.txt");
    std::fs::write(&good, b"TDAT and more").unwrap();
    std::fs::write(&other, b"nope").unwrap();
    std::fs::write(&named, b"TDAT").unwrap();
    let provider = DataViewProvider;
    assert_eq!(provider.kind(&good), Some(ViewerKind::Replace));
    assert_eq!(provider.kind(&other), None, "the first bytes decide");
    assert_eq!(provider.kind(&named), None, "the extension first");
    assert!(provider.open(&good).is_ok());
    assert!(provider.open(&other).is_err());
    let _ = std::fs::remove_dir_all(dir);
}
