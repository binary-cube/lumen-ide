use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use database::sqlite::ObjectKind;
use database::transaction::{self, alter_plan, apply_script};
use database::{
    Applied, CellChange, Change, ChangeSet, Collect, ColumnDef, CompareOp, DefaultValue, Generated,
    Order, SchemaChange, Script, TableKind,
};

use super::*;

/// A folder of the system's temp dir, gone with it (never the user's files).
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(name: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "db-sqlite-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The test schema: an auto-incremented key, a rowid key with checks, a unique column, a
/// foreign key, a JSON column and indexes (one on an expression), a composite key
/// `WITHOUT ROWID`, a table with no key, views (one over the other) and a trigger.
pub const SCHEMA: &str = "
    CREATE TABLE teams (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL UNIQUE);
    CREATE TABLE users (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL DEFAULT 'x' CHECK (length(name) > 0),
        email TEXT UNIQUE,
        team INTEGER REFERENCES teams (id) ON DELETE CASCADE,
        meta JSON,
        score REAL,
        avatar BLOB,
        CHECK (score IS NULL OR score >= 0)
    );
    CREATE INDEX users_name ON users (name);
    CREATE INDEX users_lower ON users (lower(email));
    CREATE TABLE pairs (b TEXT, a TEXT, v INTEGER, PRIMARY KEY (a, b)) WITHOUT ROWID;
    CREATE TABLE log (line TEXT);
    CREATE VIEW named AS SELECT id, name FROM users;
    CREATE VIEW named_twice AS SELECT * FROM named;
    CREATE TRIGGER users_touch AFTER UPDATE ON users BEGIN
        INSERT INTO log (line) VALUES ('updated ' || new.id);
    END;
    INSERT INTO teams (name) VALUES ('red'), ('blue');
";

/// A database of [`SCHEMA`] with `users` rows (`user 1`…), at `dir/name`.
pub fn database(dir: &TempDir, name: &str, users: usize) -> PathBuf {
    let path = dir.file(name);
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(SCHEMA).unwrap();
    for i in 1..=users {
        conn.execute(
            "INSERT INTO users (name, email, team, meta, score, avatar) VALUES (?1, ?2, 1, ?3, ?4, ?5)",
            rusqlite::params![
                format!("user {i}"),
                format!("u{i}@example.com"),
                format!("{{\"n\":{i}}}"),
                i as f64 / 2.0,
                vec![i as u8; 3],
            ],
        )
        .unwrap();
    }
    path
}

pub fn target(path: &Path) -> Target {
    Target { database: path.to_string_lossy().into_owned(), ..Target::default() }
}

fn open(path: &Path) -> Box<dyn Session> {
    SqliteDriver.connect(&target(path), &Secret::default()).unwrap()
}

fn run(session: &mut dyn Session, sql: &str) -> Result<(Outcome, Collect), DbError> {
    let mut rows = Collect::default();
    let outcome = session.execute(sql, &mut rows, &Cancel::default())?;
    Ok((outcome, rows))
}

#[test]
fn the_driver_says_what_it_can_and_how_it_writes() {
    let factory = factory();
    assert_eq!((&*factory.id, factory.default_port), ("sqlite", None));
    let driver = (factory.create)();
    let can = driver.capabilities();
    assert!(can.transactions && can.edit_rows && can.file_based && can.cancel && can.returning);
    assert!(can.schema_changes.alter_columns, "by rebuilding the table");
    assert_eq!(driver.dialect().map(|d| d.name()), Some("sqlite"));
    assert!(is_database(b"SQLite format 3\0\x10\x00"));
    assert!(!is_database(b"SQLite format 2\0"));
}

#[test]
fn a_missing_file_is_an_error_unless_it_is_asked_for() {
    let dir = TempDir::new("missing");
    let path = dir.file("none.db");
    let opened = SqliteDriver.connect(&target(&path), &Secret::default());
    assert!(matches!(opened, Err(DbError::Connect(_))), "{:?}", opened.err());
    assert!(!path.exists(), "never made silently");
    let mut asked = target(&path);
    asked.options.insert("create".into(), "true".into());
    let mut session = SqliteDriver.connect(&asked, &Secret::default()).unwrap();
    assert!(path.exists());
    assert!(session.server().starts_with("SQLite 3."), "{}", session.server());
    assert!(session.tables(&[]).unwrap().is_empty());
}

#[test]
fn a_file_that_isnt_a_database_is_refused_and_left_as_it_is() {
    let dir = TempDir::new("notadb");
    let path = dir.file("notes.db");
    let text = "not a database, just some text that is long enough to have a header....\n";
    std::fs::write(&path, text.repeat(20)).unwrap();
    let opened = SqliteDriver.connect(&target(&path), &Secret::default());
    assert_eq!(opened.err(), Some(DbError::Connect("not a SQLite database".into())));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text.repeat(20));
}

#[test]
fn tables_of_every_schema_with_their_estimates_once_analyzed() {
    let dir = TempDir::new("tables");
    let path = database(&dir, "shop.db", 30);
    let other = dir.file("other.db");
    Connection::open(&other).unwrap().execute_batch("CREATE TABLE notes (body TEXT)").unwrap();
    let mut session = open(&path);
    let tables = session.tables(&[]).unwrap();
    let listed: Vec<(String, TableKind)> =
        tables.iter().map(|t| (t.table.to_string(), t.kind)).collect();
    assert_eq!(
        listed,
        vec![
            ("log".to_owned(), TableKind::Table),
            ("named".to_owned(), TableKind::View),
            ("named_twice".to_owned(), TableKind::View),
            ("pairs".to_owned(), TableKind::Table),
            ("teams".to_owned(), TableKind::Table),
            ("users".to_owned(), TableKind::Table),
        ],
        "SQLite's own (sqlite_sequence) left out"
    );
    assert!(tables.iter().all(|t| t.rows.is_none()), "nothing counted to list them");

    run(session.as_mut(), "ANALYZE").unwrap();
    let attach =
        format!("ATTACH DATABASE '{}' AS aux", other.to_string_lossy().replace('\'', "''"));
    run(session.as_mut(), &attach).unwrap();
    let tables = session.tables(&[]).unwrap();
    let users = tables.iter().find(|t| t.table == TableRef::new("users")).unwrap();
    assert_eq!(users.rows, Some(30), "from sqlite_stat1");
    assert!(tables.iter().any(|t| t.table == TableRef::in_schema("aux", "notes")));
    assert_eq!(session.tables(&["aux".into()]).unwrap().len(), 1);

    // The explorer: schemas › tables and views › columns and indexes.
    let schemas: Vec<String> = session.children(&[]).unwrap().into_iter().map(|n| n.name).collect();
    assert_eq!(schemas, vec!["main", "aux"], "temp only once something is in it");
    let main = session.children(&["main".into()]).unwrap();
    let users = main.iter().find(|n| n.name == "users").unwrap();
    assert_eq!((users.kind, users.detail.as_str()), ("table", "30"));
    let inside = session.children(&["main".into(), "users".into()]).unwrap();
    assert!(inside.iter().any(|n| n.kind == "column" && n.name == "meta" && n.detail == "JSON"));
    assert!(inside.iter().any(|n| n.kind == "index" && n.name == "users_name"));
}

