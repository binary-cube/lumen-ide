//! The bar over a `.http` file: the request at the cursor found from the text read — a
//! small file's at once, a big one's on a worker (the one before serving meanwhile).

use super::*;

fn bar() -> HttpBar {
    let runs = Runs::of(Some(kernel::ProjectId::new()), None);
    HttpBar::new(runs, Path::new("/p/a.http"), None, Arc::new(|| {}))
}

fn state(text: &str, version: u64, cursor: usize) -> EditorState {
    EditorState { text: Arc::from(text), version, cursor, selection: None }
}

#[test]
fn a_small_file_is_read_at_once() {
    let mut bar = bar();
    let text = "### A\nGET https://e.com/a\n\n### B\nGET https://e.com/b\n";
    bar.sync_editor(&state(text, 1, text.find("e.com/b").unwrap()));
    assert_eq!(bar.parsed_version, 1);
    let current = bar.current().unwrap();
    assert!(text[current].contains("/b"));
    assert!(!bar.parsing.is_busy());
}

#[test]
fn a_big_file_is_read_on_a_worker() {
    let mut bar = bar();
    let body = "x".repeat(PARSED_HERE + 1);
    let text = format!("### A\nPOST https://e.com/a\n\n{body}\n\n### B\nGET https://e.com/b\n");
    bar.sync_editor(&state(&text, 1, text.find("e.com/b").unwrap()));
    assert_ne!(bar.parsed_version, 1, "not read here");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while bar.parsed_version != 1 && std::time::Instant::now() < deadline {
        bar.poll();
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(bar.parsed_version, 1, "read on its worker, taken by a poll");
    let current = bar.current().unwrap();
    assert!(text[current].contains("/b"));
    // An edit meanwhile: the read before still serves, never cut where the text isn't.
    let shorter = format!("é{}", &text[1..]);
    bar.sync_editor(&state(&shorter, 2, shorter.len() - 1));
    assert_eq!(bar.parsed_version, 1, "the read before serves until the new one's read");
}
