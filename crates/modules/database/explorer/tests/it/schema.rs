//! The structure's modal end to end, on real files in the system's temp dir (never the
//! user's): its form filled as a user would, its plan asked of the source's worker (the
//! statements shown are the ones that run), what's lost confirmed first, the changes run
//! in one transaction — a table created with a foreign key and an index, a column renamed
//! with the view naming it rewritten, a column dropped losing a view only once confirmed,
//! a table renamed, a partial unique index, a failure leaving the file byte for byte as it
//! was; a CSV file's columns added, renamed, moved and dropped through the same model; the
//! grids' kept layouts following renames.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use database::danger::Danger;
use database::layouts::{GridLayout, LayoutKey, LayoutStore};
use database::sqlite::ObjectKind;
use database::{
    Cancel, Collect, Driver, FkAction, Order, SchemaChange, Secret, Structure, TableInfo,
    TableKind, TableRef, Target, Value,
};
use database_explorer::schema::{
    Context, Done, Kind, SchemaRequest, SchemaView, Tick, form::ColumnRow, form::KeyRow,
};
use database_explorer::{Request, Response, Runtime, SessionWorker, Sessions};
use kernel::{EventBus, ProjectId};
use ui_components::Viewer;

const WAIT: Duration = Duration::from_secs(20);

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "explorer-schema-{name}-{}-{}",
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

const SCHEMA: &[&str] = &[
    "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT)",
    "CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER, total REAL, \
     status TEXT DEFAULT 'new')",
    "CREATE VIEW big_orders AS SELECT id, total FROM orders WHERE total > 100",
    "INSERT INTO customers (name, email) VALUES ('ana', 'a@x'), ('bob', 'b@x')",
    "INSERT INTO orders (customer_id, total, status) VALUES (1, 50, 'paid'), \
     (2, 150, 'new'), (1, 200, 'cancelled')",
];

fn db(dir: &TempDir) -> PathBuf {
    dir.0.join("shop.db")
}

fn project(name: &str) -> TempDir {
    let dir = TempDir::new(name);
    let mut target =
        Target { database: db(&dir).to_string_lossy().into_owned(), ..Target::default() };
    target.options.insert("create".into(), "true".into());
    let mut session = db_sqlite::SqliteDriver.connect(&target, &Secret::default()).unwrap();
    for sql in SCHEMA {
        session.execute(sql, &mut Collect::default(), &Cancel::default()).unwrap();
    }
    dir
}

