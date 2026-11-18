//! The worker and the repository on a real CSV file (in the system's temp dir, never the
//! user's), its driver found as the app finds it (registered, by id): pages with their
//! keys, totals, a sort and a filter, changes committed in one copy (said on the project's
//! bus), a file changed by another program left as it is, a long count cancelled, a
//! read-only source never written.

// Its helpers fail the test they serve, as the tests' own unwraps do.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use database::{
    Applied, CellChange, Change, ChangeSet, ColumnDef, CompareOp, DataSource, DbError, Fetch,
    Filter, Order, Page, RepositoryEvent, RowKey, SchemaChange, Scope, Secret, TableRef,
    TableRepository, Value,
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
            "explorer-csv-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn file(&self) -> PathBuf {
        self.0.join("users.csv")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const USERS: &[u8] =
    b"id;name;score\r\n1;ana;10\r\n2;bob;20\r\n3;cid;30\r\n4;dan;40\r\n5;eve;50\r\n";

/// The project's folder with `users.csv` holding `bytes`.
fn project(name: &str, bytes: &[u8]) -> TempDir {
    let dir = TempDir::new(name);
    std::fs::write(dir.file(), bytes).unwrap();
    dir
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

/// `users.csv` of `dir` as the project's source, its driver found by id among those
/// registered (as the app registers the modules' contributions).
fn open(dir: &TempDir, read_only: bool) -> Open {
    database::drivers::register(db_csv::factory());
    let events = EventBus::new();
    let sessions = Sessions::new(ProjectId::new(), &dir.0, events.clone(), Runtime::pool(), None);
    let mut source = DataSource::new("users", "csv", Scope::Project);
    source.file = Some(PathBuf::from("users.csv"));
    source.read_only = read_only;
    let worker = sessions.open_source(&source, Secret::default()).unwrap();
    Open { sessions, worker, events }
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

fn applied(events: &[RepositoryEvent]) -> Option<Result<Applied, DbError>> {
    events.iter().find_map(|e| match e {
        RepositoryEvent::Applied(result) => Some(result.clone()),
        _ => None,
    })
}

fn rename(key: &RowKey, from: &str, to: &str) -> Change {
    Change::Update {
        key: key.clone(),
        cells: vec![CellChange {
            column: "name".into(),
            old: Value::Text(from.into()),
            new: Value::Text(to.into()),
        }],
    }
}

#[test]
fn a_csv_file_is_read_and_changed_through_the_repository() {
    let dir = project("repository", USERS);
    let open = open(&dir, false);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let _sub = {
        let seen = Arc::clone(&seen);
        open.events.subscribe(move |e: &database::events::DataChanged| {
            seen.lock().unwrap().push(e.clone())
        })
    };
    let (wake, rx) = waker();
    let mut repo = SessionRepository::new(&open.worker, users(), wake, Some(&open.events), 1 << 20);

    let structure = ready(&mut repo, &rx, |r| r.structure()).unwrap();
    assert_eq!(structure.key_kind(), database::KeyKind::Line);
    assert_eq!(ready(&mut repo, &rx, |r| r.count(None)), Ok(5));
    let page = ready(&mut repo, &rx, |r| r.page(&Page::first(2))).unwrap();
    assert_eq!(page.rows[1], vec![Value::Int(2), Value::Text("bob".into()), Value::Int(20)]);
    // A source that writes no SQL: nothing to preview but the changes themselves.
    let changes = ChangeSet {
        table: users(),
        changes: vec![
            Change::Insert {
                values: vec![
                    ("id".into(), Value::Int(6)),
                    ("name".into(), Value::Text("fay".into())),
                ],
            },
            rename(&page.keys[1], "bob", "rob"),
            Change::Delete { key: page.keys[0].clone() },
        ],
    };
    assert_eq!(repo.preview(&changes), Ok(Vec::new()));
    repo.apply(changes);
    // The commit's outcome and the refresh after it: the change announced on the bus may
    // refresh the repository first (a `Refreshed` before the `Applied`).
    let events =
        until(&mut repo, &rx, |e| applied(e).is_some() && e.contains(&RepositoryEvent::Refreshed));
    let result = applied(&events).unwrap().unwrap();
    assert_eq!(result.affected, 3);
    assert_eq!(result.inserted.len(), 1);
    assert_eq!(
        std::fs::read(dir.file()).unwrap(),
        b"id;name;score\r\n2;rob;20\r\n3;cid;30\r\n4;dan;40\r\n5;eve;50\r\n6;fay;\r\n"
    );
    assert_eq!(ready(&mut repo, &rx, |r| r.count(None)), Ok(5));
    let changed = seen.lock().unwrap().clone();
    assert_eq!(changed.len(), 1);
    assert_eq!((changed[0].table.clone(), changed[0].structure), (users(), false));

    // Sorted and filtered by the driver.
    let top = Page {
        order: vec![Order { column: "score".into(), descending: true }],
        filter: Some(Filter::Compare {
            column: "score".into(),
            op: CompareOp::GreaterEq,
            value: Value::Int(30),
        }),
        ..Page::first(10)
    };
    let rows = ready(&mut repo, &rx, |r| r.page(&top)).unwrap();
    let names: Vec<String> = rows.rows.iter().map(|r| r[1].display()).collect();
    assert_eq!(names, ["eve", "dan", "cid"]);
    assert_eq!(ready(&mut repo, &rx, |r| r.count(top.filter.as_ref())), Ok(3));
}

#[test]
fn a_file_changed_by_another_program_is_not_written_over() {
    let dir = project("conflict", USERS);
    let open = open(&dir, false);
    let (wake, rx) = waker();
    let mut repo = SessionRepository::new(&open.worker, users(), wake, Some(&open.events), 1 << 20);
    let page = ready(&mut repo, &rx, |r| r.page(&Page::first(5))).unwrap();
    let theirs = b"id;name;score\r\n1;ana;10\r\n2;BOB;20\r\n".to_vec();
    std::fs::write(dir.file(), &theirs).unwrap();
    repo.apply(ChangeSet { table: users(), changes: vec![rename(&page.keys[1], "bob", "rob")] });
    let events = until(&mut repo, &rx, |e| applied(e).is_some());
    assert!(matches!(applied(&events), Some(Err(DbError::Conflict(_)))), "{events:?}");
    assert_eq!(std::fs::read(dir.file()).unwrap(), theirs);
    let names: Vec<String> = std::fs::read_dir(&dir.0)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["users.csv"], "no copy left");
}

#[test]
fn columns_are_changed_through_the_worker() {
    let dir = project("alter", USERS);
    let open = open(&dir, false);
    let (wake, rx) = waker();
    let client = open.worker.client(wake);
    client.send(Request::Alter(vec![
        SchemaChange::DropColumn { table: users(), column: "score".into() },
        SchemaChange::AddColumn { table: users(), column: ColumnDef::new("team", "TEXT") },
    ]));
    assert!(matches!(reply(&client, &rx).result, Ok(Response::Applied(_))));
    assert_eq!(
        std::fs::read(dir.file()).unwrap(),
        b"id;name;team\r\n1;ana;\r\n2;bob;\r\n3;cid;\r\n4;dan;\r\n5;eve;\r\n"
    );
    client.send(Request::Structure(users()));
    let Ok(Response::Structure(structure)) = reply(&client, &rx).result else { panic!() };
    let names: Vec<&str> = structure.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["id", "name", "team"]);
}

