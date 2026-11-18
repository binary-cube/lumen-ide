//! A console's text checked against its source's schema (a real SQLite file): nothing said
//! before the schema is read, then its unknown tables and columns and its ambiguous columns
//! told on the project's bus, cleared when the text is fixed or the console closes, checked
//! again when the schema changes; what the text creates never called unknown.

use std::sync::{Arc, Mutex};

use workbench_model::{DiagnosticsFound, DiagnosticsScope, Severity};

use super::*;
use crate::fake::{FakeAccess, Manual, Project};

struct Setup {
    project: Project,
    manual: Manual,
    access: Arc<FakeAccess>,
    consoles: Arc<Consoles>,
    heard: Arc<Mutex<Vec<DiagnosticsFound>>>,
    _subscriptions: Vec<kernel::Subscription>,
}

fn setup(name: &str) -> Setup {
    let project = Project::new(name);
    let manual = Manual::new();
    let access = Arc::new(FakeAccess::new(&project));
    let consoles = crate::fake::consoles(&project, &manual, Arc::clone(&access));
    let heard: Arc<Mutex<Vec<DiagnosticsFound>>> = Arc::default();
    let into = Arc::clone(&heard);
    let listening = consoles.events().subscribe(move |e: &DiagnosticsFound| {
        if e.source == SOURCE {
            into.lock().unwrap().push(e.clone());
        }
    });
    let registered = crate::consoles::register(Arc::clone(&consoles));
    Setup { project, manual, access, consoles, heard, _subscriptions: vec![listening, registered] }
}

/// A console of the shop with `text`, told to the checks as version `version`.
fn console(s: &Setup, text: &str) -> std::path::PathBuf {
    let spec = s.consoles.create(s.access.shop, text).unwrap();
    let path = spec.path(&s.project.root);
    s.consoles.checks().schedule(&s.consoles, &path, text.into(), 1);
    path
}

fn tell(s: &Setup, path: &std::path::Path, text: &str, version: u64) {
    s.consoles.checks().schedule(&s.consoles, path, text.into(), version);
}

/// A finding told: its message, line, column and severity.
type Told = (String, u32, Option<u32>, Severity);

/// The last findings told: their messages, lines and columns.
fn last(s: &Setup) -> Option<Vec<Told>> {
    let heard = s.heard.lock().unwrap();
    heard.last().map(|f| {
        f.diagnostics.iter().map(|d| (d.message.clone(), d.line, d.column, d.severity)).collect()
    })
}

fn unknown_table(name: &str) -> String {
    i18n::tr!("console.check_unknown_table", name = name)
}

#[test]
fn nothing_is_said_before_the_schema_then_an_unknown_table_is() {
    let s = setup("checks-unknown");
    let path = console(&s, "SELECT * FROM users;\nSELECT * FROM nope;");
    // The first look asks for the schema (nothing to say yet).
    s.manual.run_all();
    s.manual.run_all();
    let found = last(&s).expect("told once the schema came");
    assert_eq!(found, [(unknown_table("nope"), 1, Some(14), Severity::Error)]);
    let heard = s.heard.lock().unwrap().last().cloned().unwrap();
    let relative = path.strip_prefix(&s.project.root).unwrap().to_path_buf();
    assert_eq!(heard.scope, DiagnosticsScope::File(relative));
    assert_eq!(heard.diagnostics[0].source, i18n::t("console.check_source"));
}

#[test]
fn a_fixed_text_clears_its_findings_and_the_same_version_isn_t_checked_twice() {
    let s = setup("checks-fixed");
    let path = console(&s, "SELECT * FROM nope");
    s.manual.run_all();
    s.manual.run_all();
    assert_eq!(last(&s).map(|f| f.len()), Some(1));
    let runs = s.consoles.checks().runs();
    tell(&s, &path, "SELECT * FROM nope", 1);
    s.manual.run_all();
    assert_eq!(s.consoles.checks().runs(), runs, "the same version");
    tell(&s, &path, "SELECT * FROM users", 2);
    s.manual.run_all();
    assert_eq!(last(&s), Some(Vec::new()), "cleared");
    // A console closed takes its findings with it.
    tell(&s, &path, "SELECT * FROM gone", 3);
    s.manual.run_all();
    assert_eq!(last(&s).map(|f| f.len()), Some(1));
    s.consoles.checks().forget(&s.consoles, &path);
    assert_eq!(last(&s), Some(Vec::new()));
}