#[test]
fn structures_are_read_from_the_catalog() {
    let dir = TempDir::new("structure");
    let path = database(&dir, "shop.db", 0);
    let mut session = open(&path);

    let users = session.structure(&TableRef::new("users")).unwrap();
    assert!(users.rowid);
    assert_eq!(users.key_kind(), KeyKind::Primary(vec!["id".into()]));
    let id = users.column("id").unwrap();
    assert!(id.in_key && !id.auto_increment, "a rowid alias, not AUTOINCREMENT");
    let name = users.column("name").unwrap();
    assert_eq!((name.nullable, name.default.as_deref()), (false, Some("'x'")));
    assert_eq!(users.column("meta").unwrap().class, TypeClass::Json);
    assert_eq!(users.column("score").unwrap().class, TypeClass::Decimal);
    assert_eq!(users.column("avatar").unwrap().class, TypeClass::Binary);
    let indexes: Vec<(&str, bool, bool)> =
        users.indexes.iter().map(|i| (i.name.as_str(), i.unique, i.implicit)).collect();
    assert_eq!(
        indexes,
        vec![
            ("sqlite_autoindex_users_1", true, true),
            ("users_lower", false, false),
            ("users_name", false, false),
        ]
    );
    assert_eq!(users.indexes[0].columns, vec!["email".to_owned()]);
    let fk = &users.foreign_keys[0];
    assert_eq!(
        (fk.columns.clone(), fk.table.clone()),
        (vec!["team".into()], TableRef::new("teams"))
    );
    assert_eq!(
        (fk.references.clone(), fk.on_delete),
        (vec!["id".into()], database::FkAction::Cascade)
    );

    let teams = session.structure(&TableRef::new("teams")).unwrap();
    assert!(teams.column("id").unwrap().auto_increment, "AUTOINCREMENT, from its SQL");

    let pairs = session.structure(&TableRef::new("pairs")).unwrap();
    assert!(!pairs.rowid, "WITHOUT ROWID");
    assert_eq!(pairs.primary_key(), vec!["a".to_owned(), "b".to_owned()], "the key's order");
    assert!(pairs.indexes.iter().any(|i| i.implicit && i.columns == ["a", "b"]));

    let log = session.structure(&TableRef::new("log")).unwrap();
    assert_eq!(log.key_kind(), KeyKind::RowId);
    let named = session.structure(&TableRef::new("named")).unwrap();
    assert_eq!((named.rowid, named.key_kind()), (false, KeyKind::None), "a view: only read");
    assert!(session.structure(&TableRef::new("nope")).is_err());
}

#[test]
fn pages_come_with_their_keys_in_order_and_filtered() {
    let dir = TempDir::new("pages");
    let path = database(&dir, "shop.db", 25);
    let mut session = open(&path);
    let users = TableRef::new("users");
    let read = |session: &mut Box<dyn Session>, page: &Page| {
        let mut rows = Collect::default();
        session.read(&users, page, &mut rows, &Cancel::default()).unwrap();
        rows
    };

    let first = read(&mut session, &Page::first(10));
    let names: Vec<&str> = first.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["id", "name", "email", "team", "meta", "score", "avatar"], "no rowid shown");
    assert_eq!(first.rows.len(), 10);
    assert_eq!(first.keys[0], RowKey::Primary(vec![("id".into(), Value::Int(1))]));
    // By storage class; JSON only where it's declared.
    assert_eq!(first.rows[1][1], Value::Text("user 2".into()));
    assert_eq!(first.rows[1][4], Value::Json("{\"n\":2}".into()));
    assert_eq!(first.rows[1][5], Value::Float(1.0));
    assert_eq!(first.rows[1][6], Value::Bytes(vec![2; 3]));

    // The next page starts after the last rowid read: the same rows as an OFFSET would.
    let second = read(&mut session, &Page { offset: 10, ..Page::first(10) });
    assert_eq!(second.keys[0], RowKey::Primary(vec![("id".into(), Value::Int(11))]));
    let third = read(&mut session, &Page { offset: 20, ..Page::first(10) });
    assert_eq!(third.rows.len(), 5);
    // Out of turn: by OFFSET.
    let again = read(&mut session, &Page { offset: 10, ..Page::first(10) });
    assert_eq!(again.rows, second.rows);

    let filtered = Page {
        offset: 0,
        limit: 3,
        order: vec![Order { column: "score".into(), descending: true }],
        filter: Some(Filter::Compare {
            column: "score".into(),
            op: CompareOp::Less,
            value: Value::Float(5.0),
        }),
    };
    let rows = read(&mut session, &filtered);
    let ids: Vec<&Value> = rows.rows.iter().map(|r| &r[0]).collect();
    assert_eq!(ids, [&Value::Int(9), &Value::Int(8), &Value::Int(7)]);
    assert_eq!(session.count(&users, filtered.filter.as_ref(), &Cancel::default()), Ok(9));
    assert_eq!(session.count(&users, None, &Cancel::default()), Ok(25));

    // A table without a key: found by rowid; a view: no keys.
    run(session.as_mut(), "INSERT INTO log (line) VALUES ('a'), ('b')").unwrap();
    let mut log = Collect::default();
    session.read(&TableRef::new("log"), &Page::first(10), &mut log, &Cancel::default()).unwrap();
    assert_eq!(log.keys, vec![RowKey::RowId(1), RowKey::RowId(2)]);
    let mut named = Collect::default();
    session.read(&TableRef::new("named"), &Page::first(5), &mut named, &Cancel::default()).unwrap();
    assert_eq!((named.rows.len(), named.keys.len()), (5, 0));
    // A composite key, in its order.
    run(session.as_mut(), "INSERT INTO pairs VALUES ('b1', 'a1', 1)").unwrap();
    let mut pairs = Collect::default();
    session.read(&TableRef::new("pairs"), &Page::first(5), &mut pairs, &Cancel::default()).unwrap();
    assert_eq!(
        pairs.keys,
        vec![RowKey::Primary(vec![
            ("a".into(), Value::Text("a1".into())),
            ("b".into(), Value::Text("b1".into()))
        ])]
    );
}

