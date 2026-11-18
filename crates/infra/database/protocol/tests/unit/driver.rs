use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use database::transaction::run;
use database::{
    CellChange, Change, Collect, ColumnDef, DriverFailure, Order, RowKey, TableRef, Value,
};

use super::*;
use crate::toy::{PASSWORD, Toy, description};

/// The toy served on a thread of this process, through pipes, speaking `versions`.
struct InProcess {
    toy: Toy,
    versions: dto::Versions,
    launched: AtomicUsize,
}

impl InProcess {
    fn new(versions: dto::Versions) -> Arc<Self> {
        Arc::new(Self { toy: Toy::default(), versions, launched: AtomicUsize::new(0) })
    }
}

impl Launch for InProcess {
    fn launch(&self) -> Result<Transport, DbError> {
        self.launched.fetch_add(1, Ordering::SeqCst);
        let (editor_reads, driver_writes) = std::io::pipe().unwrap();
        let (driver_reads, editor_writes) = std::io::pipe().unwrap();
        let toy: Arc<dyn Driver> = Arc::new(self.toy.clone());
        let versions = self.versions;
        std::thread::spawn(move || {
            let _ = crate::server::serve(toy, description(), versions, driver_reads, driver_writes);
        });
        Ok(Transport {
            input: Box::new(editor_reads),
            output: Box::new(editor_writes),
            errors: None,
            process: None,
        })
    }
}

fn driver(launch: Arc<InProcess>) -> ProtocolDriver {
    ProtocolDriver::new(Arc::new(description()), VERSIONS, REQUEST_TIMEOUT, launch)
}

fn connect(driver: &ProtocolDriver) -> Box<dyn Session> {
    driver.connect(&Target::default(), &Secret::new(PASSWORD)).unwrap()
}

fn users() -> TableRef {
    TableRef::new("users")
}

fn key(id: i64) -> RowKey {
    RowKey::Primary(vec![("id".into(), Value::Int(id))])
}

fn read(session: &mut dyn Session, page: &Page) -> Collect {
    let mut rows = Collect::default();
    session.read(&users(), page, &mut rows, &Cancel::default()).unwrap();
    rows
}

#[test]
fn a_session_through_the_protocol_reads_as_the_driver_does_directly() {
    let launch = InProcess::new(VERSIONS);
    let external = driver(Arc::clone(&launch));
    assert_eq!(launch.launched.load(Ordering::SeqCst), 0, "nothing started to describe it");
    assert_eq!(external.capabilities(), Toy::default().capabilities());
    assert!(external.dialect().is_none(), "it writes no SQL");
    assert_eq!(external.connection().field("port").map(|f| f.default.as_str()), Some("7"));
    let mut remote = connect(&external);
    let mut direct = Toy::default().connect(&Target::default(), &Secret::new(PASSWORD)).unwrap();
    assert_eq!(remote.server(), "Toy 1.0");
    assert_eq!(remote.tables(&[]).unwrap(), direct.tables(&[]).unwrap());
    assert_eq!(remote.structure(&users()).unwrap(), direct.structure(&users()).unwrap());
    let page = Page {
        offset: 1,
        limit: 5,
        order: vec![Order { column: "score".into(), descending: true }],
        filter: Some(database::Filter::NotNull("name".into())),
    };
    let (a, b) = (read(remote.as_mut(), &page), read(direct.as_mut(), &page));
    assert_eq!((&a.columns, &a.rows, &a.keys), (&b.columns, &b.rows, &b.keys));
    assert_eq!(a.keys, vec![key(2), key(1)], "each row with its key");
    let filter = database::Filter::Compare {
        column: "score".into(),
        op: database::CompareOp::Greater,
        value: Value::Int(15),
    };
    assert_eq!(remote.count(&users(), Some(&filter), &Cancel::default()).unwrap(), 2);
    assert_eq!(launch.launched.load(Ordering::SeqCst), 1);
}