fn look(file: &Path, sql: &str) -> Vec<Vec<Value>> {
    let target = Target { database: file.to_string_lossy().into_owned(), ..Target::default() };
    let mut session = db_sqlite::SqliteDriver.connect(&target, &Secret::default()).unwrap();
    let mut rows = Collect::default();
    session.execute(sql, &mut rows, &Cancel::default()).unwrap();
    rows.rows
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

/// A layout store kept in memory (what the project's folder would hold).
#[derive(Default)]
struct Memory(Mutex<Vec<(LayoutKey, GridLayout)>>);

impl LayoutStore for Memory {
    fn load(&self) -> Vec<(LayoutKey, GridLayout)> {
        self.0.lock().unwrap().clone()
    }

    fn save(&self, layouts: &[(LayoutKey, GridLayout)]) -> std::io::Result<()> {
        *self.0.lock().unwrap() = layouts.to_vec();
        Ok(())
    }
}

struct Open {
    sessions: Arc<Sessions>,
    worker: SessionWorker,
    _lease: database_explorer::sessions::FileLease,
}

impl Drop for Open {
    fn drop(&mut self) {
        self.sessions.close_all();
    }
}

/// `file` of the project in `dir` opened as data, as the view opens it.
fn open(dir: &TempDir, file: &Path, driver: &str) -> Open {
    database::drivers::register(db_sqlite::factory());
    database::drivers::register(db_csv::factory());
    let sessions = Sessions::new(ProjectId::new(), &dir.0, EventBus::new(), Runtime::pool(), None);
    let (worker, lease) = sessions.open_file(file, driver, &[]).unwrap();
    Open { sessions, worker, _lease: lease }
}

/// What the worker answers to `request`.
fn ask(worker: &SessionWorker, request: Request) -> Response {
    let (wake, rx) = waker();
    let client = worker.client(wake);
    client.send(request);
    loop {
        if let Some(reply) = client.poll().into_iter().next() {
            return reply.result.unwrap();
        }
        rx.recv_timeout(WAIT).expect("an answer in time");
    }
}

fn tables(worker: &SessionWorker) -> Vec<TableInfo> {
    match ask(worker, Request::Tables(Vec::new())) {
        Response::Tables(t) => t,
        other => panic!("{other:?}"),
    }
}

fn structure(worker: &SessionWorker, table: &str) -> Structure {
    match ask(worker, Request::Structure(TableRef::new(table))) {
        Response::Structure(s) => s,
        other => panic!("{other:?}"),
    }
}

/// The modal for `kind` on `table`, as the data view opens it.
struct Modal {
    view: SchemaView,
    answer: Arc<Mutex<Option<Done>>>,
    rx: Receiver<()>,
}

fn modal(open: &Open, kind: Kind, table: Option<&str>, rows: Vec<(TableRef, u64)>) -> Modal {
    let source = open.worker.source();
    let ctx = Context {
        table: table.map(TableRef::new),
        schema: None,
        structure: table.map(|t| structure(&open.worker, t)),
        tables: tables(&open.worker),
        rows,
        can: source.capabilities.schema_changes,
        dialect: source.dialect.clone(),
    };
    let (wake, rx) = waker();
    let answer = Arc::new(Mutex::new(None));
    let request = SchemaRequest {
        kind,
        ctx,
        worker: open.worker.clone(),
        answer: Arc::clone(&answer),
        wake: Arc::clone(&wake),
    };
    Modal { view: SchemaView::open(request, Some(wake)), answer, rx }
}

impl Modal {
    /// Polls until the plan of the form as it is came back.
    fn planned(&mut self) {
        self.view.replan();
        while self.view.plan().is_none() {
            assert!(self.view.changes().is_ok(), "{:?}", self.view.changes());
            self.rx.recv_timeout(WAIT).expect("a plan in time");
            self.view.poll();
        }
    }

    /// The statements shown.
    fn shown(&mut self) -> Vec<String> {
        self.planned();
        match self.view.plan() {
            Some(Ok(Some(plan))) => plan.script.statements(),
            other => panic!("{other:?}"),
        }
    }

    /// Submits, and waits for the outcome: the changes made, or why not (shown in it).
    fn run(&mut self) -> Result<Done, String> {
        self.planned();
        assert!(self.view.submit(), "submitted");
        loop {
            self.rx.recv_timeout(WAIT).expect("an outcome in time");
            self.view.poll();
            if let Some(done) = self.answer.lock().unwrap().take() {
                return Ok(done);
            }
            if let Some(failed) = self.view.failed_text() {
                return Err(failed);
            }
        }
    }
}

#[test]
fn a_table_is_created_with_a_foreign_key_then_a_partial_unique_index() {
    let dir = project("create");
    let open = open(&dir, &db(&dir), "sqlite");
    let mut m = modal(&open, Kind::CreateTable, None, Vec::new());
    {
        let form = m.view.form_mut().unwrap();
        form.table_name.set_text("invoices");
        let mut customer = ColumnRow::new("customer_id", "INTEGER");
        customer.not_null = true;
        let mut number = ColumnRow::new("number", "TEXT");
        number.unique = true;
        number.collate.set_text("NOCASE");
        let mut total = ColumnRow::new("total", "NUMERIC");
        total.default.set_text("0");
        total.check.set_text("total >= 0");
        form.columns.insert(1, customer);
        form.columns.insert(2, number);
        form.columns.insert(3, total);
        form.keys.push(KeyRow {
            columns: vec!["customer_id".into()],
            target: Some(TableRef::new("customers")),
            references: vec!["id".into()],
            on_delete: FkAction::Cascade,
            ..KeyRow::default()
        });
    }
    let shown = m.shown();
    assert_eq!(
        shown,
        vec![
            "BEGIN IMMEDIATE",
            "CREATE TABLE \"invoices\" (\"id\" INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, \
             \"customer_id\" INTEGER NOT NULL, \"number\" TEXT COLLATE \"NOCASE\", \
             \"total\" NUMERIC DEFAULT 0, FOREIGN KEY (\"customer_id\") REFERENCES \
             \"customers\" (\"id\") ON DELETE CASCADE, UNIQUE (\"number\"), CHECK (total >= 0))",
            "COMMIT",
        ]
    );
    // Nothing lost: no confirmation asked.
    assert!(m.view.dangers().is_empty());
    let done = m.run().unwrap();
    assert!(matches!(done.changes[0], SchemaChange::CreateTable(_)));
    let fk =
        look(&db(&dir), "SELECT \"table\", on_delete FROM pragma_foreign_key_list('invoices')");
    assert_eq!(fk, vec![vec![Value::Text("customers".into()), Value::Text("CASCADE".into())]]);
    look(&db(&dir), "INSERT INTO invoices (customer_id, number) VALUES (1, 'A1')");

    // A partial unique index on it.
    let mut m = modal(&open, Kind::CreateIndex, Some("invoices"), Vec::new());
    {
        let form = m.view.form_mut().unwrap();
        form.toggle_index_column("customer_id");
        form.toggle_index_column("total");
        form.flip_index_order("total");
        form.set_unique(true);
        form.predicate.set_text("total > 0");
        assert_eq!(form.index_name.text(), "invoices_customer_id_total_unique");
    }
    let shown = m.shown();
    assert_eq!(
        shown[1],
        "CREATE UNIQUE INDEX \"invoices_customer_id_total_unique\" ON \"invoices\" \
         (\"customer_id\", \"total\" DESC) WHERE total > 0"
    );
    m.run().unwrap();
    let s = structure(&open.worker, "invoices");
    let index = s.indexes.iter().find(|i| i.name == "invoices_customer_id_total_unique").unwrap();
    assert!(index.unique);
    assert_eq!(index.descending, vec![false, true]);
    assert_eq!(index.predicate.as_deref(), Some("total > 0"));
}

#[test]
fn a_renamed_column_keeps_the_view_working_and_says_it_is_rewritten() {
    let dir = project("rename");
    let open = open(&dir, &db(&dir), "sqlite");
    let mut m = modal(&open, Kind::AlterColumn("total".into()), Some("orders"), Vec::new());
    m.view.form_mut().unwrap().columns[0].name.set_text("amount");
    let shown = m.shown();
    // Only its name: no rebuild.
    assert_eq!(shown[1], "ALTER TABLE \"orders\" RENAME COLUMN \"total\" TO \"amount\"");
    let Some(Ok(Some(plan))) = m.view.plan() else { panic!() };
    assert_eq!(plan.rewritten.len(), 1);
    assert_eq!(
        (plan.rewritten[0].kind, plan.rewritten[0].name.as_str()),
        (ObjectKind::View, "big_orders")
    );
    m.run().unwrap();
    assert_eq!(look(&db(&dir), "SELECT count(*) FROM big_orders"), vec![vec![Value::Int(2)]]);
}

#[test]
fn a_dropped_column_lists_the_lost_view_and_drops_only_once_confirmed() {
    let dir = project("drop");
    let open = open(&dir, &db(&dir), "sqlite");
    let rows = vec![(TableRef::new("orders"), 3)];
    // SQLite's own `DROP COLUMN` refuses a column a view names: rebuilt, the view left out.
    let mut m = modal(&open, Kind::DropColumn("total".into()), Some("orders"), rows);
    m.planned();
    let dangers = m.view.dangers();
    assert!(dangers.contains(&Danger::DropColumn {
        table: TableRef::new("orders"),
        column: "total".into(),
        rows: Some(3)
    }));
    let lost: Vec<(ObjectKind, &str)> = dangers
        .iter()
        .filter_map(|d| match d {
            Danger::LostObject { kind, name, .. } => Some((*kind, name.as_str())),
            _ => None,
        })
        .collect();
    assert!(lost.contains(&(ObjectKind::View, "big_orders")), "{dangers:?}");
    assert!(m.shown().iter().any(|s| s.starts_with("CREATE TABLE \"new_orders\"")));
    // Not before it's confirmed.
    assert!(!m.view.can_submit());
    assert!(!m.view.submit());
    assert_eq!(look(&db(&dir), "SELECT count(*) FROM big_orders"), vec![vec![Value::Int(2)]]);
    m.view.tick(Tick::Confirm);
    assert!(m.view.can_submit());
    m.run().unwrap();
    let views = look(&db(&dir), "SELECT name FROM sqlite_schema WHERE type = 'view'");
    assert!(views.is_empty());
    let columns = look(&db(&dir), "SELECT name FROM pragma_table_info('orders')");
    assert_eq!(columns.len(), 3);
    assert_eq!(look(&db(&dir), "SELECT count(*) FROM orders"), vec![vec![Value::Int(3)]]);
}

#[test]
fn a_table_is_renamed_and_dropped() {
    let dir = project("table");
    let open = open(&dir, &db(&dir), "sqlite");
    let mut m = modal(&open, Kind::RenameTable, Some("customers"), Vec::new());
    m.view.form_mut().unwrap().table_name.set_text("orders");
    assert!(m.view.changes().is_err(), "a name taken");
    m.view.form_mut().unwrap().table_name.set_text("clients");
    assert_eq!(m.shown()[1], "ALTER TABLE \"customers\" RENAME TO \"clients\"");
    m.run().unwrap();
    let names: Vec<String> = tables(&open.worker).iter().map(|t| t.table.name.clone()).collect();
    assert!(names.contains(&"clients".to_owned()) && !names.contains(&"customers".to_owned()));

    let rows = vec![(TableRef::new("clients"), 2)];
    let mut m = modal(&open, Kind::DropTable, Some("clients"), rows);
    m.planned();
    assert_eq!(
        m.view.dangers(),
        vec![Danger::DropTable { table: TableRef::new("clients"), rows: Some(2) }]
    );
    assert!(!m.view.can_submit());
    m.view.tick(Tick::Confirm);
    m.run().unwrap();
    assert!(tables(&open.worker).iter().all(|t| t.table.name != "clients"));
}

#[test]
fn a_failed_change_leaves_the_file_byte_for_byte() {
    let dir = project("failed");
    let open = open(&dir, &db(&dir), "sqlite");
    // A NOT NULL column without a default can't be added to rows (a rebuild copying NULLs).
    let before = std::fs::read(db(&dir)).unwrap();
    let mut m = modal(&open, Kind::AlterColumn("status".into()), Some("orders"), Vec::new());
    {
        let row = &mut m.view.form_mut().unwrap().columns[0];
        row.not_null = true;
        row.default.set_text("");
    }
    look(&db(&dir), "UPDATE orders SET status = NULL WHERE id = 1");
    let before_update = std::fs::read(db(&dir)).unwrap();
    assert_ne!(before, before_update);
    m.planned();
    m.view.tick(Tick::Confirm);
    let failed = m.run().unwrap_err();
    assert!(!failed.is_empty());
    assert_eq!(std::fs::read(db(&dir)).unwrap(), before_update, "the file as it was");
    assert_eq!(
        look(
            &db(&dir),
            "SELECT \"notnull\" FROM pragma_table_info('orders') WHERE name = 'status'"
        ),
        vec![vec![Value::Int(0)]]
    );
}

#[test]
fn a_csv_file_s_columns_change_through_the_same_model_and_layouts_follow() {
    let dir = TempDir::new("csv");
    let file = dir.0.join("people.csv");
    std::fs::write(&file, "name,age\nana,30\nbob,40\n").unwrap();
    let open = open(&dir, &file, "csv");
    let store = Arc::new(Memory::default());
    open.sessions.set_layout_store(store, Duration::ZERO);
    let table = tables(&open.worker)[0].table.clone();
    open.sessions.keep_layout(
        &file,
        &table,
        GridLayout {
            widths: vec![("age".into(), 90.0)],
            order: vec!["age".into(), "name".into()],
            sort: vec![Order { column: "age".into(), descending: true }],
            filter: Some("age > 20".into()),
            ..GridLayout::default()
        },
    );
    let name = table.name.clone();
    let csv_modal = |kind: Kind| {
        let source = open.worker.source();
        let ctx = Context {
            table: Some(table.clone()),
            schema: None,
            structure: Some(structure(&open.worker, &name)),
            tables: tables(&open.worker),
            rows: Vec::new(),
            can: source.capabilities.schema_changes,
            dialect: None,
        };
        let (wake, rx) = waker();
        let answer = Arc::new(Mutex::new(None));
        let request = SchemaRequest {
            kind,
            ctx,
            worker: open.worker.clone(),
            answer: Arc::clone(&answer),
            wake: Arc::clone(&wake),
        };
        Modal { view: SchemaView::open(request, Some(wake)), answer, rx }
    };
    // Added, filled.
    let mut m = csv_modal(Kind::AddColumn);
    assert_eq!(m.view.form().unwrap().texts().len(), 1, "a CSV column is only named");
    m.view.form_mut().unwrap().columns[0].name.set_text("city");
    m.planned();
    assert!(matches!(m.view.plan(), Some(Ok(None))), "no SQL: summed up");
    m.run().unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "name,age,city\nana,30,\nbob,40,\n");
    // Renamed: the layout follows.
    let mut m = csv_modal(Kind::AlterColumn("age".into()));
    m.view.form_mut().unwrap().columns[0].name.set_text("years");
    let done = m.run().unwrap();
    assert!(matches!(&done.changes[0], SchemaChange::RenameColumn { to, .. } if to == "years"));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "name,years,city\nana,30,\nbob,40,\n");
    let kept = open.sessions.layout(&file, &table).unwrap();
    assert_eq!(kept.widths, vec![("years".to_owned(), 90.0)]);
    assert_eq!(kept.order, vec!["years".to_owned(), "name".to_owned()]);
    assert_eq!(kept.sort[0].column, "years");
    assert_eq!(kept.filter.as_deref(), Some("years > 20"));
    // Moved first.
    let mut m = csv_modal(Kind::MoveColumn("city".into()));
    m.view.form_mut().unwrap().position = 0;
    m.run().unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "city,name,years\n,ana,30\n,bob,40\n");
    // Dropped: confirmed (it holds values), the layout lets it go.
    let mut m = csv_modal(Kind::DropColumn("years".into()));
    m.planned();
    assert!(!m.view.can_submit());
    m.view.tick(Tick::Confirm);
    m.run().unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "city,name\n,ana\n,bob\n");
    let kept = open.sessions.layout(&file, &table).unwrap();
    assert!(kept.widths.is_empty() && kept.sort.is_empty() && kept.filter.is_none());
    assert_eq!(kept.order, vec!["name".to_owned()]);
    let _ = TableKind::Table;
}

