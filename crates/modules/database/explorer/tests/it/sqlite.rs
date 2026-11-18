//! The worker and the repository on a real SQLite file (in the system's temp dir, never the
//! user's), its driver found as the app finds it (registered, by id): pages, totals,
//! changes applied in one transaction (a row changed since it was read: nothing written),
//! a table rebuilt with its data, indexes, triggers, views and foreign keys kept, a long
//! query cancelled, a read-only source left untouched.

// Its helpers fail the test they serve, as the tests' own unwraps do.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use database::{
    Applied, Cancel, CellChange, Change, ChangeSet, Collect, ColumnDef, DataSource, DbError,
    Driver, Fetch, Page, RepositoryEvent, RowKey, SchemaChange, Scope, Secret, TableRef,
    TableRepository, Target, Value,
};
use database_explorer::{Client, Reply, Request, Response, Runtime, SessionRepository, Sessions};
use kernel::{EventBus, ProjectId};

const WAIT: Duration = Duration::from_secs(20);

/// A folder of the system's temp dir, gone with it.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "explorer-sqlite-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const SCHEMA: &[&str] = &[
    "CREATE TABLE teams (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL UNIQUE)",
    "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT UNIQUE, \
     team INTEGER REFERENCES teams (id), score INTEGER DEFAULT 0, CHECK (score >= 0))",
    "CREATE INDEX users_name ON users (name)",
    "CREATE INDEX users_score ON users (score)",
    "CREATE TABLE log (line TEXT)",
    "CREATE VIEW named AS SELECT id, name FROM users",
    "CREATE TRIGGER users_touch AFTER UPDATE ON users BEGIN \
     INSERT INTO log (line) VALUES ('updated ' || new.id); END",
    "CREATE VIEW endless AS WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM n) \
     SELECT count(*) AS c FROM n",
    "INSERT INTO teams (name) VALUES ('red'), ('blue')",
    "INSERT INTO users (name, email, team, score) VALUES \
     ('ana', 'ana@example.com', 1, 10), ('bob', 'bob@example.com', 2, 20), \
     ('cid', 'cid@example.com', 1, 30), ('dan', 'dan@example.com', 2, 40), \
     ('eve', 'eve@example.com', 1, 50)",
];

/// The project's folder with `shop.db` made of [`SCHEMA`].
fn project(name: &str) -> TempDir {
    let dir = TempDir::new(name);
    let mut target = Target {
        database: dir.0.join("shop.db").to_string_lossy().into_owned(),
        ..Target::default()
    };
    target.options.insert("create".into(), "true".into());
    let mut session = db_sqlite::SqliteDriver.connect(&target, &Secret::default()).unwrap();
    for sql in SCHEMA {
        session.execute(sql, &mut Collect::default(), &Cancel::default()).unwrap();
    }
    dir
}

/// A direct look at the file (another connection).
fn look(dir: &Path, sql: &str) -> Vec<Vec<Value>> {
    let target = Target {
        database: dir.join("shop.db").to_string_lossy().into_owned(),
        ..Target::default()
    };
    let mut session = db_sqlite::SqliteDriver.connect(&target, &Secret::default()).unwrap();
    let mut rows = Collect::default();
    session.execute(sql, &mut rows, &Cancel::default()).unwrap();
    rows.rows
}

/// A woken view: `wake` for the worker, `rx` to wait for it.
fn waker() -> (Arc<dyn Fn() + Send + Sync>, Receiver<()>) {
    let (tx, rx) = channel();
    let tx = Mutex::new(tx);
    (
        Arc::new(move || {
            let _ = tx.lock().unwrap().send(());
        }),
        rx,
    )
}

struct Open {
    sessions: Arc<Sessions>,
    worker: database_explorer::SessionWorker,
    events: EventBus,
}

impl Drop for Open {
    fn drop(&mut self) {
        self.sessions.close_all();
    }
}