#[test]
fn changes_apply_all_or_none_and_a_row_changed_since_it_was_read_is_a_conflict() {
    let launch = InProcess::new(VERSIONS);
    let mut session = connect(&driver(launch));
    let stale = ChangeSet {
        table: users(),
        changes: vec![
            Change::Delete { key: key(3) },
            Change::Update {
                key: key(1),
                cells: vec![CellChange {
                    column: "name".into(),
                    old: Value::Text("not ana".into()),
                    new: Value::Text("x".into()),
                }],
            },
        ],
    };
    assert!(matches!(session.apply(&stale), Err(DbError::Conflict(_))));
    assert_eq!(read(session.as_mut(), &Page::first(10)).rows.len(), 3, "nothing applied");
    let good = ChangeSet {
        table: users(),
        changes: vec![
            Change::Insert { values: vec![("name".into(), Value::Text("dan".into()))] },
            Change::Delete { key: key(3) },
        ],
    };
    let applied = session.apply(&good).unwrap();
    assert_eq!(applied, Applied { affected: 2, inserted: vec![key(4)] });
    let names: Vec<Value> =
        read(session.as_mut(), &Page::first(10)).rows.into_iter().map(|r| r[1].clone()).collect();
    assert_eq!(names, ["ana", "bob", "dan"].map(|n| Value::Text(n.into())).to_vec());
}

#[test]
fn a_driver_plans_its_own_statements_and_they_run_as_shown() {
    let launch = InProcess::new(VERSIONS);
    let mut session = connect(&driver(launch));
    let add =
        vec![SchemaChange::AddColumn { table: users(), column: ColumnDef::new("mood", "mood") }];
    let script = session.plan_alter(&add).unwrap().expect("its own statements");
    assert_eq!(script.statements(), ["BEGIN", "ADD COLUMN users mood mood", "COMMIT"]);
    run(session.as_mut(), &script, &Cancel::default(), &mut |_, _| {}).unwrap();
    let structure = session.structure(&users()).unwrap();
    assert_eq!(structure.columns.last().map(|c| c.name.as_str()), Some("mood"));
    // A table: made by the driver itself (described in words).
    let create = vec![SchemaChange::CreateTable(database::TableDef::new(
        TableRef::new("notes"),
        vec![ColumnDef::new("id", "INTEGER")],
    ))];
    assert_eq!(session.plan_alter(&create).unwrap(), None);
    session.alter(&create).unwrap();
    assert!(session.tables(&[]).unwrap().iter().any(|t| t.table.name == "notes"));
    // What it said it can't do isn't asked.
    let index = vec![SchemaChange::DropIndex { table: users(), name: "i".into() }];
    assert!(matches!(session.alter(&index), Err(DbError::Unsupported(_))));
}

#[test]
fn a_cancelled_read_stops_and_the_session_goes_on() {
    let launch = InProcess::new(VERSIONS);
    let mut session = connect(&driver(launch));
    let cancel = Cancel::default();
    let later = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        later.cancel();
    });
    let started = Instant::now();
    let mut rows = Collect::default();
    let slow = session.read(&TableRef::new("slow"), &Page::first(10), &mut rows, &cancel);
    assert_eq!(slow, Err(DbError::Cancelled));
    assert!(started.elapsed() < Duration::from_secs(5), "stopped, not waited for");
    assert_eq!(read(session.as_mut(), &Page::first(10)).rows.len(), 3, "still there");
}

#[test]
fn a_crash_ends_the_session_with_why_and_the_next_one_starts_it_again() {
    let launch = InProcess::new(VERSIONS);
    let external = driver(Arc::clone(&launch));
    let mut session = connect(&external);
    let mut rows = Collect::default();
    let crashed =
        session.read(&TableRef::new("crash"), &Page::first(10), &mut rows, &Cancel::default());
    assert!(
        matches!(&crashed, Err(DbError::Driver(DriverFailure::Stopped { driver, .. })) if driver == "Toy"),
        "{crashed:?}"
    );
    assert!(matches!(session.tables(&[]), Err(DbError::Driver(DriverFailure::Stopped { .. }))));
    drop(session);
    let mut again = connect(&external);
    assert_eq!(again.tables(&[]).unwrap().len(), 3);
    assert_eq!(launch.launched.load(Ordering::SeqCst), 2);
}