#[test]
fn the_keys_type_tab_through_the_fields_submit_with_enter_and_close_with_esc() {
    use ui_components::{ViewerAction, ViewerKey};
    let dir = project("keys");
    let open = open(&dir, &db(&dir), "sqlite");
    let mut m = modal(&open, Kind::AddColumn, Some("orders"), Vec::new());
    assert!(m.view.wants_text(), "the first field has the keys");
    m.view.text_input("note");
    m.view.edit_key(ViewerKey::Tab);
    m.view.text_input("TEXT");
    // Typing a type suggests the dialect's, ↓ ↵ take one.
    m.view.edit_key(ViewerKey::Backspace);
    m.view.edit_key(ViewerKey::Backspace);
    assert!(m.view.popup_rows().iter().any(|(s, _)| s == "TEXT"));
    m.view.edit_key(ViewerKey::Down);
    m.view.edit_key(ViewerKey::Enter);
    assert_eq!(m.view.form().unwrap().columns[0].ty.text(), "TEXT");
    m.view.form_mut().unwrap().columns[0].ty.set_text("TEXT");
    m.planned();
    assert!(m.view.edit_key(ViewerKey::Enter), "↵ submits once it can");
    loop {
        m.rx.recv_timeout(WAIT).expect("an outcome in time");
        m.view.poll();
        if m.answer.lock().unwrap().is_some() {
            break;
        }
    }
    assert_eq!(m.view.take_action(), Some(ViewerAction::Close));
    let columns = look(&db(&dir), "SELECT name FROM pragma_table_info('orders')");
    assert_eq!(columns.last(), Some(&vec![Value::Text("note".into())]));

    let mut m = modal(&open, Kind::RenameTable, Some("orders"), Vec::new());
    m.view.edit_key(ViewerKey::Escape);
    assert_eq!(m.view.take_action(), Some(ViewerAction::Close));
    assert!(tables(&open.worker).iter().any(|t| t.table.name == "orders"), "nothing ran");
}
