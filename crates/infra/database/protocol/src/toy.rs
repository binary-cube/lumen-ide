//! A small in-memory driver for the tests (feature `toy`): it writes no SQL, so the editor
//! sends it the changes as they are, and it plans its own statements for column changes
//! (shown, then run by the editor through `execute`). Tables: `users` (a key, three
//! rows), `slow` (reading it waits until it's cancelled), `crash` (reading it ends the
//! driver, as a crash would). It wants the password [`PASSWORD`], and — as a careless
//! driver might — prints what it's given on its error output.
//!
//! Its console (protocol 2) runs a few statements of its own: `SELECT * | a, b FROM t`
//! (an unknown table or column refused with its place and SQLSTATE), `SERIES n` (`n` rows
//! made as they're read, cancelled between any two), `WAIT` (until cancelled), `NOTICE
//! text`, `BEGIN` / `COMMIT` / `ROLLBACK`; with cursors, a plan, one schema (`main`). Its
//! profiles (protocol 3) are a scan with times, I/O, an extra of each type and a key the
//! editor doesn't know; its tables' statistics, their rows.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use database::sql::EnginePosition;
use database::transaction::{Script, Step};
use database::{
    Applied, Cancel, Capabilities, Change, ChangeSet, Column, ColumnInfo, ConsoleCaps, CursorToken,
    DbError, Driver, Expect, ExplainKinds, Filter, Flow, Isolation, Isolations, Kind, Language,
    Node, Notice, Outcome, Page, Place, PlanNode, Routine, RowKey, RowSink, RunOptions, RunOutcome,
    SchemaChange, SchemaChanges, SchemaSwitch, Secret, Session, Sorting, SqlError, Structure,
    TableInfo, TableKind, TableRef, Target, TxOptions, Value, eval,
};

use crate::dto;

/// The password it wants.
pub const PASSWORD: &str = "toy-secret";

/// What it says of itself (its manifest's `[driver]`).
pub fn description() -> dto::Description {
    dto::Description {
        id: "toy".to_owned(),
        name: "Toy".to_owned(),
        version: "1.0".to_owned(),
        kind: dto::Kind::Relational,
        default_port: Some(7),
        sql: dto::Sql::None,
        capabilities: capabilities().into(),
        value_editors: vec![dto::ValueEditor {
            id: "toy-mood".to_owned(),
            classes: Vec::new(),
            types: vec!["mood".to_owned()],
            format: dto::ValueFormat::Choice(vec!["happy".to_owned(), "sad".to_owned()]),
            large: false,
            priority: 5,
        }],
        ..dto::Description::default()
    }
}

fn capabilities() -> Capabilities {
    Capabilities {
        transactions: true,
        edit_rows: true,
        schema_changes: SchemaChanges {
            tables: true,
            columns: true,
            renames: true,
            ..SchemaChanges::default()
        },
        cancel: true,
        sorting: Sorting::Any,
        console: Some(CONSOLE),
        ..Capabilities::default()
    }
}

/// What its console does.
pub const CONSOLE: ConsoleCaps = ConsoleCaps {
    cursors: true,
    explain: ExplainKinds { plan: true, analyze: true },
    isolation: Isolations::of(&[Isolation::Serializable]),
    read_only: false,
    switch_schema: SchemaSwitch::Statement,
    databases: true,
    error_positions: true,
    notices: true,
    limited: false,
    profile: database::ProfileCaps {
        estimated: true,
        actual: true,
        node_timing: true,
        buffers: true,
        table_stats: true,
        undo_writes: true,
        ..database::ProfileCaps::NONE
    },
};

/// Where a statement's rows come from, read as they're asked for.
#[derive(Debug)]
enum Source {
    Rows(VecDeque<Vec<Value>>),
    /// `SERIES`: `next` up to `end`, made one at a time.
    Series {
        next: i64,
        end: i64,
    },
    /// `WAIT`: none, until cancelled.
    Wait,
}

