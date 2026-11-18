//! For the tests: a pool run by hand and a clock set by hand ([`Manual`]), and an in-memory
//! database ([`FakeDb`]) behind a driver whose sessions do what the extended `Session` says
//! — tables, structure, pages with order / filter / paging, counts, statements run with
//! their affected rows, transactions (what's committed kept, what's rolled back gone).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use database::sqlite::{RebuildSource, Sqlite, SqliteTable};
use database::{
    Applied, Cancel, Capabilities, ChangeSet, Column, ColumnInfo, DataSourceId, DbError, Dialect,
    Filter, Kind, Language, Node, Outcome, Page, RowKey, RowSink, SchemaChange, SchemaChanges,
    Secret, Session, Structure, TableInfo, TableKind, TableRef, Target, Value,
};

use crate::runtime::{Job, Runtime};
use crate::worker::Source;

/// A pool whose jobs run when the test says, and a clock that moves when it says.
#[derive(Clone)]
pub struct Manual {
    jobs: Arc<Mutex<VecDeque<Job>>>,
    now: Arc<Mutex<Instant>>,
}

impl Manual {
    pub fn new() -> Self {
        Self { jobs: Arc::default(), now: Arc::new(Mutex::new(Instant::now())) }
    }

    pub fn runtime(&self) -> Runtime {
        let (jobs, now) = (Arc::clone(&self.jobs), Arc::clone(&self.now));
        Runtime::new(move |job| jobs.lock().unwrap().push_back(job), move || *now.lock().unwrap())
    }

    /// Runs the next job: `false` if there's none.
    pub fn run_next(&self) -> bool {
        let job = self.jobs.lock().unwrap().pop_front();
        job.map(|job| job()).is_some()
    }

    /// Runs jobs until none is left; how many ran.
    pub fn run_all(&self) -> usize {
        let mut ran = 0;
        while self.run_next() {
            ran += 1;
        }
        ran
    }

    pub fn jobs(&self) -> usize {
        self.jobs.lock().unwrap().len()
    }

    pub fn advance(&self, by: Duration) {
        *self.now.lock().unwrap() += by;
    }

    pub fn now(&self) -> Instant {
        *self.now.lock().unwrap()
    }
}

/// A table of the fake database: its structure and its rows (each found by its row id,
/// its position + 1).
#[derive(Debug, Clone)]
pub struct FakeTable {
    pub structure: Structure,
    pub rows: Vec<Vec<Value>>,
}

/// The fake database: its tables, every statement run (in order), what was committed.
#[derive(Default)]
pub struct FakeDb {
    pub tables: Vec<FakeTable>,
    /// Every statement `execute` was given.
    pub log: Vec<String>,
    /// The statements of the transactions committed.
    pub committed: Vec<String>,
    /// The transaction open: its statements.
    pub open: Option<Vec<String>>,
    /// The rows a statement changes, by its first words (the default: 1).
    pub affected: Vec<(String, u64)>,
    /// The rows a statement returns, by its first words (the default: none).
    pub returned: Vec<(String, u64)>,
    /// The rows the next statements starting with these words return, once each, in
    /// order (before `returned`).
    pub results: VecDeque<(String, Vec<Vec<Value>>)>,
    /// What `read` saw (table, offset, limit), in order.
    pub reads: Vec<(String, u64, u32)>,
    /// The pages `read` was asked for, whole (their order and filter).
    pub pages: Vec<Page>,
    /// Called while a read runs (a test cancels it from there).
    pub on_read: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Called while a count runs, outside the lock (a test holds it there: a slow count).
    pub on_count: Option<Arc<dyn Fn() + Send + Sync>>,
    /// The counts run (`COUNT(*)` statements and the session's own), in all.
    pub counts: usize,
    /// The rows applied through `apply` (a source that writes no SQL).
    pub applied: Vec<ChangeSet>,
    pub foreign_keys: bool,
    /// The next connections fail with this.
    pub refuse: Option<DbError>,
    /// What each connection was asked to open, in order.
    pub targets: Vec<Target>,
}

impl FakeDb {
    /// `users (id INTEGER PRIMARY KEY, name TEXT)` with `rows` rows (`user 1`…).
    pub fn users(rows: usize) -> Self {
        let mut id = ColumnInfo::new("id", "INTEGER");
        id.in_key = true;
        id.nullable = false;
        let structure = Structure {
            table: TableRef::new("users"),
            columns: vec![id, ColumnInfo::new("name", "TEXT")],
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            rowid: true,
            lines: false,
        };
        let rows = (1..=rows as i64)
            .map(|i| vec![Value::Int(i), Value::Text(format!("user {i}"))])
            .collect();
        Self { tables: vec![FakeTable { structure, rows }], ..Self::default() }
    }

    fn table(&self, table: &TableRef) -> Result<&FakeTable, DbError> {
        self.tables
            .iter()
            .find(|t| t.structure.table == *table)
            .ok_or_else(|| DbError::Query(format!("no table {table}")))
    }

    fn first(rules: &[(String, u64)], sql: &str) -> Option<u64> {
        rules.iter().find(|(start, _)| sql.starts_with(start.as_str())).map(|(_, n)| *n)
    }
}

