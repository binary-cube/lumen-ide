//! A console's completion on a real SQLite file: the schema read in the background the
//! first time (said meanwhile), then the columns after `alias.`, the tables with their
//! source, `JOIN … ON` from the file's own keys and those its names say, an open list made
//! again when what it waited for came; `SELECT *` expanded in the console.

use std::sync::Arc;

use code_intelligence::{AssistContext, CompletionKind, DocumentAssist};
use workbench_model::EditorHost;

use super::*;
use crate::fake::{FakeAccess, FakeHost, Manual, Project};

struct Setup {
    project: Project,
    manual: Manual,
    access: Arc<FakeAccess>,
    consoles: Arc<Consoles>,
    _registered: kernel::Subscription,
}

fn setup(name: &str) -> Setup {
    let project = Project::new(name);
    project.look(
        "CREATE TABLE items (id INTEGER PRIMARY KEY, order_id INTEGER REFERENCES orders(id), qty INTEGER)",
    );
    project.look("CREATE TABLE events (id INTEGER PRIMARY KEY, kind TEXT, at TEXT)");
    let manual = Manual::new();
    let access = Arc::new(FakeAccess::new(&project));
    let consoles = crate::fake::consoles(&project, &manual, Arc::clone(&access));
    let registered = crate::consoles::register(Arc::clone(&consoles));
    Setup { project, manual, access, consoles, _registered: registered }
}

/// The console's suggestions at `‸` (its file made with that text).
fn at(s: &Setup, sql: &str) -> Option<code_intelligence::FrameworkCompletion> {
    let offset = sql.find('‸').unwrap();
    let text = sql.replace('‸', "");
    let spec = s.consoles.create(s.access.shop, &text).unwrap();
    let path = spec.path(&s.project.root);
    ConsoleAssist::default().complete(&AssistContext {
        project: Some(s.consoles.project()),
        root: Some(&s.project.root),
        path: &path,
        language: "sql",
        text: &text,
        offset,
        version: 1,
    })
}

fn labels(c: &code_intelligence::FrameworkCompletion) -> Vec<&str> {
    c.items.iter().map(|i| i.label.as_str()).collect()
}

#[test]
fn the_schema_is_read_the_first_time_then_columns_follow_the_alias() {
    let s = setup("assist-alias");
    let sql = "select * from events e where e.‸";
    let first = at(&s, sql).expect("a console's");
    assert!(!first.merge, "its own list");
    assert_eq!(labels(&first), [i18n::tr!("console.completion_reading", name = "shop").as_str()]);
    let before = ConsoleAssist::default().generation(Some(s.consoles.project()));
    s.manual.run_all();
    assert_ne!(
        ConsoleAssist::default().generation(Some(s.consoles.project())),
        before,
        "an open list is made again"
    );
    let c = at(&s, sql).unwrap();
    assert_eq!(labels(&c), ["id", "kind", "at"]);
    assert!(c.items.iter().all(|i| i.kind == CompletionKind::Column));
    assert_eq!(c.items[1].detail.as_deref(), Some("events e · TEXT"));
    let offset = sql.find('‸').unwrap();
    assert_eq!(c.replace, offset..offset);
    // Best first, in the order given.
    assert!(c.items.windows(2).all(|w| w[0].score > w[1].score));
}

#[test]
fn tables_show_their_schema_and_source() {
    let s = setup("assist-tables");
    at(&s, "SELECT * FROM ‸");
    s.manual.run_all();
    let c = at(&s, "SELECT * FROM ord‸").unwrap();
    assert_eq!(c.items[0].label, "orders");
    assert_eq!(c.items[0].kind, CompletionKind::Table);
    assert_eq!(c.items[0].detail.as_deref(), Some("main · shop"));
    assert_eq!(c.replace, 14..17);
}