impl Source {
    fn next(&mut self, cancel: &Cancel) -> Result<Option<Vec<Value>>, DbError> {
        match self {
            Source::Rows(rows) => Ok(rows.pop_front()),
            Source::Series { next, end } => {
                if cancel.is_cancelled() {
                    return Err(DbError::Cancelled);
                }
                if next > end {
                    return Ok(None);
                }
                let n = *next;
                *next += 1;
                Ok(Some(vec![Value::Int(n), Value::Text(format!("n{n}"))]))
            }
            Source::Wait => {
                let started = Instant::now();
                while !cancel.is_cancelled() && started.elapsed() < Duration::from_secs(30) {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(DbError::Cancelled)
            }
        }
    }
}

/// A result kept for `fetch`: its columns, the row read past the last page, the rest.
#[derive(Debug)]
struct Cursor {
    columns: Vec<Column>,
    peeked: Option<Vec<Value>>,
    source: Source,
}

/// What one of its statements gives.
enum Answer {
    Rows(Vec<Column>, Source),
    Notice(String),
}

/// A refusal of `sql` at `token` (its first place), as an engine would say it.
fn refused(sql: &str, token: &str, code: &str, message: String) -> DbError {
    let at = sql.find(token).map_or(1, |byte| sql[..byte].chars().count() + 1);
    DbError::sql(SqlError {
        sqlstate: Some(code.to_owned()),
        position: Some(EnginePosition::Character(at)),
        hint: (code == "42601").then(|| "the toy knows SELECT, SERIES, WAIT and NOTICE".to_owned()),
        ..SqlError::new(message)
    })
}

#[derive(Debug, Clone)]
struct Table {
    name: String,
    /// Names and types; the first is the key (`id`).
    columns: Vec<(String, String)>,
    rows: Vec<Vec<Value>>,
    next: i64,
}

#[derive(Debug, Clone)]
struct Db {
    tables: Vec<Table>,
}

impl Db {
    fn new() -> Self {
        let users = Table {
            name: "users".to_owned(),
            columns: vec![
                ("id".to_owned(), "INTEGER".to_owned()),
                ("name".to_owned(), "TEXT".to_owned()),
                ("score".to_owned(), "INTEGER".to_owned()),
            ],
            rows: [("ana", 10), ("bob", 20), ("cid", 30)]
                .iter()
                .enumerate()
                .map(|(i, (name, score))| {
                    vec![
                        Value::Int(i as i64 + 1),
                        Value::Text((*name).to_owned()),
                        Value::Int(*score),
                    ]
                })
                .collect(),
            next: 4,
        };
        let one = |name: &str| Table {
            name: name.to_owned(),
            columns: vec![("id".to_owned(), "INTEGER".to_owned())],
            rows: vec![vec![Value::Int(1)]],
            next: 2,
        };
        Self { tables: vec![users, one("slow"), one("crash")] }
    }

    fn table(&self, name: &str) -> Result<&Table, DbError> {
        self.tables
            .iter()
            .find(|t| t.name == name)
            .ok_or_else(|| DbError::Query(format!("no table {name}")))
    }

    fn table_mut(&mut self, name: &str) -> Result<&mut Table, DbError> {
        self.tables
            .iter_mut()
            .find(|t| t.name == name)
            .ok_or_else(|| DbError::Query(format!("no table {name}")))
    }
}

impl Table {
    fn at(&self, column: &str) -> Option<usize> {
        self.columns.iter().position(|(c, _)| c == column)
    }

    fn row_of(&self, key: &RowKey) -> Option<usize> {
        let RowKey::Primary(values) = key else { return None };
        let (_, id) = values.first()?;
        self.rows.iter().position(|r| r.first() == Some(id))
    }

    fn structure(&self) -> Structure {
        let columns = self
            .columns
            .iter()
            .enumerate()
            .map(|(i, (name, ty))| {
                let mut column = ColumnInfo::new(name, ty);
                if i == 0 {
                    column.in_key = true;
                    column.key_position = Some(1);
                    column.nullable = false;
                    column.auto_increment = true;
                }
                column
            })
            .collect();
        Structure { table: TableRef::new(&self.name), columns, ..Structure::default() }
    }

    fn kept(&self, filter: Option<&Filter>) -> Vec<&Vec<Value>> {
        self.rows
            .iter()
            .filter(|row| {
                filter.is_none_or(|f| {
                    eval::matches(f, &|c| self.at(c).and_then(|at| row.get(at).cloned()))
                })
            })
            .collect()
    }