#[test]
fn a_driver_of_another_protocol_version_is_refused_with_both_ranges() {
    // It says, at the handshake, it speaks only newer versions.
    let newer = InProcess::new(dto::Versions { min: VERSIONS.max + 1, max: VERSIONS.max + 2 });
    let refused = driver(newer).connect(&Target::default(), &Secret::new(PASSWORD)).err();
    let Some(DbError::Driver(failure)) = refused else { panic!("{refused:?}") };
    assert_eq!(
        failure,
        DriverFailure::Protocol {
            driver: "Toy".into(),
            theirs: (VERSIONS.max + 1, VERSIONS.max + 2),
            ours: VERSIONS.pair()
        }
    );
    assert!(!failure.driver_too_old(), "the editor is the older one");
    // Its manifest says it speaks only older ones: nothing is even started.
    let launch = InProcess::new(VERSIONS);
    let old = ProtocolDriver::new(
        Arc::new(description()),
        dto::Versions { min: 0, max: 0 },
        REQUEST_TIMEOUT,
        Arc::clone(&launch) as Arc<dyn Launch>,
    );
    let refused = old.connect(&Target::default(), &Secret::new(PASSWORD)).err();
    let Some(DbError::Driver(failure)) = refused else { panic!("{refused:?}") };
    assert!(failure.driver_too_old());
    assert_eq!(launch.launched.load(Ordering::SeqCst), 0);
}

#[test]
fn a_wrong_password_is_refused_and_another_drivers_answer_too() {
    let launch = InProcess::new(VERSIONS);
    let refused = driver(Arc::clone(&launch)).connect(&Target::default(), &Secret::new("nope"));
    assert!(matches!(refused.err(), Some(DbError::Auth(_))));
    let mut other = description();
    other.id = "someone-else".into();
    let mismatched = ProtocolDriver::new(Arc::new(other), VERSIONS, REQUEST_TIMEOUT, launch)
        .connect(&Target::default(), &Secret::new(PASSWORD));
    assert!(matches!(
        mismatched.err(),
        Some(DbError::Driver(DriverFailure::NotStarted { why, .. })) if why.contains("toy")
    ));
}

#[test]
fn a_silent_driver_is_stopped_after_its_timeout() {
    let launch = InProcess::new(VERSIONS);
    let external =
        ProtocolDriver::new(Arc::new(description()), VERSIONS, Duration::from_millis(300), launch);
    let mut session = external.connect(&Target::default(), &Secret::new(PASSWORD)).unwrap();
    let mut rows = Collect::default();
    let silent =
        session.read(&TableRef::new("slow"), &Page::first(1), &mut rows, &Cancel::default());
    assert!(
        matches!(&silent, Err(DbError::Driver(DriverFailure::TimedOut { what, .. })) if what.contains("read")),
        "{silent:?}"
    );
    assert!(matches!(session.tables(&[]), Err(DbError::Driver(DriverFailure::Stopped { .. }))));
}

// ─── The console (protocol 2) ───

fn console_run(
    session: &mut dyn Session,
    sql: &str,
    limit: u32,
    keep_cursor: bool,
) -> Result<(RunOutcome, Collect), DbError> {
    let mut rows = Collect::default();
    let options = RunOptions { fetch_limit: limit, keep_cursor };
    let outcome = session.run(sql, &options, &mut rows, &Cancel::default())?;
    Ok((outcome, rows))
}

/// A sink keeping every set's rows (a `Collect` keeps only the last set's).
#[derive(Default)]
struct Sets(Vec<(Vec<database::Column>, Vec<Vec<Value>>)>);

impl RowSink for Sets {
    fn columns(&mut self, columns: &[database::Column]) {
        self.0.push((columns.to_vec(), Vec::new()));
    }

    fn row(&mut self, values: Vec<Value>) -> Flow {
        if let Some((_, rows)) = self.0.last_mut() {
            rows.push(values);
        }
        Flow::Continue
    }
}

