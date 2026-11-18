use super::*;

/// A fake page's conversation: what it sends, what it's sent.
fn sent(effects: &[Effect]) -> Vec<Value> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Send(text) => Some(serde_json::from_str(text).unwrap()),
            _ => None,
        })
        .collect()
}

fn session() -> Session {
    Session::new("flow.drawio", json!({"css": "x"}))
}

#[test]
fn the_page_is_configured_then_given_the_file_once_it_is_read() {
    let mut s = session();
    let configure = s.page_message(r#"{"event":"configure"}"#);
    assert_eq!(sent(&configure), vec![json!({"action": "configure", "config": {"css": "x"}})]);
    // Asked before the file is read: given it when it is.
    assert!(s.page_message(r#"{"event":"init"}"#).is_empty());
    assert!(s.ready());
    let loaded = sent(&s.file_read(Arc::from("<mxfile/>")));
    assert_eq!(loaded[0]["action"], "load");
    assert_eq!(loaded[0]["xml"], "<mxfile/>");
    assert_eq!(loaded[0]["autosave"], 1);
    assert_eq!(loaded[0]["title"], "flow.drawio");
    assert_eq!(loaded[0]["exportProtocol"], true);
    // The page made anew asks again: given what's known now.
    s.new_page();
    assert!(!s.ready());
    s.page_message(r#"{"event":"configure"}"#);
    let again = sent(&s.page_message(r#"{"event":"init"}"#));
    assert_eq!(again[0]["xml"], "<mxfile/>");
}

#[test]
fn saves_are_written_one_at_a_time_the_latest_last() {
    let mut s = session();
    s.page_message(r#"{"event":"init"}"#);
    s.file_read(Arc::from("a"));
    assert!(!s.dirty());
    let first = s.page_message(r#"{"event":"autosave","xml":"b"}"#);
    assert_eq!(first, vec![Effect::Write(Arc::from("b"))]);
    assert!(s.dirty());
    // Two more while it's written: only the last waits.
    assert!(s.page_message(r#"{"event":"autosave","xml":"c"}"#).is_empty());
    assert!(s.page_message(r#"{"event":"save","xml":"d"}"#).is_empty());
    let next = s.written(&Arc::from("b"), true);
    assert_eq!(next, vec![Effect::Write(Arc::from("d"))]);
    assert!(s.dirty());
    let done = s.written(&Arc::from("d"), true);
    assert_eq!(sent(&done), vec![json!({"action": "status", "modified": false})]);
    assert!(!s.dirty());
    // The same text again: nothing to write.
    assert!(s.page_message(r#"{"event":"autosave","xml":"d"}"#).is_empty());
}

#[test]
fn a_failed_write_stays_unsaved_until_the_next_one() {
    let mut s = session();
    s.page_message(r#"{"event":"init"}"#);
    s.file_read(Arc::from("a"));
    s.page_message(r#"{"event":"autosave","xml":"b"}"#);
    assert!(s.written(&Arc::from("b"), false).is_empty());
    assert!(s.dirty());
    // The same text is written again (it isn't on disk).
    assert_eq!(
        s.page_message(r#"{"event":"autosave","xml":"b"}"#),
        vec![Effect::Write(Arc::from("b"))]
    );
    s.written(&Arc::from("b"), true);
    assert!(!s.dirty());
}

#[test]
fn a_change_on_disk_is_merged_only_when_nothing_is_unsaved() {
    let mut s = session();
    s.page_message(r#"{"event":"init"}"#);
    s.file_read(Arc::from("a"));
    // Our own write, heard back: nothing.
    s.page_message(r#"{"event":"autosave","xml":"b"}"#);
    s.written(&Arc::from("b"), true);
    assert!(s.disk_changed(Arc::from("b")).is_empty());
    // Someone else's: loaded again.
    let loaded = sent(&s.disk_changed(Arc::from("c")));
    assert_eq!(loaded[0]["action"], "load");
    assert_eq!(loaded[0]["xml"], "c");
    assert_eq!(s.xml().map(|x| &**x), Some("c"));
    // While a write is under way: what's here is kept.
    s.page_message(r#"{"event":"autosave","xml":"d"}"#);
    assert!(s.disk_changed(Arc::from("e")).is_empty());
    assert_eq!(s.xml().map(|x| &**x), Some("d"));
}

#[test]
fn exit_and_links_and_nonsense() {
    let mut s = session();
    assert_eq!(s.page_message(r#"{"event":"exit"}"#), vec![Effect::Close]);
    assert_eq!(
        s.page_message(r#"{"event":"openLink","href":"https://www.drawio.com/doc"}"#),
        vec![Effect::OpenLink("https://www.drawio.com/doc".into())]
    );
    for refused in [
        r#"{"event":"openLink","href":"javascript:alert(1)"}"#,
        r#"{"event":"openLink","href":"file:///etc/passwd"}"#,
        r#"{"event":"openLink"}"#,
        "not json",
        "[1,2]",
        r#"{"event":42}"#,
        r#"{"event":"autosave"}"#,
        r#"{"event":"unknown"}"#,
        "",
    ] {
        assert!(s.page_message(refused).is_empty(), "{refused}");
    }
    let huge = format!(r#"{{"event":"exit","pad":"{}"}}"#, "x".repeat(MAX_MESSAGE));
    assert!(s.page_message(&huge).is_empty());
    assert!(web_link("http://example.com"));
    assert!(!web_link("https://a\nb"));
    assert!(!web_link("mailto:a@b.c"));
}

#[test]
fn a_save_and_exit_closes_after_writing() {
    let mut s = session();
    s.file_read(Arc::from("a"));
    let effects = s.page_message(r#"{"event":"save","xml":"b","exit":true}"#);
    assert_eq!(effects, vec![Effect::Write(Arc::from("b")), Effect::Close]);
}

#[test]
fn an_export_comes_as_bytes_to_save() {
    let mut s = session();
    let png = s.page_message(
        r#"{"event":"export","format":"png","filename":"../../flow.png","data":"data:image/png;base64,iVBORw0KGgo="}"#,
    );
    assert_eq!(
        png,
        vec![Effect::SaveAs {
            name: "flow.png".into(),
            bytes: vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]
        }]
    );
    let svg = s.page_message(r#"{"event":"export","format":"svg","filename":"","data":"<svg/>"}"#);
    assert_eq!(svg, vec![Effect::SaveAs { name: "diagram.svg".into(), bytes: b"<svg/>".to_vec() }]);
    assert!(s.page_message(r#"{"event":"export","data":"data:x;base64,!!"}"#).is_empty());
    assert_eq!(file_name("a\\b:c?.xml", "xml"), "bc.xml");
    assert_eq!(file_name("...", "x/y"), "diagram.xy");
    assert_eq!(base64_decode("TWFu").unwrap(), b"Man");
    assert_eq!(base64_decode("TQ==").unwrap(), b"M");
}

/// A write that found the file changed by someone else: what waited is let go, the page is
/// given the file as it is now; nothing's left unsaved. The latest text is what a save at
/// closing writes.
#[test]
fn a_conflict_gives_the_page_the_file_as_it_is() {
    let mut s = session();
    s.page_message(r#"{"event":"init"}"#);
    s.file_read(Arc::from("<a/>"));
    assert_eq!(s.unsaved(), None, "as read");
    let first = s.page_message(r#"{"event":"autosave","xml":"<b/>"}"#);
    assert_eq!(first, vec![Effect::Write(Arc::from("<b/>"))]);
    s.page_message(r#"{"event":"autosave","xml":"<c/>"}"#);
    assert_eq!(s.unsaved().as_deref(), Some("<c/>"), "the latest");
    let loaded = s.conflict(Arc::from("<theirs/>"));
    assert_eq!(sent(&loaded)[0]["xml"], "<theirs/>");
    assert!(!s.dirty());
    assert_eq!(s.on_disk().as_deref(), Some("<theirs/>"));
    assert_eq!(s.unsaved(), None);
}