    fn change(&mut self, change: &SchemaChange) -> Result<(), DbError> {
        match change {
            SchemaChange::AddColumn { column, .. } => {
                self.columns.push((column.name.clone(), column.declared_type.clone()));
                for row in &mut self.rows {
                    row.push(Value::Null);
                }
            }
            SchemaChange::DropColumn { column, .. } => {
                let at = self.at(column).ok_or_else(|| DbError::Query(format!("no {column}")))?;
                self.columns.remove(at);
                for row in &mut self.rows {
                    row.remove(at);
                }
            }
            SchemaChange::RenameColumn { from, to, .. } => {
                let at = self.at(from).ok_or_else(|| DbError::Query(format!("no {from}")))?;
                self.columns[at].0.clone_from(to);
            }
            other => return Err(DbError::Unsupported(format!("{other:?}"))),
        }
        Ok(())
    }
}

/// The driver: one database in memory, shared by its sessions.
#[derive(Debug, Clone)]
pub struct Toy {
    db: Arc<Mutex<Db>>,
}

impl Default for Toy {
    fn default() -> Self {
        Self { db: Arc::new(Mutex::new(Db::new())) }
    }
}

impl Driver for Toy {
    fn kind(&self) -> Kind {
        Kind::Relational
    }

    fn language(&self) -> Language {
        Language::Sql
    }

    fn capabilities(&self) -> Capabilities {
        capabilities()
    }

    fn connect(&self, target: &Target, secret: &Secret) -> Result<Box<dyn Session>, DbError> {
        // A careless driver: what it's given, on its error output.
        eprintln!("toy: connecting as {} with {}", target.user, secret.expose());
        if secret.expose() != PASSWORD {
            return Err(DbError::Auth("wrong password".to_owned()));
        }
        Ok(Box::new(ToySession {
            db: Arc::clone(&self.db),
            before: None,
            cursors: HashMap::new(),
            next_cursor: 1,
            schema: "main".to_owned(),
        }))
    }
}

struct ToySession {
    db: Arc<Mutex<Db>>,
    /// The database as it was when a transaction began.
    before: Option<Db>,
    cursors: HashMap<String, Cursor>,
    next_cursor: u64,
    schema: String,
}

impl ToySession {
    fn db(&self) -> std::sync::MutexGuard<'_, Db> {
        self.db.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// One of its console's statements (`None`: not one of those).
    fn answer(&self, sql: &str) -> Option<Result<Answer, DbError>> {
        let words: Vec<&str> = sql.split_whitespace().collect();
        let upper = |i: usize| words.get(i).map(|w| w.to_ascii_uppercase());
        match upper(0).as_deref()? {
            "SERIES" => {
                let n = words.get(1).and_then(|n| n.parse::<i64>().ok());
                Some(match n {
                    Some(end) => Ok(Answer::Rows(
                        vec![
                            Column { name: "n".into(), type_name: "INTEGER".into() },
                            Column { name: "label".into(), type_name: "TEXT".into() },
                        ],
                        Source::Series { next: 1, end },
                    )),
                    None => Err(refused(sql, "SERIES", "42601", "SERIES needs a count".into())),
                })
            }
            "WAIT" => Some(Ok(Answer::Rows(Vec::new(), Source::Wait))),
            "NOTICE" => Some(Ok(Answer::Notice(words[1..].join(" ")))),
            "SELECT" => Some(self.select(sql)),
            _ => None,
        }
    }

    /// `SELECT * | a, b FROM t`.
    fn select(&self, sql: &str) -> Result<Answer, DbError> {
        let upper = sql.to_ascii_uppercase();
        let Some(from) = upper.find(" FROM ") else {
            return Err(refused(sql, "SELECT", "42601", "a SELECT without FROM".into()));
        };
        let list = sql["SELECT".len()..from].trim();
        let name = sql[from + 6..].trim().trim_end_matches(';').trim();
        let db = self.db();
        let table = db.table(name).map_err(|_| {
            refused(sql, name, "42P01", format!("relation \"{name}\" does not exist"))
        })?;
        let wanted: Vec<usize> = if list == "*" {
            (0..table.columns.len()).collect()
        } else {
            list.split(',')
                .map(|c| {
                    let c = c.trim();
                    table.at(c).ok_or_else(|| {
                        refused(sql, c, "42703", format!("column \"{c}\" does not exist"))
                    })
                })
                .collect::<Result<_, _>>()?
        };
        let columns = wanted
            .iter()
            .map(|&i| Column {
                name: table.columns[i].0.clone(),
                type_name: table.columns[i].1.clone(),
            })
            .collect();
        let rows =
            table.rows.iter().map(|r| wanted.iter().map(|&i| r[i].clone()).collect()).collect();
        Ok(Answer::Rows(columns, Source::Rows(rows)))
    }

