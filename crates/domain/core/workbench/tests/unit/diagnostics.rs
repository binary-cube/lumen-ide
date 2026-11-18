use super::*;

fn diagnostic(path: &str, severity: Severity) -> Diagnostic {
    Diagnostic {
        path: PathBuf::from(path),
        line: 1,
        column: None,
        end: None,
        severity,
        message: "Bad".to_owned(),
        source: "PHPStan".to_owned(),
        rule: None,
    }
}

fn found(source: &str, scope: DiagnosticsScope, diagnostics: Vec<Diagnostic>) -> DiagnosticsFound {
    DiagnosticsFound { source: source.to_owned(), scope, diagnostics }
}

fn count(store: &ProjectDiagnostics, file: &str) -> usize {
    store.of(Path::new(file)).count()
}

#[test]
fn a_files_findings_replace_that_files_and_a_projects_the_sources() {
    let events = EventBus::new();
    let mut store = ProjectDiagnostics::listen(&events);
    assert!(!store.sync());
    let a = || diagnostic("src/A.php", Severity::Error);
    let b = || diagnostic("src/B.php", Severity::Warning);
    let file = || DiagnosticsScope::File("src/A.php".into());
    events.publish(&found("phpstan", DiagnosticsScope::Project, vec![a(), a(), b()]));
    events.publish(&found("phpcs", file(), vec![a(), b()]));
    assert!(store.sync());
    // phpcs's run on A keeps only A's findings.
    assert_eq!((count(&store, "src/A.php"), count(&store, "src/B.php")), (3, 1));
    let generation = store.generation();

    // A clean file run clears that file for that source only.
    events.publish(&found("phpstan", file(), Vec::new()));
    assert!(store.sync());
    assert!(store.generation() > generation);
    assert_eq!((count(&store, "src/A.php"), count(&store, "src/B.php")), (1, 1));

    // A project run replaces all of that source's files.
    events.publish(&found("phpstan", DiagnosticsScope::Project, Vec::new()));
    store.sync();
    assert_eq!((count(&store, "src/A.php"), count(&store, "src/B.php")), (1, 0));
}

/// Another project's bus (its own events): nothing reaches this one.
#[test]
fn only_the_projects_own_bus_is_heard() {
    let (mine, other) = (EventBus::new(), EventBus::new());
    let mut store = ProjectDiagnostics::listen(&mine);
    other.publish(&found(
        "phpstan",
        DiagnosticsScope::Project,
        vec![diagnostic("a", Severity::Info)],
    ));
    assert!(!store.sync());
    assert_eq!(count(&store, "a"), 0);
}

/// A file's diagnostics from some sources only (a language server's, told apart by their
/// source's name).
#[test]
fn a_files_diagnostics_from_some_sources() {
    let events = EventBus::new();
    let mut store = ProjectDiagnostics::listen(&events);
    let file = || DiagnosticsScope::File("a".into());
    events.publish(&found("phpstan", file(), vec![diagnostic("a", Severity::Error)]));
    events.publish(&found("lsp-x", file(), vec![diagnostic("a", Severity::Warning)]));
    store.sync();
    let from = |keep: fn(&str) -> bool| store.of_sources(Path::new("a"), keep).count();
    assert_eq!(from(|s| s.starts_with("lsp-")), 1);
    assert_eq!(from(|_| true), 2);
    assert_eq!(from(|_| false), 0);
}
