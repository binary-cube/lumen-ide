use database::DataSourceId;

use super::*;
use crate::fake::{FakeDb, FakeDriver, Manual, counted_wake};

fn read(rows: usize, limit: usize) -> (Manual, PaneRows) {
    let manual = Manual::new();
    let driver = FakeDriver::new(FakeDb::users(rows), true);
    let worker =
        SessionWorker::new(driver.source(DataSourceId::new()), manual.runtime(), None, None);
    let (wake, _) = counted_wake();
    let ask = ReadAsk { table: TableRef::new("users"), order: Vec::new(), filter: None, limit };
    let reading = PaneRows::start(&worker, manual.runtime(), ask, wake);
    manual.run_all();
    (manual, reading)
}

/// Every page is read until the table ends: its rows kept as they came, none more.
#[test]
fn a_table_is_read_to_its_end() {
    let (_manual, reading) = read(4500, 1_000_000);
    let read = reading.read();
    assert_eq!((read.rows, read.done, read.more), (4500, true, false));
    assert_eq!(read.pages.len(), 3, "pages as they came");
    let lent = LentPages::chained(Vec::new(), read.pages);
    assert_eq!(charts_model::Tabular::rows(&lent), 4500);
}

/// Past the limit nothing more is read, and it says the table holds more.
#[test]
fn reading_stops_at_the_limit() {
    let (_manual, reading) = read(4500, 3000);
    let read = reading.read();
    assert_eq!((read.rows, read.done, read.more), (3000, true, true));
}

/// The limit reached inside the table's last (short) page: the rows cut off are said to be
/// there (more to fetch), never "all of them".
#[test]
fn rows_cut_in_the_last_page_are_still_more() {
    let (_manual, reading) = read(1500, 500);
    let read = reading.read();
    assert_eq!((read.rows, read.done, read.more), (500, true, true));
}