    /// At most `limit` of `cursor`'s rows to `sink`, one more read to know: what's left.
    fn page(
        cursor: &mut Cursor,
        limit: u32,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
    ) -> Result<(u64, bool, bool), DbError> {
        sink.columns(&cursor.columns);
        let mut sent = 0u64;
        while sent < u64::from(limit) {
            let row = match cursor.peeked.take() {
                Some(row) => Some(row),
                None => cursor.source.next(cancel)?,
            };
            let Some(row) = row else { return Ok((sent, false, false)) };
            sent += 1;
            if sink.row(row) == Flow::Stop {
                return Ok((sent, false, true));
            }
        }
        cursor.peeked = cursor.source.next(cancel)?;
        Ok((sent, cursor.peeked.is_some(), false))
    }
}

/// Its own statements for a column change (`None`: not one it plans).
fn statement(change: &SchemaChange) -> Option<String> {
    Some(match change {
        SchemaChange::AddColumn { table, column } => {
            format!("ADD COLUMN {} {} {}", table.name, column.name, column.declared_type)
        }
        SchemaChange::DropColumn { table, column } => {
            format!("DROP COLUMN {} {column}", table.name)
        }
        SchemaChange::RenameColumn { table, from, to } => {
            format!("RENAME COLUMN {} {from} {to}", table.name)
        }
        _ => return None,
    })
}

impl Session for ToySession {
    fn server(&self) -> String {
        "Toy 1.0".to_owned()
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
        match self.answer(query) {
            Some(Ok(Answer::Rows(columns, mut source))) => {
                sink.columns(&columns);
                let mut outcome = Outcome::default();
                while let Some(row) = source.next(_cancel)? {
                    outcome.rows += 1;
                    if sink.row(row) == Flow::Stop {
                        outcome.stopped = true;
                        break;
                    }
                }
                return Ok(outcome);
            }
            Some(Ok(Answer::Notice(_))) => {
                sink.columns(&[]);
                return Ok(Outcome { affected: Some(0), rows: 0, stopped: false });
            }
            Some(Err(e)) => return Err(e),
            None => {}
        }
        sink.columns(&[]);
        let words: Vec<&str> = query.split_whitespace().collect();
        let mut db = self.db.lock().unwrap_or_else(PoisonError::into_inner);
        let table = |name: &str| TableRef::new(name);
        match words.as_slice() {
            ["BEGIN"] => self.before = Some(db.clone()),
            ["COMMIT"] => self.before = None,
            ["ROLLBACK"] => {
                if let Some(before) = self.before.take() {
                    *db = before;
                }
            }
            ["ADD", "COLUMN", t, name, ty] => {
                db.table_mut(t)?.change(&SchemaChange::AddColumn {
                    table: table(t),
                    column: database::ColumnDef::new(*name, *ty),
                })?
            }
            ["DROP", "COLUMN", t, column] => {
                db.table_mut(t)?.change(&SchemaChange::DropColumn {
                    table: table(t),
                    column: (*column).to_owned(),
                })?
            }
            ["RENAME", "COLUMN", t, from, to] => {
                db.table_mut(t)?.change(&SchemaChange::RenameColumn {
                    table: table(t),
                    from: (*from).to_owned(),
                    to: (*to).to_owned(),
                })?
            }
            _ => {
                let first = words.first().copied().unwrap_or_default();
                return Err(refused(query, first, "42601", format!("toy doesn't know `{query}`")));
            }
        }
        Ok(Outcome { affected: Some(0), rows: 0, stopped: false })
    }

    fn console(&self) -> Option<ConsoleCaps> {
        Some(CONSOLE)
    }

