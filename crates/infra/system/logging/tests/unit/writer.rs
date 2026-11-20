use super::*;
use std::sync::mpsc;

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("logging-writer-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 2026-11-20 08:10 UTC, and the day after.
const DAY: i64 = 1_795_162_200_000;
const NEXT_DAY: i64 = DAY + 86_400_000;

fn written(dir: &Path) -> Vec<(String, String)> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| {
            (
                e.file_name().to_string_lossy().into_owned(),
                std::fs::read_to_string(e.path()).unwrap(),
            )
        })
        .collect();
    files.sort();
    files
}

use std::path::Path;

/// Lines go into the day's file, one a line; a flush writes them before it answers; a full
/// file continues in the next; a new day opens its own.
#[test]
fn lines_go_into_the_day_s_files_turned_over_by_size_and_day() {
    let dir = scratch();
    let (tx, rx) = mpsc::sync_channel(64);
    let sinks = Sinks {
        dir: Some(dir.clone()),
        console: false,
        limits: Limits { file: 30, ..Limits::default() },
        run: "r1".into(),
    };
    let thread = std::thread::spawn(move || run(&rx, &sinks));
    let line = |text: &str, at_ms: i64| Msg::Line { text: text.into(), at_ms, urgent: false };
    tx.send(line("{\"n\":1}", DAY)).unwrap();
    tx.send(line("{\"n\":2}", DAY)).unwrap();
    let (done, wait) = mpsc::sync_channel(1);
    tx.send(Msg::Flush(done)).unwrap();
    wait.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(written(&dir), [("app-2026-11-20.jsonl".into(), "{\"n\":1}\n{\"n\":2}\n".into())]);
    // Past 30 bytes: the next file; then a new day.
    tx.send(line("{\"n\":3,\"pad\":\"xxxxxxxx\"}", DAY)).unwrap();
    tx.send(line("{\"n\":4}", NEXT_DAY)).unwrap();
    drop(tx);
    thread.join().unwrap();
    let files = written(&dir);
    let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["app-2026-11-20.1.jsonl", "app-2026-11-20.jsonl", "app-2026-11-21.jsonl"]);
    assert!(files[0].1.contains("\"n\":3"));
    assert_eq!(files[2].1, "{\"n\":4}\n");
    let _ = std::fs::remove_dir_all(dir);
}

/// The log off on disk: nothing written, nothing made; a flush still answers.
#[test]
fn without_a_folder_nothing_is_written() {
    let (tx, rx) = mpsc::sync_channel(4);
    let sinks = Sinks { dir: None, console: false, limits: Limits::default(), run: "r".into() };
    let thread = std::thread::spawn(move || run(&rx, &sinks));
    tx.send(Msg::Line { text: "{}".into(), at_ms: DAY, urgent: true }).unwrap();
    let (done, wait) = mpsc::sync_channel(1);
    tx.send(Msg::Flush(done)).unwrap();
    assert!(wait.recv_timeout(Duration::from_secs(2)).is_ok());
    drop(tx);
    thread.join().unwrap();
}

/// Told another folder: the lines go there from then on; told none: nothing on disk.
#[test]
fn the_folder_can_change_while_it_writes() {
    let (first, second) = (scratch(), scratch());
    let (tx, rx) = mpsc::sync_channel(16);
    let sinks = Sinks {
        dir: Some(first.clone()),
        console: false,
        limits: Limits::default(),
        run: "r".into(),
    };
    let thread = std::thread::spawn(move || run(&rx, &sinks));
    let line = |n: u32| Msg::Line { text: format!("{{\"n\":{n}}}"), at_ms: DAY, urgent: false };
    tx.send(line(1)).unwrap();
    tx.send(Msg::Dir(Some(second.clone()))).unwrap();
    tx.send(line(2)).unwrap();
    tx.send(Msg::Dir(None)).unwrap();
    tx.send(line(3)).unwrap();
    drop(tx);
    thread.join().unwrap();
    assert_eq!(written(&first)[0].1, "{\"n\":1}\n");
    assert_eq!(written(&second)[0].1, "{\"n\":2}\n", "the third one nowhere");
    let _ = std::fs::remove_dir_all(first);
    let _ = std::fs::remove_dir_all(second);
}
