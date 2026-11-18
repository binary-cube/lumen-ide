use super::*;

fn file(name: &str) -> Target {
    Target::File(name.into())
}

/// Two runs on the same file don't overlap: the second waits, asked for twice it runs once.
#[test]
fn runs_on_a_file_wait_for_each_other_and_run_once() {
    let mut runs = Runs::default();
    assert_eq!(runs.ask(&file("a.php"), Kind::Formatter), Start::Now);
    assert_eq!(runs.ask(&file("a.php"), Kind::Formatter), Start::Queued);
    assert_eq!(runs.ask(&file("a.php"), Kind::Formatter), Start::Queued);
    assert_eq!(runs.ask(&file("b.php"), Kind::Formatter), Start::Now, "another file: at once");
    assert_eq!(runs.ask(&file("a.php"), Kind::Checker), Start::Now, "a checker on it: at once");
    assert_eq!(runs.finished(&file("a.php"), Kind::Formatter), [file("a.php")], "once");
    assert!(runs.finished(&file("a.php"), Kind::Formatter).is_empty());
}

/// The whole project's run and a file's don't overlap either.
#[test]
fn the_project_s_run_and_a_file_s_wait_for_each_other() {
    let mut runs = Runs::default();
    assert_eq!(runs.ask(&Target::Project, Kind::Formatter), Start::Now);
    assert_eq!(runs.ask(&file("a.php"), Kind::Formatter), Start::Queued);
    assert!(runs.busy());
    assert_eq!(runs.finished(&Target::Project, Kind::Formatter), [file("a.php")]);
    assert_eq!(runs.finished(&file("a.php"), Kind::Formatter), Vec::<Target>::new());
    assert!(!runs.busy());
}