#[test]
fn rows_stream_through_the_protocol_as_the_driver_gives_them_directly() {
    let launch = InProcess::new(VERSIONS);
    let external = driver(launch);
    assert_eq!(external.capabilities().console, Some(crate::toy::CONSOLE));
    let mut remote = connect(&external);
    let mut direct = Toy::default().connect(&Target::default(), &Secret::new(PASSWORD)).unwrap();
    assert_eq!(remote.console(), Some(crate::toy::CONSOLE));
    // More rows than a chunk holds, several chunks, the same rows and outcome as directly.
    let options = RunOptions { fetch_limit: 2_500, keep_cursor: false };
    let (mut a, mut b) = (Sets::default(), Sets::default());
    let ra = remote.run("SERIES 2600", &options, &mut a, &Cancel::default()).unwrap();
    let rb = direct.run("SERIES 2600", &options, &mut b, &Cancel::default()).unwrap();
    assert_eq!(a.0.len(), 1, "one set, its columns once: {}", a.0.len());
    assert_eq!(a.0, b.0);
    assert_eq!(a.0[0].1.len(), 2_500);
    assert_eq!(a.0[0].1[2_499], vec![Value::Int(2_500), Value::Text("n2500".into())]);
    assert_eq!(ra, rb);
    assert!(ra.truncated && ra.more.is_none());
    // A set without rows still says its columns.
    let (outcome, rows) = console_run(remote.as_mut(), "SERIES 0", 10, false).unwrap();
    assert_eq!((rows.columns.len(), rows.rows.len(), outcome.rows), (2, 0, 0));
    // A table's rows, typed.
    let (_, users) =
        console_run(remote.as_mut(), "SELECT name, score FROM users", 10, false).unwrap();
    assert_eq!(users.columns[1].type_name, "INTEGER");
    assert_eq!(users.rows[2], vec![Value::Text("cid".into()), Value::Int(30)]);
}

#[test]
fn a_result_is_read_on_from_its_cursor_and_closed() {
    let external = driver(InProcess::new(VERSIONS));
    let mut session = connect(&external);
    let (outcome, first) = console_run(session.as_mut(), "SERIES 1050", 500, true).unwrap();
    assert_eq!(first.rows.len(), 500);
    let token = outcome.more.expect("a cursor");
    let mut all = first.rows;
    let mut next = Some(token.clone());
    let mut pages = 1;
    while let Some(cursor) = next {
        let mut page = Collect::default();
        let outcome = session.fetch(&cursor, 500, &mut page, &Cancel::default()).unwrap();
        assert_eq!(page.columns.len(), 2, "each page says its columns");
        all.extend(page.rows);
        next = outcome.more;
        pages += 1;
    }
    assert_eq!((all.len(), pages), (1050, 3));
    assert_eq!(all[1049][0], Value::Int(1050));
    // Gone once read to its end; one closed early is gone too.
    let gone = session.fetch(&token, 10, &mut Collect::default(), &Cancel::default());
    assert!(matches!(gone, Err(DbError::Query(_))), "{gone:?}");
    let (outcome, _) = console_run(session.as_mut(), "SERIES 30", 10, true).unwrap();
    let token = outcome.more.unwrap();
    session.close_cursor(&token).unwrap();
    assert!(session.fetch(&token, 10, &mut Collect::default(), &Cancel::default()).is_err());
}

#[test]
fn a_run_cancelled_in_the_middle_of_its_stream_stops_and_the_session_goes_on() {
    let external = driver(InProcess::new(VERSIONS));
    let mut session = connect(&external);
    /// Cancels once it has seen `after` rows.
    struct Canceller {
        cancel: Cancel,
        seen: u64,
        after: u64,
    }
    impl RowSink for Canceller {
        fn columns(&mut self, _: &[database::Column]) {}
        fn row(&mut self, _: Vec<Value>) -> Flow {
            self.seen += 1;
            if self.seen == self.after {
                self.cancel.cancel();
            }
            Flow::Continue
        }
    }
    let cancel = Cancel::default();
    let mut sink = Canceller { cancel: cancel.clone(), seen: 0, after: 1_500 };
    let options = RunOptions { fetch_limit: u32::MAX, keep_cursor: false };
    let started = Instant::now();
    let result = session.run("SERIES 100000000", &options, &mut sink, &cancel);
    assert_eq!(result, Err(DbError::Cancelled));
    assert!(sink.seen >= 1_500 && sink.seen < 100_000_000, "stopped midway: {}", sink.seen);
    assert!(started.elapsed() < Duration::from_secs(20), "{:?}", started.elapsed());
    let (_, rows) = console_run(session.as_mut(), "SERIES 3", 10, false).unwrap();
    assert_eq!(rows.rows.len(), 3, "the session goes on");
    // A sink with enough stops it without an error.
    let mut few = Collect { limit: Some(5), ..Collect::default() };
    let outcome = session.run("SERIES 100000000", &options, &mut few, &Cancel::default()).unwrap();
    assert!(outcome.stopped && few.rows.len() == 5, "{outcome:?}");
    assert_eq!(console_run(session.as_mut(), "SERIES 2", 10, false).unwrap().1.rows.len(), 2);
}