/// `values` kept by `filter` (equality, `IS NULL`, contains, and / or / not).
fn keeps(columns: &[ColumnInfo], values: &[Value], filter: &Filter) -> bool {
    let at = |name: &str| columns.iter().position(|c| c.name == name).map(|i| &values[i]);
    match filter {
        Filter::Compare { column, op, value } => {
            let Some(v) = at(column) else { return false };
            let order = match (v, value) {
                (Value::Int(a), Value::Int(b)) => a.cmp(b),
                (a, b) => a.display().cmp(&b.display()),
            };
            use database::CompareOp::*;
            match op {
                Eq => order.is_eq(),
                NotEq => order.is_ne(),
                Less => order.is_lt(),
                LessEq => order.is_le(),
                Greater => order.is_gt(),
                GreaterEq => order.is_ge(),
            }
        }
        Filter::IsNull(column) => matches!(at(column), Some(Value::Null)),
        Filter::NotNull(column) => !matches!(at(column), Some(Value::Null) | None),
        Filter::Contains { column, text } => {
            at(column).is_some_and(|v| v.display().to_lowercase().contains(&text.to_lowercase()))
        }
        Filter::And(all) => all.iter().all(|f| keeps(columns, values, f)),
        Filter::Or(any) => any.iter().any(|f| keeps(columns, values, f)),
        Filter::Not(inner) => !keeps(columns, values, inner),
    }
}

/// A session of the fake database (counted while open).
pub struct FakeSession {
    db: Arc<Mutex<FakeDb>>,
    live: Arc<AtomicUsize>,
}

impl Drop for FakeSession {
    fn drop(&mut self) {
        self.live.fetch_sub(1, Ordering::SeqCst);
    }
}

impl FakeSession {
    /// A count starts: said, then held as long as the test wants.
    fn counting(&self) {
        let on_count = {
            let mut db = self.db.lock().unwrap();
            db.counts += 1;
            db.on_count.clone()
        };
        if let Some(on_count) = on_count {
            on_count();
        }
    }

    fn rows(
        &self,
        table: &TableRef,
        page_filter: Option<&Filter>,
    ) -> Result<Vec<(i64, Vec<Value>)>, DbError> {
        let db = self.db.lock().unwrap();
        let t = db.table(table)?;
        Ok(t.rows
            .iter()
            .enumerate()
            .filter(|(_, r)| page_filter.is_none_or(|f| keeps(&t.structure.columns, r, f)))
            .map(|(i, r)| (i as i64 + 1, r.clone()))
            .collect())
    }
}

impl Session for FakeSession {
    fn server(&self) -> String {
        "Fake 1.0".into()
    }

    fn children(&mut self, _path: &[String]) -> Result<Vec<Node>, DbError> {
        Ok(Vec::new())
    }

    fn execute(
        &mut self,
        query: &str,
        sink: &mut dyn RowSink,
        _cancel: &Cancel,
    ) -> Result<Outcome, DbError> {
        if query.starts_with("SELECT COUNT(*)") {
            self.counting();
        }
        let mut db = self.db.lock().unwrap();
        db.log.push(query.to_owned());
        if query.starts_with("SELECT COUNT(*)") {
            let total = db.tables[0].rows.len() as i64;
            sink.columns(&[Column { name: "count".into(), type_name: "INTEGER".into() }]);
            sink.row(vec![Value::Int(total)]);
            return Ok(Outcome { affected: None, rows: 1, stopped: false });
        }
        if query.starts_with("FAIL") {
            return Err(DbError::Query("refused".into()));
        }
        match query {
            q if q.starts_with("BEGIN") => db.open = Some(Vec::new()),
            "COMMIT" => {
                let done = db.open.take().unwrap_or_default();
                db.committed.extend(done);
            }
            "ROLLBACK" => db.open = None,
            q => {
                if let Some(open) = &mut db.open {
                    open.push(q.to_owned());
                }
            }
        }
        let next = db.results.iter().position(|(start, _)| query.starts_with(start.as_str()));
        let returned = match next.and_then(|at| db.results.remove(at)) {
            Some((_, rows)) => {
                let n = rows.len() as u64;
                for row in rows {
                    sink.row(row);
                }
                n
            }
            None => {
                let n = FakeDb::first(&db.returned, query).unwrap_or(0);
                for _ in 0..n {
                    sink.row(vec![Value::Int(1)]);
                }
                n
            }
        };
        let affected = FakeDb::first(&db.affected, query).unwrap_or(1);
        Ok(Outcome { affected: Some(affected), rows: returned, stopped: false })
    }

    fn tables(&mut self, _path: &[String]) -> Result<Vec<TableInfo>, DbError> {
        let db = self.db.lock().unwrap();
        Ok(db
            .tables
            .iter()
            .map(|t| TableInfo {
                table: t.structure.table.clone(),
                kind: TableKind::Table,
                rows: Some(t.rows.len() as u64),
            })
            .collect())
    }

    fn structure(&mut self, table: &TableRef) -> Result<Structure, DbError> {
        Ok(self.db.lock().unwrap().table(table)?.structure.clone())
    }