    fn run(
        &mut self,
        statement: &str,
        options: &RunOptions,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
    ) -> Result<RunOutcome, DbError> {
        let (columns, source) = match self.answer(statement) {
            Some(Ok(Answer::Rows(columns, source))) => (columns, source),
            Some(Ok(Answer::Notice(text))) => {
                sink.columns(&[]);
                let notice =
                    Notice { severity: "NOTICE".into(), message: text, ..Notice::default() };
                return Ok(RunOutcome {
                    affected: Some(0),
                    notices: vec![notice],
                    ..RunOutcome::default()
                });
            }
            Some(Err(e)) => return Err(e),
            None => {
                let outcome = self.execute(statement, sink, cancel)?;
                return Ok(RunOutcome { affected: outcome.affected, ..RunOutcome::default() });
            }
        };
        let mut cursor = Cursor { columns, peeked: None, source };
        let (rows, left, stopped) = Self::page(&mut cursor, options.fetch_limit, sink, cancel)?;
        let more = (left && options.keep_cursor).then(|| {
            let token = format!("toy-cursor-{}", self.next_cursor);
            self.next_cursor += 1;
            self.cursors.insert(token.clone(), cursor);
            CursorToken(token)
        });
        Ok(RunOutcome { rows, truncated: left, more, stopped, ..RunOutcome::default() })
    }

    fn fetch(
        &mut self,
        token: &CursorToken,
        rows: u32,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
    ) -> Result<RunOutcome, DbError> {
        let Some(mut cursor) = self.cursors.remove(&token.0) else {
            return Err(DbError::Query(format!("no cursor {}", token.0)));
        };
        let (rows, left, stopped) = Self::page(&mut cursor, rows, sink, cancel)?;
        let more = left.then(|| {
            self.cursors.insert(token.0.clone(), cursor);
            token.clone()
        });
        Ok(RunOutcome { rows, truncated: left, more, stopped, ..RunOutcome::default() })
    }

    fn close_cursor(&mut self, token: &CursorToken) -> Result<(), DbError> {
        self.cursors.remove(&token.0);
        Ok(())
    }

    fn explain(
        &mut self,
        statement: &str,
        analyze: bool,
        _cancel: &Cancel,
    ) -> Result<Vec<PlanNode>, DbError> {
        let upper = statement.to_ascii_uppercase();
        let Some(from) = upper.find(" FROM ") else {
            return Err(refused(statement, statement.trim(), "42601", "nothing to explain".into()));
        };
        let name = statement[from + 6..].trim().trim_end_matches(';').trim();
        let rows = self.db().table(name)?.rows.len() as f64;
        Ok(vec![PlanNode {
            object: Some(name.to_owned()),
            total_cost: Some(rows),
            plan_rows: Some(rows),
            actual_rows: analyze.then_some(rows),
            ..PlanNode::new("Toy Scan")
        }])
    }

    /// A scan of the table named, with what a profile can hold (see the module's doc).
    fn profile(
        &mut self,
        statement: &str,
        options: &database::profile::ProfileOptions,
        cancel: &Cancel,
    ) -> Result<database::profile::Profile, DbError> {
        use database::profile::{
            Engine, Extra, ExtraValue, Io, ObjectKind, ObjectRef, OpKind, Profile, ProfileNode,
            Quantity, Unit,
        };
        let plan = self.explain(statement, options.depth.runs(), cancel)?;
        let rows = plan[0].plan_rows.unwrap_or(0.0);
        let engine = Engine {
            driver: "toy".to_owned(),
            name: "Toy".to_owned(),
            version: Some("1.0".to_owned()),
            version_num: Some(10_000),
        };
        let mut profile = Profile::new(engine, options.depth);
        let mut scan = ProfileNode::new(OpKind::SeqScan, "Toy Scan");
        scan.object = plan[0].object.clone().map(|t| ObjectRef::new(ObjectKind::Table, t));
        scan.estimates.total_cost = Some(rows);
        scan.estimates.rows = Some(rows);
        if options.depth.runs() {
            scan.actuals.rows = Some(rows);
            scan.actuals.loops = Some(1.0);
            scan.actuals.startup_ms = Some(0.01);
            scan.actuals.total_ms = Some(0.05);
            scan.io =
                Some(Io { shared_hit: Some(rows as u64), block_size: Some(4096), ..Io::default() });
            profile.statement.execution_ms = Some(0.06);
            profile.statement.rows_returned = Some(rows as u64);
        }
        scan.extra = vec![
            Extra::new("toy_bool", ExtraValue::Bool(true)),
            Extra::new("toy_int", ExtraValue::Int(7)),
            Extra::new("toy_float", ExtraValue::Float(0.25)),
            Extra::new("toy_text", ExtraValue::Text("cozy".to_owned())),
            Extra::new("toy_quantity", ExtraValue::Quantity(Quantity::new(1.5, Unit::Millis)))
                .labelled("Toy time"),
            Extra::new(
                "toy_list",
                ExtraValue::List(vec![ExtraValue::Int(1), ExtraValue::Text("two".to_owned())]),
            ),
            Extra::new(
                "toy_group",
                ExtraValue::Group(vec![Extra::new("inner", ExtraValue::Bool(false))]),
            )
            .in_group("Toy"),
            // A key no editor knows (a newer driver's): kept and shown as it is.
            Extra::new("toy_secret_sauce", ExtraValue::Text("ketchup".to_owned())),
        ];
        profile.push(None, scan);
        profile.statement.planning_ms = Some(0.01);
        profile.statement.extra.push(Extra::new("toy_statement", ExtraValue::Int(1)));
        profile.collected = CONSOLE.profile;
        profile.collected.actual = options.depth.runs();
        profile.collected.node_timing = options.depth.runs();
        Ok(profile)
    }

