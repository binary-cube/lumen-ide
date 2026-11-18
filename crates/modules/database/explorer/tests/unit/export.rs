//! The formats a file's name asks for, and the extensions given.

use std::path::Path;

use super::*;

#[test]
fn a_files_extension_names_its_format() {
    let chosen = ExportFormat::find(None, "tsv").unwrap();
    let id = |path: &str| format_of(Path::new(path), None, &chosen).map(|f| f.id().to_owned());
    assert_eq!(id("a.CSV").as_deref(), Some("csv"));
    // The chosen one's own extension keeps it (TSV without its header).
    assert_eq!(id("a.tsv").as_deref(), Some("tsv"));
    assert_eq!(id("a.tab").as_deref(), Some("tsv-header"));
    assert_eq!(id("a.json").as_deref(), Some("json"));
    assert_eq!(id("a.sql").as_deref(), Some("sql"));
    assert_eq!(id("a.markdown").as_deref(), Some("markdown"));
    assert_eq!(id("a.xlsx").as_deref(), Some("xlsx"));
    assert_eq!(id("a.bin"), None);
    for format in ExportFormat::offered(None, false) {
        let name = format!("x.{}", format.extension());
        assert_eq!(
            format_of(Path::new(&name), None, &format).map(|f| f.id().to_owned()).as_deref(),
            Some(format.id())
        );
    }
}

#[test]
fn the_temporary_file_is_hidden_beside_its_target() {
    let target = Path::new("/out/rows.csv");
    let temp = temporary(target);
    assert_eq!(temp.parent(), target.parent());
    let name = temp.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    assert!(name.starts_with(".rows.csv.") && name.ends_with(".part"), "{name}");
    assert_ne!(temporary(target), temp, "a new name each time");
}