#[test]
fn an_error_comes_back_with_its_place_and_code() {
    let external = driver(InProcess::new(VERSIONS));
    let mut session = connect(&external);
    let text = "SELECT nope FROM users";
    let Err(DbError::Sql(error)) = console_run(session.as_mut(), text, 10, false) else { panic!() };
    assert_eq!(error.sqlstate.as_deref(), Some("42703"));
    assert_eq!(error.position, Some(database::sql::EnginePosition::Character(8)));
    let located = database::sql::locate(
        text,
        text,
        &database::sql::BindMap::default(),
        error.position.unwrap(),
    )
    .unwrap();
    assert_eq!(&text[located.offset..located.offset + 4], "nope");
    let Err(DbError::Sql(unknown)) = console_run(session.as_mut(), "DANCE", 10, false) else {
        panic!()
    };
    assert!(unknown.hint.is_some() && unknown.sqlstate.as_deref() == Some("42601"));
}

#[test]
fn transactions_plans_schemas_and_the_catalog_through_the_protocol() {
    let external = driver(InProcess::new(VERSIONS));
    let mut session = connect(&external);
    session
        .begin(&TxOptions { isolation: Some(database::Isolation::Serializable), read_only: false })
        .unwrap();
    assert!(session.in_transaction());
    let mut set =
        database::ChangeSet { table: users(), changes: vec![Change::Delete { key: key(1) }] };
    session.apply(&set).unwrap();
    session.rollback().unwrap();
    assert!(!session.in_transaction());
    assert_eq!(read(session.as_mut(), &Page::first(10)).rows.len(), 3, "rolled back");
    let refused = session
        .begin(&TxOptions { isolation: Some(database::Isolation::Deferred), read_only: false });
    assert!(matches!(refused, Err(DbError::Unsupported(_))));
    // A BEGIN run by hand: the driver says the transaction is open.
    console_run(session.as_mut(), "BEGIN", 10, false).unwrap();
    assert!(session.in_transaction());
    set.changes = vec![Change::Delete { key: key(2) }];
    session.apply(&set).unwrap();
    session.commit().unwrap();
    assert!(!session.in_transaction());
    assert_eq!(read(session.as_mut(), &Page::first(10)).rows.len(), 2);
    // Notices.
    let (outcome, _) = console_run(session.as_mut(), "NOTICE hello there", 10, false).unwrap();
    assert_eq!(outcome.notices[0].message, "hello there");
    assert_eq!(outcome.notices[0].severity, "NOTICE");
    // The plan.
    let plan = session.explain("SELECT * FROM users", true, &Cancel::default()).unwrap();
    assert_eq!(plan[0].operation, "Toy Scan");
    assert_eq!((plan[0].object.as_deref(), plan[0].actual_rows), (Some("users"), Some(2.0)));
    // Where it is, its schema, its databases.
    session.set_schema("main").unwrap();
    assert!(matches!(session.set_schema("other"), Err(DbError::Query(_))));
    assert_eq!(
        session.current().unwrap(),
        Place { database: Some("toy".into()), schema: Some("main".into()) }
    );
    assert_eq!(session.databases().unwrap(), ["toy"]);
    // The catalog in bulk: the toy has no query of its own, its structures are read.
    let all = session.catalog_columns(None, &Cancel::default()).unwrap();
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].0, users());
    assert_eq!(all[0].1[0].key_position, Some(1));
    assert_eq!(
        all[0].1.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        ["id", "name", "score"]
    );
    let routines = session.routines(None, &Cancel::default()).unwrap();
    assert_eq!((routines[0].name.as_str(), routines[0].arity), ("series", Some(1)));
}