#[test]
fn a_page_read_after_a_write_sees_it() {
    let dir = TempDir::new("cursor");
    let path = database(&dir, "shop.db", 20);
    let mut session = open(&path);
    let users = TableRef::new("users");
    let mut rows = Collect::default();
    session.read(&users, &Page::first(10), &mut rows, &Cancel::default()).unwrap();
    // Rows before the next page's start go: it starts further than its last rowid.
    run(session.as_mut(), "DELETE FROM users WHERE id <= 5").unwrap();
    let mut next = Collect::default();
    session
        .read(&users, &Page { offset: 10, ..Page::first(10) }, &mut next, &Cancel::default())
        .unwrap();
    assert_eq!(next.rows.len(), 5, "by OFFSET again: rows 16…20");
    assert_eq!(next.rows[0][0], Value::Int(16));
}

#[test]
fn writes_say_the_rows_they_changed() {
    let dir = TempDir::new("affected");
    let path = database(&dir, "shop.db", 3);
    let mut session = open(&path);
    let affected =
        |session: &mut Box<dyn Session>, sql: &str| run(session.as_mut(), sql).unwrap().0.affected;
    assert_eq!(affected(&mut session, "UPDATE users SET score = 1 WHERE id = 2"), Some(1));
    assert_eq!(affected(&mut session, "UPDATE users SET score = 1 WHERE id = 99"), Some(0));
    assert_eq!(affected(&mut session, "CREATE TABLE more (x)"), Some(0), "DDL changes no row");
    assert_eq!(affected(&mut session, "DELETE FROM users"), Some(3));
    assert_eq!(affected(&mut session, "SELECT 1"), None);
    let (outcome, rows) = run(session.as_mut(), "SELECT name FROM teams ORDER BY id").unwrap();
    assert_eq!((outcome.rows, rows.rows[0][0].clone()), (2, Value::Text("red".into())));
    assert!(matches!(run(session.as_mut(), "SELECT 1; SELECT 2"), Err(DbError::Query(_))));
    // Foreign keys are on: a team that isn't there is refused.
    let refused = run(session.as_mut(), "INSERT INTO users (name, team) VALUES ('x', 42)");
    assert!(matches!(refused, Err(DbError::Query(_))), "{refused:?}");
}

#[test]
fn a_long_query_is_cancelled_from_another_thread() {
    let dir = TempDir::new("cancel");
    let path = database(&dir, "shop.db", 0);
    let mut session = open(&path);
    let cancel = Cancel::default();
    let later = cancel.clone();
    let stop = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        later.cancel();
    });
    let started = Instant::now();
    let endless = "WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM n) \
                   SELECT count(*) FROM n";
    let result = session.execute(endless, &mut Collect::default(), &cancel);
    stop.join().unwrap();
    assert_eq!(result, Err(DbError::Cancelled));
    assert!(started.elapsed() < Duration::from_secs(10));
    // The session goes on.
    assert_eq!(run(session.as_mut(), "SELECT 1").unwrap().1.rows, vec![vec![Value::Int(1)]]);
}

#[test]
fn a_read_only_source_refuses_writes() {
    let dir = TempDir::new("readonly");
    let path = database(&dir, "shop.db", 2);
    let mut read_only = target(&path);
    read_only.options.insert(Target::READ_ONLY.into(), "true".into());
    let mut session = SqliteDriver.connect(&read_only, &Secret::default()).unwrap();
    assert_eq!(session.count(&TableRef::new("users"), None, &Cancel::default()), Ok(2));
    let refused = run(session.as_mut(), "DELETE FROM users");
    assert!(matches!(refused, Err(DbError::ReadOnly(_))), "{refused:?}");
    assert_eq!(session.count(&TableRef::new("users"), None, &Cancel::default()), Ok(2));
}

#[test]
fn a_wal_database_stays_wal() {
    let dir = TempDir::new("wal");
    let path = database(&dir, "shop.db", 2);
    let mode: String = Connection::open(&path)
        .unwrap()
        .query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
    let mut session = open(&path);
    run(session.as_mut(), "UPDATE users SET score = 9 WHERE id = 1").unwrap();
    let (_, mode) = run(session.as_mut(), "PRAGMA journal_mode").unwrap();
    assert_eq!(mode.rows[0][0], Value::Text("wal".into()));
}

#[test]
fn a_rebuild_reads_the_checks_triggers_and_views_of_its_table() {
    let dir = TempDir::new("rebuild");
    let path = database(&dir, "shop.db", 0);
    let mut session = open(&path);
    let source = session.rebuild_source(&TableRef::new("users")).unwrap();
    assert!(source.foreign_keys);
    assert_eq!(source.table.checks, vec!["length(name) > 0", "score IS NULL OR score >= 0"]);
    let triggers: Vec<&str> = source.table.triggers.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(triggers, ["users_touch"]);
    let written: Vec<&str> = source.table.written_indexes.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(written, ["users_lower"], "the expression's index, as written");
    assert!(!source.table.structure.indexes.iter().any(|i| i.name == "users_lower"));
    let views: Vec<&str> = source.table.views.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(views, ["named", "named_twice"], "and the view over the view");
    let teams = session.rebuild_source(&TableRef::new("teams")).unwrap();
    assert!(teams.table.views.is_empty() && teams.table.triggers.is_empty());
    assert!(session.rebuild_source(&TableRef::new("named")).is_err(), "a view isn't rebuilt");
}

// A table's rebuild and its changes, run on a real file as the explorer runs them: the
// domain's scripts through `database::transaction::run`.

/// A file of `schema` (run as it is) in `dir`, opened.
fn made(dir: &TempDir, schema: &str) -> (PathBuf, Box<dyn Session>) {
    let path = dir.file("data.db");
    Connection::open(&path).unwrap().execute_batch(schema).unwrap();
    let session = open(&path);
    (path, session)
}

