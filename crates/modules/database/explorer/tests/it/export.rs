//! Whole results exported in the background from real files (in the system's temp dir,
//! never the user's): 50 000 rows of a SQLite table and of a CSV file written in every
//! format, byte for byte what the grid's copy of all the rows gives (a CSV exported as CSV:
//! the file itself), its order and filter applied, a cancelled export leaving nothing
//! behind, the clipboard taking only a small result.

// Its helpers fail the test they serve, as the tests' own unwraps do.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use database::{
    Cancel, Collect, CompareOp, DataSource, Driver, Filter, Order, Page, Scope, Secret, TableRef,
    Target as DbTarget, Value,
};
use database_explorer::export::{Export, ExportOutcome, ExportRequest, Target};
use database_explorer::grid_source::cell_ref;
use database_explorer::{Request, Response, Runtime, SessionWorker, Sessions};
use kernel::{EventBus, ProjectId};
use ui_components::data_grid::{CellClass, CellRef, ExportFormat, GridColumn, copy_text};

/// The formats written and compared with the copy of every row (an aligned table's widths
/// come from the first block: not compared).
const EXPORT: [&str; 12] = [
    "csv",
    "tsv-header",
    "json",
    "sql",
    "markdown",
    "sql-multirow",
    "sql-update",
    "sql-where",
    "html",
    "xml",
    "one-row",
    "pipe",
];

fn format(id: &str) -> ExportFormat {
    ExportFormat::find(None, id).unwrap()
}

const WAIT: Duration = Duration::from_secs(60);
const ROWS: u64 = 50_000;

/// A folder of the system's temp dir, gone with it.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "explorer-export-{name}-{}-{}",
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
    worker: SessionWorker,
}

impl Drop for Open {
    fn drop(&mut self) {
        self.sessions.close_all();
    }
}

fn open(dir: &TempDir, driver: &str, file: &str) -> Open {
    database::drivers::register(db_sqlite::factory());
    database::drivers::register(db_csv::factory());
    let sessions = Sessions::new(ProjectId::new(), &dir.0, EventBus::new(), Runtime::pool(), None);
    let mut source = DataSource::new(file, driver, Scope::Project);
    source.file = Some(PathBuf::from(file));
    let worker = sessions.open_source(&source, Secret::default()).unwrap();
    Open { sessions, worker }
}

/// `items` of 50 000 rows: numbers, texts with separators, quotes, pipes and line
/// breaks, NULLs.
fn sqlite(dir: &TempDir) -> Open {
    let mut target = DbTarget {
        database: dir.0.join("items.db").to_string_lossy().into_owned(),
        ..DbTarget::default()
    };
    target.options.insert("create".into(), "true".into());
    let mut session = db_sqlite::SqliteDriver.connect(&target, &Secret::default()).unwrap();
    for sql in [
        "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT, price REAL, note TEXT)",
        "WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM n WHERE x < 50000) \
         INSERT INTO items SELECT x, 'n' || x, x * 0.5, \
         CASE WHEN x % 7 = 0 THEN NULL \
              WHEN x % 11 = 0 THEN 'a, \"q\" | it''s' || char(10) || 'b' \
              ELSE 'note ' || x END FROM n",
    ] {
        session.execute(sql, &mut Collect::default(), &Cancel::default()).unwrap();
    }
    drop(session);
    open(dir, "sqlite", "items.db")
}

fn sqlite_columns() -> Vec<GridColumn> {
    vec![
        GridColumn::new("id", "INTEGER", CellClass::Integer),
        GridColumn::new("name", "TEXT", CellClass::Text),
        GridColumn::new("price", "REAL", CellClass::Decimal),
        GridColumn::new("note", "TEXT", CellClass::Text),
    ]
}

/// Every row in one page (another way than the export's blocks): what it must write.
fn all_rows(worker: &SessionWorker, table: &TableRef, page: Page) -> Vec<Vec<Value>> {
    let (wake, rx) = waker();
    let client = worker.client(wake);
    client.send(Request::ReadPage { table: table.clone(), page, ahead: false });
    loop {
        if let Some(reply) = client.poll().into_iter().next() {
            let Ok(Response::Page(rows)) = reply.result else { panic!("{:?}", reply.result) };
            return rows.rows;
        }
        rx.recv_timeout(WAIT).expect("the rows in time");
    }
}

fn expected(id: &str, table: &str, columns: &[GridColumn], rows: &[Vec<Value>]) -> String {
    let refs: Vec<Vec<CellRef<'_>>> =
        rows.iter().map(|r| r.iter().map(cell_ref).collect()).collect();
    let ExportFormat::Text(extractor) = format(id) else { panic!("{id} isn't text") };
    copy_text(extractor.as_ref(), table, columns, None, extractors::Options::default(), &refs)
}

