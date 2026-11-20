use super::*;

/// UTC to the millisecond, the epoch and a day of this year, a leap day too.
#[test]
fn a_moment_is_written_in_utc_to_the_millisecond() {
    assert_eq!(timestamp(0), "1970-01-01T00:00:00.000Z");
    assert_eq!(timestamp(1_795_162_200_123), "2026-11-20T08:10:00.123Z");
    assert_eq!(timestamp(951_782_400_000), "2000-02-29T00:00:00.000Z");
    assert_eq!(day_name(1_795_162_200_123), "2026-11-20");
    assert_eq!(timestamp(-1), "1969-12-31T23:59:59.999Z");
}

/// A duration's `Debug` read as milliseconds; anything else isn't one.
#[test]
fn a_duration_s_text_is_read_in_milliseconds() {
    assert_eq!(duration_ms("12.5ms"), Some(12.5));
    assert_eq!(duration_ms("1.5s"), Some(1500.0));
    assert_eq!(duration_ms("40µs"), Some(0.04));
    assert_eq!(duration_ms("7ns"), Some(0.0));
    for not in ["ms", "abc", "12 ms", "-1ms", "1e3s", "users"] {
        assert_eq!(duration_ms(not), None, "{not}");
    }
}

/// A long text cut at a character's edge, its length said; a short one as it was.
#[test]
fn a_long_text_is_cut_at_a_character_s_edge() {
    assert_eq!(cut("short".into()), "short");
    let long = "ă".repeat(MAX_TEXT);
    let cut = cut(long.clone());
    assert!(cut.len() < long.len());
    assert!(cut.ends_with(&format!("… ({} bytes)", long.len())), "{}", &cut[cut.len() - 30..]);
}

fn head() -> Head<'static> {
    Head { at_ms: 0, level: "INFO", target: "app::x", thread: "main", run: "abc" }
}

/// A line: one JSON object, its fields with their types, quotes and control characters
/// escaped, no newline in it.
#[test]
fn a_line_is_one_json_object_with_typed_fields() {
    let mut fields = Fields { message: Some("said \"hi\"\nthen".into()), ..Fields::default() };
    fields.fields.insert("count".into(), Value::from(3));
    fields.fields.insert("ok".into(), Value::Bool(true));
    let text = line(head(), fields, vec![serde_json::json!({"name": "project"})]);
    assert!(!text.contains('\n'));
    assert!(text.starts_with("{\"ts\":\"1970-01-01T00:00:00.000Z\",\"level\":\"INFO\""), "{text}");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["msg"], "said \"hi\"\nthen");
    assert_eq!(v["fields"]["count"], 3);
    assert_eq!(v["fields"]["ok"], true);
    assert_eq!(v["span"][0]["name"], "project");
    assert_eq!((v["level"].as_str(), v["run"].as_str()), (Some("INFO"), Some("abc")));
    // No fields, no spans: neither written.
    let bare: Value = serde_json::from_str(&line(head(), Fields::default(), Vec::new())).unwrap();
    assert!(bare.get("fields").is_none() && bare.get("span").is_none());
}

/// Many long fields: the line still within its bound, saying how long it was.
#[test]
fn a_line_too_long_says_so_within_its_bound() {
    let mut fields = Fields::default();
    for i in 0..8 {
        fields.fields.insert(format!("f{i}"), Value::String("x".repeat(MAX_TEXT)));
    }
    let text = line(head(), fields, Vec::new());
    assert!(text.len() <= MAX_LINE);
    let v: Value = serde_json::from_str(&text).unwrap();
    assert!(v["msg"].as_str().unwrap().contains("too long"));
}
