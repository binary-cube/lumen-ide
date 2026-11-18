use super::*;

#[test]
fn dirty_tracking_follows_undo() {
    let mut d = Document::new("abc");
    assert!(!d.is_dirty());
    d.apply(Transaction::single(3..3, "d"), None, EditKind::Typing);
    assert!(d.is_dirty());
    assert_eq!(d.selections().primary(), Selection::cursor(0));
    d.mark_saved(PathBuf::from("/tmp/x.php"));
    assert!(!d.is_dirty());
    d.undo();
    assert!(d.is_dirty());
    assert_eq!(d.buffer().to_string(), "abc");
}

#[test]
fn change_log_covers_edits_and_undo() {
    let mut d = Document::new("abc");
    let v0 = d.version();
    d.apply(Transaction::single(0..0, "x"), None, EditKind::Command);
    d.apply(Transaction::single(4..4, "y"), None, EditKind::Command);
    let changes = d.changes_since(v0).unwrap();
    assert_eq!(changes.len(), 2);
    assert_eq!((changes[1].start, changes[1].new_end), (4, 5));
    let v2 = d.version();
    d.undo();
    assert_eq!(d.changes_since(v2).unwrap().len(), 1);
    assert_eq!(d.changes_since(d.version()).unwrap().len(), 0);
    assert!(d.changes_since(d.version() + 5).is_none());
}

#[test]
fn detects_tab_indentation() {
    let d = Document::new("class A {\n\tfunction b() {}\n\tfunction c() {}\n}");
    assert_eq!(d.indent(), Indent::Tabs);
}

#[test]
fn a_file_changed_on_disk_reloads_keeping_the_cursor_and_counts_as_saved() {
    use crate::domain::encoding::decode;
    let mut doc = Document::new("first\nsecond\nthird\n");
    doc.mark_saved("/p/a.txt".into());
    // The cursor in "third".
    doc.set_selections(Selections::single(Selection::cursor(15)));
    assert!(doc.reload(decode(b"first\nSECOND, from the server\nthird\n")));
    assert_eq!(doc.buffer().to_string(), "first\nSECOND, from the server\nthird\n");
    assert!(!doc.is_dirty(), "what's on disk now");
    assert_eq!(
        doc.selections().primary().head,
        15 + "SECOND, from the server".len() - "second".len(),
        "still in \"third\""
    );
    // One undo step back to the previous text.
    assert!(doc.undo());
    assert_eq!(doc.buffer().to_string(), "first\nsecond\nthird\n");
    // The same text: nothing changes; CRLF on disk is read as the editor keeps it.
    let mut same = Document::new("a\nb\n");
    assert!(!same.reload(decode(b"a\r\nb\r\n")));
    assert_eq!(same.line_ending(), crate::domain::encoding::LineEnding::CrLf);
    // Multi-byte text around the change.
    let mut accents = Document::new("ăîșț");
    assert!(accents.reload(decode("ăîxșț".as_bytes())));
    assert_eq!(accents.buffer().to_string(), "ăîxșț");
}

/// An indent of 0 spaces can't be set (Tab would divide by zero): at least 1.
#[test]
fn an_indent_has_at_least_one_space() {
    let mut doc = Document::new("x");
    doc.set_indent(Indent::Spaces(0));
    assert_eq!(doc.indent(), Indent::Spaces(1));
}

#[test]
fn a_locked_document_refuses_edits_but_takes_the_disk() {
    let mut d = Document::new("abc");
    d.apply(Transaction::single(3..3, "d"), None, EditKind::Typing);
    d.set_read_only(true);
    let version = d.version();
    d.apply(Transaction::single(0..0, "x"), None, EditKind::Typing);
    assert!(!d.undo() && !d.redo());
    assert_eq!((d.buffer().to_string().as_str(), d.version()), ("abcd", version));
    assert!(d.take_refused() && !d.take_refused(), "told once");
    // The cursors still move.
    let moved = Some(Selections::single(Selection::cursor(2)));
    d.apply(Transaction::new(Vec::new()), moved, EditKind::Command);
    assert_eq!(d.selections().primary(), Selection::cursor(2));
    assert!(!d.take_refused());
    // The file changed on disk: its text comes in.
    let decoded =
        Decoded { text: "new".into(), encoding: Encoding::Utf8, line_ending: LineEnding::Lf };
    assert!(d.reload(decoded));
    assert_eq!(d.buffer().to_string(), "new");
    assert!(!d.take_refused());
    d.set_read_only(false);
    d.apply(Transaction::single(0..0, "x"), None, EditKind::Typing);
    assert_eq!(d.buffer().to_string(), "xnew");
}

/// Versions with many changes (Replace All) keep the log within its size: the oldest leave
/// (asked since one of them: `None`, done whole by whoever asks), the last always stays.
#[test]
fn the_change_log_keeps_within_its_size() {
    let mut d = Document::new(&"x ".repeat(200));
    let replace_all = |d: &mut Document| {
        let edits =
            (0..40).map(|i| crate::domain::edit::Edit::new(i * 2..i * 2 + 1, "y")).collect();
        d.apply(Transaction::new(edits), None, EditKind::Command);
    };
    let v0 = d.version();
    replace_all(&mut d);
    let v1 = d.version();
    replace_all(&mut d);
    assert!(d.changes_since(v0).is_none(), "the oldest left");
    assert_eq!(d.changes_since(v1).map(|c| c.len()), Some(40));
    let edits = (0..100).map(|i| crate::domain::edit::Edit::new(i * 2..i * 2 + 1, "z")).collect();
    let v2 = d.version();
    d.apply(Transaction::new(edits), None, EditKind::Command);
    assert_eq!(d.changes_since(v2).map(|c| c.len()), Some(100), "alone past the size: kept");
}

/// Another terminator or encoding chosen is a change to save (back to the file's own: not);
/// a project's rule applied at opening isn't; a save or a reload makes it the file's.
#[test]
fn a_format_chosen_is_unsaved_until_saved() {
    let mut doc = Document::new("a\nb\n");
    doc.set_line_ending(LineEnding::CrLf);
    assert!(!doc.is_dirty(), "the project's rule, at opening");
    doc.change_line_ending(LineEnding::Lf);
    assert!(doc.is_dirty());
    doc.change_line_ending(LineEnding::CrLf);
    assert!(!doc.is_dirty(), "back to the file's own");
    doc.change_encoding(Encoding::Utf16Le);
    assert!(doc.is_dirty());
    doc.mark_saved(std::path::PathBuf::from("/tmp/x.txt"));
    assert!(!doc.is_dirty());
    doc.change_encoding(Encoding::Latin1);
    assert!(!doc.reload(crate::domain::encoding::decode_as(b"a\r\nb\r\n", Encoding::Utf8)));
    assert!(!doc.is_dirty(), "read again: as the file is");
    assert_eq!(doc.encoding(), Encoding::Utf8);
}
