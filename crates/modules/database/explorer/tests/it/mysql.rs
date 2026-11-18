//! The "Database" window on real MySQL and MariaDB servers: the local test matrix's
//! (docker/databases), `EDITOR_TEST_MYSQL_URLS` (comma-separated, each naming the `shop`
//! database: `scripts/test-databases.sh urls`). None set: nothing runs.
//!
//! A MySQL server's databases are its schemas: the source lists every one the user may
//! reach (`analytics` besides the `shop` it names; never the server's own, nor
//! `editor_private`, where the user has no right) through its one session, each with its
//! tables and views, a table's rows read by the same worker. A source naming no database
//! sees the same.

// Its helpers fail the test they serve, as the tests' own unwraps do.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use database::{DataSource, Page, Scope, SecretPolicy, TableRef};
use database_explorer::tree::{Group, NodeKey, Part, RowKind};
use database_explorer::window::DatabaseWindow;
use database_explorer::{Request, Response, Runtime, Sessions};
use kernel::{EventBus, ProjectId};
use ui_components::ToolPanel;

const WAIT: Duration = Duration::from_secs(30);

fn urls() -> Vec<String> {
    std::env::var("EDITOR_TEST_MYSQL_URLS")
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
fn a_servers_databases_are_its_schemas_each_with_its_tables() {
    database::drivers::register(db_mysql::factory());
    for url in urls() {
        for database in ["shop", ""] {
            let events = EventBus::new();
            let project = ProjectId::new();
            let sessions = Sessions::new(
                project,
                Path::new("/projects/matrix"),
                events,
                Runtime::pool(),
                None,
            );
            let _guard = database_explorer::sessions::register(Arc::clone(&sessions));
            let mut source = DataSource::new("my", db_mysql::ID, Scope::Project);
            let secret = database::fields::from_url(&db_mysql::connection(), &url, &mut source)
                .unwrap()
                .unwrap_or_default();
            source.target.database = database.to_owned();
            source.secrets = SecretPolicy::Session;
            let mut window = DatabaseWindow::new();
            window.set_project(Some(project));
            let started = Instant::now();
            while let Err(e) = sessions.catalog().add(source.clone()) {
                assert!(started.elapsed() < WAIT, "{e}");
                window.poll();
                std::thread::sleep(Duration::from_millis(20));
            }
            sessions.credentials().set(&source, &secret).unwrap();
            wait_rows(&mut window, |rows| rows == ["my"]);
            window.expand(&NodeKey::Source(source.id));
            let rows = wait_rows(&mut window, |rows| rows.iter().any(|r| r == "  shop"));
            // Every database it may reach, as a schema; none of the server's own, none
            // it has no right on.
            assert!(rows.iter().any(|r| r == "  analytics"), "{url}: {rows:?}");
            for hidden in ["editor_private", "information_schema", "performance_schema", "mysql"] {
                assert!(!rows.iter().any(|r| r.trim() == hidden), "{url}: {rows:?}");
            }
            let shop = window.rows().iter().find(|r| r.label == "shop").unwrap();
            assert_eq!(
                (shop.kind.clone(), shop.key.clone()),
                (RowKind::Schema, NodeKey::Schema(source.id, "shop".into()))
            );
            // Another database's tables and views, by the same session.
            window.expand(&NodeKey::Schema(source.id, "analytics".into()));
            window.expand(&NodeKey::Group(source.id, Some("analytics".into()), Group::Tables));
            window.expand(&NodeKey::Group(source.id, Some("analytics".into()), Group::Views));
            let rows = wait_rows(&mut window, |rows| rows.iter().any(|r| r.trim() == "visits"));
            assert!(rows.iter().any(|r| r.trim() == "daily"), "{url}: {rows:?}");
            window.expand(&NodeKey::Schema(source.id, "shop".into()));
            window.expand(&NodeKey::Group(source.id, Some("shop".into()), Group::Tables));
            let rows = wait_rows(&mut window, |rows| rows.iter().any(|r| r.trim() == "customers"));
            assert!(rows.iter().any(|r| r.trim() == "order_lines"), "{url}: {rows:?}");
            // A table's structure, opened.
            let customers = TableRef::in_schema("shop", "customers");
            window.expand(&NodeKey::Table(source.id, customers.clone()));
            window.expand(&NodeKey::Part(source.id, customers.clone(), Part::Columns));
            wait_rows(&mut window, |rows| rows.iter().any(|r| r.trim() == "email"));
            // Its rows, through the source's own worker.
            let worker = sessions.source_worker(source.id).unwrap();
            let (tx, rx) = channel();
            let tx = Mutex::new(tx);
            let client = worker.client(Arc::new(move || {
                let _ = tx.lock().unwrap().send(());
            }));
            client.send(Request::ReadPage {
                table: TableRef::in_schema("analytics", "visits"),
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

/// The data view's reads never wait for its counts (user image 387), on MySQL / MariaDB
/// as on PostgreSQL: the tables come with the engine's estimates; a count held as long as a lock on its table lasts runs on its
/// own session, while a page of another table comes at once; the counts are cancelled,
/// and once the lock goes a table is counted exactly.
#[test]
fn a_page_never_waits_behind_a_held_count() {
    use database::{Cancel, Collect};
    use database_explorer::worker::Ticket;
    database::drivers::register(db_mysql::factory());
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
        let mut source = DataSource::new("my", db_mysql::ID, Scope::Project);
        let secret = database::fields::from_url(&db_mysql::connection(), &url, &mut source)
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
        // The tables with their estimates (`information_schema.TABLES.TABLE_ROWS`).
        let listed = client.send(Request::Tables(Vec::new()));
        replies_until(&client, &rx, &mut seen, |r| r.iter().any(|r| r.ticket == listed));
        let Some(Ok(Response::Tables(tables))) =
            seen.iter().find(|r| r.ticket == listed).map(|r| r.result.clone())
        else {
            panic!("{url}: {seen:?}")
        };
        // (InnoDB's own: kept by its statistics, so maybe not of late.)
        let estimate = |name: &str| tables.iter().find(|t| t.table.name == name)?.rows;
        assert!(estimate("orders").is_some_and(|n| n > 0), "{url}: {tables:?}");
        assert!(estimate("shipments").is_some(), "{url}: {tables:?}");
        let held = TableRef::in_schema("shop", "shipments");
        // Another session holds the shipments (no other test reads them): counting them
        // waits for it.
        let driver = sessions.driver(db_mysql::ID).unwrap();
        let mut locker = driver.connect(&source.target, &secret).unwrap();
        let cancel = Cancel::default();
        locker.execute("LOCK TABLES shipments WRITE", &mut Collect::default(), &cancel).unwrap();
        let total = client.send(Request::Count { table: held.clone(), filter: None });
        let listed_count = client.send(Request::RowCount(held.clone()));
        let page = client.send(Request::ReadPage {
            table: TableRef::in_schema("shop", "products"),
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
        locker.execute("UNLOCK TABLES", &mut Collect::default(), &cancel).unwrap();
        drop(locker);
        // Free: counted exactly.
        let exact = client.send(Request::RowCount(held));
        replies_until(&client, &rx, &mut seen, |r| r.iter().any(|r| r.ticket == exact));
        assert!(matches!(by(&seen, exact), Some(Ok(Response::Count(n))) if n > 0), "{url}");
        assert!(worker.counting_connected(), "{url}: counted on a session of its own");
        sessions.close_all();
    }
}
