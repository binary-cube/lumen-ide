//! A driver through the drivers' protocol, end to end: this test binary, started again by
//! the tests as a driver's program (`EDITOR_TEST_DRIVER` says which: the toy driver, a toy
//! of a newer protocol, or SQLite), installed by a manifest in a temp folder and found
//! among the registered drivers as the app finds it — the real worker, repository and
//! grid source over it: structure, pages with their keys, changes applied (a conflict
//! rolled back), structure changes planned by the driver and run as shown, a cancel, a
//! crash said and the driver started again, a protocol version refused, the password kept
//! off the program's arguments, environment and error output; and SQLite through the
//! protocol giving exactly what SQLite gives directly (its statements included). The
//! console's side (protocol 2): rows streamed through the program as the driver gives them,
//! read on from a cursor, a run cancelled in the middle of its stream, an error with its
//! place, SQLite's console through the protocol as directly; and a driver of the first
//! version still running statements, in a limited console.
//!
//! No test harness of its own (`harness = false`): the binary must speak only the protocol
//! on its stdout when it's a driver.

// Its helpers fail the test they serve, as the tests' own unwraps do.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use database::{
    Cancel, CellChange, Change, ChangeSet, Collect, ColumnDef, DataSource, DbError, Driver,
    DriverFailure, Fetch, Page, RepositoryEvent, RowKey, SchemaChange, Scope, Secret, Session,
    TableRef, TableRepository, Target, Value,
};
use database_explorer::grid_source::{Abilities, TableSource};
use database_explorer::{Client, Reply, Request, Response, Runtime, SessionRepository, Sessions};
use db_protocol::toy::{PASSWORD, Toy};
use db_protocol::{VERSIONS, dto};
use kernel::{EventBus, ProjectId};
use ui_components::data_grid::{CellRef, CellValue, GridQuery, GridSource, RowState};

/// Which driver this binary serves when started as one.
const SERVE: &str = "EDITOR_TEST_DRIVER";
const WAIT: Duration = Duration::from_secs(30);

// ─── This binary as a driver's program ───

/// The toy, checking that its password came only through the pipe.
#[derive(Debug, Default)]
struct Checked(Toy);

impl Driver for Checked {
    fn kind(&self) -> database::Kind {
        self.0.kind()
    }

    fn language(&self) -> database::Language {
        self.0.language()
    }

    fn connect(&self, target: &Target, secret: &Secret) -> Result<Box<dyn Session>, DbError> {
        let leaked = std::env::args().any(|a| a.contains(secret.expose()))
            || std::env::vars_os().any(|(k, v)| {
                k.to_string_lossy().contains(secret.expose())
                    || v.to_string_lossy().contains(secret.expose())
            });
        if leaked {
            eprintln!("LEAK: the password was in my arguments or my environment");
        }
        self.0.connect(target, secret)
    }
}

fn sqlite_description() -> dto::Description {
    dto::Description {
        id: "sqlite-protocol".into(),
        name: "SQLite (protocol)".into(),
        sql: dto::Sql::Sqlite,
        capabilities: db_sqlite::CAPABILITIES.into(),
        ..dto::Description::default()
    }
}

