use database::{CellChange, Change, DataSourceId, RowKey, Value};

use super::*;
use crate::cache::DEFAULT_LIMIT;
use crate::fake::{FakeDb, FakeDriver, Manual, counted_wake};

fn users() -> TableRef {
    TableRef::new("users")
}

struct Setup {
    manual: Manual,
    driver: FakeDriver,
    worker: SessionWorker,
    bus: EventBus,
}

fn setup(db: FakeDb, sql: bool) -> Setup {
    let manual = Manual::new();
    let driver = FakeDriver::new(db, sql);
    let bus = EventBus::new();
    let worker = SessionWorker::new(
        driver.source(DataSourceId::new()),
        manual.runtime(),
        Some(bus.clone()),
        None,
    );
    Setup { manual, driver, worker, bus }
}

impl Setup {
    fn repository(&self, limit: usize) -> SessionRepository {
        let (wake, _) = counted_wake();
        SessionRepository::new(&self.worker, users(), wake, Some(&self.bus), limit)
    }

    fn reads(&self) -> Vec<u64> {
        self.driver.db.lock().unwrap().reads.iter().map(|r| r.1).collect()
    }
}

fn page(offset: u64, limit: u32) -> Page {
    Page { offset, ..Page::first(limit) }
}

#[test]
fn what_isnt_known_is_pending_until_it_comes() {
    let s = setup(FakeDb::users(250), true);
    let mut repo = s.repository(DEFAULT_LIMIT);
    assert!(repo.page(&page(0, 100)).is_pending());
    assert!(repo.count(None).is_pending());
    assert!(repo.structure().is_pending());
    // Asked once, however often the view looks.
    assert!(repo.page(&page(0, 100)).is_pending());
    assert_eq!(repo.waiting(), 3);
    s.manual.run_all();
    let events = repo.poll();
    assert_eq!(
        events,
        vec![
            RepositoryEvent::Page(page(0, 100)),
            RepositoryEvent::Count,
            RepositoryEvent::Structure
        ]
    );
    let rows = repo.page(&page(0, 100)).ready().unwrap();
    assert_eq!(rows.len(), 100);
    assert_eq!(rows.rows[0][1], Value::Text("user 1".into()));
    assert_eq!(repo.count(None), Fetch::Ready(250));
    assert_eq!(repo.structure().ready().unwrap().columns.len(), 2);
    assert!(repo.memory() > 0);
}

#[test]
fn a_full_page_has_the_next_one_read_ahead() {
    let s = setup(FakeDb::users(150), true);
    let mut repo = s.repository(DEFAULT_LIMIT);
    repo.page(&page(0, 100));
    s.manual.run_all();
    repo.poll();
    s.manual.run_all();
    // Read ahead: no event (nobody asked), there when asked.
    assert!(repo.poll().is_empty());
    assert_eq!(s.reads(), vec![0, 100]);
    assert_eq!(repo.page(&page(100, 100)).ready().map(|r| r.len()), Some(50));
    // The last page wasn't full: nothing after it.
    assert_eq!(s.manual.jobs(), 0);
    assert_eq!(s.reads(), vec![0, 100]);
}

#[test]
fn a_page_read_ahead_and_then_wanted_is_asked_in_its_turn() {
    let s = setup(FakeDb::users(300), true);
    let mut repo = s.repository(DEFAULT_LIMIT);
    repo.page(&page(0, 100));
    s.manual.run_all();
    repo.poll();
    // The read ahead waits; the view wants it now.
    assert!(repo.page(&page(100, 100)).is_pending());
    assert_eq!(repo.waiting(), 1);
    s.manual.run_all();
    assert_eq!(repo.poll(), vec![RepositoryEvent::Page(page(100, 100))]);
    assert_eq!(&s.reads()[..2], &[0, 100]);
}

#[test]
fn another_order_or_filter_reads_its_own_pages() {
    let s = setup(FakeDb::users(30), true);
    let mut repo = s.repository(DEFAULT_LIMIT);
    repo.page(&page(0, 10));
    s.manual.run_all();
    repo.poll();
    let sorted = Page {
        order: vec![database::Order { column: "id".into(), descending: true }],
        ..page(0, 10)
    };
    assert!(repo.page(&sorted).is_pending());
    s.manual.run_all();
    repo.poll();
    let rows = repo.page(&sorted).ready().unwrap();
    assert_eq!(rows.rows[0][0], Value::Int(30));
    // The first order's pages went with it.
    assert!(repo.page(&page(0, 10)).is_pending());
}

#[test]
fn pages_stay_within_the_memory_limit_and_go_on_clear_caches() {
    let s = setup(FakeDb::users(1000), true);
    let one = {
        let mut repo = s.repository(DEFAULT_LIMIT);
        repo.page(&page(0, 100));
        s.manual.run_all();
        repo.poll();
        repo.memory()
    };
    s.manual.run_all();
    let mut repo = s.repository(one * 2);
    for offset in [0, 100, 200, 300, 400] {
        repo.page(&page(offset, 100));
        s.manual.run_all();
        repo.poll();
    }
    assert!(repo.memory() <= one * 2 + one / 2, "{} of {}", repo.memory(), one * 2);
    assert!(repo.page(&page(400, 100)).ready().is_some());
    repo.purge();
    assert_eq!(repo.memory(), 0);
    let before = s.reads().len();
    assert!(repo.page(&page(400, 100)).is_pending(), "read again");
    s.manual.run_all();
    repo.poll();
    assert!(s.reads().len() > before);
}

