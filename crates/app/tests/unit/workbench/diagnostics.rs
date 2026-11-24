use std::path::PathBuf;

use document::TextBuffer;
use workbench_model::{Diagnostic, Severity};

use super::{Covered, mark_of};

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

/// Whether a range meets the servers' ones, found by a binary search: what comparing it with
/// each of them gives (overlapping, or starting at the same place), empty ranges too.
#[test]
fn a_range_meets_the_servers_ones_as_a_walk_finds() {
    let walk = |all: &[std::ops::Range<usize>], b: &std::ops::Range<usize>| {
        all.iter().any(|a| (a.start < b.end && b.start < a.end) || a.start == b.start)
    };
    // Unsorted, nested, touching, empty.
    let servers: Vec<std::ops::Range<usize>> =
        vec![40..60, 5..9, 100..300, 120..130, 60..61, 200..200, 9..12, 500..520];
    let covered = Covered::new(servers.clone());
    for start in 0..560 {
        for len in [0, 1, 3, 25] {
            let range = start..start + len;
            assert_eq!(covered.meets(&range), walk(&servers, &range), "{range:?}");
        }
    }
    assert!(!Covered::default().meets(&(3..9)), "none: meets nothing");
}
