use super::*;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "grid-export-{name}-{}-{}",
        std::process::id(),
        kernel::id::new_uuid()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn id_of(path: &str, chosen: &ExportFormat) -> Option<String> {
    format_of(Path::new(path), None, chosen).map(|f| f.id().to_owned())
}

#[test]
fn a_file_s_name_says_its_format() {
    let chosen = ExportFormat::find(None, "json").unwrap();
    assert_eq!(id_of("a/rows.CSV", &chosen).as_deref(), Some("csv"));
    assert_eq!(id_of("rows.tab", &chosen).as_deref(), Some("tsv-header"));
    assert_eq!(id_of("rows.markdown", &chosen).as_deref(), Some("markdown"));
    assert_eq!(id_of("rows.html", &chosen).as_deref(), Some("html"));
    assert_eq!(id_of("rows.xlsx", &chosen).as_deref(), Some("xlsx"));
    assert_eq!(id_of("rows.bin", &chosen), None);
    // The chosen extractor's own extension keeps it (SQL UPDATEs into a .sql file).
    let updates = ExportFormat::find(None, "sql-update").unwrap();
    assert_eq!(id_of("rows.sql", &updates).as_deref(), Some("sql-update"));
    assert_eq!(id_of("rows.sql", &chosen).as_deref(), Some("sql"));
    assert_eq!(updates.extension(), "sql");
    assert_eq!(ExportFormat::Xlsx.extension(), "xlsx");
    assert!(!ExportFormat::Xlsx.is_text());
}

#[test]
fn the_workbook_is_offered_only_for_a_file() {
    let file: Vec<String> =
        ExportFormat::offered(None, false).iter().map(|f| f.id().to_owned()).collect();
    let clipboard: Vec<String> =
        ExportFormat::offered(None, true).iter().map(|f| f.id().to_owned()).collect();
    assert_eq!(file.last().map(String::as_str), Some(XLSX));
    assert!(!clipboard.contains(&XLSX.to_owned()));
    assert_eq!(file.len(), clipboard.len() + 1);
}

#[test]
fn a_writer_writes_text_or_a_workbook() {
    let columns = vec![GridColumn::new("a", "TEXT", super::super::CellClass::Text)];
    let csv = ExportFormat::find(None, "csv").unwrap();
    let mut w = ExportWriter::new(&csv, "t", &columns, None, Options::default());
    let mut all = w.begin().unwrap();
    all.extend(w.rows(&[vec![CellRef::Text("x")]]).unwrap());
    all.extend(w.finish().unwrap());
    assert_eq!(String::from_utf8(all).unwrap(), "a\nx");
    assert!(w.is_text());
    let mut x = ExportWriter::new(&ExportFormat::Xlsx, "t", &columns, None, Options::default());
    let start = x.begin().unwrap();
    assert_eq!(&start[..4], b"PK\x03\x04");
    x.rows(&[vec![CellRef::Text("x")]]).unwrap();
    assert_eq!(x.written(), 1);
    assert!(!x.is_text());
}

/// The text goes to a hidden file beside the target, renamed over it once complete; given
/// up, the target is left as it was.
#[test]
fn a_file_is_replaced_only_once_complete() {
    let dir = scratch("file");
    let target = dir.join("rows.csv");
    std::fs::write(&target, "old").unwrap();
    let mut sink = ExportSink::open(&Target::File(target.clone())).unwrap();
    sink.write("a,b\n1,2").unwrap();
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "old");
    sink.discard();
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "old");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "no temporary file left");
    let mut sink = ExportSink::open(&Target::File(target.clone())).unwrap();
    sink.write("a,b\n1,2").unwrap();
    let outcome = sink.complete(b"", 1, true);
    assert_eq!(outcome, ExportOutcome::Written { rows: 1, path: target.clone() });
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "a,b\n1,2\n");
    let (told, copied) = outcome.told();
    assert!(told.title.contains("rows.csv"), "{}", told.title);
    assert!(copied.is_none());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_clipboard_takes_a_limited_text() {
    let mut sink = ExportSink::open(&Target::Clipboard).unwrap();
    sink.write("x").unwrap();
    assert!(sink.write(&"y".repeat(CLIPBOARD_BYTES)).is_err());
    let (_, copied) = sink.complete(b"!", 1, true).told();
    assert_eq!(copied.as_deref(), Some("x!"));
}