#[test]
fn a_long_count_is_cancelled_and_the_session_goes_on() {
    let mut big = b"id,name\n".to_vec();
    for i in 0..400_000 {
        big.extend_from_slice(format!("{i},\"user\n{i}\"\n").as_bytes());
    }
    let dir = project("cancel", &big);
    let open = open(&dir, false);
    let (wake, rx) = waker();
    let client = open.worker.client(wake);
    let ticket = client.send(Request::Count { table: users(), filter: None });
    client.cancel(ticket);
    let first = reply(&client, &rx);
    assert_eq!(first.ticket, ticket);
    // Dropped while it waited, or stopped while it ran: cancelled either way, or done.
    assert!(matches!(first.result, Err(DbError::Cancelled) | Ok(Response::Count(400_000))));
    client.send(Request::ReadPage {
        table: users(),
        page: Page { offset: 399_999, ..Page::first(5) },
        ahead: false,
    });
    let Ok(Response::Page(rows)) = reply(&client, &rx).result else { panic!() };
    assert_eq!(rows.rows, vec![vec![Value::Int(399_999), Value::Text("user\n399999".into())]]);
    client.send(Request::Count { table: users(), filter: None });
    assert_eq!(reply(&client, &rx).result, Ok(Response::Count(400_000)));
}

#[test]
fn a_read_only_csv_source_is_read_and_never_written() {
    let dir = project("readonly", USERS);
    let open = open(&dir, true);
    let (wake, rx) = waker();
    let mut repo = SessionRepository::new(&open.worker, users(), wake, Some(&open.events), 1 << 20);
    let page = ready(&mut repo, &rx, |r| r.page(&Page::first(1))).unwrap();
    repo.apply(ChangeSet {
        table: users(),
        changes: vec![Change::Delete { key: page.keys[0].clone() }],
    });
    let events = until(&mut repo, &rx, |e| applied(e).is_some());
    assert!(matches!(applied(&events), Some(Err(DbError::ReadOnly(_)))), "{events:?}");
    drop(repo);
    drop(open);
    assert_eq!(std::fs::read(dir.file()).unwrap(), USERS, "the file untouched");
}