/// `shop.db` of `dir` as the project's source, its driver found by id among those
/// registered (as the app registers the modules' contributions).
fn open(dir: &TempDir, read_only: bool) -> Open {
    database::drivers::register(db_sqlite::factory());
    let events = EventBus::new();
    let sessions = Sessions::new(ProjectId::new(), &dir.0, events.clone(), Runtime::pool(), None);
    let mut source = DataSource::new("shop", "sqlite", Scope::Project);
    source.file = Some(PathBuf::from("shop.db"));
    source.read_only = read_only;
    let worker = sessions.open_source(&source, Secret::default()).unwrap();
    Open { sessions, worker, events }
}

/// The repository's events until `done` says enough.
fn until(
    repo: &mut SessionRepository,
    rx: &Receiver<()>,
    done: impl Fn(&[RepositoryEvent]) -> bool,
) -> Vec<RepositoryEvent> {
    let mut seen = Vec::new();
    loop {
        seen.extend(repo.poll());
        if done(&seen) {
            return seen;
        }
        rx.recv_timeout(WAIT).expect("an answer in time");
    }
}

fn ready<T: Clone>(
    repo: &mut SessionRepository,
    rx: &Receiver<()>,
    get: impl Fn(&mut SessionRepository) -> Fetch<T>,
) -> Result<T, DbError> {
    loop {
        match get(repo) {
            Fetch::Ready(value) => return Ok(value),
            Fetch::Failed(e) => return Err(e),
            Fetch::Pending => {
                repo.poll();
                if !get(repo).is_pending() {
                    continue;
                }
                rx.recv_timeout(WAIT).expect("an answer in time");
            }
        }
    }
}

fn reply(client: &Client, rx: &Receiver<()>) -> Reply {
    loop {
        if let Some(reply) = client.poll().into_iter().next() {
            return reply;
        }
        rx.recv_timeout(WAIT).expect("an answer in time");
    }
}

fn users() -> TableRef {
    TableRef::new("users")
}

fn key(id: i64) -> RowKey {
    RowKey::Primary(vec![("id".into(), Value::Int(id))])
}

fn applied(events: &[RepositoryEvent]) -> Option<Result<Applied, DbError>> {
    events.iter().find_map(|e| match e {
        RepositoryEvent::Applied(result) => Some(result.clone()),
        _ => None,
    })
}

#[test]
fn a_table_is_read_and_changed_through_the_repository() {
    let dir = project("repository");
    let open = open(&dir, false);
    let (wake, rx) = waker();
    let mut repo = SessionRepository::new(&open.worker, users(), wake, Some(&open.events), 1 << 20);

    let structure = ready(&mut repo, &rx, |r| r.structure()).unwrap();
    assert_eq!(structure.primary_key(), vec!["id".to_owned()]);
    assert_eq!(ready(&mut repo, &rx, |r| r.count(None)), Ok(5));
    let page = ready(&mut repo, &rx, |r| r.page(&Page::first(2))).unwrap();
    assert_eq!(page.rows.len(), 2);
    assert_eq!(page.keys, vec![key(1), key(2)]);
    assert_eq!(page.rows[1][1], Value::Text("bob".into()));

    let changes = ChangeSet {
        table: users(),
        changes: vec![
            Change::Insert {
                values: vec![
                    ("name".into(), Value::Text("fay".into())),
                    ("team".into(), Value::Int(2)),
                ],
            },
            Change::Update {
                key: key(2),
                cells: vec![CellChange {
                    column: "name".into(),
                    old: Value::Text("bob".into()),
                    new: Value::Text("rob".into()),
                }],
            },
            Change::Delete { key: key(3) },
        ],
    };
    let preview = repo.preview(&changes).unwrap();
    assert_eq!(preview.first().map(String::as_str), Some("BEGIN IMMEDIATE"));
    repo.apply(changes);
    // The commit's outcome and the refresh after it: the change announced on the bus may
    // refresh the repository first (a `Refreshed` before the `Applied`).
    let events =
        until(&mut repo, &rx, |e| applied(e).is_some() && e.contains(&RepositoryEvent::Refreshed));
    assert_eq!(applied(&events).map(|r| r.map(|a| a.affected)), Some(Ok(3)));
    assert_eq!(ready(&mut repo, &rx, |r| r.count(None)), Ok(5));
    assert_eq!(
        look(&dir.0, "SELECT name FROM users ORDER BY id"),
        vec![
            vec![Value::Text("ana".into())],
            vec![Value::Text("rob".into())],
            vec![Value::Text("dan".into())],
            vec![Value::Text("eve".into())],
            vec![Value::Text("fay".into())],
        ]
    );
    // The trigger ran with the update, in the same transaction.
    assert_eq!(look(&dir.0, "SELECT line FROM log"), vec![vec![Value::Text("updated 2".into())]]);
}

