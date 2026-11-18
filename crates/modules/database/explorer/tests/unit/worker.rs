use std::sync::atomic::Ordering;

use database::{Change, RowKey};

use super::*;
use crate::fake::{FakeDb, FakeDriver, Manual, counted_wake};

fn users() -> TableRef {
    TableRef::new("users")
}

fn read(offset: u64, ahead: bool) -> Request {
    Request::ReadPage { table: users(), page: Page { offset, ..Page::first(10) }, ahead }
}

fn setup(rows: usize) -> (Manual, FakeDriver, SessionWorker) {
    let manual = Manual::new();
    let driver = FakeDriver::new(FakeDb::users(rows), true);
    let worker =
        SessionWorker::new(driver.source(DataSourceId::new()), manual.runtime(), None, None);
    (manual, driver, worker)
}

fn tickets(replies: &[Reply]) -> Vec<Ticket> {
    replies.iter().map(|r| r.ticket).collect()
}

#[test]
fn nothing_opens_until_the_first_request() {
    let (manual, driver, worker) = setup(3);
    let (wake, _) = counted_wake();
    let client = worker.client(wake);
    assert_eq!((driver.opened(), manual.jobs()), (0, 0));
    assert!(!worker.connected());
    client.send(Request::Tables(Vec::new()));
    assert_eq!(driver.opened(), 0, "opened on the pool, not by the caller");
    manual.run_all();
    assert_eq!((driver.opened(), driver.live()), (1, 1));
    assert!(worker.connected());
}

#[test]
fn requests_run_one_per_job_and_reply_in_the_order_asked() {
    let (manual, _driver, worker) = setup(25);
    let (wake, woken) = counted_wake();
    let client = worker.client(wake);
    let asked = [
        client.send(Request::Tables(Vec::new())),
        client.send(Request::Structure(users())),
        client.send(read(0, false)),
    ];
    // One job on the pool at a time: the sessions of other sources take turns.
    assert_eq!(manual.jobs(), 1);
    assert!(manual.run_next());
    assert_eq!(tickets(&client.poll()), vec![asked[0]]);
    assert_eq!(manual.jobs(), 1);
    manual.run_all();
    let replies = client.poll();
    assert_eq!(tickets(&replies), asked[1..].to_vec());
    assert!(matches!(replies[0].result, Ok(Response::Structure(_))));
    let Ok(Response::Page(rows)) = &replies[1].result else { panic!("{replies:?}") };
    assert_eq!(rows.len(), 10);
    assert_eq!(rows.keys[0], RowKey::RowId(1));
    assert_eq!(woken.load(Ordering::SeqCst), 3, "woken once per reply");
    assert!(!worker.busy());
}

/// An SQL source counts on a session of its own: a count asked before a page never holds
/// it back (its job apart), and both sessions close with the source.
#[test]
fn an_sql_sources_counts_run_on_their_own_session() {
    let (manual, driver, worker) = setup(25);
    let (wake, _) = counted_wake();
    let client = worker.client(wake);
    let count = client.send(Request::Count { table: users(), filter: None });
    let listed = client.send(Request::RowCount(users()));
    let page = client.send(read(0, false));
    // Two jobs: the source's session and its counting one, each one request at a time.
    assert_eq!(manual.jobs(), 2);
    assert_eq!(worker.waiting(), 3);
    manual.run_all();
    let replies = client.poll();
    assert_eq!(replies.len(), 3);
    let by = |t: Ticket| replies.iter().find(|r| r.ticket == t).unwrap().result.clone();
    assert_eq!(by(count), Ok(Response::Count(25)));
    assert_eq!(by(listed), Ok(Response::Count(25)));
    assert!(matches!(by(page), Ok(Response::Page(_))));
    assert_eq!((driver.opened(), driver.live()), (2, 2));
    assert!(worker.counting_connected());
    // Closing the source closes its counting session too.
    client.send(Request::Close);
    manual.run_all();
    assert_eq!(driver.live(), 0);
    assert!(!worker.connected());
    // Asked again: opened again; close_now closes both.
    client.send(Request::RowCount(users()));
    client.send(read(0, false));
    manual.run_all();
    assert_eq!(driver.live(), 2);
    worker.close_now();
    // Its sessions close on the pool (never on the caller's thread).
    manual.run_all();
    assert_eq!(driver.live(), 0);
}

