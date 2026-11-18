//! The list's counts: the estimates shown at once (`~`), exact counts only when cheap or
//! asked, at most a few at once, a huge table's never on its own, what was asked let go
//! when the tables are listed again.

use database::{DataSourceId, TableRef};

use super::*;
use crate::fake::{FakeDb, FakeDriver, Manual, counted_wake};
use crate::worker::SessionWorker;

fn tables(n: usize, rows: Option<u64>) -> Vec<TableInfo> {
    (0..n)
        .map(|i| TableInfo { table: TableRef::new(format!("t{i}")), kind: TableKind::Table, rows })
        .collect()
}

fn setup() -> (Manual, FakeDriver, SessionWorker, Client) {
    let manual = Manual::new();
    let driver = FakeDriver::new(FakeDb::users(7), true);
    let worker =
        SessionWorker::new(driver.source(DataSourceId::new()), manual.runtime(), None, None);
    let client = worker.client(counted_wake().0);
    (manual, driver, worker, client)
}

#[test]
fn estimates_are_marked_and_exact_counts_are_not() {
    assert_eq!(ListCount::estimate(29_600).label, "~29.6k");
    assert_eq!(ListCount::estimate(73).label, "~73");
    assert_eq!(ListCount::exact(29_600).label, "29.6k");
    assert!(ListCount::exact(1).exact && !ListCount::estimate(1).exact);
}

/// 1000 tables listed: their estimates at once, nothing counted (nothing asked) unless
/// it's cheap; then all of them, never more than [`IN_FLIGHT`] at once.
#[test]
fn a_thousand_tables_show_estimates_and_are_counted_only_when_cheap() {
    let (manual, driver, worker, client) = setup();
    let list = tables(1000, Some(29_600));
    let mut counts = ListCounts::default();
    counts.reset(Some(&client), &list, false);
    assert_eq!(counts.get(0).unwrap().label, "~29.6k");
    assert_eq!((counts.in_flight(), worker.waiting()), (0, 0), "nothing asked");
    // Cheap (a small file): every one, a few at a time.
    counts.reset(Some(&client), &list, true);
    assert_eq!(counts.in_flight(), IN_FLIGHT);
    assert_eq!(worker.waiting(), IN_FLIGHT);
    let mut rounds = 0;
    while counts.in_flight() > 0 {
        manual.run_all();
        for reply in client.poll() {
            assert!(counts.take(&reply, Some(&client), &list).is_some());
        }
        assert!(counts.in_flight() <= IN_FLIGHT);
        rounds += 1;
    }
    assert!(rounds >= 1000 / IN_FLIGHT);
    assert!((0..1000).all(|i| counts.get(i).is_some_and(|c| c.exact && c.label == "7")));
    assert_eq!(driver.db.lock().unwrap().counts, 1000);
}

/// The table under the mouse goes first; a huge one, a view or one counted already isn't
/// asked; a table with no estimate shows nothing until counted.
#[test]
fn a_table_is_counted_when_asked_unless_huge() {
    let (manual, _driver, _worker, client) = setup();
    let mut list = tables(4, None);
    list[1].rows = Some(crate::grid_source::COUNT_ON_REQUEST + 1);
    list[2].kind = TableKind::View;
    let mut counts = ListCounts::default();
    counts.reset(Some(&client), &list, false);
    assert!(counts.get(0).is_none(), "no estimate: nothing shown yet");
    assert!(!counts.want(1, Some(&client), &list), "huge: its estimate stays");
    assert_eq!(counts.get(1).unwrap().label, "~1M");
    assert!(!counts.want(2, Some(&client), &list), "a view: its grid counts it");
    assert!(counts.want(0, Some(&client), &list));
    assert!(!counts.want(0, Some(&client), &list), "asked already");
    manual.run_all();
    for reply in client.poll() {
        assert_eq!(counts.take(&reply, Some(&client), &list), Some(true));
    }
    assert_eq!(counts.get(0), Some(&ListCount::exact(7)));
    assert!(!counts.want(0, Some(&client), &list), "counted already");
    // Its grid's total: shown as exact, not asked.
    assert!(counts.set_exact(3, 12));
    assert!(!counts.set_exact(3, 12));
    assert!(!counts.want(3, Some(&client), &list));
}

/// Listed again: what was asked is cancelled (the replies are no longer ours), the new
/// estimates shown.
#[test]
fn listing_again_lets_the_asked_counts_go() {
    let (manual, driver, worker, client) = setup();
    let list = tables(10, Some(5));
    let mut counts = ListCounts::default();
    counts.reset(Some(&client), &list, true);
    assert_eq!(worker.waiting(), IN_FLIGHT);
    counts.reset(Some(&client), &tables(10, Some(9)), false);
    assert_eq!((counts.in_flight(), counts.waiting(), worker.waiting()), (0, 0, 0));
    manual.run_all();
    for reply in client.poll() {
        assert_eq!(reply.result, Err(database::DbError::Cancelled));
        assert_eq!(counts.take(&reply, Some(&client), &list), None, "not ours any more");
    }
    assert_eq!(driver.db.lock().unwrap().counts, 0);
    assert_eq!(counts.get(0).unwrap().label, "~9");
}