#[test]
fn join_offers_the_related_tables_with_their_condition() {
    let s = setup("assist-join");
    at(&s, "SELECT 1 FROM orders o JOIN ‸");
    s.manual.run_all();
    let c = at(&s, "select * from orders o join ‸").unwrap();
    let joins: Vec<(&str, Option<&str>)> = c
        .items
        .iter()
        .filter(|i| i.kind == CompletionKind::Snippet)
        .map(|i| (i.label.as_str(), i.detail.as_deref()))
        .collect();
    // The file's key first, the one its names say last (marked).
    assert_eq!(joins[0], ("items i ON i.order_id = o.id", Some("items.order_id → orders.id")));
    let inferred =
        format!("orders.user_id → users.id · {}", i18n::t("console.completion_inferred"));
    assert_eq!(joins[1], ("users u ON u.id = o.user_id", Some(inferred.as_str())));
    let c = at(&s, "select * from orders o join items i on ‸").unwrap();
    assert_eq!(c.items[0].label, "i.order_id = o.id");
}

#[test]
fn a_document_not_a_console_s_isn_t_its() {
    let s = setup("assist-other");
    let path = s.project.root.join("plain.sql");
    let text = "SELECT * FROM ";
    let asked = ConsoleAssist::default().complete(&AssistContext {
        project: Some(s.consoles.project()),
        root: Some(&s.project.root),
        path: &path,
        language: "sql",
        text,
        offset: text.len(),
        version: 1,
    });
    assert!(asked.is_none());
    assert_eq!(s.manual.jobs(), 0, "nothing read");
}

#[test]
fn a_huge_schema_says_its_columns_are_loading_then_lists_them() {
    let s = setup("assist-lazy");
    s.consoles.indexes().set_limits(1, 64 << 20);
    at(&s, "SELECT * FROM ‸");
    s.manual.run_all();
    let c = at(&s, "SELECT e.‸ FROM events e").unwrap();
    assert_eq!(labels(&c), [i18n::t("console.completion_loading_columns").as_str()]);
    assert!(s.consoles.indexes().building(s.access.shop, None), "asked for");
    s.manual.run_all();
    let c = at(&s, "SELECT e.‸ FROM events e").unwrap();
    assert_eq!(labels(&c), ["id", "kind", "at"]);
}

#[test]
fn select_star_is_expanded_in_the_console() {
    let s = setup("assist-expand");
    at(&s, "SELECT * FROM ‸");
    s.manual.run_all();
    let text = "SELECT * FROM users u JOIN orders o ON o.user_id = u.id";
    let spec = s.consoles.create(s.access.shop, text).unwrap();
    let mut host = FakeHost::new(s.consoles.project(), &s.project.root);
    host.open_file(&spec.path(&s.project.root));
    // On the `*` (right before it): the same.
    host.cursor = 7;
    assert!(expand_star(&mut host));
    assert_eq!(
        host.file.as_ref().unwrap().1,
        "SELECT u.id, u.name, o.id, o.user_id, o.total FROM users u JOIN orders o ON o.user_id = u.id"
    );
    // Nowhere to expand: said.
    host.cursor = 0;
    assert!(!expand_star(&mut host));
    assert_eq!(host.told.last().map(|n| n.title.clone()), Some(i18n::t("console.expand_nothing")));
}

#[test]
fn a_document_s_tokens_are_read_once_per_version() {
    let assist = ConsoleAssist::default();
    let path = std::path::PathBuf::from("/p/consoles/x/console.sql");
    let at = |version: u64| AssistContext {
        project: None,
        root: None,
        path: &path,
        language: "sql",
        text: "SELECT 1",
        offset: 8,
        version,
    };
    let one = assist.tokens(&at(1), Flavor::Sqlite);
    assert!(Arc::ptr_eq(&one, &assist.tokens(&at(1), Flavor::Sqlite)));
    assert!(!Arc::ptr_eq(&one, &assist.tokens(&at(2), Flavor::Sqlite)));
}

/// The context of `sql` at its `‸` (its console made with that text), and the text.
fn context_of(s: &Setup, sql: &str) -> (std::path::PathBuf, String, usize) {
    let offset = sql.find('‸').unwrap();
    let text = sql.replace('‸', "");
    let spec = s.consoles.create(s.access.shop, &text).unwrap();
    (spec.path(&s.project.root), text, offset)
}

