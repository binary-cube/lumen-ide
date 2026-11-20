use super::*;

/// The log's own names read (a day, a part, another instance's); anything else isn't one.
#[test]
fn a_log_file_is_known_by_its_name() {
    assert_eq!(log_name("app-2026-11-20.jsonl"), Some(("2026-11-20", 0)));
    assert_eq!(log_name("app-2026-11-20.3.jsonl"), Some(("2026-11-20", 3)));
    assert_eq!(log_name("app-2026-11-20-6f1c2a3b4d5e.jsonl"), Some(("2026-11-20", 0)));
    assert_eq!(log_name("app-2026-11-20-6f1c2a3b4d5e.1.jsonl"), Some(("2026-11-20", 1)));
    for not in [
        "app-2026-11-20.json",
        "data.jsonl",
        "app-20261120.jsonl",
        "app-2026-11-20-x!.jsonl",
        "app-2026-11-20.a.jsonl",
    ] {
        assert_eq!(log_name(not), None, "{not}");
    }
    assert_eq!(LogFiles.kind(Path::new("/l/app-2026-11-20.jsonl")), Some(ViewerKind::Replace));
    assert_eq!(
        LogFiles.kind(Path::new("/p/events.jsonl")),
        None,
        "another JSON lines file stays text"
    );
}