    fn table_stats(
        &mut self,
        tables: &[TableRef],
        _cancel: &Cancel,
    ) -> Result<Vec<database::profile::TableStats>, DbError> {
        let db = self.db();
        tables
            .iter()
            .map(|t| {
                let rows = db.table(&t.name)?.rows.len() as u64;
                let mut stats = database::profile::TableStats::new(t.clone());
                stats.live_rows = Some(rows);
                stats.stats_present = Some(true);
                Ok(stats)
            })
            .collect()
    }

    fn begin(&mut self, options: &TxOptions) -> Result<(), DbError> {
        if options.isolation.is_some_and(|i| !CONSOLE.isolation.contains(i)) || options.read_only {
            return Err(DbError::Unsupported("the toy is serializable, and writes".to_owned()));
        }
        let snapshot = self.db().clone();
        self.before = Some(snapshot);
        Ok(())
    }

    fn commit(&mut self) -> Result<(), DbError> {
        self.before = None;
        Ok(())
    }

    fn rollback(&mut self) -> Result<(), DbError> {
        if let Some(before) = self.before.take() {
            *self.db() = before;
        }
        Ok(())
    }

    fn in_transaction(&self) -> bool {
        self.before.is_some()
    }

    fn set_schema(&mut self, schema: &str) -> Result<(), DbError> {
        if schema != "main" {
            return Err(DbError::Query(format!("no schema {schema}")));
        }
        self.schema = schema.to_owned();
        Ok(())
    }

    fn current(&mut self) -> Result<Place, DbError> {
        Ok(Place { database: Some("toy".into()), schema: Some(self.schema.clone()) })
    }

    fn databases(&mut self) -> Result<Vec<String>, DbError> {
        Ok(vec!["toy".to_owned()])
    }

    fn routines(
        &mut self,
        _schema: Option<&str>,
        _cancel: &Cancel,
    ) -> Result<Vec<Routine>, DbError> {
        Ok(vec![Routine {
            name: "series".into(),
            arguments: "n integer".into(),
            result: Some("TABLE (n integer, label text)".into()),
            arity: Some(1),
            builtin: true,
            ..Routine::default()
        }])
    }

    fn tables(&mut self, _path: &[String]) -> Result<Vec<TableInfo>, DbError> {
        Ok(self
            .db()
            .tables
            .iter()
            .map(|t| TableInfo {
                table: TableRef::new(&t.name),
                kind: TableKind::Table,
                rows: Some(t.rows.len() as u64),
            })
            .collect())
    }

    fn structure(&mut self, table: &TableRef) -> Result<Structure, DbError> {
        Ok(self.db().table(&table.name)?.structure())
    }

    fn read(
        &mut self,
        table: &TableRef,
        page: &Page,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
    ) -> Result<Outcome, DbError> {
        match table.name.as_str() {
            "slow" => {
                let started = Instant::now();
                while !cancel.is_cancelled() && started.elapsed() < Duration::from_secs(30) {
                    std::thread::sleep(Duration::from_millis(5));
                }
                return Err(DbError::Cancelled);
            }
            "crash" => {
                eprintln!("toy: reading crash, giving up");
                panic!("the toy driver crashed, as asked");
            }
            _ => {}
        }
        let db = self.db();
        let t = db.table(&table.name)?;
        let columns: Vec<Column> = t
            .columns
            .iter()
            .map(|(name, ty)| Column { name: name.clone(), type_name: ty.clone() })
            .collect();
        sink.columns(&columns);
        let mut rows = t.kept(page.filter.as_ref());
        if !page.order.is_empty() {
            let of = |row: &Vec<Value>| -> Vec<Value> {
                page.order
                    .iter()
                    .map(|o| {
                        t.at(&o.column).and_then(|at| row.get(at).cloned()).unwrap_or(Value::Null)
                    })
                    .collect()
            };
            rows.sort_by(|a, b| eval::compare_rows(&page.order, &of(a), &of(b)));
        }
        let mut outcome = Outcome::default();
        for row in rows.into_iter().skip(page.offset as usize).take(page.limit as usize) {
            sink.row_key(RowKey::Primary(vec![("id".to_owned(), row[0].clone())]));
            outcome.rows += 1;
            if sink.row(row.clone()) == Flow::Stop {
                outcome.stopped = true;
                break;
            }
        }
        Ok(outcome)
    }

