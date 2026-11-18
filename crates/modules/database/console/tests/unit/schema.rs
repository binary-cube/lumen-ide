//! A project's schema indexes on a real SQLite file: read once in the background on their
//! own connection, the old one serving until a new one comes, read again when stale,
//! forgotten with the caches, let go when unused, a huge schema's columns read as named.

use std::sync::Arc;
use std::time::{Duration, Instant};

use database::TableRef;
use database::events::{DataChanged, SchemaInvalidated};

use super::*;
use crate::fake::{FakeAccess, Manual, Project};

struct Setup {
    project: Project,
    manual: Manual,
    access: Arc<FakeAccess>,
    consoles: Arc<crate::consoles::Consoles>,
    _follow: Vec<kernel::Subscription>,
}

fn setup(name: &str) -> Setup {
    let project = Project::new(name);
    let manual = Manual::new();
    let access = Arc::new(FakeAccess::new(&project));
    let consoles = crate::fake::consoles(&project, &manual, Arc::clone(&access));
    let follow = consoles.follow();
    Setup { project, manual, access, consoles, _follow: follow }
}

fn columns(index: &SchemaIndex, table: &str) -> Vec<String> {
    let i = index.find(&TableRef::new(table)).expect(table);
    index
        .table(i)
        .columns
        .as_ref()
        .map(|c| c.iter().map(|c| c.name.to_string()).collect())
        .unwrap_or_default()
}

#[test]
fn a_source_s_schema_is_read_once_in_the_background_on_its_own_connection() {
    let s = setup("index-read");
    let indexes = s.consoles.indexes();
    let now = Instant::now();
    let before = indexes.generation();
    assert!(
        indexes.index(s.access.as_ref(), s.access.shop, None, now).is_none(),
        "nothing waited for"
    );
    assert!(indexes.building(s.access.shop, None));
    assert!(s.consoles.busy(), "the snapshots wait for it");
    assert_eq!(s.access.connections(), 0, "nothing connected on the caller's thread");
    // Asked again while it's read: one read.
    assert!(indexes.index(s.access.as_ref(), s.access.shop, None, now).is_none());
    assert_eq!(s.manual.jobs(), 1);
    s.manual.run_all();
    assert_eq!(s.access.connections(), 1);
    assert!(indexes.generation() > before, "its arrival said");
    let index = indexes.index(s.access.as_ref(), s.access.shop, None, now).expect("read");
    assert!(index.is_complete());
    assert_eq!(index.default_schema(), Some("main"));
    assert_eq!(columns(&index, "users"), ["id", "name"]);
    assert_eq!(columns(&index, "orders"), ["id", "user_id", "total"]);
    // `orders.user_id` points to users by its name.
    assert_eq!(index.relations().len(), 1);
    assert!(!index.routines().is_empty(), "SQLite's own functions");
    assert_eq!(s.manual.jobs(), 0, "kept, not read again");
    assert!(indexes.memory() > 0);
    // Another database of the source: an index of its own.
    assert!(indexes.index(s.access.as_ref(), s.access.shop, Some("other"), now).is_none());
}

#[test]
fn a_stale_index_serves_until_the_new_one_comes() {
    let s = setup("index-stale");
    let indexes = s.consoles.indexes();
    let now = Instant::now();
    indexes.index(s.access.as_ref(), s.access.shop, None, now);
    s.manual.run_all();
    s.project.look("CREATE TABLE events (id INTEGER PRIMARY KEY, kind TEXT)");
    // DDL run, Refresh: the event on the project's bus.
    s.consoles.events().publish(&SchemaInvalidated { source: s.access.shop });
    let old =
        indexes.index(s.access.as_ref(), s.access.shop, None, now).expect("the old one serves");
    assert!(old.find(&TableRef::new("events")).is_none());
    assert_eq!(s.manual.jobs(), 1, "read again");
    s.manual.run_all();
    let new = indexes.index(s.access.as_ref(), s.access.shop, None, now).unwrap();
    assert_eq!(columns(&new, "events"), ["id", "kind"]);
    assert_ne!(new.generation(), old.generation());
    // Another source's event changes nothing here.
    s.consoles.events().publish(&SchemaInvalidated { source: s.access.people });
    indexes.index(s.access.as_ref(), s.access.shop, None, now);
    assert_eq!(s.manual.jobs(), 0);
    // A structure changed by the explorer's modals; a row changed doesn't count.
    let table = TableRef::new("users");
    s.consoles.events().publish(&DataChanged {
        source: s.access.shop,
        table: table.clone(),
        structure: false,
    });
    indexes.index(s.access.as_ref(), s.access.shop, None, now);
    assert_eq!(s.manual.jobs(), 0);
    s.consoles.events().publish(&DataChanged { source: s.access.shop, table, structure: true });
    indexes.index(s.access.as_ref(), s.access.shop, None, now);
    assert_eq!(s.manual.jobs(), 1);
}

#[test]
fn a_read_made_stale_while_under_way_is_followed_by_another() {
    let s = setup("index-race");
    let indexes = s.consoles.indexes();
    let now = Instant::now();
    indexes.index(s.access.as_ref(), s.access.shop, None, now);
    // The DDL comes before the read ran: its result is dropped, a new read asked.
    indexes.invalidate(s.access.shop);
    s.manual.run_all();
    assert!(
        indexes.index(s.access.as_ref(), s.access.shop, None, now).is_none(),
        "the stale result dropped"
    );
    s.manual.run_all();
    assert!(indexes.index(s.access.as_ref(), s.access.shop, None, now).is_some());
}

