//! A session as the domain sees it: the explorer's tree, tables read a page at a time,
//! changes, the console's statements, plans and profiles, transactions, the catalog.

use std::sync::Arc;

use database::mysql::quote_backtick;
use database::profile::{Profile, ProfileDepth, ProfileOptions};
use database::run::{Limited, Overflow};
use database::sql::StatementKind;
use database::{
    Cancel, ColumnBrief, ConsoleCaps, DbError, Dialect, Flow, Node, Outcome, Page, Place, PlanNode,
    Routine, RoutineKind, RowKey, RowSink, RunOptions, RunOutcome, Session, Structure,
    TableColumns, TableInfo, TableKind, TableRef, TxOptions, Value,
};

use crate::{CURSORS, Cursor, MySession, OnStop, RoutineKey, catalog, console, stats};

impl Session for MySession {
    fn server(&self) -> String {
        self.server.clone()
    }

    fn dialect(&self) -> Option<Arc<dyn Dialect>> {
        Some(Arc::new(self.dialect()))
    }

    fn tables(&mut self, path: &[String]) -> Result<Vec<TableInfo>, DbError> {
        match path {
            [] => self.list_tables(None),
            [schema] => self.list_tables(Some(schema)),
            _ => Ok(Vec::new()),
        }
    }

    fn structure(&mut self, table: &TableRef) -> Result<Structure, DbError> {
        self.read_structure(table)
    }

