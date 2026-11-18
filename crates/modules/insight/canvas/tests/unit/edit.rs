use super::*;

#[test]
fn edits_become_a_line_replacement() {
    let code =
        vec!["    public function stop()".to_owned(), "    {".to_owned(), "    }".to_owned()];
    let mut edit = CodeEdit::new(0, PathBuf::from("/p/a.php"), 10, &code);
    assert!(CodeEdit::new(0, PathBuf::from("/p/a.php"), 10, &code).finish().is_none());
    edit.key(ViewerKey::Down);
    edit.key(ViewerKey::End);
    edit.key(ViewerKey::Enter);
    edit.insert("return 1;");
    assert_eq!(edit.line_count(), 4);
    match edit.finish() {
        Some(ViewerAction::Replace { lines, text, .. }) => {
            assert_eq!(lines, 10..=12);
            assert!(text.contains("{\n") && text.contains("return 1;"), "{text}");
        }
        other => panic!("{other:?}"),
    }
}
