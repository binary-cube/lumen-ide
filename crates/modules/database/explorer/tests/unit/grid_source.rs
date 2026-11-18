//! The grid's source over a repository of the fake driver: blocks asked one at a time,
//! the total, the order and the filter as the domain's, refreshes.

use database::{DataSourceId, Sorting};
use kernel::EventBus;
use ui_components::data_grid::{GridQuery, GridSource, SortKey};

use super::*;
use crate::cache::DEFAULT_LIMIT;
use crate::fake::{FakeDb, FakeDriver, Manual, counted_wake};
use crate::repository::SessionRepository;
use crate::worker::SessionWorker;

struct Setup {
    manual: Manual,
    driver: FakeDriver,
    source: TableSource,
}

fn setup(rows: usize, page: Option<u32>, sorting: Sorting) -> Setup {
    let manual = Manual::new();
    let driver = FakeDriver::new(FakeDb::users(rows), true);
    let worker =
        SessionWorker::new(driver.source(DataSourceId::new()), manual.runtime(), None, None);
    let (wake, _) = counted_wake();
    let bus = EventBus::new();
    let repo =
        SessionRepository::new(&worker, TableRef::new("users"), wake, Some(&bus), DEFAULT_LIMIT);
    let abilities = Abilities {
        sorting,
        edit_rows: true,
        read_only: false,
        view: false,
        bytes: Some(10),
        cancel: true,
    };
    let source = TableSource::new(Box::new(repo), TableRef::new("users"), abilities, page, None);
    Setup { manual, driver, source }
}

impl Setup {
    /// Runs what was asked and takes the replies, until nothing more comes.
    fn settle(&mut self) {
        for _ in 0..20 {
            if self.manual.run_all() == 0 {
                self.source.poll();
                if self.manual.jobs() == 0 {
                    break;
                }
            } else {
                self.source.poll();
            }
        }
    }

    fn reads(&self) -> Vec<(u64, u32)> {
        self.driver.db.lock().unwrap().reads.iter().map(|r| (r.1, r.2)).collect()
    }
}

#[test]
fn the_columns_come_from_the_structure_and_rows_by_block() {
    let mut s = setup(250, Some(100), Sorting::Any);
    assert!(s.source.columns().is_empty());
    assert_eq!(s.source.status(), SourceStatus::Loading);
    s.settle();
    let names: Vec<&str> = s.source.columns().iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["id", "name"]);
    assert!(s.source.columns()[0].key);
    s.source.request(&GridQuery::default(), 0..30);
    assert_eq!(s.source.row_state(0), RowState::Pending);
    s.settle();
    assert_eq!(s.source.row_state(0), RowState::Ready);
    assert_eq!(s.source.cell(0, 1), CellRef::Text("user 1"));
    assert_eq!(s.source.total(), Total::Known(250));
    assert_eq!(s.source.status(), SourceStatus::Ready);
    assert_eq!(s.source.row_state(250), RowState::Missing);
}

#[test]
fn blocks_are_asked_one_after_the_other() {
    let mut s = setup(1000, None, Sorting::Any);
    s.settle();
    // Rows 450–650 span two blocks of 500.
    s.source.request(&GridQuery::default(), 450..650);
    s.settle();
    assert_eq!(s.source.row_state(460), RowState::Ready);
    assert_eq!(s.source.row_state(640), RowState::Ready);
    let reads = s.reads();
    assert!(reads.contains(&(0, 500)) && reads.contains(&(500, 500)), "{reads:?}");
}

#[test]
fn a_short_block_tells_the_total() {
    let mut s = setup(42, Some(100), Sorting::Any);
    s.settle();
    s.source.request(&GridQuery::default(), 0..100);
    s.manual.run_all();
    s.source.poll();
    s.settle();
    assert_eq!(s.source.total(), Total::Known(42));
    assert_eq!(s.source.row_state(41), RowState::Ready);
    assert_eq!(s.source.row_state(42), RowState::Missing);
}