fn run(open: &Open, request: ExportRequest) -> (ExportOutcome, u64) {
    finish(start(open, request))
}

/// An export started (its waker's signals beside it).
fn start(open: &Open, request: ExportRequest) -> (Export, Receiver<()>) {
    let (wake, rx) = waker();
    let export = Export::start(
        &open.worker,
        open.sessions.runtime().clone(),
        request,
        wake,
        Some(open.sessions.clone()),
    );
    (export, rx)
}

/// What an export started ended with, and its rows.
fn finish((export, rx): (Export, Receiver<()>)) -> (ExportOutcome, u64) {
    loop {
        if let Some(outcome) = export.take_outcome() {
            return (outcome, export.rows());
        }
        rx.recv_timeout(WAIT).expect("the export in time");
    }
}

fn request(table: &str, columns: Vec<GridColumn>, id: &str, target: Target) -> ExportRequest {
    ExportRequest {
        table: TableRef::new(table),
        columns,
        order: Vec::new(),
        filter: None,
        format: format(id),
        sql: None,
        options: extractors::Options::default(),
        target,
        total: Some(ROWS),
    }
}

/// No temporary file left in `dir`.
fn no_leftovers(dir: &Path) {
    let parts: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".part"))
        .collect();
    assert!(parts.is_empty(), "{parts:?}");
}

#[test]
fn every_row_of_a_sqlite_table_is_written_in_each_format_exactly() {
    let dir = TempDir::new("sqlite");
    let open = sqlite(&dir);
    let table = TableRef::new("items");
    let rows = all_rows(&open.worker, &table, Page::first(60_000));
    assert_eq!(rows.len() as u64, ROWS);
    // Every format at once (as several exports of a project may run), each written whole.
    let started: Vec<_> = EXPORT
        .iter()
        .map(|id| {
            let path = dir.0.join(format!("out-{id}.{}", format(id).extension()));
            let request = request("items", sqlite_columns(), id, Target::File(path.clone()));
            (*id, path, start(&open, request))
        })
        .collect();
    // A workbook too.
    let path = dir.0.join("out.xlsx");
    let workbook =
        start(&open, request("items", sqlite_columns(), "xlsx", Target::File(path.clone())));
    for (id, path, export) in started {
        let (outcome, written) = finish(export);
        assert_eq!(outcome, ExportOutcome::Written { rows: ROWS, path: path.clone() });
        assert_eq!(written, ROWS);
        let bytes = std::fs::read_to_string(&path).unwrap();
        let want = expected(id, "items", &sqlite_columns(), &rows) + "\n";
        assert!(bytes == want, "{id}: the export differs from the copy of all rows");
    }
    // The workbook: every row, a zip ended by its directory.
    let (outcome, written) = finish(workbook);
    assert_eq!(outcome, ExportOutcome::Written { rows: ROWS, path: path.clone() });
    assert_eq!(written, ROWS);
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[..4], b"PK\x03\x04");
    assert_eq!(&bytes[bytes.len() - 22..bytes.len() - 18], b"PK\x05\x06");
    no_leftovers(&dir.0);
    // The Task Manager saw them, ended.
    let mut part = workbench_model::ResourceUse::new("database", "database.explorer");
    part.project = Some(open.sessions.project());
    open.sessions.activities().report(&mut part);
    assert_eq!(part.activities.iter().filter(|a| !a.running()).count(), EXPORT.len() + 1);
}

#[test]
fn the_order_and_filter_shown_are_applied_and_columns_go_as_shown() {
    let dir = TempDir::new("query");
    let open = sqlite(&dir);
    let table = TableRef::new("items");
    let order = vec![Order { column: "price".into(), descending: true }];
    let filter =
        Filter::Compare { column: "id".into(), op: CompareOp::Greater, value: Value::Int(49_000) };
    let page =
        Page { offset: 0, limit: 60_000, order: order.clone(), filter: Some(filter.clone()) };
    let rows = all_rows(&open.worker, &table, page);
    assert_eq!(rows.len(), 1000);
    // Two columns, moved: their values follow them.
    let mut columns = sqlite_columns();
    columns.swap(0, 2);
    columns.truncate(2);
    let at: Vec<usize> = columns
        .iter()
        .map(|c| sqlite_columns().iter().position(|s| s.name == c.name).unwrap())
        .collect();
    let picked: Vec<Vec<Value>> =
        rows.iter().map(|r| at.iter().map(|&i| r[i].clone()).collect()).collect();
    let path = dir.0.join("top.csv");
    let mut req = request("items", columns.clone(), "csv", Target::File(path.clone()));
    req.order = order;
    req.filter = Some(filter);
    let (outcome, _) = run(&open, req);
    assert!(matches!(outcome, ExportOutcome::Written { rows: 1000, .. }), "{outcome:?}");
    let want = expected("csv", "items", &columns, &picked) + "\n";
    assert_eq!(std::fs::read_to_string(&path).unwrap(), want);
    assert!(want.starts_with("price,name\n25000,n50000\n"), "{}", &want[..40]);
}