fn rows(session: &mut dyn Session, sql: &str) -> Vec<Vec<Value>> {
    run(session, sql).unwrap().1.rows
}

fn text(s: &str) -> Value {
    Value::Text(s.into())
}

/// What `changes` would run, and what it did.
fn alter(
    session: &mut dyn Session,
    changes: &[SchemaChange],
) -> (Script, Result<Applied, DbError>) {
    let script = alter_plan(&Sqlite, session, changes).unwrap();
    let applied = transaction::run(session, &script, &Cancel::default(), &mut |_, _| {});
    (script, applied)
}

fn apply(session: &mut dyn Session, set: &ChangeSet) -> Result<Applied, DbError> {
    let structure = session.structure(&set.table).unwrap();
    let script = apply_script(&Sqlite, set, Some(&structure)).unwrap();
    transaction::run(session, &script, &Cancel::default(), &mut |_, _| {})
}

fn retype(table: TableRef, column: &str, to: ColumnDef) -> SchemaChange {
    SchemaChange::AlterColumn { table, column: column.into(), to }
}

const NOTES: &str = "
    CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT, size INTEGER);
    CREATE TABLE log (line TEXT);
    CREATE VIEW bodies AS SELECT id, body FROM notes;
    CREATE TRIGGER noted AFTER INSERT ON notes BEGIN
        INSERT INTO log (line) VALUES (new.body);
    END;
    INSERT INTO notes (body, size) VALUES ('one', 1), ('two', 2);
";

#[test]
fn a_column_renamed_with_a_rebuild_keeps_its_views_and_triggers_working() {
    let dir = TempDir::new("rename");
    let (_, mut session) = made(&dir, NOTES);
    let notes = TableRef::new("notes");
    let changes = [
        SchemaChange::RenameColumn { table: notes.clone(), from: "body".into(), to: "text".into() },
        retype(notes.clone(), "size", ColumnDef::new("size", "REAL").not_null()),
    ];
    let (script, applied) = alter(session.as_mut(), &changes);
    assert!(applied.is_ok(), "{applied:?}");
    assert!(script.stale.is_empty(), "nothing lost: {:?}", script.stale);
    assert_eq!(
        script.steps.iter().rev().nth(1).map(|s| s.sql.as_str()),
        Some("ALTER TABLE \"notes\" RENAME COLUMN \"body\" TO \"text\""),
        "renamed last, before the foreign key check"
    );
    // The view reads the renamed column, with its data.
    assert_eq!(
        rows(session.as_mut(), "SELECT * FROM bodies ORDER BY id"),
        vec![vec![Value::Int(1), text("one")], vec![Value::Int(2), text("two")]]
    );
    let view = rows(session.as_mut(), "SELECT sql FROM sqlite_schema WHERE name = 'bodies'");
    assert!(view[0][0].display().contains("text"), "{view:?}");
    // The trigger too.
    run(session.as_mut(), "INSERT INTO notes (text, size) VALUES ('three', 3)").unwrap();
    let log = rows(session.as_mut(), "SELECT line FROM log");
    assert_eq!(log, vec![vec![text("one")], vec![text("two")], vec![text("three")]]);
    let types = rows(session.as_mut(), "SELECT typeof(size) FROM notes WHERE id = 1");
    assert_eq!(types, vec![vec![text("real")]]);

    // A rename alone isn't a rebuild.
    let rename =
        [SchemaChange::RenameColumn { table: notes, from: "text".into(), to: "body".into() }];
    let (script, applied) = alter(session.as_mut(), &rename);
    assert!(applied.is_ok(), "{applied:?}");
    assert!(!script.steps.iter().any(|s| s.sql.starts_with("CREATE TABLE")), "{script:?}");
    assert_eq!(
        rows(session.as_mut(), "SELECT body FROM bodies WHERE id = 2"),
        vec![vec![text("two")]]
    );
}

#[test]
fn a_dropped_column_takes_what_names_it_and_says_so() {
    let dir = TempDir::new("stale");
    let (_, mut session) = made(&dir, &format!("{NOTES} CREATE INDEX sized ON notes (size);"));
    let notes = TableRef::new("notes");
    // `size` is indexed: a rebuild. The view names `body`, dropped too.
    let changes = [
        SchemaChange::DropColumn { table: notes.clone(), column: "size".into() },
        SchemaChange::DropColumn { table: notes, column: "body".into() },
    ];
    let (script, applied) = alter(session.as_mut(), &changes);
    assert!(applied.is_ok(), "{applied:?}");
    let stale: Vec<(ObjectKind, &str)> =
        script.stale.iter().map(|s| (s.kind, s.name.as_str())).collect();
    assert_eq!(
        stale,
        [
            (ObjectKind::Index, "sized"),
            (ObjectKind::View, "bodies"),
            (ObjectKind::Trigger, "noted")
        ]
    );
    assert_eq!(database::danger::script_dangers(&script).len(), 3);
    let left = rows(session.as_mut(), "SELECT name FROM sqlite_schema ORDER BY name");
    assert_eq!(left, vec![vec![text("log")], vec![text("notes")]]);
}

#[test]
fn a_collation_survives_a_rebuild() {
    let dir = TempDir::new("collate");
    let (_, mut session) = made(
        &dir,
        "CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT COLLATE NOCASE, age INTEGER);
         INSERT INTO people (name, age) VALUES ('Ana', 30);",
    );
    let people = TableRef::new("people");
    let before = session.structure(&people).unwrap();
    assert_eq!(before.column("name").unwrap().collation.as_deref(), Some("NOCASE"));
    let (_, applied) =
        alter(session.as_mut(), &[retype(people.clone(), "age", ColumnDef::new("age", "TEXT"))]);
    assert!(applied.is_ok(), "{applied:?}");
    let after = session.structure(&people).unwrap();
    assert_eq!(after.column("name").unwrap().collation.as_deref(), Some("NOCASE"));
    assert_eq!(
        rows(session.as_mut(), "SELECT count(*) FROM people WHERE name = 'ANA'"),
        vec![vec![Value::Int(1)]]
    );
}

