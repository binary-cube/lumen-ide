use super::*;

fn row(command: &str, place: &str, shortcuts: &[&str]) -> BindingRow {
    BindingRow {
        command: command.into(),
        label: command.into(),
        group: String::new(),
        place: place.into(),
        shortcuts: shortcuts.iter().map(|s| (*s).to_owned()).collect(),
        shown: shortcuts.iter().map(|s| (*s).to_owned()).collect(),
        changed: false,
    }
}

fn editor(rows: Vec<BindingRow>, open: &str) -> ShortcutsEditor {
    ShortcutsEditor {
        rows: Arc::new(rows),
        open: Some(open.into()),
        recording: true,
        ..ShortcutsEditor::default()
    }
}

/// A click on what was drawn as `hit`.
fn press(e: &mut ShortcutsEditor, hit: Hit) -> Option<ShortcutChanges> {
    e.hits = vec![(Rect::new(0.0, 0.0, 10.0, 10.0), hit)];
    e.click(Point::new(5.0, 5.0))
}

/// Free keys: added to the command at once.
#[test]
fn free_keys_are_added() {
    let mut e = editor(vec![row("navigate.line", "", &["CmdOrCtrl+L"])], "navigate.line");
    let changes = e.record("CmdOrCtrl+J", "⌘J").unwrap();
    assert_eq!(
        changes,
        [("navigate.line".to_owned(), Some(vec!["CmdOrCtrl+L".into(), "CmdOrCtrl+J".into()]))]
    );
    assert!(!e.recording());
}

/// Keys another command has where this one works too (everywhere covers every place):
/// asked — removed from the others, left to all, or given up; another
/// view's are free.
#[test]
fn keys_taken_in_the_same_place_are_asked_about() {
    let rows = vec![
        row("navigate.line", "image", &["CmdOrCtrl+L"]),
        row("file.save", "", &["CmdOrCtrl+S", "CmdOrCtrl+J"]),
        row("canvas.fit", "canvas", &["CmdOrCtrl+J"]),
    ];
    let mut e = editor(rows.clone(), "navigate.line");
    assert_eq!(e.record("CmdOrCtrl+J", "⌘J"), None, "asked");
    assert_eq!(e.conflict.as_ref().unwrap().others, ["file.save"], "not another view's");
    let changes = press(&mut e, Hit::RemoveOthers).unwrap();
    assert_eq!(
        changes,
        [
            ("navigate.line".to_owned(), Some(vec!["CmdOrCtrl+L".into(), "CmdOrCtrl+J".into()])),
            ("file.save".to_owned(), Some(vec!["CmdOrCtrl+S".into()])),
        ]
    );
    let mut e = editor(rows.clone(), "navigate.line");
    e.record("CmdOrCtrl+J", "⌘J");
    assert_eq!(press(&mut e, Hit::Leave).unwrap().len(), 1, "left to all: only this one changes");
    let mut e = editor(rows, "navigate.line");
    e.record("CmdOrCtrl+J", "⌘J");
    assert_eq!(press(&mut e, Hit::Cancel), None);
    assert!(e.conflict.is_none());
}

/// A plain letter where text is typed isn't taken (still recording); in a view it is.
#[test]
fn a_plain_letter_is_refused_where_text_is_typed() {
    let mut e = editor(vec![row("file.save", "", &[])], "file.save");
    assert_eq!(e.record("K", "K"), None);
    assert!(e.recording(), "still recording");
    let mut e = editor(vec![row("canvas.fit", "canvas", &[])], "canvas.fit");
    assert!(e.record("K", "K").is_some());
}

/// × removes one shortcut; "back to the preset" forgets the user's.
#[test]
fn keys_are_removed_and_reset() {
    let mut e = editor(vec![row("file.save", "", &["CmdOrCtrl+S", "F2"])], "file.save");
    let changes = press(&mut e, Hit::Remove("file.save".into(), 0)).unwrap();
    assert_eq!(changes, [("file.save".to_owned(), Some(vec!["F2".into()]))]);
    let changes = press(&mut e, Hit::Reset("file.save".into())).unwrap();
    assert_eq!(changes, [("file.save".to_owned(), None)]);
}
