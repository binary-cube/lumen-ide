//! A session as the domain sees it: the explorer's tree, tables read a page at a time,
//! changes, the console's statements, plans and profiles, transactions, the catalog.

use std::sync::Arc;

use database::profile::{Engine, Profile, ProfileDepth, ProfileNote, ProfileOptions, RawFormat};
use database::{
    Cancel, Collect, ColumnBrief, ConsoleCaps, CursorToken, DbError, Dialect, Isolation, Node,
    Outcome, Page, Place, PlanNode, Routine, RoutineKind, RowSink, RunOptions, RunOutcome, Session,
    Structure, TableColumns, TableInfo, TableKind, TableRef, TxOptions, Value,
};

use crate::{CURSORS, Cursor, ID, Keyed, OnStop, PgSession, catalog, console, profile, stats};

impl Session for PgSession {
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
    /// page after one read starting after its last key (`(k1, k2) > (…)`, which every
    /// version compares), so a far page costs no more than the first.
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
        let outcome = self.execute(&sql, &mut keyed, cancel)?;
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

    /// From the catalog (`pg_namespace`, `pg_class`, `pg_attribute`), as [`Self::tables`] and
    /// [`Self::structure`] read it — never `information_schema`, which hides what the user
    /// has no privilege on (another role's schemas and tables) and materialized views.
    fn children(&mut self, path: &[String]) -> Result<Vec<Node>, DbError> {
        match path {
            [] => Ok(self
                .rows(&catalog::schemas(), &[])?
                .into_iter()
                .map(|r| Self::node("schema", &r[0], String::new(), path, true))
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
        self.release_cursor();
        // The columns' types (a single statement can be prepared; several can't: untyped).
        let types = self.types(query, cancel)?;
        // A write may move rows: the pages' places are read again.
        if !query.trim_start().get(..6).is_some_and(|w| w.eq_ignore_ascii_case("select")) {
            self.cursors.clear();
        }
        let outcome = self.stream(query, types, sink, cancel, OnStop::Cancel)?;
        self.in_tx = database::run::transaction_after(query, self.flavor(), self.in_tx);
        Ok(outcome)
    }

    fn console(&self) -> Option<ConsoleCaps> {
        Some(ConsoleCaps { profile: profile::caps(self.version), ..console::CONSOLE })
    }

    /// A query from a cursor when asked (its page fetched, one row more kept to know the
    /// rest); otherwise streamed, a write's rows past the page read to its end.
    fn run(
        &mut self,
        statement: &str,
        options: &RunOptions,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
    ) -> Result<RunOutcome, DbError> {
        self.release_cursor();
        self.take_notices();
        let read = console::statement(statement, self.version);
        let mut outcome = None;
        if options.keep_cursor && read.cursorable() {
            outcome = self.run_cursor(&read, options, sink, cancel)?;
        }
        let mut outcome = match outcome {
            Some(outcome) => outcome,
            None => self.run_plain(statement, &read, options, sink, cancel)?,
        };
        self.in_tx = database::run::transaction_after(statement, self.flavor(), self.in_tx);
        outcome.notices = self.take_notices();
        Ok(outcome)
    }

    fn fetch(
        &mut self,
        cursor: &CursorToken,
        rows: u32,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
    ) -> Result<RunOutcome, DbError> {
        if self.cursor.as_ref().is_none_or(|c| c.name != cursor.0) {
            return Err(DbError::Query("that result's cursor was closed".to_owned()));
        }
        self.take_notices();
        let mut outcome = self.fetch_page(rows, sink, cancel).map_err(|(e, _)| e)?;
        outcome.notices = self.take_notices();
        Ok(outcome)
    }

    fn close_cursor(&mut self, cursor: &CursorToken) -> Result<(), DbError> {
        if self.cursor.as_ref().is_some_and(|c| c.name == cursor.0) {
            self.release_cursor();
        }
        Ok(())
    }

    /// The profile's projection: `EXPLAIN (FORMAT JSON)`, or as it runs (`analyze`: a write
    /// undone).
    fn explain(
        &mut self,
        statement: &str,
        analyze: bool,
        cancel: &Cancel,
    ) -> Result<Vec<PlanNode>, DbError> {
        let depth =
            if analyze { ProfileDepth::Actual { timing: true } } else { ProfileDepth::Estimated };
        let options = ProfileOptions { settings: false, ..ProfileOptions::of(depth) };
        Ok(self.profile(statement, &options, cancel)?.plan_nodes())
    }

    /// `EXPLAIN (…, FORMAT JSON)` with what the version takes ([`profile::options`]). Run, it
    /// goes in a transaction of its own — read-only for a read (a function that writes,
    /// called from a `SELECT`, fails instead) — or a savepoint inside the user's, rolled back
    /// at the end with its `SET LOCAL statement_timeout`: nothing it did stays.
    fn profile(
        &mut self,
        statement: &str,
        options: &ProfileOptions,
        cancel: &Cancel,
    ) -> Result<Profile, DbError> {
        self.release_cursor();
        let read = console::statement(statement, self.version);
        let runs = options.depth.runs();
        if runs && let Some(words) = database::profile::actual_refused(read.body, self.flavor()) {
            return Err(DbError::Unsupported(format!(
                "profiling {words} as it runs: it can't run inside a transaction"
            )));
        }
        let writes = read.kind.writes();
        // The query's history (14+: by the server's `Query Identifier`, which `VERBOSE` gives).
        let history = options.statement_history && self.version >= 140_000;
        let extension = if history { self.statement_stats_schema() } else { None };
        let asked = ProfileOptions { verbose: options.verbose || extension.is_some(), ..*options };
        let prefix = profile::options(self.version, &asked, writes);
        let sql = format!("{prefix}{}", read.body);
        let mut io_timing = None;
        if runs {
            if self.in_tx {
                self.simple(&format!("SAVEPOINT {}", profile::SAVEPOINT))?;
            } else {
                self.simple(if writes || read.locks { "BEGIN" } else { "BEGIN READ ONLY" })?;
            }
            let prepared = (|| {
                if let Some(timeout) = options.timeout {
                    self.simple(&format!(
                        "SET LOCAL statement_timeout = {}",
                        timeout.as_millis().max(1)
                    ))?;
                }
                if options.buffers {
                    io_timing = self.show("track_io_timing");
                }
                Ok::<(), DbError>(())
            })();
            if let Err(e) = prepared {
                self.undo_profile();
                return Err(e);
            }
        }
        let mut rows = Collect::default();
        let result = self.stream(&sql, None, &mut rows, cancel, OnStop::Cancel);
        if runs {
            self.undo_profile();
        }
        result.map_err(|e| console::shifted(e, prefix.chars().count(), read.leading))?;
        let json: String = rows.rows.iter().filter_map(|r| r.first()).map(Value::display).collect();
        let mut p = profile::parse(&json, self.block_size)?;
        p.engine = Engine {
            driver: ID.to_owned(),
            name: "PostgreSQL".to_owned(),
            version: self.server.strip_prefix("PostgreSQL ").map(str::to_owned),
            version_num: Some(self.version),
        };
        p.depth = options.depth;
        let caps = profile::caps(self.version);
        let has = |f: &dyn Fn(&database::profile::ProfileNode) -> bool| p.nodes.iter().any(f);
        p.collected = database::ProfileCaps {
            estimated: true,
            actual: runs,
            node_timing: options.depth == (ProfileDepth::Actual { timing: true }),
            buffers: options.buffers && (runs || self.version >= 130_000),
            io_timing: has(&|n| n.io.is_some_and(|i| i.read_ms.is_some() || i.write_ms.is_some())),
            wal: options.wal && runs && writes && caps.wal,
            memory: has(&|n| n.memory.is_some()),
            spill: has(&|n| n.spill.is_some()),
            parallel: has(&|n| n.parallel.is_some()),
            settings: options.settings && caps.settings,
            verbose_output: options.verbose,
            jit: p.statement.jit.is_some(),
            triggers: runs,
            undo_writes: runs && writes,
            ..database::ProfileCaps::NONE
        };
        if runs && writes {
            p.note(ProfileNote::RolledBack);
        }
        if runs && options.buffers && io_timing.as_deref().is_some_and(|v| v == "off") {
            p.note(ProfileNote::IoTimingOff);
        }
        if options.depth == (ProfileDepth::Actual { timing: true }) {
            p.note(ProfileNote::InstrumentationOverhead);
        }
        if !p.statement.settings.is_empty() {
            p.note(ProfileNote::PlannerSettingsChanged);
        }
        if extension.is_some() && !options.verbose {
            // Asked only for the query's identifier: the columns weren't.
            for node in &mut p.nodes {
                node.output.clear();
            }
        }
        if history {
            match &extension {
                Some(schema) => {
                    p.statement.history = p
                        .statement
                        .query_id
                        .filter(|id| *id != 0)
                        .and_then(|id| self.read_history(schema, id, &mut p));
                    p.collected.statement_history = p.statement.history.is_some();
                }
                None => p.note(ProfileNote::NoStatementStats),
            }
        }
        if runs && !writes {
            p.statement.rows_returned =
                p.roots.first().and_then(|r| p.node(*r).total_rows()).map(|r| r.round() as u64);
        }
        p.keep_raw(RawFormat::Json, &json, options.max_raw_bytes);
        Ok(p)
    }

    /// One query for the tables (`pg_class`, `pg_stat_user_tables`, their sizes), one for
    /// their indexes ([`stats`]); inside the user's transaction under a savepoint of its own.
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
        self.release_cursor();
        let version = self.version;
        let params = stats::params(tables);
        let refs: Vec<&str> = params.iter().map(String::as_str).collect();
        let (rows, indexes) = self.guarded(|s| {
            let rows = s.catalog(&stats::tables_sql(version, tables.len()), &refs)?;
            if cancel.is_cancelled() {
                return Err(DbError::Cancelled);
            }
            let indexes = s.catalog(&stats::indexes_sql(version, tables.len()), &refs)?;
            Ok((rows, indexes))
        })?;
        Ok(stats::assemble(&rows, &indexes))
    }

    fn begin(&mut self, options: &TxOptions) -> Result<(), DbError> {
        if let Some(kind @ (Isolation::Deferred | Isolation::Immediate | Isolation::Exclusive)) =
            options.isolation
        {
            return Err(DbError::Unsupported(format!("{} in PostgreSQL", kind.id())));
        }
        self.release_cursor();
        self.simple(&database::run::begin_statement("BEGIN", options))?;
        self.in_tx = true;
        Ok(())
    }

    fn commit(&mut self) -> Result<(), DbError> {
        self.release_cursor();
        self.cursors.clear();
        // Failed or not, the transaction is over (a failed one rolled back).
        self.in_tx = false;
        self.simple("COMMIT")
    }

    fn rollback(&mut self) -> Result<(), DbError> {
        self.release_cursor();
        self.cursors.clear();
        self.in_tx = false;
        self.simple("ROLLBACK")
    }

    fn in_transaction(&self) -> bool {
        self.in_tx
    }

    /// `search_path`: `schema`, then `public`.
    fn set_schema(&mut self, schema: &str) -> Result<(), DbError> {
        self.release_cursor();
        let quoted = self.dialect().quote_identifier(schema)?;
        self.simple(&console::search_path(schema, &quoted))
    }

    fn current(&mut self) -> Result<Place, DbError> {
        let rows = self.catalog("SELECT current_database()::text, current_schema()::text", &[])?;
        let row = rows.into_iter().next().unwrap_or_default();
        let mut row = row.into_iter();
        Ok(Place { database: row.next().flatten(), schema: row.next().flatten() })
    }

    fn databases(&mut self) -> Result<Vec<String>, DbError> {
        Ok(self
            .catalog(catalog::databases(), &[])?
            .into_iter()
            .filter_map(|r| r.into_iter().next().flatten())
            .collect())
    }

    /// One query over `pg_attribute`, `pg_class` and `pg_namespace` (as 9.2 reads them).
    fn catalog_columns(
        &mut self,
        schema: Option<&str>,
        cancel: &Cancel,
    ) -> Result<Vec<TableColumns>, DbError> {
        if cancel.is_cancelled() {
            return Err(DbError::Cancelled);
        }
        let sql = catalog::catalog_columns(self.version, schema.is_some());
        let params: Vec<&str> = schema.into_iter().collect();
        let mut all: Vec<TableColumns> = Vec::new();
        for r in self.catalog(&sql, &params)? {
            let text = |i: usize| r.get(i).cloned().flatten();
            let table = TableRef { schema: text(0), name: text(1).unwrap_or_default() };
            let column = ColumnBrief {
                name: text(2).unwrap_or_default(),
                type_name: text(3).unwrap_or_default(),
                nullable: text(4).as_deref() == Some("true"),
                default: text(5),
                comment: text(6),
                key_position: text(7).and_then(|k| k.parse().ok()),
                values: text(8).map(|labels| catalog::enum_labels(&labels)),
            };
            match all.last_mut() {
                Some((last, columns)) if *last == table => columns.push(column),
                _ => all.push((table, vec![column])),
            }
        }
        Ok(all)
    }

    /// From `pg_constraint`, every key at once (see [`catalog::catalog_keys`]).
    fn catalog_relations(&mut self, cancel: &Cancel) -> Result<Vec<TableRef>, DbError> {
        if cancel.is_cancelled() {
            return Err(DbError::Cancelled);
        }
        Ok(self
            .catalog(&catalog::relations(self.version), &[])?
            .into_iter()
            .map(|r| {
                let text = |i: usize| r.get(i).cloned().flatten();
                TableRef { schema: text(0), name: text(1).unwrap_or_default() }
            })
            .collect())
    }

    fn catalog_keys(
        &mut self,
        schema: Option<&str>,
        cancel: &Cancel,
    ) -> Result<Vec<database::TableKeys>, DbError> {
        if cancel.is_cancelled() {
            return Err(DbError::Cancelled);
        }
        let sql = catalog::catalog_keys(schema.is_some());
        let params: Vec<&str> = schema.into_iter().collect();
        let mut all: Vec<database::TableKeys> = Vec::new();
        let mut last_key: Option<(TableRef, String)> = None;
        for r in self.catalog(&sql, &params)? {
            let text = |i: usize| r.get(i).cloned().flatten();
            let table = TableRef { schema: text(0), name: text(1).unwrap_or_default() };
            let name = text(2).unwrap_or_default();
            if all.last().is_none_or(|(t, _)| *t != table) {
                all.push((table.clone(), Vec::new()));
            }
            let Some((_, keys)) = all.last_mut() else { continue };
            let same = last_key.as_ref().is_some_and(|(t, n)| *t == table && *n == name);
            if !same {
                keys.push(database::ForeignKey {
                    name: Some(name.clone()),
                    columns: Vec::new(),
                    table: TableRef { schema: text(3), name: text(4).unwrap_or_default() },
                    references: Vec::new(),
                    on_update: catalog::action(&text(8).unwrap_or_default()),
                    on_delete: catalog::action(&text(9).unwrap_or_default()),
                });
                last_key = Some((table, name));
            }
            if let Some(key) = keys.last_mut() {
                key.columns.extend(text(5));
                key.references.extend(text(6));
            }
        }
        Ok(all)
    }

    /// From `pg_proc` (`prokind` from 11, `proisagg` / `proiswindow` before).
    fn routines(&mut self, schema: Option<&str>, cancel: &Cancel) -> Result<Vec<Routine>, DbError> {
        if cancel.is_cancelled() {
            return Err(DbError::Cancelled);
        }
        let sql = catalog::routines(self.version, schema.is_some());
        let params: Vec<&str> = schema.into_iter().collect();
        Ok(self
            .catalog(&sql, &params)?
            .into_iter()
            .map(|r| {
                let text = |i: usize| r.get(i).cloned().flatten();
                let variadic = text(6).as_deref() == Some("true");
                Routine {
                    schema: text(0),
                    name: text(1).unwrap_or_default(),
                    arguments: text(2).unwrap_or_default(),
                    result: text(3),
                    kind: match text(4).as_deref() {
                        Some("p") => RoutineKind::Procedure,
                        Some("a") => RoutineKind::Aggregate,
                        Some("w") => RoutineKind::Window,
                        _ => RoutineKind::Function,
                    },
                    arity: if variadic { None } else { text(5).and_then(|n| n.parse().ok()) },
                    builtin: text(7).as_deref() == Some("true"),
                }
            })
            .collect())
    }
}