/// A source with one session (it writes no SQL: a CSV file) runs what the user waits for
/// first: the pages in the order asked, then the totals, then the pages read ahead, then
/// the lists' counts.
#[test]
fn one_session_runs_pages_before_totals_and_the_lists_counts_last() {
    let manual = Manual::new();
    let driver = FakeDriver::new(FakeDb::users(30), false);
    let worker =
        SessionWorker::new(driver.source(DataSourceId::new()), manual.runtime(), None, None);
    let (wake, _) = counted_wake();
    let client = worker.client(wake);
    let listed = [client.send(Request::RowCount(users())), client.send(Request::RowCount(users()))];
    let ahead = client.send(read(10, true));
    let total = client.send(Request::Count { table: users(), filter: None });
    let page = client.send(read(0, false));
    let structure = client.send(Request::Structure(users()));
    assert_eq!(manual.jobs(), 1, "one session");
    manual.run_all();
    let order = tickets(&client.poll());
    assert_eq!(order, vec![page, structure, total, ahead, listed[0], listed[1]]);
    assert_eq!(driver.opened(), 1);
}

/// What a client asked on the counting lane is cancelled and forgotten like the rest.
#[test]
fn counts_are_cancelled_and_forgotten_on_their_lane() {
    let (manual, driver, worker) = setup(5);
    let (wake, _) = counted_wake();
    let client = worker.client(wake);
    let first = client.send(Request::RowCount(users()));
    let second = client.send(Request::RowCount(users()));
    client.cancel(second);
    assert_eq!(tickets(&client.poll()), vec![second], "a waiting one dropped at once");
    let other = worker.client(counted_wake().0);
    other.send(Request::RowCount(users()));
    drop(other);
    manual.run_all();
    let replies = client.poll();
    assert_eq!(tickets(&replies), vec![first]);
    assert_eq!(driver.db.lock().unwrap().counts, 1, "the gone client's count never ran");
    assert!(!worker.busy());
}

#[test]
fn the_latest_page_of_a_table_wins_and_waits_behind_what_was_asked_before() {
    let (manual, driver, worker) = setup(100);
    let (wake, _) = counted_wake();
    let client = worker.client(wake);
    let first = client.send(read(0, false));
    let apply = client.send(Request::Apply(ChangeSet {
        table: users(),
        changes: vec![Change::Delete { key: RowKey::RowId(1) }],
    }));
    let second = client.send(read(10, false));
    let latest = client.send(read(20, false));
    // The pages replaced are answered at once: cancelled.
    let replaced = client.poll();
    assert_eq!(tickets(&replaced), vec![first, second]);
    assert!(replaced.iter().all(|r| r.result == Err(DbError::Cancelled)));
    manual.run_all();
    assert_eq!(tickets(&client.poll()), vec![apply, latest]);
    assert_eq!(driver.db.lock().unwrap().reads, vec![("users".to_owned(), 20, 10)]);
}

#[test]
fn another_clients_page_and_a_page_read_ahead_are_not_replaced() {
    let (manual, driver, worker) = setup(100);
    let (wake, _) = counted_wake();
    let (one, two) = (worker.client(wake.clone()), worker.client(wake));
    one.send(read(0, false));
    one.send(read(10, true));
    two.send(read(30, false));
    assert!(one.poll().is_empty() && two.poll().is_empty());
    manual.run_all();
    // What's read ahead goes after the rest.
    let reads: Vec<u64> = driver.db.lock().unwrap().reads.iter().map(|r| r.1).collect();
    assert_eq!(reads, vec![0, 30, 10]);
}

#[test]
fn a_waiting_request_cancelled_is_dropped() {
    let (manual, driver, worker) = setup(5);
    let (wake, _) = counted_wake();
    let client = worker.client(wake);
    let kept = client.send(read(0, false));
    let dropped = client.send(Request::Count { table: users(), filter: None });
    client.cancel(dropped);
    assert_eq!(client.poll(), vec![Reply { ticket: dropped, result: Err(DbError::Cancelled) }]);
    manual.run_all();
    assert_eq!(tickets(&client.poll()), vec![kept]);
    assert!(!driver.db.lock().unwrap().log.iter().any(|q| q.starts_with("SELECT COUNT")));
}

