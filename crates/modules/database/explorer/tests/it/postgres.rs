//! The "Database" window on real PostgreSQL servers: the local test matrix's
//! (docker/databases), `EDITOR_TEST_POSTGRES_URLS` (comma-separated, each naming the
//! `postgres` database: `scripts/test-databases.sh urls`). None set: nothing runs.
//!
//! The user's report, reproduced: a source naming `postgres` (no tables of its own) showed
//! "No tables" while its data lived in the server's other databases. Now its databases are
//! listed under it — every one the user may connect to — each read through a session of
//! its own once opened: schemas (another role's too), tables, a table's rows.

// Its helpers fail the test they serve, as the tests' own unwraps do.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use database::{DataSource, Page, Scope, SecretPolicy, TableRef};
use database_explorer::tree::{Group, NodeKey, RowKind};
use database_explorer::window::DatabaseWindow;
use database_explorer::{Request, Response, Runtime, Sessions};
use kernel::{EventBus, ProjectId};
use ui_components::ToolPanel;

const WAIT: Duration = Duration::from_secs(30);

fn urls() -> Vec<String> {
    std::env::var("EDITOR_TEST_POSTGRES_URLS")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The window's rows (depth and label) once `done` says they're there (polled, the reads
/// in the background).
fn wait_rows(window: &mut DatabaseWindow, done: impl Fn(&[String]) -> bool) -> Vec<String> {
    let started = Instant::now();
    loop {
        window.poll();
        let rows: Vec<String> = window
            .rows()
            .iter()
            .map(|r| format!("{}{}", "  ".repeat(r.depth as usize), r.label))
            .collect();
        if done(&rows) {
            return rows;
        }
        assert!(started.elapsed() < WAIT, "not in time: {rows:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_servers_databases_and_their_tables_are_listed_from_its_default_one() {
    database::drivers::register(db_postgres::factory());
    for url in urls() {
        let events = EventBus::new();
        let project = ProjectId::new();
        let sessions =
            Sessions::new(project, Path::new("/projects/matrix"), events, Runtime::pool(), None);
        let _guard = database_explorer::sessions::register(Arc::clone(&sessions));
        let mut source = DataSource::new("pg", db_postgres::ID, Scope::Project);
        let secret = database::fields::from_url(&db_postgres::connection(), &url, &mut source)
            .unwrap()
            .unwrap_or_default();
        source.secrets = SecretPolicy::Session;
        assert_eq!(source.target.database, "postgres", "{url}");
        let mut window = DatabaseWindow::new();
        window.set_project(Some(project));
        let started = Instant::now();
        while let Err(e) = sessions.catalog().add(source.clone()) {
            assert!(started.elapsed() < WAIT, "{e}");
            window.poll();
            std::thread::sleep(Duration::from_millis(20));
        }
        sessions.credentials().set(&source, &secret).unwrap();
        wait_rows(&mut window, |rows| rows == ["pg"]);
        window.expand(&NodeKey::Source(source.id));
        let rows = wait_rows(&mut window, |rows| rows.iter().any(|r| r == "  shop"));
        // Every database it may connect to; its own marked; none it may not.
        for wanted in ["  analytics", "  postgres", "  shop"] {
            assert!(rows.iter().any(|r| r == wanted), "{url}: {rows:?}");
        }
        assert!(!rows.iter().any(|r| r.contains("editor_private")), "{url}: {rows:?}");
        let own = window.rows().iter().find(|r| r.label == "postgres").unwrap();
        assert_eq!(
            (own.kind.clone(), own.key.clone()),
            (RowKind::Database, NodeKey::Database(source.id))
        );
        assert!(!own.detail.is_empty(), "the source's own database marked");
        // Its own database: nothing there (the report's case), said under it.
        window.expand(&NodeKey::Database(source.id));
        let no_tables = i18n::t("database.no_tables");
        wait_rows(&mut window, |rows| rows.iter().any(|r| r.trim() == no_tables));
        // Another one: its schemas (another role's too) and tables, by a session of its own.
        let shop = source.id.of_database("shop");
        window.expand(&NodeKey::Database(shop));
        let rows = wait_rows(&mut window, |rows| rows.iter().any(|r| r == "    sales"));
        for schema in ["    audit", "    public", "    sales"] {
            assert!(rows.iter().any(|r| r == schema), "{url}: {rows:?}");
        }
        window.expand(&NodeKey::Schema(shop, "audit".into()));
        window.expand(&NodeKey::Group(shop, Some("audit".into()), Group::Tables));
        let rows = wait_rows(&mut window, |rows| rows.iter().any(|r| r.trim() == "secrets"));
        assert!(rows.iter().any(|r| r.trim() == "events"), "{url}: {rows:?}");
        // Its rows, through its own worker (connected to `shop`).
        let worker = sessions.source_worker(shop).unwrap();
        let (tx, rx) = channel();
        let tx = Mutex::new(tx);
        let client = worker.client(Arc::new(move || {
            let _ = tx.lock().unwrap().send(());
        }));
        client.send(Request::ReadPage {
            table: TableRef::in_schema("sales", "orders"),
            page: Page::first(25),
            ahead: false,
        });
        let reply = loop {
            if let Some(reply) = client.poll().into_iter().next() {
                break reply;
            }
            rx.recv_timeout(WAIT).expect("an answer in time");
        };
        match reply.result {
            Ok(Response::Page(rows)) => assert_eq!(rows.len(), 25, "{url}"),
            other => panic!("{url}: {other:?}"),
        }
        sessions.close_all();
    }
}

/// `client`'s replies until `done` has what it waits for (the reads in the background).
fn replies_until(
    client: &database_explorer::worker::Client,
    rx: &std::sync::mpsc::Receiver<()>,
    seen: &mut Vec<database_explorer::worker::Reply>,
    done: impl Fn(&[database_explorer::worker::Reply]) -> bool,
) {
    let started = Instant::now();
    loop {
        seen.extend(client.poll());
        if done(seen) {
            return;
        }
        assert!(started.elapsed() < WAIT, "not in time: {seen:?}");
        let _ = rx.recv_timeout(Duration::from_millis(200));
    }
}

/// The data view's reads never wait for its counts (user image 387): the tables come with
/// the engine's estimates; a count held as long as a lock on its table lasts runs on its
/// own session, while a page of another table comes at once; the counts are cancelled,
/// and once the lock goes a table is counted exactly.
#[test]
fn a_page_never_waits_behind_a_held_count() {
    use database::{Cancel, Collect};
    use database_explorer::worker::Ticket;
    database::drivers::register(db_postgres::factory());
    for url in urls() {
        let project = ProjectId::new();
        let sessions = Sessions::new(
            project,
            Path::new("/projects/matrix"),
            EventBus::new(),
            Runtime::pool(),
            None,
        );
        let _guard = database_explorer::sessions::register(Arc::clone(&sessions));
        let mut source = DataSource::new("pg", db_postgres::ID, Scope::Project);
        let secret = database::fields::from_url(&db_postgres::connection(), &url, &mut source)
            .unwrap()
            .unwrap_or_default();
        source.secrets = SecretPolicy::Session;
        source.target.database = "shop".into();
        sessions.catalog().ensure_loaded();
        let started = Instant::now();
        while let Err(e) = sessions.catalog().add(source.clone()) {
            assert!(started.elapsed() < WAIT, "{e}");
            std::thread::sleep(Duration::from_millis(20));
        }
        sessions.credentials().set(&source, &secret).unwrap();
        let worker = sessions.source_worker(source.id).unwrap();
        let (tx, rx) = channel();
        let tx = Mutex::new(tx);
        let client = worker.client(Arc::new(move || {
            let _ = tx.lock().unwrap().send(());
        }));
        let mut seen = Vec::new();
        // The tables with their estimates (the planner's or the statistics').
        let listed = client.send(Request::Tables(Vec::new()));
        replies_until(&client, &rx, &mut seen, |r| r.iter().any(|r| r.ticket == listed));
        let Some(Ok(Response::Tables(tables))) =
            seen.iter().find(|r| r.ticket == listed).map(|r| r.result.clone())
        else {
            panic!("{url}: {seen:?}")
        };
        let held = TableRef::in_schema("sales", "shipments");
        let estimate = tables.iter().find(|t| t.table == held).and_then(|t| t.rows);
        assert!(estimate.is_some_and(|n| n > 0), "{url}: {estimate:?}");
        // Another session holds the shipments (no other test reads them): counting them
        // waits for it.
        let driver = sessions.driver(db_postgres::ID).unwrap();
        let mut locker = driver.connect(&source.target, &secret).unwrap();
        let cancel = Cancel::default();
        locker.execute("BEGIN", &mut Collect::default(), &cancel).unwrap();
        locker
            .execute(
                "LOCK TABLE sales.shipments IN ACCESS EXCLUSIVE MODE",
                &mut Collect::default(),
                &cancel,
            )
            .unwrap();
        let total = client.send(Request::Count { table: held.clone(), filter: None });
        let listed_count = client.send(Request::RowCount(held.clone()));
        let page = client.send(Request::ReadPage {
            table: TableRef::in_schema("public", "customers"),
            page: Page::first(25),
            ahead: false,
        });
        replies_until(&client, &rx, &mut seen, |r| r.iter().any(|r| r.ticket == page));
        let by = |seen: &[database_explorer::worker::Reply], t: Ticket| {
            seen.iter().find(|r| r.ticket == t).map(|r| r.result.clone())
        };
        assert!(matches!(by(&seen, page), Some(Ok(Response::Page(_)))), "{url}: {seen:?}");
        assert_eq!(by(&seen, total), None, "{url}: the count still held");
        assert!(worker.busy(), "{url}: the count still runs (on a session of its own)");
        // Cancelled: both replies come while the lock is still held.
        client.cancel(total);
        client.cancel(listed_count);
        replies_until(&client, &rx, &mut seen, |r| {
            r.iter().any(|r| r.ticket == total) && r.iter().any(|r| r.ticket == listed_count)
        });
        assert!(matches!(by(&seen, total), Some(Err(_))), "{url}: {seen:?}");
        locker.execute("ROLLBACK", &mut Collect::default(), &cancel).unwrap();
        drop(locker);
        // Free: counted exactly.
        let exact = client.send(Request::RowCount(held));
        replies_until(&client, &rx, &mut seen, |r| r.iter().any(|r| r.ticket == exact));
        assert!(matches!(by(&seen, exact), Some(Ok(Response::Count(n))) if n > 0), "{url}");
        assert!(worker.counting_connected(), "{url}: counted on a session of its own");
        sessions.close_all();
    }
}