#[test]
fn a_driver_of_the_first_version_still_has_a_limited_console() {
    let launch = InProcess::new(dto::Versions { min: 1, max: 1 });
    let speaks_one = ProtocolDriver::new(
        Arc::new(description()),
        dto::Versions { min: 1, max: 1 },
        REQUEST_TIMEOUT,
        launch,
    );
    assert_eq!(speaks_one.capabilities().console, Some(ConsoleCaps::LIMITED));
    let session = speaks_one.open(&Target::default(), &Secret::new(PASSWORD)).unwrap();
    assert_eq!(session.protocol(), 1);
    let mut session: Box<dyn Session> = Box::new(session);
    assert_eq!(session.console(), Some(ConsoleCaps::LIMITED));
    // Its statements through `execute`: whole, then cut to the page.
    let (outcome, rows) = console_run(session.as_mut(), "SERIES 120", 50, true).unwrap();
    assert_eq!(rows.rows.len(), 50);
    assert!(outcome.truncated && outcome.more.is_none(), "no cursor: {outcome:?}");
    // No place in its errors; no cursors, plans or schemas; transactions by its words.
    let error = console_run(session.as_mut(), "SELECT nope FROM users", 10, false).unwrap_err();
    assert!(matches!(&error, DbError::Query(m) if m.contains("nope")), "{error:?}");
    let token = CursorToken("x".into());
    assert!(matches!(
        session.fetch(&token, 1, &mut Collect::default(), &Cancel::default()),
        Err(DbError::Unsupported(_))
    ));
    assert!(matches!(
        session.explain("SELECT * FROM users", false, &Cancel::default()),
        Err(DbError::Unsupported(_))
    ));
    assert_eq!(session.current().unwrap(), Place::default());
    session.begin(&TxOptions::default()).unwrap();
    assert!(session.in_transaction());
    session.rollback().unwrap();
    assert!(!session.in_transaction());
    // The catalog from the structures it reads.
    assert_eq!(session.catalog_columns(None, &Cancel::default()).unwrap().len(), 3);
}

#[test]
fn a_session_of_the_first_version_is_refused_the_consoles_methods() {
    // The driver's own end: protocol 1 chosen, `run` isn't one of its methods.
    let (editor_reads, driver_writes) = std::io::pipe().unwrap();
    let (driver_reads, editor_writes) = std::io::pipe().unwrap();
    std::thread::spawn(move || {
        let toy: Arc<dyn Driver> = Arc::new(Toy::default());
        crate::server::serve(toy, description(), VERSIONS, driver_reads, driver_writes)
    });
    let connection = Connection::new(
        "Toy",
        Transport {
            input: Box::new(editor_reads),
            output: Box::new(editor_writes),
            errors: None,
            process: None,
        },
    )
    .unwrap();
    let old = dto::Versions { min: 1, max: 1 };
    let hello: dto::Initialized = connection
        .call(
            method::INITIALIZE,
            &dto::Initialize { protocol: old, editor: "old".into() },
            None,
            REQUEST_TIMEOUT,
            (1, 1),
        )
        .unwrap();
    assert_eq!(hello.protocol, 1);
    let connect = dto::Connect { target: dto::Target::default(), password: PASSWORD.into() };
    let _: dto::Connected =
        connection.call(method::CONNECT, &connect, None, REQUEST_TIMEOUT, (1, 1)).unwrap();
    let run: Result<dto::RunOutcome, DbError> = connection.call(
        method::RUN,
        &dto::Run { sql: "SERIES 1".into(), fetch_limit: 1, keep_cursor: false },
        None,
        REQUEST_TIMEOUT,
        (1, 1),
    );
    assert!(matches!(run, Err(DbError::Unsupported(m)) if m.contains("protocol 2")));
}