    fn count(
        &mut self,
        table: &TableRef,
        filter: Option<&Filter>,
        _cancel: &Cancel,
    ) -> Result<u64, DbError> {
        Ok(self.db().table(&table.name)?.kept(filter).len() as u64)
    }

    fn apply(&mut self, set: &ChangeSet) -> Result<Applied, DbError> {
        let mut db = self.db();
        let mut table = db.table(&set.table.name)?.clone();
        let mut applied = Applied::default();
        for change in &database::transaction::ordered(set).changes {
            match change {
                Change::Insert { values } => {
                    let id = table.next;
                    table.next += 1;
                    let mut row = vec![Value::Null; table.columns.len()];
                    row[0] = Value::Int(id);
                    for (column, value) in values {
                        if let Some(at) = table.at(column).filter(|at| *at > 0) {
                            row[at] = value.clone();
                        }
                    }
                    table.rows.push(row);
                    applied.inserted.push(RowKey::Primary(vec![("id".into(), Value::Int(id))]));
                }
                Change::Update { key, cells } => {
                    let at = table
                        .row_of(key)
                        .ok_or_else(|| DbError::Conflict("the row was deleted".to_owned()))?;
                    for cell in cells {
                        let column = table
                            .at(&cell.column)
                            .ok_or_else(|| DbError::Query(format!("no {}", cell.column)))?;
                        if table.rows[at][column] != cell.old {
                            return Err(DbError::Conflict("the row changed".to_owned()));
                        }
                        table.rows[at][column] = cell.new.clone();
                    }
                }
                Change::Delete { key } => {
                    let at = table
                        .row_of(key)
                        .ok_or_else(|| DbError::Conflict("the row was deleted".to_owned()))?;
                    table.rows.remove(at);
                }
            }
            applied.affected += 1;
        }
        // All of them, or (an error above) none.
        *db.table_mut(&set.table.name)? = table;
        Ok(applied)
    }

    fn alter(&mut self, changes: &[SchemaChange]) -> Result<Applied, DbError> {
        let mut db = self.db();
        let mut changed = db.clone();
        for change in changes {
            match change {
                SchemaChange::CreateTable(def) => changed.tables.push(Table {
                    name: def.table.name.clone(),
                    columns: def
                        .columns
                        .iter()
                        .map(|c| (c.name.clone(), c.declared_type.clone()))
                        .collect(),
                    rows: Vec::new(),
                    next: 1,
                }),
                SchemaChange::DropTable { table } => {
                    changed.table(&table.name)?;
                    changed.tables.retain(|t| t.name != table.name);
                }
                SchemaChange::RenameTable { table, to } => {
                    changed.table_mut(&table.name)?.name.clone_from(to);
                }
                other => changed.table_mut(&other.table().name)?.change(other)?,
            }
        }
        *db = changed;
        Ok(Applied { affected: changes.len() as u64, inserted: Vec::new() })
    }

    fn plan_alter(&mut self, changes: &[SchemaChange]) -> Result<Option<Script>, DbError> {
        // Column changes: its own statements; tables: made by itself (`alter`).
        let Some(statements) = changes.iter().map(statement).collect::<Option<Vec<String>>>()
        else {
            return Ok(None);
        };
        Ok(Some(Script {
            before: Vec::new(),
            begin: "BEGIN".to_owned(),
            steps: statements.into_iter().map(|sql| Step { sql, expect: Expect::Any }).collect(),
            commit: "COMMIT".to_owned(),
            rollback: "ROLLBACK".to_owned(),
            after: Vec::new(),
            stale: Vec::new(),
        }))
    }
}