#[test]
fn the_grids_order_and_filter_become_the_domains() {
    let mut s = setup(100, Some(50), Sorting::Any);
    s.settle();
    let query = GridQuery {
        sort: vec![SortKey { column: "name".into(), descending: true }],
        filter: Some(ui_components::data_grid::GridFilter::IsNull("name".into())),
    };
    s.source.request(&query, 0..10);
    s.settle();
    let db = s.driver.db.lock().unwrap();
    let last = db.pages.last().cloned().expect("a page read");
    assert_eq!(last.order, vec![Order { column: "name".into(), descending: true }]);
    assert_eq!(last.filter, Some(Filter::IsNull("name".into())));
}

#[test]
fn a_source_that_cant_sort_says_why_and_reads_its_own_order() {
    let mut s = setup(100, Some(50), Sorting::UpTo(5));
    s.settle();
    let caps = s.source.capabilities();
    assert!(!caps.sort && !caps.filter);
    assert!(caps.limit.is_some());
    let query = GridQuery {
        sort: vec![SortKey { column: "name".into(), descending: false }],
        filter: None,
    };
    s.source.request(&query, 0..10);
    s.settle();
    let db = s.driver.db.lock().unwrap();
    assert!(db.pages.last().is_some_and(|p| p.order.is_empty()));
}

#[test]
fn a_refresh_reads_again_and_a_cancel_stops() {
    let mut s = setup(100, Some(50), Sorting::Any);
    s.settle();
    s.source.request(&GridQuery::default(), 0..10);
    s.settle();
    let before = s.reads().len();
    s.source.refresh();
    s.settle();
    s.source.request(&GridQuery::default(), 0..10);
    s.settle();
    assert!(s.reads().len() > before, "read again");
    assert_eq!(s.source.row_state(0), RowState::Ready);
    s.source.cancel();
    assert!(matches!(s.source.status(), SourceStatus::Failed(_)));
    s.settle();
    // Nothing asked until the rows wanted change.
    let after = s.reads().len();
    s.source.request(&GridQuery::default(), 0..10);
    s.settle();
    assert_eq!(s.reads().len(), after);
    s.source.request(&GridQuery::default(), 0..20);
    s.settle();
    assert_eq!(s.source.row_state(0), RowState::Ready);
}

#[test]
fn values_and_filters_convert_both_ways() {
    let v = Value::Text("x".into());
    assert_eq!(cell_ref(&v), CellRef::Text("x"));
    assert_eq!(value(&CellValue::Int(3)), Value::Int(3));
    let f = filter(&GridFilter::Not(Box::new(GridFilter::Contains {
        column: "a".into(),
        text: "b".into(),
    })));
    assert_eq!(f, Filter::Not(Box::new(Filter::Contains { column: "a".into(), text: "b".into() })));
    assert_eq!(cell_class(TypeClass::Decimal), CellClass::Decimal);
    assert_eq!(message(&DbError::InUse("x".into())), i18n::t("database.file_in_use"));
}

#[test]
fn a_refusal_with_its_place_reads_as_one_without() {
    let placed = DbError::sql(database::SqlError {
        sqlstate: Some("23505".into()),
        position: Some(database::sql::EnginePosition::Character(3)),
        detail: Some("Key (id)=(1) already exists.".into()),
        hint: Some("pick another".into()),
        ..database::SqlError::new("duplicate key")
    });
    let plain = DbError::Query("duplicate key (Key (id)=(1) already exists.)".into());
    assert_eq!(message(&placed), message(&plain));
}

/// The source with its first page in.
fn loaded(rows: usize) -> Setup {
    let mut s = setup(rows, Some(50), Sorting::Any);
    s.settle();
    s.source.request(&GridQuery::default(), 0..10);
    s.settle();
    s
}

