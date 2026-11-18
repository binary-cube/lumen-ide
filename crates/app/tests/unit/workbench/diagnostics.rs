use std::path::PathBuf;

use document::TextBuffer;
use workbench_model::{Diagnostic, Severity};

use super::mark_of;

fn problem(path: &str, line: u32, column: Option<u32>, severity: Severity) -> Diagnostic {
    Diagnostic {
        path: PathBuf::from(path),
        line,
        column,
        end: None,
        severity,
        message: "Bad".to_owned(),
        rule: None,
        source: "PHPStan".to_owned(),
    }
}

#[test]
fn a_problem_marks_its_word_its_character_or_its_line() {
    let buffer = TextBuffer::new("<?php\n    $user->name = 1;\n\nend");
    let at = |column, line| {
        let mut p = problem("a.php", line, column, Severity::Error);
        p.rule = Some("rule.id".to_owned());
        mark_of(&p, &buffer, "$").map(|m| (buffer.slice(m.range.clone()), m))
    };
    // The word at the column (`$` included).
    let (text, mark) = at(Some(4), 1).unwrap();
    assert_eq!(text, "$user");
    assert_eq!(mark.code.as_deref(), Some("PHPStan · rule.id"));
    assert!(!mark.warning);
    // A character that isn't a word's.
    assert_eq!(at(Some(9), 1).unwrap().0, "-");
    // Past the line's end: its last character.
    assert_eq!(at(Some(99), 1).unwrap().0, ";");
    // No column: the line without its indent.
    assert_eq!(at(None, 1).unwrap().0, "$user->name = 1;");
    // Nothing to mark: an empty line, past the text.
    assert!(at(None, 2).is_none());
    assert!(at(None, 9).is_none());
}

#[test]
fn warnings_and_notes_mark_as_such() {
    let buffer = TextBuffer::new("x");
    let mark = |severity| mark_of(&problem("a", 0, None, severity), &buffer, "$").unwrap();
    let warning = mark(Severity::Warning);
    assert!(warning.warning && !warning.weak);
    let info = mark(Severity::Info);
    assert!(info.warning && info.weak);
    assert_eq!(mark(Severity::Error).code.as_deref(), Some("PHPStan"));
}