    fn read(
        &mut self,
        table: &TableRef,
        page: &Page,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
    ) -> Result<Outcome, DbError> {
        let on_read = {
            let mut db = self.db.lock().unwrap();
            db.reads.push((table.name.clone(), page.offset, page.limit));
            db.pages.push(page.clone());
            db.on_read.clone()
        };
        if let Some(on_read) = on_read {
            on_read();
        }
        if cancel.is_cancelled() {
            return Err(DbError::Cancelled);
        }
        let mut rows = self.rows(table, page.filter.as_ref())?;
        let columns = self.db.lock().unwrap().table(table)?.structure.columns.clone();
        for order in page.order.iter().rev() {
            let at = columns.iter().position(|c| c.name == order.column).unwrap_or(0);
            rows.sort_by(|(_, a), (_, b)| {
                let o = match (&a[at], &b[at]) {
                    (Value::Int(x), Value::Int(y)) => x.cmp(y),
                    (x, y) => x.display().cmp(&y.display()),
                };
                if order.descending { o.reverse() } else { o }
            });
        }
        sink.columns(
            &columns
                .iter()
                .map(|c| Column { name: c.name.clone(), type_name: c.declared_type.clone() })
                .collect::<Vec<_>>(),
        );
        let mut read = 0;
        for (id, row) in rows.into_iter().skip(page.offset as usize).take(page.limit as usize) {
            sink.row_key(RowKey::RowId(id));
            read += 1;
            if sink.row(row) == database::Flow::Stop {
                break;
            }
        }
        Ok(Outcome { affected: None, rows: read, stopped: false })
    }

    fn count(
        &mut self,
        table: &TableRef,
        filter: Option<&Filter>,
        _cancel: &Cancel,
    ) -> Result<u64, DbError> {
        self.counting();
        Ok(self.rows(table, filter)?.len() as u64)
    }

    fn apply(&mut self, changes: &ChangeSet) -> Result<Applied, DbError> {
        let mut db = self.db.lock().unwrap();
        db.applied.push(changes.clone());
        Ok(Applied { affected: changes.changes.len() as u64, inserted: Vec::new() })
    }

    fn alter(&mut self, changes: &[SchemaChange]) -> Result<Applied, DbError> {
        Ok(Applied { affected: changes.len() as u64, inserted: Vec::new() })
    }

    fn rebuild_source(&mut self, table: &TableRef) -> Result<RebuildSource, DbError> {
        let db = self.db.lock().unwrap();
        Ok(RebuildSource {
            table: SqliteTable {
                structure: db.table(table)?.structure.clone(),
                ..SqliteTable::default()
            },
            foreign_keys: db.foreign_keys,
        })
    }
}

/// The fake driver: its sessions open on `db`, counted in `live`, every opening counted in
/// `opened`.
#[derive(Clone)]
pub struct FakeDriver {
    pub db: Arc<Mutex<FakeDb>>,
    pub live: Arc<AtomicUsize>,
    pub opened: Arc<AtomicUsize>,
    /// SQL (SQLite's dialect) or not (it applies changes itself, like a CSV file).
    pub sql: bool,
}

impl FakeDriver {
    pub fn new(db: FakeDb, sql: bool) -> Self {
        Self { db: Arc::new(Mutex::new(db)), live: Arc::default(), opened: Arc::default(), sql }
    }

    pub fn live(&self) -> usize {
        self.live.load(Ordering::SeqCst)
    }

    pub fn opened(&self) -> usize {
        self.opened.load(Ordering::SeqCst)
    }

    pub fn source(&self, id: DataSourceId) -> Source {
        let driver = self.clone();
        Source {
            id,
            connect: Arc::new(move || {
                database::Driver::connect(&driver, &Target::default(), &Secret::default())
            }),
            dialect: database::Driver::dialect(self),
            capabilities: database::Driver::capabilities(self),
            read_only: false,
        }
    }
}

impl database::Driver for FakeDriver {
    fn kind(&self) -> Kind {
        Kind::Relational
    }

    fn language(&self) -> Language {
        Language::Sql
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            transactions: true,
            edit_rows: true,
            schema_changes: SchemaChanges::ALL,
            file_based: true,
            cancel: true,
            ..Capabilities::default()
        }
    }

    fn dialect(&self) -> Option<Arc<dyn Dialect>> {
        self.sql.then(|| Arc::new(Sqlite) as Arc<dyn Dialect>)
    }

    fn connect(&self, target: &Target, _secret: &Secret) -> Result<Box<dyn Session>, DbError> {
        self.db.lock().unwrap().targets.push(target.clone());
        if let Some(e) = self.db.lock().unwrap().refuse.clone() {
            return Err(e);
        }
        self.live.fetch_add(1, Ordering::SeqCst);
        self.opened.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(FakeSession { db: Arc::clone(&self.db), live: Arc::clone(&self.live) }))
    }
}

/// A wake that counts its calls.
pub fn counted_wake() -> (Arc<dyn Fn() + Send + Sync>, Arc<AtomicUsize>) {
    let count = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&count);
    (
        Arc::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
        }),
        count,
    )
}