#[test]
fn edits_overlay_the_rows_and_mark_them() {
    let mut s = loaded(5);
    assert!(s.source.editing().allowed());
    s.source.set(0, 1, CellValue::Text("Ana".into())).unwrap();
    assert_eq!(s.source.cell(0, 1), CellRef::Text("Ana"));
    assert_eq!(s.source.mark(0, 1), CellMark::Modified);
    assert_eq!(s.source.mark(0, 0), CellMark::None);
    s.source.delete(&[1]);
    assert_eq!(s.source.mark(1, 0), CellMark::Deleted);
    assert!(s.source.set(1, 1, CellValue::Null).is_err(), "a row to delete isn't changed");
    let new = s.source.insert(Some(2)).unwrap();
    assert_eq!(s.source.new_rows(), vec![new]);
    assert_eq!(s.source.row_state(new), RowState::Ready);
    assert_eq!(s.source.cell(new, 0), CellRef::Null, "the key is the engine's");
    assert_eq!(s.source.cell(new, 1), CellRef::Text("user 3"));
    assert_eq!(s.source.pending(), 3);
    s.source.revert(Some(&[1]));
    assert_eq!(s.source.mark(1, 0), CellMark::None);
    s.source.revert(None);
    assert_eq!(s.source.pending(), 0);
    assert!(s.source.new_rows().is_empty());
}

#[test]
fn a_submit_clears_the_edits_and_a_conflict_keeps_them() {
    let mut s = loaded(5);
    s.source.set(0, 1, CellValue::Text("Ana".into())).unwrap();
    let set = s.source.change_set().unwrap();
    assert_eq!(set.changes.len(), 1);
    assert!(!s.source.preview(&set).unwrap().is_empty(), "the SQL that runs");
    // The row changed meanwhile: refused, nothing lost.
    s.driver.db.lock().unwrap().affected = vec![("UPDATE".into(), 0)];
    s.source.submit(set);
    assert!(!s.source.editing().allowed(), "editing waits");
    s.settle();
    assert!(matches!(s.source.take_outcome(), Some(Err(DbError::Conflict(_)))));
    assert_eq!(s.source.pending(), 1);
    assert!(s.source.editing().allowed());
    s.driver.db.lock().unwrap().affected.clear();
    let set = s.source.change_set().unwrap();
    s.source.submit(set);
    s.settle();
    assert!(matches!(s.source.take_outcome(), Some(Ok(_))));
    assert_eq!(s.source.pending(), 0);
}

#[test]
fn editing_is_refused_with_a_reason() {
    let mut s = loaded(3);
    s.source.block(Some("save first".into()));
    assert_eq!(s.source.editing(), Editing::ReadOnly("save first".into()));
    assert!(s.source.set(0, 1, CellValue::Null).is_err());
    assert!(s.source.insert(None).is_none());
    s.source.block(None);
    s.source.abilities.read_only = true;
    assert!(!s.source.editing().allowed());
    s.source.abilities.read_only = false;
    s.source.abilities.view = true;
    assert!(!s.source.editing().allowed());
}

#[test]
fn an_external_drivers_failures_are_said_in_the_users_words() {
    use database::DriverFailure as F;
    let said = |failure: F| message(&DbError::Driver(failure));
    let old = said(F::Protocol { driver: "Duck".into(), theirs: (0, 0), ours: (1, 2) });
    let new = said(F::Protocol { driver: "Duck".into(), theirs: (3, 3), ours: (1, 1) });
    assert!(old.contains("Duck") && old.contains("1–2") && old.contains('0'), "{old}");
    assert_ne!(old, new, "too old and too new say different things");
    assert!(new.contains('3') && !new.contains("database."), "{new}");
    for failure in [
        F::NotStarted { driver: "Duck".into(), why: "not found".into() },
        F::Stopped { driver: "Duck".into(), why: "exit code 3".into() },
        F::TimedOut { driver: "Duck".into(), what: "`read`".into() },
    ] {
        let text = said(failure);
        assert!(text.contains("Duck") && !text.starts_with("database."), "{text}");
    }
}
