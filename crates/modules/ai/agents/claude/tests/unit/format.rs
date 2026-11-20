use super::*;

fn line(value: serde_json::Value) -> String {
    format!("{value}\n")
}

/// The project's folder as Claude Code names it: every character that isn't a letter or a
/// digit a dash.
#[test]
fn a_project_s_folder_is_named_as_claude_names_it() {
    assert_eq!(folder_name("/Users/a/www/api.shop.ro"), "-Users-a-www-api-shop-ro");
    assert_eq!(folder_name("C:\\work\\my app"), "C--work-my-app");
}

/// The name the user gave wins over Claude's, Claude's over the last request, that over the
/// first one typed; the latest of each; the id, folder and start from the first rows.
#[test]
fn a_session_s_name_and_start_are_read() {
    let mut text = String::new();
    text += &line(serde_json::json!({"type": "mode", "sessionId": "s-1"}));
    text += &line(serde_json::json!({"type": "user", "sessionId": "s-1", "cwd": "/p",
        "timestamp": "2026-11-13T14:55:21.690Z", "isMeta": true,
        "message": {"content": "<command-name>/clear</command-name>"}}));
    text += &line(serde_json::json!({"type": "user", "sessionId": "s-1", "cwd": "/p",
        "timestamp": "2026-11-13T14:56:00Z",
        "message": {"content": [{"type": "text", "text": "  Fix   the\nlogin  "}]}}));
    let facts = facts(text.as_bytes(), &[], true);
    assert_eq!(facts.id.as_deref(), Some("s-1"));
    assert_eq!(facts.cwds, ["/p"]);
    assert_eq!(facts.started, Some(1_794_581_721));
    assert_eq!(facts.title.as_deref(), Some("Fix the login"), "the first one typed");
    text += &line(serde_json::json!({"type": "last-prompt", "lastPrompt": "and the tests"}));
    assert_eq!(facts_of(&text).title.as_deref(), Some("and the tests"));
    text += &line(serde_json::json!({"type": "ai-title", "aiTitle": "Login fix"}));
    text += &line(serde_json::json!({"type": "ai-title", "aiTitle": "Login fix and tests"}));
    assert_eq!(facts_of(&text).title.as_deref(), Some("Login fix and tests"), "the latest");
    text += &line(serde_json::json!({"type": "custom-title", "customTitle": "matching-system"}));
    text += &line(serde_json::json!({"type": "ai-title", "aiTitle": "Later"}));
    assert_eq!(facts_of(&text).title.as_deref(), Some("matching-system"), "the user's");
}

fn facts_of(text: &str) -> Facts {
    facts(text.as_bytes(), &[], true)
}

/// A long file read from its ends: the lines cut at either edge are skipped, the names at
/// its end are the ones taken; a long name is cut with "…".
#[test]
fn a_long_session_is_read_from_its_ends() {
    let start = line(serde_json::json!({"type": "user", "sessionId": "s-2", "cwd": "/p",
        "timestamp": "2026-11-12T08:00:00Z", "message": {"content": "first"}}));
    let head = format!("{start}{{\"type\":\"user\",\"mess");
    let long = "word ".repeat(40);
    let tail = format!(
        "ge\":\"cut\"}}\n{}{}",
        line(serde_json::json!({"type": "ai-title", "aiTitle": "Old"})),
        line(serde_json::json!({"type": "ai-title", "aiTitle": long})),
    );
    let facts = facts(head.as_bytes(), tail.as_bytes(), false);
    assert_eq!(facts.id.as_deref(), Some("s-2"));
    let title = facts.title.unwrap();
    assert!(title.ends_with('…') && title.chars().count() <= 80, "{title}");
    assert!(title.starts_with("word word"));
}

/// An id read from a file is used only as it may be named on a command line: letters,
/// digits, `-` and `_`; one with anything else isn't taken.
#[test]
fn a_hostile_id_is_refused() {
    assert!(valid_id("5bb0b221-1d4e-4a5b-9c1e-0a1b2c3d4e5f"));
    for bad in ["", "a&calc", "x|y", "q\"r", "../up", &"a".repeat(65)] {
        assert!(!valid_id(bad), "{bad}");
    }
    let row = serde_json::json!({"type": "user", "sessionId": "x & del", "cwd": "/p"});
    assert_eq!(facts(format!("{row}\n").as_bytes(), &[], true).id, None);
}

/// A text field found in a piece of a line (its escapes undone); cut before its end: not.
#[test]
fn a_text_field_is_read_from_a_piece_of_a_line() {
    let line = br#"{"a":"x","cwd":"/work/a \"b\"","timestamp":"2026-11-12T08:00:00Z"}"#;
    assert_eq!(text_field(line, "cwd").as_deref(), Some("/work/a \"b\""));
    assert_eq!(text_field(&line[..20], "cwd"), None, "cut: not whole");
    let mut facts = Facts::default();
    assert!(deep_facts(&mut facts, line));
    assert_eq!(facts.cwds, ["/work/a \"b\""]);
}