    /// A page: in the asked order (`OFFSET`), or, with none, in its primary key's — the
    /// page after one read starting after its last key (`(k1, k2) > (…)`), so a far page
    /// costs no more than the first.
    fn read(
        &mut self,
        table: &TableRef,
        page: &Page,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
    ) -> Result<Outcome, DbError> {
        let structure = self.read_structure(table)?;
        let d = self.dialect();
        let key = structure.primary_key();
        let natural = !key.is_empty() && page.order.is_empty();
        let after = if natural {
            self.cursors
                .iter()
                .find(|c| c.table == *table && c.filter == page.filter && c.next == page.offset)
                .map(|c| c.last.clone())
        } else {
            None
        };
        let mut sql = format!("SELECT * FROM {}", d.table_name(table)?);
        let mut conditions = Vec::new();
        if let Some(filter) = &page.filter {
            conditions.push(format!("({})", d.filter(filter)?));
        }
        if let Some(last) = &after {
            let names: Vec<String> =
                key.iter().map(|k| d.quote_identifier(k)).collect::<Result<_, _>>()?;
            let values: Vec<String> =
                last.iter().map(|v| d.literal(v)).collect::<Result<_, _>>()?;
            conditions.push(format!("({}) > ({})", names.join(", "), values.join(", ")));
        }
        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }
        let order: Vec<String> = if natural {
            key.iter()
                .map(|k| d.quote_identifier(k).map(|c| format!("{c} ASC")))
                .collect::<Result<_, _>>()?
        } else {
            page.order
                .iter()
                .map(|o| {
                    let c = d.quote_identifier(&o.column)?;
                    Ok(if o.descending { format!("{c} DESC") } else { format!("{c} ASC") })
                })
                .collect::<Result<_, DbError>>()?
        };
        if !order.is_empty() {
            sql.push_str(" ORDER BY ");
            sql.push_str(&order.join(", "));
        }
        sql.push(' ');
        sql.push_str(&d.pagination(page.limit, if after.is_some() { 0 } else { page.offset }));
        let mut keyed = Keyed { sink, key, at: Vec::new(), last: None };
        let outcome = self.stream(&sql, &mut keyed, cancel, None, OnStop::Kill)?;
        if natural
            && let Some(last) = keyed.last
            && outcome.rows >= u64::from(page.limit)
            && !outcome.stopped
        {
            let next = page.offset + outcome.rows;
            self.cursors.retain(|c| !(c.table == *table && c.filter == page.filter));
            if self.cursors.len() >= CURSORS {
                self.cursors.remove(0);
            }
            self.cursors.push(Cursor {
                table: table.clone(),
                filter: page.filter.clone(),
                next,
                last,
            });
        }
        Ok(outcome)
    }

    /// Schemas (MySQL's databases) › tables and views › columns and indexes, from
    /// `information_schema`.
    fn children(&mut self, path: &[String]) -> Result<Vec<Node>, DbError> {
        match path {
            [] => Ok(self
                .catalog(&catalog::schemas())?
                .into_iter()
                .filter_map(|r| r.into_iter().next().flatten())
                .map(|name| Self::node("schema", &name, String::new(), path, true))
                .collect()),
            [schema] => Ok(self
                .list_tables(Some(schema))?
                .into_iter()
                .map(|t| {
                    let kind = if t.kind == TableKind::View { "view" } else { "table" };
                    Self::node(kind, &t.table.name, String::new(), path, true)
                })
                .collect()),
            [schema, table] => {
                let table = TableRef { schema: Some(schema.clone()), name: table.clone() };
                let structure = self.read_structure(&table)?;
                let mut nodes: Vec<Node> = structure
                    .columns
                    .iter()
                    .map(|c| {
                        let detail = if c.nullable {
                            format!("{}, null", c.declared_type)
                        } else {
                            c.declared_type.clone()
                        };
                        Self::node("column", &c.name, detail, path, false)
                    })
                    .collect();
                nodes.extend(structure.indexes.iter().map(|i| {
                    Self::node("index", &i.name, format!("({})", i.columns.join(", ")), path, false)
                }));
                Ok(nodes)
            }
            _ => Ok(Vec::new()),
        }
    }

    fn execute(
        &mut self,
        query: &str,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
    ) -> Result<Outcome, DbError> {
        // A write may move rows: the pages' places are read again.
        if !query.trim_start().get(..6).is_some_and(|w| w.eq_ignore_ascii_case("select")) {
            self.cursors.clear();
        }
        self.stream(query, sink, cancel, None, OnStop::Kill)
    }

    fn console(&self) -> Option<ConsoleCaps> {
        Some(console::caps(self.version))
    }

    /// Streamed: a single query stopped (killed) one row past the page, anything else read
    /// to its end, the rows past the page dropped; the warnings it left with the outcome.
    fn run(
        &mut self,
        statement: &str,
        options: &RunOptions,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
    ) -> Result<RunOutcome, DbError> {
        let read = console::statement(statement);
        let query = read.kind == StatementKind::Query && !read.several;
        if !query {
            self.cursors.clear();
        }
        let overflow = if query { Overflow::Stop } else { Overflow::Drop };
        let on_stop = if query { OnStop::Kill } else { OnStop::Drain };
        let mut limited = Limited::new(sink, options.fetch_limit, overflow);
        let outcome = self.stream(statement, &mut limited, cancel, None, on_stop)?;
        let (rows, truncated, stopped) = (limited.delivered, limited.truncated, limited.stopped);
        Ok(RunOutcome {
            affected: outcome.affected,
            rows,
            truncated,
            more: None,
            stopped,
            notices: self.take_warnings(),
        })
    }

    /// The profile's projection: the plan, or as it ran (a write undone).
    fn explain(
        &mut self,
        statement: &str,
        analyze: bool,
        cancel: &Cancel,
    ) -> Result<Vec<PlanNode>, DbError> {
        let depth =
            if analyze { ProfileDepth::Actual { timing: true } } else { ProfileDepth::Estimated };
        Ok(self.profile(statement, &ProfileOptions::of(depth), cancel)?.plan_nodes())
    }

    fn profile(
        &mut self,
        statement: &str,
        options: &ProfileOptions,
        cancel: &Cancel,
    ) -> Result<Profile, DbError> {
        self.run_profile(statement, options, cancel)
    }

    fn table_stats(
        &mut self,
        tables: &[TableRef],
        cancel: &Cancel,
    ) -> Result<Vec<database::profile::TableStats>, DbError> {
        if tables.is_empty() {
            return Ok(Vec::new());
        }
        if cancel.is_cancelled() {
            return Err(DbError::Cancelled);
        }
        let rows = self.catalog(&stats::tables_sql(tables))?;
        let indexes = self.catalog(&stats::indexes_sql(tables))?;
        // `performance_schema`'s reads per table and index, when the user may see them.
        let io = match self.catalog(&stats::io_sql(tables)) {
            Ok(rows) => rows,
            Err(e) => {
                tracing::debug!(error = %e, "performance_schema's table I/O wasn't read");
                Vec::new()
            }
        };
        Ok(stats::assemble(tables, &rows, &indexes, &io))
    }

    fn begin(&mut self, options: &TxOptions) -> Result<(), DbError> {
        for statement in console::begin_statements(options)? {
            self.simple(&statement)?;
        }
        self.in_tx = true;
        Ok(())
    }

    fn commit(&mut self) -> Result<(), DbError> {
        self.cursors.clear();
        // Failed or not, the transaction is over.
        self.in_tx = false;
        self.simple("COMMIT")
    }

    fn rollback(&mut self) -> Result<(), DbError> {
        self.cursors.clear();
        self.in_tx = false;
        self.simple("ROLLBACK")
    }

    fn in_transaction(&self) -> bool {
        self.in_tx
    }

    /// `USE schema`: MySQL's databases are its schemas.
    fn set_schema(&mut self, schema: &str) -> Result<(), DbError> {
        self.simple(&format!("USE {}", quote_backtick(schema)?))
    }

    /// The session's database, as its schema (MySQL has no level above).
    fn current(&mut self) -> Result<Place, DbError> {
        let rows = self.catalog("SELECT DATABASE()")?;
        let schema = rows.into_iter().next().and_then(|r| r.into_iter().next().flatten());
        Ok(Place { database: None, schema })
    }

    /// Its databases (schemas), those the user has any privilege on.
    fn databases(&mut self) -> Result<Vec<String>, DbError> {
        Ok(self
            .catalog(&catalog::schemas())?
            .into_iter()
            .filter_map(|r| r.into_iter().next().flatten())
            .collect())
    }

    /// One query over `information_schema.COLUMNS` (with the primary keys' places).
    fn catalog_columns(
        &mut self,
        schema: Option<&str>,
        cancel: &Cancel,
    ) -> Result<Vec<TableColumns>, DbError> {
        if cancel.is_cancelled() {
            return Err(DbError::Cancelled);
        }
        let mariadb = self.version.mariadb;
        let mut all: Vec<TableColumns> = Vec::new();
        for r in self.catalog(&catalog::catalog_columns(schema))? {
            let text = |i: usize| r.get(i).cloned().flatten();
            let table = TableRef { schema: text(0), name: text(1).unwrap_or_default() };
            let extra = text(8).unwrap_or_default();
            let column = ColumnBrief {
                name: text(2).unwrap_or_default(),
                type_name: text(3).unwrap_or_default(),
                nullable: text(4).as_deref() == Some("YES"),
                default: catalog::default_of(
                    text(5).as_deref(),
                    &extra,
                    &text(9).unwrap_or_default(),
                    mariadb,
                ),
                comment: text(6).filter(|c| !c.is_empty()),
                key_position: text(7).and_then(|k| k.parse().ok()),
                // In its type (`enum('S','M')`): read from there.
                values: None,
            };
            match all.last_mut() {
                Some((last, columns)) if *last == table => columns.push(column),
                _ => all.push((table, vec![column])),
            }
        }
        Ok(all)
    }

    fn catalog_keys(
        &mut self,
        schema: Option<&str>,
        cancel: &Cancel,
    ) -> Result<Vec<database::TableKeys>, DbError> {
        if cancel.is_cancelled() {
            return Err(DbError::Cancelled);
        }
        // `None`: every user schema (not only the session's).
        match schema {
            Some(schema) => self.read_keys(Some(schema), None),
            None => self.read_keys(None, None),
        }
    }

    /// MariaDB's sequences: names a query may read that the tables don't list.
    fn catalog_relations(&mut self, cancel: &Cancel) -> Result<Vec<TableRef>, DbError> {
        if cancel.is_cancelled() {
            return Err(DbError::Cancelled);
        }
        if !self.version.has(database::mysql::Feature::Sequences) {
            return Ok(Vec::new());
        }
        Ok(self
            .catalog(&catalog::sequences())?
            .into_iter()
            .map(|r| {
                let text = |i: usize| r.get(i).cloned().flatten();
                TableRef { schema: text(0), name: text(1).unwrap_or_default() }
            })
            .collect())
    }

    /// The user's routines from `information_schema.ROUTINES` and `PARAMETERS`; for every
    /// schema (`None`), the server's built-in functions too.
    fn routines(&mut self, schema: Option<&str>, cancel: &Cancel) -> Result<Vec<Routine>, DbError> {
        if cancel.is_cancelled() {
            return Err(DbError::Cancelled);
        }
        let mut parameters: Vec<(RoutineKey, Vec<String>)> = Vec::new();
        for r in self.catalog(&catalog::parameters(schema))? {
            let text = |i: usize| r.get(i).cloned().flatten();
            let key = (text(0), text(1).unwrap_or_default());
            let mut argument = String::new();
            if let Some(mode) = text(4).filter(|m| m != "IN") {
                argument.push_str(&mode);
                argument.push(' ');
            }
            if let Some(name) = text(2) {
                argument.push_str(&name);
                argument.push(' ');
            }
            argument.push_str(&text(3).unwrap_or_default());
            match parameters.last_mut() {
                Some((k, list)) if *k == key => list.push(argument),
                _ => parameters.push((key, vec![argument])),
            }
        }
        let mut out: Vec<Routine> = self
            .catalog(&catalog::routines(schema))?
            .into_iter()
            .map(|r| {
                let text = |i: usize| r.get(i).cloned().flatten();
                let key = (text(0), text(1).unwrap_or_default());
                let arguments = parameters
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, list)| list.clone())
                    .unwrap_or_default();
                let procedure = text(2).as_deref() == Some("PROCEDURE");
                Routine {
                    schema: key.0,
                    name: key.1,
                    arity: u16::try_from(arguments.len()).ok(),
                    arguments: arguments.join(", "),
                    result: if procedure { None } else { text(3) },
                    kind: if procedure { RoutineKind::Procedure } else { RoutineKind::Function },
                    builtin: false,
                }
            })
            .collect();
        if schema.is_none() {
            out.extend(catalog::BUILTINS.iter().map(|(name, arity)| Routine {
                schema: None,
                name: (*name).to_owned(),
                arguments: String::new(),
                result: None,
                kind: catalog::builtin_kind(name),
                arity: *arity,
                builtin: true,
            }));
        }
        Ok(out)
    }
}

/// Gives each row its key (the primary key's columns found among the result's) before the
/// row, and keeps the last key read.
struct Keyed<'a> {
    sink: &'a mut dyn RowSink,
    key: Vec<String>,
    at: Vec<usize>,
    last: Option<Vec<Value>>,
}

impl RowSink for Keyed<'_> {
    fn columns(&mut self, columns: &[database::Column]) {
        self.at =
            self.key.iter().filter_map(|k| columns.iter().position(|c| c.name == *k)).collect();
        self.sink.columns(columns);
    }

    fn row(&mut self, values: Vec<Value>) -> Flow {
        if !self.key.is_empty() && self.at.len() == self.key.len() {
            let key: Vec<Value> = self.at.iter().filter_map(|i| values.get(*i).cloned()).collect();
            self.sink.row_key(RowKey::Primary(self.key.iter().cloned().zip(key.clone()).collect()));
            self.last = Some(key);
        }
        self.sink.row(values)
    }
}