#[test]
fn a_running_request_cancelled_stops_and_the_session_stays() {
    let (manual, driver, worker) = setup(5);
    let (wake, _) = counted_wake();
    let client = Arc::new(worker.client(wake));
    let ticket = Arc::new(Mutex::new(None));
    {
        let (client, ticket) = (Arc::clone(&client), Arc::clone(&ticket));
        // While the read runs, the view cancels it.
        driver.db.lock().unwrap().on_read = Some(Arc::new(move || {
            if let Some(t) = *ticket.lock().unwrap() {
                client.cancel(t);
            }
        }));
    }
    let asked = client.send(read(0, false));
    *ticket.lock().unwrap() = Some(asked);
    manual.run_all();
    assert_eq!(client.poll(), vec![Reply { ticket: asked, result: Err(DbError::Cancelled) }]);
    assert_eq!(driver.live(), 1);
    assert!(worker.connected());
}

#[test]
fn close_ends_the_session_and_the_next_request_opens_it_again() {
    let (manual, driver, worker) = setup(5);
    let (wake, _) = counted_wake();
    let client = worker.client(wake);
    client.send(Request::Tables(Vec::new()));
    client.send(Request::Close);
    manual.run_all();
    let replies = client.poll();
    assert_eq!(replies[1].result, Ok(Response::Closed));
    assert_eq!(driver.live(), 0);
    assert!(!worker.connected());
    client.send(Request::Tables(Vec::new()));
    manual.run_all();
    assert_eq!((driver.opened(), driver.live()), (2, 1));
}

#[test]
fn closing_now_cancels_what_waits_and_closes_once_the_running_one_ends() {
    let (manual, driver, worker) = setup(5);
    let (wake, _) = counted_wake();
    let client = worker.client(wake);
    client.send(Request::Tables(Vec::new()));
    manual.run_all();
    client.poll();
    let waiting = client.send(Request::Structure(users()));
    worker.close_now();
    assert_eq!(client.poll(), vec![Reply { ticket: waiting, result: Err(DbError::Cancelled) }]);
    // Its sessions close on the pool (never on the caller's thread).
    manual.run_all();
    assert_eq!(driver.live(), 0);
    // The job left on the pool finds nothing.
    manual.run_all();
    assert!(client.poll().is_empty());

    // While a request runs: the session goes when it ends.
    let w = worker.clone();
    driver.db.lock().unwrap().on_read = Some(Arc::new(move || w.close_now()));
    client.send(read(0, false));
    manual.run_all();
    assert_eq!(client.poll()[0].result, Err(DbError::Cancelled));
    assert_eq!(driver.live(), 0);
    assert!(!worker.connected());
}

#[test]
fn a_refused_connection_is_the_requests_error_and_is_tried_again_next_time() {
    let (manual, driver, worker) = setup(5);
    driver.db.lock().unwrap().refuse = Some(DbError::Auth("no".into()));
    let (wake, _) = counted_wake();
    let client = worker.client(wake);
    client.send(Request::Tables(Vec::new()));
    manual.run_all();
    assert_eq!(client.poll()[0].result, Err(DbError::Auth("no".into())));
    assert!(!worker.connected());
    driver.db.lock().unwrap().refuse = None;
    client.send(Request::Tables(Vec::new()));
    manual.run_all();
    assert!(client.poll()[0].result.is_ok());
    assert!(worker.connected());
}

#[test]
fn a_client_gone_takes_its_waiting_requests_with_it() {
    let (manual, driver, worker) = setup(5);
    let (wake, _) = counted_wake();
    let (gone, stays) = (worker.client(wake.clone()), worker.client(wake));
    gone.send(read(0, false));
    gone.send(Request::Structure(users()));
    let kept = stays.send(Request::Tables(Vec::new()));
    drop(gone);
    assert_eq!(worker.waiting(), 1);
    manual.run_all();
    assert_eq!(tickets(&stays.poll()), vec![kept]);
    assert!(driver.db.lock().unwrap().reads.is_empty());
}