#[test]
fn generated_columns_survive_a_rebuild_and_are_never_written() {
    let dir = TempDir::new("generated");
    let (_, mut session) = made(
        &dir,
        "CREATE TABLE items (
            id INTEGER PRIMARY KEY,
            price INTEGER,
            qty INTEGER,
            total INTEGER GENERATED ALWAYS AS (price * qty) STORED,
            label TEXT AS (upper(name)),
            name TEXT
         );
         INSERT INTO items (price, qty, name) VALUES (3, 2, 'pen');",
    );
    let items = TableRef::new("items");
    let before = session.structure(&items).unwrap();
    let total = before.column("total").unwrap().generated.clone();
    assert_eq!(total, Some(Generated { expression: "price * qty".into(), stored: true }));
    let label = before.column("label").unwrap().generated.clone();
    assert_eq!(label, Some(Generated { expression: "upper(name)".into(), stored: false }));

    let changes = [retype(
        items.clone(),
        "qty",
        ColumnDef::new("qty", "INTEGER").not_null().default(DefaultValue::Value(Value::Int(1))),
    )];
    let (_, applied) = alter(session.as_mut(), &changes);
    assert!(applied.is_ok(), "{applied:?}");
    assert_eq!(
        session.structure(&items).unwrap().columns,
        before
            .columns
            .iter()
            .map(|c| {
                let mut c = c.clone();
                if c.name == "qty" {
                    c.nullable = false;
                    c.default = Some("1".into());
                }
                c
            })
            .collect::<Vec<_>>()
    );
    run(session.as_mut(), "UPDATE items SET price = 5").unwrap();
    assert_eq!(
        rows(session.as_mut(), "SELECT total, label FROM items"),
        vec![vec![Value::Int(10), text("PEN")]],
        "still computed"
    );
    let hidden = rows(
        session.as_mut(),
        "SELECT hidden FROM pragma_table_xinfo('items') WHERE name IN ('total', 'label') ORDER BY name",
    );
    assert_eq!(hidden, vec![vec![Value::Int(2)], vec![Value::Int(3)]]);
    let mut set = ChangeSet::new(items);
    set.changes.push(Change::Insert { values: vec![("total".into(), Value::Int(1))] });
    let structure = session.structure(&set.table).unwrap();
    assert!(apply_script(&Sqlite, &set, Some(&structure)).is_err(), "a generated column");
}

#[test]
fn views_triggers_and_indexes_of_an_attached_schema_stay_in_it() {
    let dir = TempDir::new("attached");
    let (_, mut session) = made(&dir, "CREATE TABLE here (a TEXT);");
    let other = dir.file("other.db");
    Connection::open(&other)
        .unwrap()
        .execute_batch(
            "CREATE TABLE t (id INTEGER PRIMARY KEY, a TEXT, b INTEGER);
             CREATE INDEX t_a ON t (lower(a));
             CREATE VIEW v AS SELECT id, a FROM t;
             CREATE TRIGGER tr AFTER DELETE ON t BEGIN SELECT 1; END;
             INSERT INTO t (a, b) VALUES ('x', 1);",
        )
        .unwrap();
    let attach =
        format!("ATTACH DATABASE '{}' AS aux", other.to_string_lossy().replace('\'', "''"));
    run(session.as_mut(), &attach).unwrap();
    let t = TableRef::in_schema("aux", "t");
    let (_, applied) = alter(session.as_mut(), &[retype(t, "b", ColumnDef::new("b", "TEXT"))]);
    assert!(applied.is_ok(), "{applied:?}");
    let objects = |session: &mut dyn Session, schema: &str| {
        rows(session, &format!("SELECT name FROM {schema}.sqlite_schema ORDER BY name"))
    };
    assert_eq!(objects(session.as_mut(), "main"), vec![vec![text("here")]], "nothing made in main");
    assert_eq!(
        objects(session.as_mut(), "aux"),
        vec![vec![text("t")], vec![text("t_a")], vec![text("tr")], vec![text("v")]]
    );
    assert_eq!(rows(session.as_mut(), "SELECT a FROM aux.v"), vec![vec![text("x")]]);
}

#[test]
fn a_partial_index_survives_and_a_dropped_one_stays_dropped() {
    let dir = TempDir::new("indexes");
    let (_, mut session) = made(
        &dir,
        "CREATE TABLE t (id INTEGER PRIMARY KEY, a TEXT, b INTEGER, c INTEGER);
         CREATE INDEX t_partial ON t (a) WHERE b > 0;
         CREATE INDEX t_lower ON t (lower(a));
         CREATE INDEX t_c ON t (c) WHERE c IS NOT NULL;",
    );
    let t = TableRef::new("t");
    let changes = [
        retype(t.clone(), "b", ColumnDef::new("b", "REAL")),
        SchemaChange::DropIndex { table: t.clone(), name: "t_lower".into() },
    ];
    let (_, applied) = alter(session.as_mut(), &changes);
    assert!(applied.is_ok(), "{applied:?}");
    let indexes = |session: &mut dyn Session| {
        rows(session, "SELECT name, sql FROM sqlite_schema WHERE type = 'index' ORDER BY name")
    };
    assert_eq!(
        indexes(session.as_mut()),
        vec![
            vec![text("t_c"), text("CREATE INDEX t_c ON t (c) WHERE c IS NOT NULL")],
            vec![text("t_partial"), text("CREATE INDEX t_partial ON t (a) WHERE b > 0")],
        ]
    );
    // Its column dropped: it goes, and says so.
    let drop = [SchemaChange::DropColumn { table: t, column: "c".into() }];
    let (script, applied) = alter(session.as_mut(), &drop);
    assert!(applied.is_ok(), "{applied:?}");
    assert_eq!(script.stale.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["t_c"]);
    assert_eq!(indexes(session.as_mut()).len(), 1);
}

#[test]
fn a_table_with_a_column_named_rowid_is_edited_by_another_name_of_its_row_id() {
    let dir = TempDir::new("rowid");
    let (_, mut session) = made(
        &dir,
        "CREATE TABLE r (rowid TEXT, line TEXT);
         INSERT INTO r VALUES ('a', 'one'), ('b', 'two');",
    );
    let r = TableRef::new("r");
    let structure = session.structure(&r).unwrap();
    assert_eq!((structure.row_id_name(), structure.key_kind()), (Some("_rowid_"), KeyKind::RowId));
    let mut page = Collect::default();
    session.read(&r, &Page::first(10), &mut page, &Cancel::default()).unwrap();
    assert_eq!(page.keys, vec![RowKey::RowId(1), RowKey::RowId(2)]);
    assert_eq!(page.rows[1], vec![text("b"), text("two")], "its own rowid column, as it is");

    let mut set = ChangeSet::new(r.clone());
    set.changes.push(Change::Update {
        key: RowKey::RowId(2),
        cells: vec![CellChange { column: "line".into(), old: text("two"), new: text("TWO") }],
    });
    set.changes.push(Change::Insert { values: vec![("line".into(), text("three"))] });
    let applied = apply(session.as_mut(), &set).unwrap();
    assert_eq!(applied, Applied { affected: 2, inserted: vec![RowKey::RowId(3)] });
    assert_eq!(
        rows(session.as_mut(), "SELECT line FROM r ORDER BY _rowid_"),
        vec![vec![text("one")], vec![text("TWO")], vec![text("three")],]
    );

    // All three names hidden, no key: only read.
    let (_, mut hidden) =
        made(&TempDir::new("rowid-hidden"), "CREATE TABLE h (rowid, _rowid_, oid)");
    let h = hidden.structure(&TableRef::new("h")).unwrap();
    assert_eq!(h.key_kind(), KeyKind::None);
}