/// Protocol 3: the driver's own profile, through the protocol exactly as it gives it
/// directly (its extras of each type and a key no editor knows included); its tables'
/// statistics.
#[test]
fn a_profile_crosses_the_protocol_as_the_driver_made_it() {
    use database::profile::{ExtraValue, OpKind, ProfileOptions};
    let driver = driver(InProcess::new(VERSIONS));
    let console = driver.capabilities().console.unwrap();
    assert!(console.profile.actual && console.profile.table_stats, "{:?}", console.profile);
    let mut session = connect(&driver);
    let options = ProfileOptions::ACTUAL;
    let mut direct = Toy::default().connect(&Target::default(), &Secret::new(PASSWORD)).unwrap();
    let expected = direct.profile("SELECT * FROM users", &options, &Cancel::default()).unwrap();
    let mut got = session.profile("SELECT * FROM users", &options, &Cancel::default()).unwrap();
    got.id = expected.id;
    assert_eq!(got, expected);
    let scan = &got.nodes[0];
    assert_eq!((scan.op, scan.actuals.rows), (OpKind::SeqScan, Some(3.0)));
    assert!(scan.extra.iter().any(|e| e.key == "toy_secret_sauce"));
    assert!(scan.extra.iter().any(|e| matches!(e.value, ExtraValue::Group(_))));
    let stats = session.table_stats(&[users()], &Cancel::default()).unwrap();
    assert_eq!(stats[0].live_rows, Some(3));
    // Its error, as for any statement.
    let refused = session.profile("NOTHING", &options, &Cancel::default());
    assert!(matches!(refused, Err(DbError::Sql(_))), "{refused:?}");
}

/// A driver of protocol 2: profiled through its plan (the domain's default), no statistics.
#[test]
fn a_driver_of_protocol_two_is_profiled_through_its_plan() {
    let two = dto::Versions { min: 1, max: 2 };
    let speaks_two =
        ProtocolDriver::new(Arc::new(description()), two, REQUEST_TIMEOUT, InProcess::new(two));
    let session = speaks_two.open(&Target::default(), &Secret::new(PASSWORD)).unwrap();
    assert_eq!(session.protocol(), 2);
    let mut session: Box<dyn Session> = Box::new(session);
    let console = session.console().unwrap();
    assert_eq!(console.profile, database::ProfileCaps::from_explain(console.explain));
    let profile = session
        .profile(
            "SELECT * FROM users",
            &database::profile::ProfileOptions::ACTUAL,
            &Cancel::default(),
        )
        .unwrap();
    assert_eq!(profile.nodes[0].name, "Toy Scan");
    assert!(profile.nodes[0].extra.is_empty(), "only what a plan says");
    assert_eq!(profile.nodes[0].actuals.rows, Some(3.0));
    assert!(matches!(
        session.table_stats(&[users()], &Cancel::default()),
        Err(DbError::Unsupported(_))
    ));
}

/// A session that chose protocol 2 is refused protocol 3's methods by the driver's end.
#[test]
fn a_session_of_protocol_two_is_refused_profiles() {
    let (editor_reads, driver_writes) = std::io::pipe().unwrap();
    let (driver_reads, editor_writes) = std::io::pipe().unwrap();
    std::thread::spawn(move || {
        let toy: Arc<dyn Driver> = Arc::new(Toy::default());
        crate::server::serve(toy, description(), VERSIONS, driver_reads, driver_writes)
    });
    let transport = Transport {
        input: Box::new(editor_reads),
        output: Box::new(editor_writes),
        errors: None,
        process: None,
    };
    let connection = Connection::new("Toy", transport).unwrap();
    let two = dto::Versions { min: 2, max: 2 };
    let hello: dto::Initialized = connection
        .call(
            method::INITIALIZE,
            &dto::Initialize { protocol: two, editor: "old".into() },
            None,
            REQUEST_TIMEOUT,
            (2, 2),
        )
        .unwrap();
    assert_eq!(hello.protocol, 2);
    let connect = dto::Connect { target: dto::Target::default(), password: PASSWORD.into() };
    let _: dto::Connected =
        connection.call(method::CONNECT, &connect, None, REQUEST_TIMEOUT, (2, 2)).unwrap();
    let asked = dto::ProfileParams {
        sql: "SELECT * FROM users".into(),
        options: dto::ProfileOptions::default(),
    };
    let refused: Result<dto::Profile, DbError> =
        connection.call(method::PROFILE, &asked, None, REQUEST_TIMEOUT, (2, 2));
    assert!(matches!(refused, Err(DbError::Unsupported(m)) if m.contains("protocol 3")));
}