fn rename(id: i64, old: &str, new: &str) -> ChangeSet {
    ChangeSet {
        table: users(),
        changes: vec![Change::Update {
            key: RowKey::RowId(id),
            cells: vec![CellChange {
                column: "name".into(),
                old: Value::Text(old.into()),
                new: Value::Text(new.into()),
            }],
        }],
    }
}

#[test]
fn a_commit_refreshes_this_view_and_the_tables_other_views() {
    let s = setup(FakeDb::users(20), true);
    let mut repo = s.repository(DEFAULT_LIMIT);
    let mut other = s.repository(DEFAULT_LIMIT);
    repo.page(&page(0, 10));
    other.page(&page(0, 10));
    s.manual.run_all();
    repo.poll();
    other.poll();
    s.manual.run_all();
    assert_eq!(
        repo.preview(&rename(1, "user 1", "one")).unwrap(),
        vec![
            "BEGIN IMMEDIATE".to_owned(),
            "UPDATE \"users\" SET \"name\" = 'one' WHERE rowid = 1 AND \"name\" = 'user 1'"
                .to_owned(),
            "COMMIT".to_owned(),
        ]
    );
    repo.apply(rename(1, "user 1", "one"));
    s.manual.run_all();
    let events = repo.poll();
    assert!(matches!(&events[0], RepositoryEvent::Applied(Ok(a)) if a.affected == 1), "{events:?}");
    assert_eq!(events[1..], [RepositoryEvent::Refreshed]);
    assert_eq!(repo.memory(), 0);
    assert!(repo.page(&page(0, 10)).is_pending());
    // The other view of the table heard it on the project's bus.
    assert_eq!(other.poll(), vec![RepositoryEvent::Refreshed]);
    assert!(other.page(&page(0, 10)).is_pending());
}

#[test]
fn a_conflict_applies_nothing_and_keeps_what_was_read() {
    let s = setup(FakeDb::users(20), true);
    s.driver.db.lock().unwrap().affected = vec![("UPDATE".into(), 0)];
    let mut repo = s.repository(DEFAULT_LIMIT);
    repo.page(&page(0, 10));
    s.manual.run_all();
    repo.poll();
    repo.apply(rename(1, "user 1", "one"));
    s.manual.run_all();
    let events = repo.poll();
    assert!(
        matches!(events.as_slice(), [RepositoryEvent::Applied(Err(DbError::Conflict(_)))]),
        "{events:?}"
    );
    assert!(s.driver.db.lock().unwrap().committed.is_empty());
    assert!(repo.page(&page(0, 10)).ready().is_some());
}

#[test]
fn a_table_without_a_key_is_only_read() {
    let mut db = FakeDb::users(3);
    db.tables[0].structure.rowid = false;
    db.tables[0].structure.columns[0].in_key = false;
    let s = setup(db, true);
    let mut repo = s.repository(DEFAULT_LIMIT);
    repo.structure();
    s.manual.run_all();
    repo.poll();
    repo.apply(rename(1, "user 1", "one"));
    assert!(matches!(
        repo.poll().as_slice(),
        [RepositoryEvent::Applied(Err(DbError::ReadOnly(_)))]
    ));
    assert_eq!(s.manual.jobs(), 0, "nothing sent");
}

#[test]
fn a_source_without_sql_works_through_the_same_contract() {
    let s = setup(FakeDb::users(12), false);
    let mut repo = s.repository(DEFAULT_LIMIT);
    let filter = database::Filter::Contains { column: "name".into(), text: "user 1".into() };
    let filtered = Page { filter: Some(filter.clone()), ..page(0, 10) };
    repo.page(&filtered);
    repo.count(Some(&filter));
    s.manual.run_all();
    repo.poll();
    // `user 1`, `user 10`, `user 11`, `user 12`.
    assert_eq!(repo.count(Some(&filter)), Fetch::Ready(4));
    assert_eq!(repo.page(&filtered).ready().map(|r| r.len()), Some(4));
    assert_eq!(repo.preview(&rename(1, "user 1", "one")), Ok(Vec::new()));
    repo.apply(rename(1, "user 1", "one"));
    s.manual.run_all();
    assert!(matches!(repo.poll()[0], RepositoryEvent::Applied(Ok(_))));
    assert_eq!(s.driver.db.lock().unwrap().applied.len(), 1);
}

#[test]
fn a_refresh_drops_what_was_asked_but_waits_for_a_commit() {
    let s = setup(FakeDb::users(20), true);
    let mut repo = s.repository(DEFAULT_LIMIT);
    repo.page(&page(0, 10));
    repo.apply(rename(1, "user 1", "one"));
    repo.refresh();
    assert_eq!(repo.poll(), vec![RepositoryEvent::Refreshed]);
    assert_eq!(repo.waiting(), 1);
    s.manual.run_all();
    let events = repo.poll();
    assert!(matches!(events[0], RepositoryEvent::Applied(Ok(_))));
    // The page dropped was never read.
    assert!(s.reads().is_empty());
}