#[test]
fn a_row_changed_since_it_was_read_writes_nothing() {
    let dir = project("conflict");
    let open = open(&dir, false);
    let (wake, rx) = waker();
    let mut repo = SessionRepository::new(&open.worker, users(), wake, Some(&open.events), 1 << 20);
    ready(&mut repo, &rx, |r| r.structure()).unwrap();
    // Changed by another program after it was read.
    look(&dir.0, "UPDATE users SET name = 'BOB' WHERE id = 2");
    let changes = ChangeSet {
        table: users(),
        changes: vec![
            Change::Delete { key: key(5) },
            Change::Update {
                key: key(2),
                cells: vec![CellChange {
                    column: "name".into(),
                    old: Value::Text("bob".into()),
                    new: Value::Text("rob".into()),
                }],
            },
            Change::Insert { values: vec![("name".into(), Value::Text("new".into()))] },
        ],
    };
    repo.apply(changes);
    let events = until(&mut repo, &rx, |e| applied(e).is_some());
    assert!(matches!(applied(&events), Some(Err(DbError::Conflict(_)))), "{events:?}");
    // Rolled back: the delete before it too, the insert never ran.
    assert_eq!(look(&dir.0, "SELECT count(*) FROM users"), vec![vec![Value::Int(5)]]);
    assert_eq!(
        look(&dir.0, "SELECT name FROM users WHERE id = 2"),
        vec![vec![Value::Text("BOB".into())]]
    );
    // Only the other program's update ran the trigger.
    assert_eq!(look(&dir.0, "SELECT line FROM log"), vec![vec![Value::Text("updated 2".into())]]);
}

fn alter(open: &Open, changes: Vec<SchemaChange>) -> Result<Response, DbError> {
    let (wake, rx) = waker();
    let client = open.worker.client(wake);
    client.send(Request::Alter(changes));
    reply(&client, &rx).result
}

#[test]
fn a_table_is_rebuilt_with_its_data_indexes_triggers_views_and_keys() {
    let dir = project("rebuild");
    let open = open(&dir, false);
    let changes = vec![
        // SQLite can't: dropping a column with a unique constraint, changing a type.
        SchemaChange::DropColumn { table: users(), column: "email".into() },
        SchemaChange::AlterColumn {
            table: users(),
            column: "score".into(),
            to: ColumnDef::new("score", "REAL").not_null(),
        },
        SchemaChange::RenameColumn { table: users(), from: "team".into(), to: "squad".into() },
    ];
    let result = alter(&open, changes);
    assert!(matches!(result, Ok(Response::Applied(_))), "{result:?}");

    let columns = look(&dir.0, "SELECT name, type, \"notnull\" FROM pragma_table_info('users')");
    let columns: Vec<String> = columns
        .iter()
        .map(|r| format!("{}:{}:{}", r[0].display(), r[1].display(), r[2].display()))
        .collect();
    assert_eq!(columns, ["id:INTEGER:0", "name:TEXT:1", "squad:INTEGER:0", "score:REAL:1"]);
    assert_eq!(
        look(&dir.0, "SELECT id, name, squad, typeof(score) FROM users WHERE id = 3"),
        vec![vec![
            Value::Int(3),
            Value::Text("cid".into()),
            Value::Int(1),
            Value::Text("real".into())
        ]],
        "the data copied, the rowids kept"
    );
    let indexes = look(&dir.0, "SELECT name FROM pragma_index_list('users') ORDER BY name");
    assert_eq!(
        indexes,
        vec![vec![Value::Text("users_name".into())], vec![Value::Text("users_score".into())]]
    );
    // The foreign key followed the rename, and holds.
    let fk = look(&dir.0, "SELECT \"from\", \"table\" FROM pragma_foreign_key_list('users')");
    assert_eq!(fk, vec![vec![Value::Text("squad".into()), Value::Text("teams".into())]]);
    assert!(look(&dir.0, "PRAGMA foreign_key_check").is_empty());
    // The check stays.
    let session_check = look(&dir.0, "SELECT sql FROM sqlite_schema WHERE name = 'users'");
    assert!(session_check[0][0].display().contains("CHECK (score >= 0)"), "{session_check:?}");
    // The trigger and the view are back.
    look(&dir.0, "UPDATE users SET name = 'ann' WHERE id = 1");
    assert_eq!(look(&dir.0, "SELECT line FROM log"), vec![vec![Value::Text("updated 1".into())]]);
    assert_eq!(look(&dir.0, "SELECT count(*) FROM named"), vec![vec![Value::Int(5)]]);
}

