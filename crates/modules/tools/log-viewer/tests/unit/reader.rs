use super::*;
use std::io::Write;

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("log-viewer-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn line(n: usize, level: &str) -> String {
    format!(
        r#"{{"ts":"2026-11-20T08:00:00.000Z","level":"{level}","target":"t","thread":"main","run":"r","msg":"line {n}"}}"#
    )
}

fn msg(e: &Entry) -> String {
    e.cell(crate::entry::MSG).to_owned()
}

/// Only whole lines are known (one being written waits for its end); what's added is
/// walked from where the walk was; a file that shrank is read anew.
#[test]
fn whole_lines_are_walked_as_the_file_grows() {
    let dir = scratch();
    let path = dir.join("app-2026-11-20.jsonl");
    std::fs::write(&path, format!("{}\n{}\n{{\"ts\":", line(0, "INFO"), line(1, "WARN"))).unwrap();
    let mut reader = Reader::new(path.clone());
    reader.grow().unwrap();
    assert_eq!(reader.count(), 2, "the third isn't ended");
    // Its end comes, then more.
    let mut file = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
    write!(file, "\"2026-11-20T08:00:00.000Z\",\"msg\":\"line 2\"}}\n{}\n", line(3, "ERROR"))
        .unwrap();
    drop(file);
    reader.grow().unwrap();
    assert_eq!(reader.count(), 4);
    // The newest first.
    let rows: Vec<String> = reader.block(0, 4).iter().map(msg).collect();
    assert_eq!(rows, ["line 3", "line 2", "line 1", "line 0"]);
    // Shrunk (replaced): read anew.
    std::fs::write(&path, format!("{}\n", line(9, "INFO"))).unwrap();
    reader.grow().unwrap();
    assert_eq!(reader.count(), 1);
    assert_eq!(msg(&reader.block(0, 1)[0]), "line 9");
    let _ = std::fs::remove_dir_all(dir);
}

/// A file far larger than a piece: walked whole, every line known, read by blocks; a block
/// counts from the count the view was told (new lines after don't move it).
#[test]
fn a_big_file_is_walked_in_pieces_and_read_by_blocks() {
    let dir = scratch();
    let path = dir.join("app-2026-11-20.jsonl");
    let text: String = (0..5000).map(|n| line(n, "INFO") + "\n").collect();
    assert!(text.len() > PIECE * 4);
    std::fs::write(&path, &text).unwrap();
    let mut reader = Reader::new(path);
    reader.grow().unwrap();
    assert_eq!(reader.count(), 5000);
    let second = reader.block(1, 5000);
    assert_eq!(second.len() as u64, BLOCK);
    assert_eq!(msg(&second[0]), format!("line {}", 4999 - BLOCK));
    // The view still shows 4000 of them: its rows are those it was told of.
    assert_eq!(msg(&reader.block(0, 4000)[0]), "line 3999");
    let last = reader.block(24, 5000);
    assert_eq!((last.len(), msg(last.last().unwrap()).as_str()), (200, "line 0"));
    let _ = std::fs::remove_dir_all(dir);
}

/// A filter: only the lines that pass, filtered in pieces, the new ones as they come.
#[test]
fn only_the_lines_that_pass_are_counted() {
    use ui_components::data_grid::filter::{Compare, GridFilter};
    use ui_components::data_grid::source::CellValue;
    let dir = scratch();
    let path = dir.join("app-2026-11-20.jsonl");
    let levels = ["INFO", "ERROR", "INFO", "ERROR", "WARN"];
    let text: String = levels.iter().enumerate().map(|(n, l)| line(n, l) + "\n").collect();
    std::fs::write(&path, text).unwrap();
    let mut reader = Reader::new(path);
    reader.grow().unwrap();
    reader.filter = Some(GridFilter::Compare {
        column: "level".into(),
        op: Compare::Eq,
        value: CellValue::Text("ERROR".into()),
    });
    reader.passing = Some(Vec::new());
    assert!(!reader.filter_some(2), "two at a time");
    assert!(reader.filter_some(usize::MAX));
    assert_eq!(reader.count(), 2);
    let rows: Vec<String> = reader.block(0, 2).iter().map(msg).collect();
    assert_eq!(rows, ["line 3", "line 1"]);
    let _ = std::fs::remove_dir_all(dir);
}

/// The files beside one: by day, then part; others in the folder left out.
#[test]
fn the_files_before_and_after_are_found() {
    let dir = scratch();
    for name in [
        "app-2026-11-19.jsonl",
        "app-2026-11-20.jsonl",
        "app-2026-11-20.1.jsonl",
        "notes.txt",
        "app-2026-11-22.jsonl",
    ] {
        std::fs::write(dir.join(name), "").unwrap();
    }
    let (older, newer) = neighbors(&dir.join("app-2026-11-20.jsonl"));
    assert_eq!(older, Some(dir.join("app-2026-11-19.jsonl")));
    assert_eq!(newer, Some(dir.join("app-2026-11-20.1.jsonl")));
    let (older, newer) = neighbors(&dir.join("app-2026-11-22.jsonl"));
    assert_eq!((older, newer), (Some(dir.join("app-2026-11-20.1.jsonl")), None));
    let _ = std::fs::remove_dir_all(dir);
}

/// Filtered a piece of the file at a time, a line longer than a piece read alone: what
/// passes is what each line read by itself passes, CRLF ends too.
#[test]
fn the_filter_reads_the_file_in_pieces_as_line_by_line() {
    use ui_components::data_grid::filter::{Compare, GridFilter};
    use ui_components::data_grid::source::CellValue;
    let dir = scratch();
    let path = dir.join("app-2026-11-21.jsonl");
    let long = "x".repeat(FILTER_PIECE as usize + 10);
    let mut text = String::new();
    for n in 0..30_000 {
        let level = if n % 7 == 0 { "ERROR" } else { "INFO" };
        text += &line(n, level);
        text += if n % 2 == 0 { "\n" } else { "\r\n" };
        if n == 12_000 {
            text += &format!("{{\"level\":\"ERROR\",\"msg\":\"{long}\"}}\n");
        }
    }
    std::fs::write(&path, text).unwrap();
    let mut reader = Reader::new(path);
    reader.grow().unwrap();
    reader.filter = Some(GridFilter::Compare {
        column: "level".into(),
        op: Compare::Eq,
        value: CellValue::Text("ERROR".into()),
    });
    // Each line read by itself, as the filter did before.
    let lines = reader.starts.len();
    let one_by_one: Vec<u64> = (0..lines)
        .filter(|&n| {
            reader.line(n).is_some_and(|l| entry::passes(&entry::parse(&l), reader.filter.as_ref()))
        })
        .map(|n| n as u64)
        .collect();
    reader.passing = Some(Vec::new());
    while !reader.filter_some(5_000) {}
    assert_eq!(reader.passing.as_deref(), Some(&one_by_one[..]));
    assert!(one_by_one.len() > 4_000);
    let _ = std::fs::remove_dir_all(dir);
}