#[test]
fn a_commit_is_said_on_the_projects_bus() {
    let manual = Manual::new();
    let driver = FakeDriver::new(FakeDb::users(3), true);
    let bus = EventBus::new();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let _sub = {
        let seen = Arc::clone(&seen);
        bus.subscribe(move |e: &database::events::DataChanged| seen.lock().unwrap().push(e.clone()))
    };
    let id = DataSourceId::new();
    let worker = SessionWorker::new(driver.source(id), manual.runtime(), Some(bus), None);
    let (wake, _) = counted_wake();
    let client = worker.client(wake);
    client.send(read(0, false));
    client.send(Request::Apply(ChangeSet {
        table: users(),
        changes: vec![Change::Delete { key: RowKey::RowId(2) }],
    }));
    client.send(Request::Alter(vec![SchemaChange::RenameTable {
        table: users(),
        to: "people".into(),
    }]));
    manual.run_all();
    let seen = seen.lock().unwrap();
    assert_eq!(
        *seen,
        vec![
            database::events::DataChanged { source: id, table: users(), structure: false },
            database::events::DataChanged { source: id, table: users(), structure: true },
            database::events::DataChanged {
                source: id,
                table: TableRef::new("people"),
                structure: true
            },
        ]
    );
}

/// What the user waits on is an operation of the project's (the Task Manager's, the
/// question before closing): a page being read, its source said; its Stop cancels it as
/// the view's would, and it's kept Stopped. A quick read isn't kept; one the user could
/// have waited on is, with its rows.
#[test]
fn a_read_is_an_operation_stopped_from_the_task_manager() {
    let (manual, driver, worker) = setup(5);
    let activities = Arc::new(workbench_model::Activities::new(Instant::now));
    let project = kernel::ProjectId::new();
    worker.track(Tracking { activities: Arc::clone(&activities), project, source: "shop".into() });
    let (wake, _) = counted_wake();
    let client = worker.client(wake);
    let report = || {
        let mut part = workbench_model::ResourceUse::new("Database", "database.explorer");
        part.project = Some(project);
        activities.report(&mut part);
        part
    };
    // Quick: forgotten once it's done.
    client.send(read(0, false));
    manual.run_all();
    client.poll();
    assert!(report().activities.is_empty(), "a quick read isn't kept");
    // Long: listed while it runs, stopped from the Task Manager.
    let seen = Arc::new(Mutex::new(None));
    {
        let (activities, seen, clock) =
            (Arc::clone(&activities), Arc::clone(&seen), manual.clone());
        driver.db.lock().unwrap().on_read = Some(Arc::new(move || {
            clock.advance(std::time::Duration::from_secs(3));
            let mut part = workbench_model::ResourceUse::new("Database", "database.explorer");
            part.project = Some(project);
            activities.report(&mut part);
            let running = part.activities.into_iter().find(|a| a.running());
            if let Some(a) = &running {
                assert!(activities.stop(a.id), "its Stop");
            }
            *seen.lock().unwrap() = running;
        }));
    }
    let asked = client.send(read(0, false));
    manual.run_all();
    let running = seen.lock().unwrap().clone().expect("listed while it ran");
    assert_eq!(running.kind, workbench_model::ActivityKind::Query);
    assert!(running.subject.contains("users"), "{}", running.subject);
    assert_eq!(running.detail, "shop");
    assert!(running.stoppable);
    assert_eq!(client.poll(), vec![Reply { ticket: asked, result: Err(DbError::Cancelled) }]);
    let ended = report().activities.into_iter().find(|a| a.id == running.id).unwrap();
    assert_eq!(ended.ended.map(|(_, o)| o), Some(workbench_model::Outcome::Stopped));
    // Long and done: kept with its rows.
    driver.db.lock().unwrap().on_read = Some({
        let clock = manual.clone();
        Arc::new(move || clock.advance(std::time::Duration::from_secs(3)))
    });
    client.send(read(0, false));
    manual.run_all();
    let done = report()
        .activities
        .into_iter()
        .find(|a| a.ended.as_ref().is_some_and(|(_, o)| *o == workbench_model::Outcome::Done))
        .unwrap();
    assert_eq!(done.done, 5, "its rows");
}