#[test]
fn a_rebuild_that_fails_changes_nothing() {
    let dir = project("failed");
    let open = open(&dir, false);
    let before = look(&dir.0, "SELECT sql FROM sqlite_schema ORDER BY name");
    // `score` has an index (a rebuild) and the table's CHECK names it: the new table can't
    // be made, and the whole rebuild fails with it.
    let changes = vec![
        SchemaChange::RenameColumn { table: users(), from: "team".into(), to: "squad".into() },
        SchemaChange::DropColumn { table: users(), column: "score".into() },
    ];
    let result = alter(&open, changes);
    // SQLite says where in its statement (a refusal with its place).
    assert!(matches!(&result, Err(DbError::Sql(e)) if e.message.contains("score")), "{result:?}");
    assert_eq!(look(&dir.0, "SELECT sql FROM sqlite_schema ORDER BY name"), before);
    assert_eq!(look(&dir.0, "SELECT count(*) FROM users"), vec![vec![Value::Int(5)]]);
}

#[test]
fn a_long_query_is_cancelled_and_the_session_goes_on() {
    let dir = project("cancel");
    let open = open(&dir, false);
    let (wake, rx) = waker();
    let client = open.worker.client(wake);
    let ticket = client.send(Request::ReadPage {
        table: TableRef::new("endless"),
        page: Page::first(10),
        ahead: false,
    });
    // Running (nothing left waiting), then cancelled.
    while open.worker.waiting() > 0 || !open.worker.busy() {
        std::thread::sleep(Duration::from_millis(5));
    }
    std::thread::sleep(Duration::from_millis(50));
    client.cancel(ticket);
    let reply = reply(&client, &rx);
    assert_eq!((reply.ticket, reply.result), (ticket, Err(DbError::Cancelled)));
    client.send(Request::Count { table: users(), filter: None });
    assert_eq!(self::reply(&client, &rx).result, Ok(Response::Count(5)));
}

#[test]
fn a_read_only_source_is_read_and_never_written() {
    let dir = project("readonly");
    let file = dir.0.join("shop.db");
    let before = std::fs::read(&file).unwrap();
    let open = open(&dir, true);
    let (wake, rx) = waker();
    let mut repo = SessionRepository::new(&open.worker, users(), wake, Some(&open.events), 1 << 20);
    assert_eq!(ready(&mut repo, &rx, |r| r.count(None)), Ok(5));
    repo.apply(ChangeSet { table: users(), changes: vec![Change::Delete { key: key(1) }] });
    let events = until(&mut repo, &rx, |e| applied(e).is_some());
    assert!(matches!(applied(&events), Some(Err(DbError::ReadOnly(_)))), "{events:?}");
    assert!(matches!(
        alter(&open, vec![SchemaChange::DropTable { table: users() }]),
        Err(DbError::ReadOnly(_))
    ));
    drop(repo);
    drop(open);
    assert_eq!(std::fs::read(&file).unwrap(), before, "the file untouched");
}