#[test]
fn inserts_give_back_their_rows_keys() {
    let dir = TempDir::new("inserted");
    let path = database(&dir, "shop.db", 2);
    let mut session = open(&path);
    let mut set = ChangeSet::new(TableRef::new("users"));
    for name in ["a", "b"] {
        set.changes.push(Change::Insert { values: vec![("name".into(), text(name))] });
    }
    let applied = apply(session.as_mut(), &set).unwrap();
    let key = |id| RowKey::Primary(vec![("id".into(), Value::Int(id))]);
    assert_eq!(applied.inserted, vec![key(3), key(4)]);
    let mut pairs = ChangeSet::new(TableRef::new("pairs"));
    pairs
        .changes
        .push(Change::Insert { values: vec![("a".into(), text("x")), ("b".into(), text("y"))] });
    let applied = apply(session.as_mut(), &pairs).unwrap();
    assert_eq!(
        applied.inserted,
        vec![RowKey::Primary(vec![("a".into(), text("x")), ("b".into(), text("y"))])]
    );
}

const FAMILY: &str = "
    CREATE TABLE parents (id INTEGER PRIMARY KEY, name TEXT);
    CREATE TABLE kids (id INTEGER PRIMARY KEY, parent INTEGER REFERENCES parents (id), note TEXT);
    INSERT INTO parents (name) VALUES ('p');
    PRAGMA foreign_keys = OFF;
    INSERT INTO kids (parent, note) VALUES (1, 'ok'), (99, 'orphan');
";

#[test]
fn foreign_keys_broken_before_dont_block_a_rebuild_but_new_ones_roll_it_back() {
    let dir = TempDir::new("foreign");
    let (path, mut session) = made(&dir, FAMILY);
    assert_eq!(rows(session.as_mut(), "PRAGMA foreign_key_check").len(), 1, "broken before");
    let kids = TableRef::new("kids");
    let note = || {
        retype(
            kids.clone(),
            "note",
            ColumnDef::new("note", "TEXT").not_null().default(DefaultValue::Value(text(""))),
        )
    };
    let (_, applied) = alter(session.as_mut(), &[note()]);
    assert!(applied.is_ok(), "{applied:?}");
    assert_eq!(rows(session.as_mut(), "SELECT count(*) FROM kids"), vec![vec![Value::Int(2)]]);
    assert_eq!(
        rows(session.as_mut(), "PRAGMA foreign_keys"),
        vec![vec![Value::Int(1)]],
        "on again"
    );

    // Rebuilding `kids` and dropping `parents` (several tables, one transaction): the kid
    // that was fine is broken now — everything rolled back.
    drop(session);
    let before = std::fs::read(&path).unwrap();
    let mut session = open(&path);
    let changes = [
        retype(kids.clone(), "note", ColumnDef::new("note", "TEXT")),
        SchemaChange::DropTable { table: TableRef::new("parents") },
    ];
    let (script, applied) = alter(session.as_mut(), &changes);
    assert!(
        matches!(&applied, Err(DbError::Query(why)) if why.contains("foreign key")),
        "{applied:?}"
    );
    assert!(script.steps.iter().any(|s| s.sql == "DROP TABLE \"parents\""));
    assert_eq!(rows(session.as_mut(), "PRAGMA foreign_keys"), vec![vec![Value::Int(1)]]);
    drop(session);
    assert_eq!(std::fs::read(&path).unwrap(), before, "the file unchanged");
}

#[test]
fn several_tables_are_rebuilt_in_one_transaction() {
    let dir = TempDir::new("several");
    let (_, mut session) = made(&dir, FAMILY);
    let changes = [
        retype(
            TableRef::new("parents"),
            "name",
            ColumnDef::new("name", "TEXT").not_null().default(DefaultValue::Value(text("?"))),
        ),
        retype(TableRef::new("kids"), "note", ColumnDef::new("note", "VARCHAR(20)")),
        SchemaChange::AddColumn {
            table: TableRef::new("parents"),
            column: ColumnDef::new("age", "INTEGER"),
        },
    ];
    let (script, applied) = alter(session.as_mut(), &changes);
    assert!(applied.is_ok(), "{applied:?}");
    let rebuilt = script.steps.iter().filter(|s| s.sql.starts_with("DROP TABLE")).count();
    assert_eq!(rebuilt, 2);
    let columns = rows(session.as_mut(), "SELECT name, type FROM pragma_table_info('parents')");
    assert_eq!(columns.len(), 3);
    let kids =
        rows(session.as_mut(), "SELECT type FROM pragma_table_info('kids') WHERE name = 'note'");
    assert_eq!(kids, vec![vec![text("VARCHAR(20)")]]);
    let fk = rows(session.as_mut(), "SELECT \"table\" FROM pragma_foreign_key_list('kids')");
    assert_eq!(fk, vec![vec![text("parents")]]);
}

// ─── The console (plan 39c) ───

fn console_run(session: &mut dyn Session, sql: &str, limit: u32) -> (RunOutcome, Collect) {
    let mut rows = Collect::default();
    let options = RunOptions { fetch_limit: limit, keep_cursor: true };
    let outcome = session.run(sql, &options, &mut rows, &Cancel::default()).unwrap();
    (outcome, rows)
}