#[test]
fn the_decided_relations_changing_reads_it_again() {
    let s = setup("index-notes");
    let indexes = s.consoles.indexes();
    let now = Instant::now();
    indexes.index(s.access.as_ref(), s.access.shop, None, now);
    s.manual.run_all();
    s.consoles
        .events()
        .publish(&database::events::DataSourcesChanged { scope: database::Scope::Project });
    indexes.index(s.access.as_ref(), s.access.shop, None, now);
    assert_eq!(s.manual.jobs(), 0, "the same notes: kept");
    s.access.notes.lock().unwrap().ignore(database::relations::VirtualKey {
        table: TableRef::in_schema("main", "orders"),
        columns: vec!["user_id".into()],
        target: TableRef::in_schema("main", "users"),
        references: vec!["id".into()],
    });
    s.consoles
        .events()
        .publish(&database::events::DataSourcesChanged { scope: database::Scope::Project });
    indexes.index(s.access.as_ref(), s.access.shop, None, now);
    s.manual.run_all();
    let index = indexes.index(s.access.as_ref(), s.access.shop, None, now).unwrap();
    assert!(index.relations().is_empty(), "set aside");
}

#[test]
fn clear_and_rebuild_caches_forgets_them() {
    let s = setup("index-purge");
    let indexes = s.consoles.indexes();
    let now = Instant::now();
    indexes.index(s.access.as_ref(), s.access.shop, None, now);
    s.manual.run_all();
    assert_eq!(indexes.len(), 1);
    let before = indexes.generation();
    s.consoles
        .events()
        .publish(&workspace::events::ReindexRequested { root: s.project.root.clone() });
    assert!(indexes.is_empty());
    assert!(indexes.generation() > before);
    assert!(indexes.index(s.access.as_ref(), s.access.shop, None, now).is_none());
    s.manual.run_all();
    assert!(indexes.index(s.access.as_ref(), s.access.shop, None, now).is_some());
    // The consoles' own purge too.
    s.consoles.purge();
    assert!(indexes.is_empty());
}

#[test]
fn an_unused_index_is_let_go_once_no_console_of_its_source_is_open() {
    let s = setup("index-idle");
    let indexes = s.consoles.indexes();
    let now = Instant::now();
    indexes.index(s.access.as_ref(), s.access.shop, None, now);
    s.manual.run_all();
    let spec = s.consoles.create(s.access.shop, "SELECT 1;").unwrap();
    let runner = s.consoles.runner(spec.id, s.access.shop);
    let lease = runner.lease();
    let later = now + Duration::from_secs(3600);
    s.consoles.sweep(later);
    assert_eq!(indexes.len(), 1, "a console of it is open");
    drop(lease);
    assert!(s.consoles.next_due().is_some_and(|due| due <= later));
    s.consoles.sweep(now + Duration::from_secs(60));
    assert_eq!(indexes.len(), 1, "not idle long enough");
    s.consoles.sweep(later);
    assert!(indexes.is_empty());
}

#[test]
fn a_huge_schema_reads_its_columns_when_named() {
    let s = setup("index-lazy");
    let indexes = s.consoles.indexes();
    indexes.set_limits(1, 64 << 20);
    let now = Instant::now();
    indexes.index(s.access.as_ref(), s.access.shop, None, now);
    s.manual.run_all();
    let index = indexes.index(s.access.as_ref(), s.access.shop, None, now).unwrap();
    assert!(!index.is_complete());
    assert!(index.find(&TableRef::new("users")).is_some(), "the names at once");
    assert!(columns(&index, "users").is_empty());
    let before = indexes.generation();
    indexes.request(s.access.shop, None, &[TableRef::new("users"), TableRef::new("nope")]);
    // Asked twice while read: once.
    indexes.request(s.access.shop, None, &[TableRef::new("users")]);
    assert_eq!(s.manual.jobs(), 1);
    assert!(indexes.building(s.access.shop, None));
    s.manual.run_all();
    assert!(indexes.generation() > before);
    let index = indexes.index(s.access.as_ref(), s.access.shop, None, now).unwrap();
    assert_eq!(columns(&index, "users"), ["id", "name"]);
    assert!(columns(&index, "orders").is_empty());
    assert_eq!(s.access.connections(), 1, "the second tier on the index's connection");
    indexes.request(s.access.shop, None, &[TableRef::new("users")]);
    assert_eq!(s.manual.jobs(), 0, "read already");
}

#[test]
fn a_failed_read_is_said_and_tried_again_when_stale() {
    let project = Project::new("index-broken");
    let manual = Manual::new();
    let mut fake = FakeAccess::new(&project);
    fake.broken = true;
    let access = Arc::new(fake);
    let consoles = crate::fake::consoles(&project, &manual, Arc::clone(&access));
    let indexes = consoles.indexes();
    let now = Instant::now();
    indexes.index(access.as_ref(), access.shop, None, now);
    manual.run_all();
    assert!(indexes.index(access.as_ref(), access.shop, None, now).is_none());
    assert_eq!(manual.jobs(), 0, "not read again by itself");
    assert!(indexes.error(access.shop, None).is_some_and(|e| e.contains("refused")));
    indexes.invalidate(access.shop);
    indexes.index(access.as_ref(), access.shop, None, now);
    assert_eq!(manual.jobs(), 1);
}