fn ask<R>(
    s: &Setup,
    sql: &str,
    then: impl Fn(&ConsoleAssist, &AssistContext<'_>) -> R,
) -> (R, String) {
    let (path, text, offset) = context_of(s, sql);
    let assist = ConsoleAssist::default();
    let at = AssistContext {
        project: Some(s.consoles.project()),
        root: Some(&s.project.root),
        path: &path,
        language: "sql",
        text: &text,
        offset,
        version: 1,
    };
    (then(&assist, &at), text)
}

/// The schema read once (as the first question does).
fn read_schema(s: &Setup) {
    at(s, "SELECT * FROM ‸");
    s.manual.run_all();
}

#[test]
fn a_table_or_a_column_goes_to_the_explorer_an_alias_to_its_place() {
    let s = setup("assist-definition");
    read_schema(&s);
    let (target, _) = ask(&s, "SELECT * FROM eve‸nts e", |a, at| a.definition(at));
    assert_eq!(target, Some(code_intelligence::AssistTarget::Command(GOTO.0.to_owned())));
    let (target, _) = ask(&s, "SELECT e.ki‸nd FROM events e", |a, at| a.definition(at));
    assert_eq!(target, Some(code_intelligence::AssistTarget::Command(GOTO.0.to_owned())));
    // An alias used: where it's given (line and offset).
    let (target, text) = ask(&s, "SELECT e‸.kind\nFROM events e", |a, at| a.definition(at));
    let Some(code_intelligence::AssistTarget::Location(location)) = target else {
        panic!("a place: {target:?}")
    };
    assert_eq!(location.offset, text.rfind('e').unwrap());
    assert_eq!(location.line, 1);
    // A CTE used: its definition; its definition itself: nothing.
    let sql = "WITH recent AS (SELECT id FROM events) SELECT * FROM rec‸ent";
    let (target, text) = ask(&s, sql, |a, at| a.definition(at));
    let Some(code_intelligence::AssistTarget::Location(location)) = target else { panic!() };
    assert_eq!(location.offset, text.find("recent").unwrap());
    let (target, _) =
        ask(&s, "WITH rec‸ent AS (SELECT id FROM events) SELECT 1", |a, at| a.definition(at));
    assert_eq!(target, None);
    // Nothing on a keyword, a string, an unknown name.
    for sql in ["SEL‸ECT 1", "SELECT 'ev‸ents'", "SELECT * FROM nop‸e"] {
        let (target, _) = ask(&s, sql, |a, at| a.definition(at));
        assert_eq!(target, None, "{sql}");
    }
}

#[test]
fn go_to_opens_a_table_s_data_and_a_column_s_structure() {
    let s = setup("assist-goto");
    read_schema(&s);
    let (path, text, offset) = context_of(&s, "SELECT e.kind FROM eve‸nts e");
    let mut host = crate::fake::FakeHost::new(s.consoles.project(), &s.project.root);
    host.file = Some((path.clone(), text.clone()));
    host.cursor = offset;
    assert!(goto(&mut host));
    assert!(host.views.iter().any(|(_, kind)| kind == crate::fake::OPEN));
    let opened = s.access.opened.lock().unwrap().clone();
    assert_eq!(opened, [(database::TableRef::new("events"), false, None)]);
    // A column: its table's structure, the column pointed at (a file's source: its view).
    host.cursor = text.find("kind").unwrap() + 1;
    assert!(goto(&mut host));
    assert_eq!(host.opened, std::slice::from_ref(&s.access.db));
    let opened = s.access.opened.lock().unwrap().clone();
    assert_eq!(opened[1], (database::TableRef::new("events"), true, Some("kind".to_owned())));
    // Elsewhere: nothing.
    host.file = Some((path, text));
    host.cursor = 2;
    assert!(!goto(&mut host));
}

#[test]
fn hover_cards_tell_a_table_s_columns_and_a_column_s_details() {
    let s = setup("assist-hover");
    read_schema(&s);
    let (card, text) = ask(&s, "SELECT * FROM it‸ems i", |a, at| a.hover(at));
    let card = card.expect("a table's card");
    assert_eq!(&text[card.range.clone()], "items");
    assert_eq!(card.title, Some(("items".to_owned(), Some(CompletionKind::Table))));
    assert_eq!(card.subtitle.as_deref(), Some("main · shop"));
    assert_eq!(card.code, "TABLE items");
    assert_eq!(card.params_label, Some(i18n::t("console.hover_columns")));
    let columns: Vec<(&str, Option<&str>, Option<&str>)> =
        card.params.iter().map(|p| (p.name.as_str(), p.ty.as_deref(), p.doc.as_deref())).collect();
    assert_eq!(columns[0], ("id", Some("INTEGER"), Some("PK")));
    assert_eq!(columns[1], ("order_id", Some("INTEGER"), Some("→ orders.id")));
    // A column: its type, key and what it points to.
    let (card, _) = ask(&s, "SELECT i.order‸_id FROM items i", |a, at| a.hover(at));
    let card = card.expect("a column's card");
    assert_eq!(card.code, "items.order_id\nINTEGER");
    assert_eq!(
        card.doc.as_deref(),
        Some(i18n::tr!("console.hover_references", target = "orders.id").as_str())
    );
    let (card, _) = ask(&s, "SELECT i.i‸d FROM items i", |a, at| a.hover(at));
    assert_eq!(card.unwrap().code, "items.id\nINTEGER PRIMARY KEY");
    // An ambiguous one says so; a CTE lists its columns.
    let (card, _) = ask(&s, "SELECT i‸d FROM items JOIN events ON 1", |a, at| a.hover(at));
    let doc = card.unwrap().doc.unwrap();
    assert!(doc.contains("items") && doc.contains("events"), "{doc}");
    let sql = "WITH r AS (SELECT id, kind FROM events) SELECT * FROM r‸";
    let (card, _) = ask(&s, sql, |a, at| a.hover(at));
    let card = card.unwrap();
    assert_eq!(card.subtitle, Some(i18n::t("console.completion_cte")));
    let names: Vec<&str> = card.params.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["id", "kind"]);
}