#[test]
fn the_console_runs_a_page_and_says_more_are_left() {
    let dir = TempDir::new("console-run");
    let path = database(&dir, "shop.db", 30);
    let mut session = open(&path);
    assert_eq!(session.console(), Some(CONSOLE));
    assert_eq!(SqliteDriver.capabilities().console, Some(CONSOLE));
    let (outcome, rows) =
        console_run(session.as_mut(), "SELECT id, name FROM users ORDER BY id", 10);
    assert_eq!(rows.rows.len(), 10);
    assert_eq!(rows.rows[9][1], Value::Text("user 10".into()));
    assert!(outcome.truncated && outcome.more.is_none() && !outcome.stopped);
    assert_eq!((outcome.rows, outcome.affected), (10, None));
    let (outcome, rows) = console_run(session.as_mut(), "SELECT id FROM users", 30);
    assert!(!outcome.truncated && rows.rows.len() == 30, "exactly the page: nothing left");
    // A write past the page runs to its end: every row changed, only the page shown.
    let (outcome, rows) =
        console_run(session.as_mut(), "UPDATE users SET score = score + 1 RETURNING id", 5);
    assert_eq!((rows.rows.len(), outcome.truncated, outcome.affected), (5, true, Some(30)));
    let changed = rows_of(session.as_mut(), "SELECT count(*) FROM users WHERE score > 1");
    assert_eq!(changed, vec![vec![Value::Int(30)]]);
    // No cursor in SQLite: the console runs a pure query again.
    let token = CursorToken("x".into());
    let fetched = session.fetch(&token, 10, &mut Collect::default(), &Cancel::default());
    assert!(matches!(fetched, Err(DbError::Unsupported(_))));
}

fn rows_of(session: &mut dyn Session, sql: &str) -> Vec<Vec<Value>> {
    run(session, sql).unwrap().1.rows
}

#[test]
fn a_console_run_is_cancelled_through_the_interrupt() {
    let dir = TempDir::new("console-cancel");
    let path = database(&dir, "shop.db", 0);
    let mut session = open(&path);
    let cancel = Cancel::default();
    let later = cancel.clone();
    let stop = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        later.cancel();
    });
    let endless = "WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM n) \
                   SELECT count(*) FROM n";
    let started = Instant::now();
    let result = session.run(endless, &RunOptions::default(), &mut Collect::default(), &cancel);
    stop.join().unwrap();
    assert_eq!(result, Err(DbError::Cancelled));
    assert!(started.elapsed() < Duration::from_secs(10));
    assert_eq!(console_run(session.as_mut(), "SELECT 2", 10).1.rows, vec![vec![Value::Int(2)]]);
}

#[test]
fn a_syntax_error_says_where_it_is() {
    let dir = TempDir::new("console-error");
    let path = database(&dir, "shop.db", 0);
    let mut session = open(&path);
    let sql = "SELECT id FROM users WHERE name ==== 'x'";
    let error =
        session.run(sql, &RunOptions::default(), &mut Collect::default(), &Cancel::default());
    let Err(DbError::Sql(error)) = error else { panic!("{error:?}") };
    let Some(database::sql::EnginePosition::Byte(at)) = error.position else { panic!("{error:?}") };
    assert!(sql[at..].starts_with("=="), "at the operator: {:?}", &sql[at..]);
    assert!(error.message.contains("syntax error"), "{}", error.message);
    // An error SQLite places nowhere reads as before.
    let error = session
        .run(
            "SELECT * FROM nope",
            &RunOptions::default(),
            &mut Collect::default(),
            &Cancel::default(),
        )
        .unwrap_err();
    assert!(error.to_string().contains("no such table: nope"), "{error}");
}

#[test]
fn transactions_begin_by_kind_and_are_seen_open() {
    let dir = TempDir::new("console-tx");
    let path = database(&dir, "shop.db", 3);
    let mut session = open(&path);
    assert!(!session.in_transaction());
    let immediate = TxOptions { isolation: Some(Isolation::Immediate), read_only: false };
    session.begin(&immediate).unwrap();
    assert!(session.in_transaction());
    console_run(session.as_mut(), "DELETE FROM users", 10);
    session.rollback().unwrap();
    assert!(!session.in_transaction());
    assert_eq!(rows_of(session.as_mut(), "SELECT count(*) FROM users"), vec![vec![Value::Int(3)]]);
    // A BEGIN written by hand is seen too; a commit keeps the change.
    console_run(session.as_mut(), "BEGIN", 10);
    assert!(session.in_transaction());
    console_run(session.as_mut(), "DELETE FROM users WHERE id = 1", 10);
    session.commit().unwrap();
    assert!(!session.in_transaction());
    assert_eq!(rows_of(session.as_mut(), "SELECT count(*) FROM users"), vec![vec![Value::Int(2)]]);
    // What SQLite doesn't have is refused.
    let serializable = TxOptions { isolation: Some(Isolation::Serializable), read_only: false };
    assert!(matches!(session.begin(&serializable), Err(DbError::Unsupported(_))));
    let read_only = TxOptions { isolation: None, read_only: true };
    assert!(matches!(session.begin(&read_only), Err(DbError::Unsupported(_))));
    assert!(!session.in_transaction());
}

#[test]
fn a_transaction_held_by_the_console_makes_another_session_wait_then_refuse() {
    let dir = TempDir::new("console-lock");
    let path = database(&dir, "shop.db", 2);
    let mut console = open(&path);
    console.begin(&TxOptions { isolation: Some(Isolation::Immediate), read_only: false }).unwrap();
    let conn = Connection::open(&path).unwrap();
    conn.busy_timeout(Duration::from_millis(50)).unwrap();
    let mut grid = SqliteSession { conn, cursors: Vec::new(), schema: None };
    let refused = run(&mut grid, "UPDATE users SET score = 0");
    assert!(matches!(refused, Err(DbError::InUse(_))), "{refused:?}");
    console.rollback().unwrap();
    assert!(run(&mut grid, "UPDATE users SET score = 0").is_ok());
}

#[test]
fn the_plan_is_read_as_a_tree() {
    let dir = TempDir::new("console-plan");
    let path = database(&dir, "shop.db", 2);
    let mut session = open(&path);
    let plan = session
        .explain(
            "SELECT u.name, (SELECT name FROM teams t WHERE t.id = u.team) FROM users u \
             WHERE u.name = 'x' ORDER BY u.email;",
            false,
            &Cancel::default(),
        )
        .unwrap();
    let all: Vec<&PlanNode> = plan.iter().flat_map(PlanNode::walk).collect();
    assert!(
        all.iter().any(|n| n.object.as_deref() == Some("u") && n.operation.contains("users_name")),
        "{plan:#?}"
    );
    assert!(plan.iter().any(|n| !n.children.is_empty()), "the subquery under its node: {plan:#?}");
    // As it runs: measured by the driver (SQLite has no `EXPLAIN ANALYZE`).
    let analyzed = session.explain("SELECT * FROM users", true, &Cancel::default()).unwrap();
    assert_eq!(analyzed[0].actual_rows, Some(2.0), "{analyzed:#?}");
}

