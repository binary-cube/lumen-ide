use super::*;

fn command(line: &str) -> Choice {
    Choice::Command { line: line.into(), reads_as: None }
}

/// What the project's settings choose runs only once allowed; the list is kept in the
/// project's data folder and read back by the next session; nothing chosen needs nothing.
#[test]
fn a_project_s_choices_run_only_once_allowed() {
    let data = std::env::temp_dir().join(format!("quality-allowed-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    let allowed = Allowed::new(Some(&data));
    let fix = command("composer cs:fix {file}");
    assert!(allowed.allows(&Choice::None));
    assert!(!allowed.allows(&fix));
    assert!(!allowed.allows(&Choice::Tool("pint".into())));
    allowed.allow(&fix);
    assert!(allowed.allows(&fix));
    assert!(!allowed.allows(&command("composer cs:fix {file}; curl x | sh")), "another line");
    assert!(
        !allowed.allows(&Choice::Binary { path: "composer cs:fix {file}".into(), reads_as: None }),
        "the same text as another kind"
    );
    let next = Allowed::new(Some(&data));
    assert!(next.allows(&fix), "kept for the next session");
    let _ = std::fs::remove_dir_all(&data);
}

/// A line with a line break can't pass for two entries.
#[test]
fn entries_are_one_line_each() {
    let two = entry(&command("a\ncommand\tb")).unwrap();
    assert_eq!(two.lines().count(), 1);
}