#[test]
fn before_the_schema_is_read_nothing_is_said_and_it_s_asked_for() {
    let s = setup("assist-hover-first");
    let (card, _) = ask(&s, "SELECT * FROM it‸ems", |a, at| a.hover(at));
    assert!(card.is_none());
    assert!(s.consoles.indexes().building(s.access.shop, None), "asked for");
    s.manual.run_all();
    let (card, _) = ask(&s, "SELECT * FROM it‸ems", |a, at| a.hover(at));
    assert!(card.is_some());
}

/// A console is laid out by its source's dialect (SQLite's here); another file isn't one.
#[test]
fn a_console_s_dialect_is_its_source_s() {
    let s = setup("assist-flavor");
    let spec = s.consoles.create(s.access.shop, "select 1").unwrap();
    let path = spec.path(&s.project.root);
    assert_eq!(flavor_of(s.consoles.project(), &path), Some(Flavor::Sqlite));
    assert_eq!(flavor_of(s.consoles.project(), &s.project.root.join("a.sql")), None);
}

/// A project's SQL file attached to the shop (39n) completes from its schema as a console
/// does, and its dialect is its source's (laid out by SQLite's rules); attached to nothing,
/// or detached, the module has nothing to say of it.
#[test]
fn an_attached_file_completes_from_its_source() {
    let s = setup("assist-attached");
    let path = s.project.root.join("queries").join("report.sql");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let sql = "select * from events e where e.‸";
    let offset = sql.find('‸').unwrap();
    let text = sql.replace('‸', "");
    std::fs::write(&path, &text).unwrap();
    let ask = || {
        ConsoleAssist::default().complete(&AssistContext {
            project: Some(s.consoles.project()),
            root: Some(&s.project.root),
            path: &path,
            language: "sql",
            text: &text,
            offset,
            version: 1,
        })
    };
    assert!(ask().is_none(), "attached to nothing");
    assert_eq!(flavor_of(s.consoles.project(), &path), None, "laid out by its name");
    s.consoles.attach(&path, s.access.shop).unwrap();
    assert!(ask().is_some(), "its source's schema read");
    s.manual.run_all();
    assert_eq!(labels(&ask().unwrap()), ["id", "kind", "at"]);
    assert_eq!(flavor_of(s.consoles.project(), &path), Some(database::sql::Flavor::Sqlite));
    assert!(attachment(s.consoles.project(), &path).is_some());
    s.consoles.detach(&path).unwrap();
    assert!(ask().is_none() && attachment(s.consoles.project(), &path).is_none());
}