/// Serves driver `which` on stdin and stdout.
fn serve(which: &str) -> ExitCode {
    let (driver, description, versions): (Arc<dyn Driver>, dto::Description, dto::Versions) =
        match which {
            "toy" => (Arc::new(Checked::default()), db_protocol::toy::description(), VERSIONS),
            "toy-v1" => {
                let mut description = db_protocol::toy::description();
                description.id = "toy-v1".into();
                (Arc::new(Checked::default()), description, dto::Versions { min: 1, max: 1 })
            }
            "toy-newer" => (
                Arc::new(Checked::default()),
                db_protocol::toy::description(),
                dto::Versions { min: VERSIONS.max + 1, max: VERSIONS.max + 2 },
            ),
            "sqlite" => (Arc::new(db_sqlite::SqliteDriver), sqlite_description(), VERSIONS),
            other => {
                eprintln!("no driver {other}");
                return ExitCode::FAILURE;
            }
        };
    let served = db_protocol::server::serve(
        driver,
        description,
        versions,
        std::io::stdin(),
        std::io::stdout(),
    );
    if served.is_err() { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}

// ─── The tests' helpers ───

/// A folder of the system's temp dir, gone with it.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "explorer-protocol-{name}-{}-{}",
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

/// Installs this binary as driver `which` (its manifest in `dir`), registered as the app
/// registers the installed ones; `description` is its manifest's `[driver]`.
fn install(dir: &Path, which: &str, description: &dto::Description) -> db_protocol::Installed {
    install_speaking(dir, which, description, VERSIONS)
}

/// [`install`], its manifest saying it speaks `versions`.
fn install_speaking(
    dir: &Path,
    which: &str,
    description: &dto::Description,
    versions: dto::Versions,
) -> db_protocol::Installed {
    let folder = dir.join(format!("driver-{which}"));
    std::fs::create_dir_all(&folder).unwrap();
    let exe = std::env::current_exe().unwrap();
    let driver = toml::to_string(&serde_json::json!({ "driver": description })).unwrap();
    let manifest = format!(
        "protocol = {{ min = {}, max = {} }}\ncommand = '{}'\n[env]\n{SERVE} = \"{which}\"\n{driver}",
        versions.min,
        versions.max,
        exe.display()
    );
    std::fs::write(folder.join(db_protocol::manifest::FILE), manifest).unwrap();
    let installed = db_protocol::manifest::read(&folder.join(db_protocol::manifest::FILE)).unwrap();
    let launch = Arc::new(db_protocol::ProgramLaunch::of(&installed));
    database::drivers::register(db_protocol::manifest::factory(&installed.manifest, launch));
    installed
}

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

fn open(root: &Path, source: &DataSource, secret: &str) -> Open {
    let events = EventBus::new();
    let sessions = Sessions::new(ProjectId::new(), root, events.clone(), Runtime::pool(), None);
    let worker = sessions.open_source(source, Secret::new(secret)).unwrap();
    Open { sessions, worker, events }
}

fn toy_source() -> DataSource {
    let mut source = DataSource::new("toys", "toy", Scope::Project);
    source.target.host = "localhost".into();
    source.target.user = "ana".into();
    source
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

fn applied(events: &[RepositoryEvent]) -> Option<Result<database::Applied, DbError>> {
    events.iter().find_map(|e| match e {
        RepositoryEvent::Applied(result) => Some(result.clone()),
        _ => None,
    })
}

fn reply(client: &Client, rx: &Receiver<()>) -> Reply {
    loop {
        if let Some(reply) = client.poll().into_iter().next() {
            return reply;
        }
        rx.recv_timeout(WAIT).expect("an answer in time");
    }
}

fn ask(open: &Open, request: Request) -> Result<Response, DbError> {
    let (wake, rx) = waker();
    let client = open.worker.client(wake);
    client.send(request);
    reply(&client, &rx).result
}

fn users() -> TableRef {
    TableRef::new("users")
}

fn key(id: i64) -> RowKey {
    RowKey::Primary(vec![("id".into(), Value::Int(id))])
}

fn rename(id: i64, old: &str, new: &str) -> Change {
    Change::Update {
        key: key(id),
        cells: vec![CellChange {
            column: "name".into(),
            old: Value::Text(old.into()),
            new: Value::Text(new.into()),
        }],
    }
}

// ─── The tests ───

fn the_toy_is_read_and_changed_through_its_program_by_the_repository_and_the_grid() {
    let dir = TempDir::new("toy");
    install(&dir.0, "toy", &db_protocol::toy::description());
    let found = database::drivers::find("toy").unwrap();
    assert!(found.external, "listed as external");
    let open = open(&dir.0, &toy_source(), PASSWORD);
    let (wake, rx) = waker();
    let mut repo = SessionRepository::new(&open.worker, users(), wake, Some(&open.events), 1 << 20);
    let structure = ready(&mut repo, &rx, |r| r.structure()).unwrap();
    assert_eq!(structure.primary_key(), vec!["id".to_owned()]);
    assert_eq!(open.worker.server().as_deref(), Some("Toy 1.0"));
    assert_eq!(ready(&mut repo, &rx, |r| r.count(None)), Ok(3));
    let page = ready(&mut repo, &rx, |r| r.page(&Page::first(2))).unwrap();
    assert_eq!(page.keys, vec![key(1), key(2)]);
    assert_eq!(page.rows[1][1], Value::Text("bob".into()));
    // A row changed since it was read: nothing applied.
    let stale = ChangeSet {
        table: users(),
        changes: vec![Change::Delete { key: key(3) }, rename(2, "not bob", "rob")],
    };
    repo.apply(stale);
    let events = until(&mut repo, &rx, |e| applied(e).is_some());
    assert!(matches!(applied(&events), Some(Err(DbError::Conflict(_)))), "{events:?}");
    assert_eq!(ready(&mut repo, &rx, |r| r.count(None)), Ok(3), "rolled back");
    drop(repo);

    // The grid over it: rows by block, a cell edited, submitted.
    let (wake, rx) = waker();
    let repo = SessionRepository::new(&open.worker, users(), wake, Some(&open.events), 1 << 20);
    let abilities = Abilities {
        sorting: database::Sorting::Any,
        edit_rows: true,
        read_only: false,
        view: false,
        bytes: None,
        cancel: true,
    };
    let mut grid = TableSource::new(Box::new(repo), users(), abilities, Some(100), None);
    let settle = |grid: &mut TableSource, done: &dyn Fn(&TableSource) -> bool| {
        while !done(grid) {
            grid.poll();
            if done(grid) {
                break;
            }
            rx.recv_timeout(WAIT).expect("an answer in time");
        }
    };
    settle(&mut grid, &|g| !g.columns().is_empty());
    grid.request(&GridQuery::default(), 0..10);
    settle(&mut grid, &|g| g.row_state(0) == RowState::Ready);
    assert_eq!(grid.cell(0, 1), CellRef::Text("ana"));
    grid.set(0, 1, CellValue::Text("zoe".into())).unwrap();
    let set = grid.change_set().expect("one change");
    assert!(grid.preview(&set).unwrap().is_empty(), "no SQL: the changes in words");
    grid.submit(set);
    settle(&mut grid, &|g| !g.submitting());
    assert!(matches!(grid.take_outcome(), Some(Ok(a)) if a.affected == 1));
    grid.request(&GridQuery::default(), 0..10);
    settle(&mut grid, &|g| {
        g.row_state(0) == RowState::Ready && g.cell(0, 1) != CellRef::Text("ana")
    });
    assert_eq!(grid.cell(0, 1), CellRef::Text("zoe"));
}

fn the_drivers_own_statements_are_shown_then_run_as_shown() {
    let dir = TempDir::new("plan");
    install(&dir.0, "toy", &db_protocol::toy::description());
    let open = open(&dir.0, &toy_source(), PASSWORD);
    let add =
        vec![SchemaChange::AddColumn { table: users(), column: ColumnDef::new("mood", "mood") }];
    let Ok(Response::Plan(Some(plan))) = ask(&open, Request::PlanAlter(add.clone())) else {
        panic!("the driver's statements");
    };
    assert_eq!(plan.script.statements(), ["BEGIN", "ADD COLUMN users mood mood", "COMMIT"]);
    let ran = ask(&open, Request::AlterAsShown { changes: add, shown: Some(plan.script.clone()) });
    assert!(matches!(ran, Ok(Response::Applied(_))), "{ran:?}");
    let Ok(Response::Structure(s)) = ask(&open, Request::Structure(users())) else { panic!() };
    assert_eq!(s.columns.last().map(|c| c.name.as_str()), Some("mood"));
    // What was shown isn't what it would run now: refused.
    let drop = vec![SchemaChange::DropColumn { table: users(), column: "mood".into() }];
    let refused = ask(&open, Request::AlterAsShown { changes: drop, shown: Some(plan.script) });
    assert!(matches!(refused, Err(DbError::Conflict(_))), "{refused:?}");
    // A table: made by the driver itself, described in words.
    let create = vec![SchemaChange::CreateTable(database::TableDef::new(
        TableRef::new("notes"),
        vec![ColumnDef::new("id", "INTEGER")],
    ))];
    assert!(matches!(ask(&open, Request::PlanAlter(create.clone())), Ok(Response::Plan(None))));
    let made = ask(&open, Request::AlterAsShown { changes: create, shown: None });
    assert!(matches!(made, Ok(Response::Applied(_))), "{made:?}");
}

fn a_cancel_stops_a_read_and_a_crash_is_said_then_the_driver_starts_again() {
    let dir = TempDir::new("crash");
    install(&dir.0, "toy", &db_protocol::toy::description());
    let open = open(&dir.0, &toy_source(), PASSWORD);
    let (wake, rx) = waker();
    let client = open.worker.client(wake);
    let page = |table: &str| Request::ReadPage {
        table: TableRef::new(table),
        page: Page::first(10),
        ahead: false,
    };
    let slow = client.send(page("slow"));
    std::thread::sleep(Duration::from_millis(200));
    client.cancel(slow);
    assert_eq!(reply(&client, &rx).result, Err(DbError::Cancelled));
    client.send(page("users"));
    assert!(matches!(reply(&client, &rx).result, Ok(Response::Page(p)) if p.rows.len() == 3));

    client.send(page("crash"));
    let crashed = reply(&client, &rx).result;
    let Err(DbError::Driver(DriverFailure::Stopped { driver, why })) = &crashed else {
        panic!("{crashed:?}")
    };
    assert_eq!(driver, "Toy");
    assert!(why.contains("exit code") || why.contains("killed"), "{why}");
    let said = database_explorer::grid_source::message(crashed.as_ref().unwrap_err());
    assert!(said.contains("Toy"), "{said}");
    // The next request starts it again.
    client.send(Request::Tables(Vec::new()));
    assert!(matches!(reply(&client, &rx).result, Ok(Response::Tables(t)) if t.len() == 3));
}

fn a_driver_of_a_newer_protocol_is_refused_with_why() {
    let dir = TempDir::new("newer");
    let mut description = db_protocol::toy::description();
    description.id = "toy-newer".into();
    description.name = "Toy Next".into();
    install(&dir.0, "toy-newer", &description);
    let mut source = toy_source();
    source.driver = "toy-newer".into();
    let open = open(&dir.0, &source, PASSWORD);
    let refused = ask(&open, Request::Tables(Vec::new()));
    let Err(DbError::Driver(failure)) = &refused else { panic!("{refused:?}") };
    assert!(matches!(failure, DriverFailure::Protocol { theirs, .. } if theirs.0 > VERSIONS.max));
    assert!(!failure.driver_too_old(), "the editor is the older one");
    let said = database_explorer::grid_source::message(refused.as_ref().unwrap_err());
    assert!(said.contains("Toy Next"), "{said}");
}

fn the_password_goes_only_through_the_pipe_and_never_stays_in_the_error_output() {
    let dir = TempDir::new("secret");
    let installed = install(&dir.0, "toy", &db_protocol::toy::description());
    let driver = db_protocol::ProtocolDriver::new(
        Arc::new(installed.manifest.driver.clone()),
        installed.manifest.protocol,
        installed.manifest.timeout(),
        Arc::new(db_protocol::ProgramLaunch::of(&installed)),
    );
    let target = Target { user: "ana".into(), ..Target::default() };
    let mut session = driver.open(&target, &Secret::new(PASSWORD)).unwrap();
    session.tables(&[]).unwrap();
    // Its error output read: the toy says what it was given.
    let mut log = Vec::new();
    for _ in 0..200 {
        log = session.log();
        if log.iter().any(|l| l.contains("connecting as")) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(log.iter().any(|l| l == "toy: connecting as ana with ••••"), "{log:?}");
    assert!(!log.iter().any(|l| l.contains(PASSWORD)), "scrubbed");
    assert!(!log.iter().any(|l| l.contains("LEAK")), "not in its arguments nor environment");
    let refused = driver.open(&target, &Secret::new("wrong"));
    assert!(matches!(refused.err(), Some(DbError::Auth(_))));
}

/// SQLite's file `shop.db` in `dir`.
fn sqlite_project(dir: &Path) {
    let mut target = Target {
        database: dir.join("shop.db").to_string_lossy().into_owned(),
        ..Target::default()
    };
    target.options.insert("create".into(), "true".into());
    let mut session = db_sqlite::SqliteDriver.connect(&target, &Secret::default()).unwrap();
    for sql in [
        "CREATE TABLE teams (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE)",
        "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, team INTEGER \
         REFERENCES teams (id), score REAL DEFAULT 0, photo BLOB, CHECK (score >= 0))",
        "CREATE INDEX users_name ON users (name)",
        "CREATE VIEW named AS SELECT id, name FROM users",
        "INSERT INTO teams (name) VALUES ('red'), ('blue')",
        "INSERT INTO users (name, team, score, photo) VALUES ('ana', 1, 1.5, x'00ff'), \
         ('bob', 2, 2.5, NULL), ('cid', 1, 3.5, x'01')",
    ] {
        session.execute(sql, &mut Collect::default(), &Cancel::default()).unwrap();
    }
}

fn sqlite_through_the_protocol_is_sqlite_directly() {
    let dir = TempDir::new("sqlite");
    sqlite_project(&dir.0);
    install(&dir.0, "sqlite", &sqlite_description());
    database::drivers::register(db_sqlite::factory());
    let source = |driver: &str| {
        let mut source = DataSource::new("shop", driver, Scope::Project);
        source.file = Some(PathBuf::from("shop.db"));
        source
    };
    let direct = open(&dir.0, &source("sqlite"), "");
    let external = open(&dir.0, &source("sqlite-protocol"), "");
    let both = |request: Request| (ask(&direct, request.clone()), ask(&external, request));
    let (a, b) = both(Request::Tables(Vec::new()));
    assert_eq!(a, b);
    let (a, b) = both(Request::Structure(users()));
    assert!(a.is_ok());
    assert_eq!(a, b);
    let page = Page {
        order: vec![database::Order { column: "score".into(), descending: true }],
        ..Page::first(10)
    };
    let (a, b) = both(Request::ReadPage { table: users(), page, ahead: false });
    assert!(matches!(&a, Ok(Response::Page(p)) if p.rows.len() == 3 && p.keys.len() == 3));
    assert_eq!(a, b, "the same rows, values and keys");
    // The same statements planned (a rebuild: what it needs read through the protocol).
    let alter = vec![
        SchemaChange::AlterColumn {
            table: users(),
            column: "score".into(),
            to: ColumnDef::new("score", "INTEGER"),
        },
        SchemaChange::RenameColumn { table: users(), from: "team".into(), to: "squad".into() },
    ];
    let (a, b) = both(Request::PlanAlter(alter.clone()));
    let Ok(Response::Plan(Some(plan))) = &a else { panic!("{a:?}") };
    assert_eq!(a, b, "exactly the statements SQLite plans directly");
    // Run through the protocol as shown; seen directly.
    let ran =
        ask(&external, Request::AlterAsShown { changes: alter, shown: Some(plan.script.clone()) });
    assert!(matches!(ran, Ok(Response::Applied(_))), "{ran:?}");
    let changes = ChangeSet {
        table: users(),
        changes: vec![rename(2, "bob", "rob"), Change::Delete { key: RowKey::RowId(99) }],
    };
    // A row that isn't there: a conflict, all rolled back.
    let refused = ask(&external, Request::Apply(changes));
    assert!(matches!(refused, Err(DbError::Conflict(_))), "{refused:?}");
    let ok = ChangeSet { table: users(), changes: vec![rename(2, "bob", "rob")] };
    assert!(
        matches!(ask(&external, Request::Apply(ok)), Ok(Response::Applied(a)) if a.affected == 1)
    );
    direct.worker.close_now();
    let Ok(Response::Structure(s)) = ask(&direct, Request::Structure(users())) else { panic!() };
    let names: Vec<&str> = s.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["id", "name", "squad", "score", "photo"]);
    let read =
        ask(&direct, Request::ReadPage { table: users(), page: Page::first(10), ahead: false });
    let Ok(Response::Page(rows)) = read else { panic!("{read:?}") };
    assert_eq!(rows.rows[1][1], Value::Text("rob".into()));
    assert_eq!(rows.rows[0][4], Value::Bytes(vec![0, 255]), "bytes cross whole");
}

// ─── The console (protocol 2) ───

/// A sink keeping every set's columns and rows.
#[derive(Debug, Default, PartialEq)]
struct Sets(Vec<(Vec<database::Column>, Vec<Vec<Value>>)>);

impl database::RowSink for Sets {
    fn columns(&mut self, columns: &[database::Column]) {
        self.0.push((columns.to_vec(), Vec::new()));
    }

    fn row(&mut self, values: Vec<Value>) -> database::Flow {
        if let Some((_, rows)) = self.0.last_mut() {
            rows.push(values);
        }
        database::Flow::Continue
    }
}

/// A session of the installed driver `id`, opened as the console opens its own.
fn console_session(id: &str, target: &Target, secret: &str) -> Box<dyn Session> {
    let found = database::drivers::find(id).unwrap();
    (found.create)().connect(target, &Secret::new(secret)).unwrap()
}

fn options(limit: u32, keep_cursor: bool) -> database::RunOptions {
    database::RunOptions { fetch_limit: limit, keep_cursor }
}

fn the_console_streams_through_the_drivers_program_and_a_cancel_stops_it_midway() {
    let dir = TempDir::new("console");
    install(&dir.0, "toy", &db_protocol::toy::description());
    let target = toy_source().target;
    let mut remote = console_session("toy", &target, PASSWORD);
    let mut direct = Toy::default().connect(&target, &Secret::new(PASSWORD)).unwrap();
    assert_eq!(remote.console(), Some(db_protocol::toy::CONSOLE));
    // Several chunks of rows, exactly as the driver gives them directly.
    let (mut a, mut b) = (Sets::default(), Sets::default());
    let ra = remote.run("SERIES 4321", &options(5_000, false), &mut a, &Cancel::default()).unwrap();
    let rb = direct.run("SERIES 4321", &options(5_000, false), &mut b, &Cancel::default()).unwrap();
    assert_eq!((a.0.len(), a.0[0].1.len()), (1, 4321));
    assert_eq!(a, b, "the same rows streamed");
    assert_eq!(ra, rb);
    // Read on from a cursor.
    let mut first = Sets::default();
    let outcome =
        remote.run("SERIES 1200", &options(1000, true), &mut first, &Cancel::default()).unwrap();
    let cursor = outcome.more.expect("a cursor");
    let mut rest = Sets::default();
    let outcome = remote.fetch(&cursor, 1000, &mut rest, &Cancel::default()).unwrap();
    assert_eq!((first.0[0].1.len(), rest.0[0].1.len(), outcome.more), (1000, 200, None));
    assert_eq!(rest.0[0].1[199][0], Value::Int(1200));
    // Cancelled once its first rows came: it stops midway, and the program goes on.
    struct Canceller(Cancel, u64);
    impl database::RowSink for Canceller {
        fn columns(&mut self, _: &[database::Column]) {}
        fn row(&mut self, _: Vec<Value>) -> database::Flow {
            self.1 += 1;
            if self.1 == 2_000 {
                self.0.cancel();
            }
            database::Flow::Continue
        }
    }
    let cancel = Cancel::default();
    let mut sink = Canceller(cancel.clone(), 0);
    let result = remote.run("SERIES 500000000", &options(u32::MAX, false), &mut sink, &cancel);
    assert_eq!(result, Err(DbError::Cancelled));
    assert!(sink.1 >= 2_000 && sink.1 < 500_000_000, "{}", sink.1);
    // An error with its place and code.
    let error = remote.run(
        "SELECT nope FROM users",
        &options(10, false),
        &mut Sets::default(),
        &Cancel::default(),
    );
    let Err(DbError::Sql(error)) = error else { panic!("{error:?}") };
    assert_eq!(error.position, Some(database::sql::EnginePosition::Character(8)));
    assert_eq!(error.sqlstate.as_deref(), Some("42703"));
}

fn a_driver_of_the_first_version_runs_in_a_limited_console() {
    let dir = TempDir::new("console-v1");
    let mut description = db_protocol::toy::description();
    description.id = "toy-v1".into();
    install_speaking(&dir.0, "toy-v1", &description, dto::Versions { min: 1, max: 1 });
    let found = database::drivers::find("toy-v1").unwrap();
    assert_eq!((found.create)().capabilities().console, Some(database::ConsoleCaps::LIMITED));
    let mut session = console_session("toy-v1", &toy_source().target, PASSWORD);
    assert_eq!(session.console(), Some(database::ConsoleCaps::LIMITED));
    let mut rows = Sets::default();
    let outcome =
        session.run("SERIES 300", &options(100, true), &mut rows, &Cancel::default()).unwrap();
    assert_eq!(rows.0[0].1.len(), 100);
    assert!(outcome.truncated && outcome.more.is_none());
    let error = session.run(
        "SELECT nope FROM users",
        &options(10, false),
        &mut Sets::default(),
        &Cancel::default(),
    );
    assert!(matches!(&error, Err(DbError::Query(m)) if m.contains("nope")), "no place: {error:?}");
}

fn sqlites_console_through_the_protocol_is_sqlites_directly() {
    let dir = TempDir::new("sqlite-console");
    sqlite_project(&dir.0);
    install(&dir.0, "sqlite", &sqlite_description());
    let target = Target {
        database: dir.0.join("shop.db").to_string_lossy().into_owned(),
        ..Target::default()
    };
    let mut direct = db_sqlite::SqliteDriver.connect(&target, &Secret::default()).unwrap();
    let mut external = console_session("sqlite-protocol", &target, "");
    assert_eq!(external.console(), direct.console());
    let both = |sql: &str, limit: u32, a: &mut Box<dyn Session>, b: &mut Box<dyn Session>| {
        let (mut x, mut y) = (Sets::default(), Sets::default());
        let rx = a.run(sql, &options(limit, true), &mut x, &Cancel::default());
        let ry = b.run(sql, &options(limit, true), &mut y, &Cancel::default());
        assert_eq!(rx, ry, "{sql}");
        assert_eq!(x, y, "{sql}");
        rx
    };
    let outcome = both("SELECT * FROM users ORDER BY id", 2, &mut direct, &mut external).unwrap();
    assert!(outcome.truncated);
    both("SELECT photo, score FROM users WHERE photo IS NOT NULL", 10, &mut direct, &mut external)
        .unwrap();
    let error = both("SELECT FROM WHERE", 10, &mut direct, &mut external).unwrap_err();
    assert!(matches!(
        error.sql_error().and_then(|e| e.position),
        Some(database::sql::EnginePosition::Byte(_))
    ));
    let explain = |s: &mut Box<dyn Session>| {
        s.explain("SELECT * FROM users WHERE name = 'x'", false, &Cancel::default())
    };
    assert_eq!(explain(&mut direct), explain(&mut external));
    assert_eq!(
        direct.catalog_columns(None, &Cancel::default()),
        external.catalog_columns(None, &Cancel::default())
    );
    assert_eq!(
        direct.routines(None, &Cancel::default()),
        external.routines(None, &Cancel::default())
    );
    assert_eq!(direct.databases(), external.databases());
    // A transaction through the protocol, seen by the driver's session.
    let immediate =
        database::TxOptions { isolation: Some(database::Isolation::Immediate), read_only: false };
    external.begin(&immediate).unwrap();
    assert!(external.in_transaction());
    both_write(&mut external, "DELETE FROM users");
    external.rollback().unwrap();
    assert!(!external.in_transaction());
    let mut count = Collect::default();
    direct.execute("SELECT count(*) FROM users", &mut count, &Cancel::default()).unwrap();
    assert_eq!(count.rows, vec![vec![Value::Int(3)]], "rolled back");
}

fn both_write(session: &mut Box<dyn Session>, sql: &str) {
    session.run(sql, &options(10, false), &mut Sets::default(), &Cancel::default()).unwrap();
}

// ─── Running them ───

type Test = (&'static str, fn());

const TESTS: &[Test] = &[
    (
        "the_toy_is_read_and_changed_through_its_program_by_the_repository_and_the_grid",
        the_toy_is_read_and_changed_through_its_program_by_the_repository_and_the_grid,
    ),
    (
        "the_drivers_own_statements_are_shown_then_run_as_shown",
        the_drivers_own_statements_are_shown_then_run_as_shown,
    ),
    (
        "a_cancel_stops_a_read_and_a_crash_is_said_then_the_driver_starts_again",
        a_cancel_stops_a_read_and_a_crash_is_said_then_the_driver_starts_again,
    ),
    (
        "a_driver_of_a_newer_protocol_is_refused_with_why",
        a_driver_of_a_newer_protocol_is_refused_with_why,
    ),
    (
        "the_password_goes_only_through_the_pipe_and_never_stays_in_the_error_output",
        the_password_goes_only_through_the_pipe_and_never_stays_in_the_error_output,
    ),
    (
        "sqlite_through_the_protocol_is_sqlite_directly",
        sqlite_through_the_protocol_is_sqlite_directly,
    ),
    (
        "the_console_streams_through_the_drivers_program_and_a_cancel_stops_it_midway",
        the_console_streams_through_the_drivers_program_and_a_cancel_stops_it_midway,
    ),
    (
        "a_driver_of_the_first_version_runs_in_a_limited_console",
        a_driver_of_the_first_version_runs_in_a_limited_console,
    ),
    (
        "sqlites_console_through_the_protocol_is_sqlites_directly",
        sqlites_console_through_the_protocol_is_sqlites_directly,
    ),
];

fn main() -> ExitCode {
    if let Some(which) = std::env::var_os(SERVE) {
        return serve(&which.to_string_lossy());
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--list") {
        for (name, _) in TESTS {
            println!("{name}: test");
        }
        return ExitCode::SUCCESS;
    }
    let exact = args.iter().any(|a| a == "--exact");
    let filters: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    let chosen: Vec<&Test> = TESTS
        .iter()
        .filter(|(name, _)| {
            filters.is_empty()
                || filters
                    .iter()
                    .any(|f| if exact { *name == f.as_str() } else { name.contains(f.as_str()) })
        })
        .collect();
    println!("\nrunning {} tests", chosen.len());
    let mut failed = Vec::new();
    for (name, test) in &chosen {
        let ok = std::panic::catch_unwind(test).is_ok();
        println!("test {name} ... {}", if ok { "ok" } else { "FAILED" });
        if !ok {
            failed.push(*name);
        }
    }
    let passed = chosen.len() - failed.len();
    println!(
        "\ntest result: {}. {passed} passed; {} failed\n",
        if failed.is_empty() { "ok" } else { "FAILED" },
        failed.len()
    );
    if failed.is_empty() { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