#[test]
fn schemas_are_the_attached_files_and_only_named() {
    let dir = TempDir::new("console-schema");
    let path = database(&dir, "shop.db", 1);
    let other = dir.file("other.db");
    Connection::open(&other).unwrap().execute_batch("CREATE TABLE things (x)").unwrap();
    let mut session = open(&path);
    run(session.as_mut(), &format!("ATTACH '{}' AS aux", other.display())).unwrap();
    assert_eq!(session.databases().unwrap(), ["main", "aux"]);
    assert_eq!(session.current().unwrap().schema.as_deref(), Some("main"));
    session.set_schema("AUX").unwrap();
    assert_eq!(session.current().unwrap(), Place { database: None, schema: Some("aux".into()) });
    assert!(matches!(session.set_schema("nope"), Err(DbError::Query(_))));
    let aux = session.catalog_columns(Some("aux"), &Cancel::default()).unwrap();
    assert_eq!(aux.len(), 1);
    assert_eq!(aux[0].0, TableRef::in_schema("aux", "things"));
}

#[test]
fn every_column_of_two_thousand_tables_comes_in_one_query() {
    let dir = TempDir::new("console-catalog");
    let path = dir.file("big.db");
    let conn = Connection::open(&path).unwrap();
    let mut ddl = String::from("BEGIN;");
    for t in 0..2000 {
        ddl.push_str(&format!(
            "CREATE TABLE t{t:04} (id INTEGER PRIMARY KEY, name TEXT NOT NULL DEFAULT 'n', v REAL);"
        ));
    }
    ddl.push_str("CREATE VIEW v_all AS SELECT id, name FROM t0000; COMMIT;");
    conn.execute_batch(&ddl).unwrap();
    drop(conn);
    let mut session = open(&path);
    let started = Instant::now();
    let all = session.catalog_columns(None, &Cancel::default()).unwrap();
    let took = started.elapsed();
    assert_eq!(all.len(), 2001);
    assert_eq!(all[0].0, TableRef::new("t0000"));
    let columns = &all[0].1;
    assert_eq!(columns.len(), 3);
    assert_eq!((columns[0].name.as_str(), columns[0].key_position), ("id", Some(1)));
    assert!(!columns[1].nullable && columns[1].default.as_deref() == Some("'n'"));
    assert_eq!(columns[2].type_name, "REAL");
    assert!(all.iter().any(|(t, c)| t.name == "v_all" && c.len() == 2), "views too");
    assert!(took < Duration::from_secs(5), "one query: {took:?}");
}

#[test]
fn every_foreign_key_comes_in_one_query() {
    let dir = TempDir::new("console-keys");
    let path = dir.file("keys.db");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
         CREATE TABLE orders (id INTEGER PRIMARY KEY, user_id INTEGER REFERENCES users(id) ON DELETE CASCADE);
         CREATE TABLE items (order_id INTEGER REFERENCES orders, a INT, b INT,
             FOREIGN KEY (a, b) REFERENCES pairs (x, y));
         CREATE TABLE pairs (x INT, y INT, PRIMARY KEY (x, y));
         CREATE VIEW v AS SELECT * FROM orders;",
    )
    .unwrap();
    drop(conn);
    let mut session = open(&path);
    let all = session.catalog_keys(None, &Cancel::default()).unwrap();
    let names: Vec<&str> = all.iter().map(|(t, _)| t.name.as_str()).collect();
    assert_eq!(names, ["items", "orders"], "tables without keys left out");
    let orders = &all[1].1;
    assert_eq!(orders.len(), 1);
    assert_eq!(orders[0].table, TableRef::new("users"));
    assert_eq!(
        (orders[0].columns.as_slice(), orders[0].references.as_slice()),
        (&["user_id".to_owned()][..], &["id".to_owned()][..])
    );
    assert_eq!(orders[0].on_delete, database::FkAction::Cascade);
    let items = &all[0].1;
    assert_eq!(items.len(), 2);
    let pair = items.iter().find(|k| k.table.name == "pairs").unwrap();
    assert_eq!(pair.columns, ["a", "b"]);
    assert_eq!(pair.references, ["x", "y"]);
    let implied = items.iter().find(|k| k.table.name == "orders").unwrap();
    assert!(implied.references.is_empty(), "`REFERENCES orders`: its key, resolved by the index");
}

#[test]
fn a_broken_view_doesnt_hide_the_other_tables_columns() {
    let dir = TempDir::new("console-broken");
    let path = database(&dir, "shop.db", 0);
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE gone (x); CREATE VIEW broken AS SELECT x FROM gone; DROP TABLE gone;",
    )
    .unwrap();
    drop(conn);
    let mut session = open(&path);
    let all = session.catalog_columns(Some("main"), &Cancel::default()).unwrap();
    let names: Vec<&str> = all.iter().map(|(t, _)| t.name.as_str()).collect();
    assert!(names.contains(&"users") && names.contains(&"teams"), "{names:?}");
    assert!(!names.contains(&"broken"), "{names:?}");
}

#[test]
fn the_functions_come_from_sqlite_itself() {
    let dir = TempDir::new("console-routines");
    let path = database(&dir, "shop.db", 0);
    let mut session = open(&path);
    let routines = session.routines(None, &Cancel::default()).unwrap();
    let find = |name: &str| routines.iter().filter(|r| r.name == name).collect::<Vec<_>>();
    assert!(find("substr").iter().any(|r| r.arity == Some(3) && r.builtin));
    assert!(
        find("max").iter().any(|r| r.kind == database::RoutineKind::Function && r.arity.is_none())
    );
    assert!(find("json_group_array").iter().all(|r| r.kind != database::RoutineKind::Function));
    assert!(find("row_number").iter().any(|r| r.kind == database::RoutineKind::Window));
    assert!(find("printf").iter().any(|r| r.arity.is_none()), "any number of arguments");
    let once: std::collections::HashSet<(&str, Option<u16>)> =
        routines.iter().map(|r| (r.name.as_str(), r.arity)).collect();
    assert!(once.len() >= routines.len() - routines.len() / 10, "listed about once each");
}