#[test]
fn a_cancelled_export_leaves_the_target_as_it_was() {
    let dir = TempDir::new("cancel");
    let open = sqlite(&dir);
    let path = dir.0.join("out.json");
    std::fs::write(&path, "before").unwrap();
    let (wake, rx) = waker();
    let export = Export::start(
        &open.worker,
        open.sessions.runtime().clone(),
        request("items", sqlite_columns(), "json", Target::File(path.clone())),
        wake,
        Some(open.sessions.clone()),
    );
    export.cancel();
    let outcome = loop {
        if let Some(outcome) = export.take_outcome() {
            break outcome;
        }
        rx.recv_timeout(WAIT).expect("the cancel in time");
    };
    assert_eq!(outcome, ExportOutcome::Cancelled);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "before");
    no_leftovers(&dir.0);
    // Dropped before its end (its view closed): cancelled too, nothing left.
    let other = dir.0.join("other.sql");
    let (wake, _rx) = waker();
    drop(Export::start(
        &open.worker,
        open.sessions.runtime().clone(),
        request("items", sqlite_columns(), "sql", Target::File(other.clone())),
        wake,
        None,
    ));
    // Its jobs run on: given a moment to start, then to clean up.
    std::thread::sleep(Duration::from_millis(300));
    let deadline = std::time::Instant::now() + WAIT;
    while std::time::Instant::now() < deadline
        && std::fs::read_dir(&dir.0)
            .unwrap()
            .filter_map(Result::ok)
            .any(|e| e.file_name().to_string_lossy().ends_with(".part"))
    {
        std::thread::sleep(Duration::from_millis(20));
    }
    no_leftovers(&dir.0);
    assert!(!other.exists());
}

#[test]
fn the_clipboard_takes_a_small_result_only() {
    let dir = TempDir::new("clipboard");
    let open = sqlite(&dir);
    let filter =
        Filter::Compare { column: "id".into(), op: CompareOp::LessEq, value: Value::Int(100) };
    let page = Page { offset: 0, limit: 1000, order: Vec::new(), filter: Some(filter.clone()) };
    let rows = all_rows(&open.worker, &TableRef::new("items"), page);
    let mut req = request("items", sqlite_columns(), "markdown", Target::Clipboard);
    req.filter = Some(filter);
    let (outcome, _) = run(&open, req);
    let want = expected("markdown", "items", &sqlite_columns(), &rows);
    assert_eq!(outcome, ExportOutcome::Copied { rows: 100, text: want });
    // All 50 000: too many, said so.
    let (outcome, _) = run(&open, request("items", sqlite_columns(), "json", Target::Clipboard));
    assert!(matches!(outcome, ExportOutcome::Failed(_)), "{outcome:?}");
}

#[test]
fn a_csv_file_is_exported_as_itself_and_in_the_other_formats() {
    let dir = TempDir::new("csv");
    let mut text = String::from("id,name,score\n");
    for i in 1..=ROWS {
        let score = if i % 9 == 0 { String::new() } else { (i * 3 % 1000).to_string() };
        text.push_str(&format!("{i},user {i},{score}\n"));
    }
    std::fs::write(dir.0.join("items.csv"), &text).unwrap();
    let open = open(&dir, "csv", "items.csv");
    let columns = vec![
        GridColumn::new("id", "INTEGER", CellClass::Integer),
        GridColumn::new("name", "TEXT", CellClass::Text),
        GridColumn::new("score", "INTEGER", CellClass::Integer),
    ];
    let path = dir.0.join("copy.csv");
    let (outcome, _) =
        run(&open, request("items", columns.clone(), "csv", Target::File(path.clone())));
    assert!(matches!(outcome, ExportOutcome::Written { rows: ROWS, .. }), "{outcome:?}");
    assert!(std::fs::read_to_string(&path).unwrap() == text, "the CSV written again as it was");
    let rows = all_rows(&open.worker, &TableRef::new("items"), Page::first(60_000));
    for id in ["json", "tsv-header", "sql"] {
        let path = dir.0.join(format!("out.{}", format(id).extension()));
        let (outcome, _) =
            run(&open, request("items", columns.clone(), id, Target::File(path.clone())));
        assert!(matches!(outcome, ExportOutcome::Written { rows: ROWS, .. }), "{outcome:?}");
        let want = expected(id, "items", &columns, &rows) + "\n";
        assert!(std::fs::read_to_string(&path).unwrap() == want, "{id}");
    }
    no_leftovers(&dir.0);
}