#[test]
fn what_the_text_makes_is_never_unknown_and_ambiguity_is_a_warning() {
    let s = setup("checks-created");
    let text = "CREATE TEMP TABLE scratch (a);\nSELECT * FROM scratch;\n\
                WITH r AS (SELECT 1) SELECT * FROM r;\n\
                SELECT id FROM users u JOIN orders o ON o.user_id = u.id;";
    console(&s, text);
    s.manual.run_all();
    s.manual.run_all();
    let found = last(&s).unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].1, 3);
    assert_eq!(found[0].3, Severity::Warning);
}

#[test]
fn a_clean_text_tells_nothing_at_all() {
    let s = setup("checks-clean");
    console(&s, "SELECT u.name FROM users u");
    s.manual.run_all();
    s.manual.run_all();
    assert!(s.heard.lock().unwrap().is_empty(), "nothing to clear the first time");
}

#[test]
fn columns_read_later_are_checked_when_they_come() {
    let s = setup("checks-lazy");
    s.consoles.indexes().set_limits(1, 64 << 20);
    console(&s, "SELECT o.nope FROM orders o");
    // The tables first; then the columns the statement names, asked for; then told.
    for _ in 0..4 {
        s.manual.run_all();
    }
    let found = last(&s).expect("told once the columns came");
    let message = i18n::tr!("console.check_unknown_column", column = "nope", table = "orders");
    assert_eq!(found, [(message, 0, Some(9), Severity::Error)]);
}

#[test]
fn a_schema_change_checks_the_text_again() {
    let s = setup("checks-invalidated");
    console(&s, "SELECT * FROM later");
    s.manual.run_all();
    s.manual.run_all();
    assert_eq!(last(&s).map(|f| f.len()), Some(1));
    s.project.look("CREATE TABLE later (id INTEGER)");
    s.consoles.indexes().invalidate(s.access.shop);
    // Asked again by the next look (the text checked once more, on the old index), then
    // the new index arrives and the text is checked on it.
    s.consoles.checks().recheck(&s.consoles);
    for _ in 0..3 {
        s.manual.run_all();
    }
    assert_eq!(last(&s), Some(Vec::new()));
}

#[test]
fn quoted_names_are_pointed_at_inside_their_quotes() {
    let s = setup("checks-quoted");
    console(&s, "SELECT * FROM \"Nope\"");
    s.manual.run_all();
    s.manual.run_all();
    let found = last(&s).unwrap();
    assert_eq!(found[0].2, Some(15));
}

/// A project's SQL file attached to the shop (39n) is checked as a console is; detached,
/// its findings go.
#[test]
fn an_attached_file_is_checked_against_its_source() {
    let s = setup("checks-attached");
    let path = s.project.root.join("report.sql");
    let text = "SELECT * FROM nowhere;";
    std::fs::write(&path, text).unwrap();
    tell(&s, &path, text, 1);
    s.manual.run_all();
    assert!(last(&s).is_none(), "attached to nothing: not checked");
    s.consoles.attach(&path, s.access.shop).unwrap();
    tell(&s, &path, text, 2);
    for _ in 0..4 {
        s.manual.run_all();
        tell(&s, &path, text, 2);
    }
    let found = last(&s).expect("checked once the schema is read");
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, unknown_table("nowhere"));
    s.consoles.detach(&path).unwrap();
    assert_eq!(last(&s), Some(Vec::new()), "its findings gone with the attachment");
}
