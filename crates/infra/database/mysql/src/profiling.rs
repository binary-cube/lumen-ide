//! A statement profiled: run (or only explained) with the server's counters read before
//! and after, its stamp, a write checked to be undoable first.

use std::time::Instant;

use database::profile::{Profile, ProfileDepth, ProfileOptions};
use database::{Cancel, DbError, Flow, RowSink, TableRef, Value};

use crate::{ID, MARIADB_ID, MySession, OnStop, console, profile, stats};

/// A sink that keeps nothing (a profiled write's rows, if any).
struct Discard;

impl RowSink for Discard {
    fn columns(&mut self, _columns: &[database::Column]) {}

    fn row(&mut self, _values: Vec<Value>) -> Flow {
        Flow::Continue
    }
}

impl MySession {
    /// See [`profile`]: the statement explained, or run and measured — in a transaction of
    /// its own (read-only for a read: a function that writes, called from a `SELECT`,
    /// fails instead) or under a savepoint inside the user's, rolled back at the end:
    /// nothing it did stays. What can't be rolled back isn't run (a schema change commits
    /// by itself; a table of an engine without transactions keeps its changes).
    pub(crate) fn run_profile(
        &mut self,
        statement: &str,
        options: &ProfileOptions,
        cancel: &Cancel,
    ) -> Result<Profile, DbError> {
        let read = console::statement(statement);
        let v = self.version;
        let runs = options.depth.runs();
        let writes = read.kind.writes();
        if runs {
            if !profile::runs(v) {
                return Err(DbError::Unsupported(format!(
                    "profiling as it runs on {} {} (MySQL 8.0.18 or MariaDB 10.1 or newer)",
                    v.engine(),
                    v.text()
                )));
            }
            if let Some(why) = profile::refused(read.kind, read.body) {
                return Err(DbError::Unsupported(format!(
                    "profiling {why} as it runs: it can't be undone"
                )));
            }
            if writes {
                self.check_undoable(read.body)?;
            }
        }
        let deadline = options.timeout.map(|t| Instant::now() + t);
        let shift =
            |e: DbError, prefix: &str| console::shifted(e, prefix.chars().count(), read.leading);
        if !runs {
            let prefix = "EXPLAIN FORMAT=JSON ";
            let json = self
                .single_text(&format!("{prefix}{}", read.body), cancel, deadline)
                .map_err(|e| shift(e, prefix))?;
            let mut p = profile::parse_json(&json, options.depth)?;
            self.stamp(&mut p, options, false, writes);
            p.keep_raw(database::profile::RawFormat::Json, &json, options.max_raw_bytes);
            return Ok(p);
        }
        let user_tx = self.in_tx;
        if user_tx {
            self.simple(&format!("SAVEPOINT {}", profile::SAVEPOINT))?;
        } else {
            let locks = read.locks;
            self.simple(if writes || locks {
                "START TRANSACTION"
            } else {
                "START TRANSACTION READ ONLY"
            })?;
        }
        let measured = self.measure(&read, options, cancel, deadline);
        // Undone whatever happened (a failure leaves nothing more to undo).
        if user_tx {
            let point = profile::SAVEPOINT;
            let _ = self.simple(&format!("ROLLBACK TO SAVEPOINT {point}"));
            let _ = self.simple(&format!("RELEASE SAVEPOINT {point}"));
        } else {
            let _ = self.simple("ROLLBACK");
        }
        let (mut p, raw, format) = measured?;
        self.stamp(&mut p, options, true, writes);
        p.keep_raw(format, &raw, options.max_raw_bytes);
        Ok(p)
    }

    /// The statement run inside the profile's transaction, measured: its plan as it ran
    /// (MariaDB's `ANALYZE FORMAT=JSON`, MySQL's `EXPLAIN ANALYZE`; for what MySQL can't
    /// run as a tree, its plan and the statement run by itself), the session's counters
    /// around it, its time.
    fn measure(
        &mut self,
        read: &console::Statement<'_>,
        options: &ProfileOptions,
        cancel: &Cancel,
        deadline: Option<Instant>,
    ) -> Result<(Profile, String, database::profile::RawFormat), DbError> {
        use database::profile::RawFormat;
        let shift =
            |e: DbError, prefix: &str| console::shifted(e, prefix.chars().count(), read.leading);
        let before = self.status();
        let started = Instant::now();
        let (mut p, raw, format) = if self.version.mariadb {
            let prefix = "ANALYZE FORMAT=JSON ";
            let json = self
                .single_text(&format!("{prefix}{}", read.body), cancel, deadline)
                .map_err(|e| shift(e, prefix))?;
            (profile::parse_json(&json, options.depth)?, json, RawFormat::Json)
        } else {
            let prefix = "EXPLAIN ANALYZE ";
            let tree = self
                .single_text(&format!("{prefix}{}", read.body), cancel, deadline)
                .map_err(|e| shift(e, prefix))?;
            if tree.trim().trim_start_matches("->").trim() == profile::NOT_A_TREE {
                // Its plan, then the statement itself, timed.
                let prefix = "EXPLAIN FORMAT=JSON ";
                let json = self
                    .single_text(&format!("{prefix}{}", read.body), cancel, deadline)
                    .map_err(|e| shift(e, prefix))?;
                let mut p = profile::parse_json(&json, options.depth)?;
                let run = Instant::now();
                let outcome = self
                    .stream(read.body, &mut Discard, cancel, deadline, OnStop::Drain)
                    .map_err(|e| shift(e, ""))?;
                p.statement.execution_ms = Some(run.elapsed().as_secs_f64() * 1000.0);
                p.statement.rows_affected = outcome.affected;
                (p, json, RawFormat::Json)
            } else {
                (profile::parse_tree(&tree, options.depth)?, tree, RawFormat::Text)
            }
        };
        p.statement.wall_ms = Some(started.elapsed().as_secs_f64() * 1000.0);
        let after = self.status();
        if !before.is_empty() && !after.is_empty() {
            let (counters, extra) = profile::counters(&before, &after);
            p.statement.counters = Some(counters);
            p.statement.extra.extend(extra);
        }
        // What `performance_schema` kept of it, when the user may read it.
        let prefix = if self.version.mariadb { "ANALYZE" } else { "EXPLAIN ANALYZE" };
        match self.catalog(&profile::history_sql(prefix)) {
            Ok(rows) => {
                if let Some(row) = rows.first() {
                    let (examined, extra) = profile::history_row(row);
                    if let (Some(examined), Some(counters)) =
                        (examined, p.statement.counters.as_mut())
                    {
                        counters.rows_examined = Some(examined);
                    }
                    p.statement.extra.extend(extra);
                }
            }
            Err(e) => tracing::debug!(error = %e, "performance_schema's history wasn't read"),
        }
        Ok((p, raw, format))
    }

    /// The profile's engine, what it holds and its notes.
    fn stamp(&self, p: &mut Profile, options: &ProfileOptions, runs: bool, writes: bool) {
        use database::profile::{Engine, ProfileNote};
        let v = self.version;
        p.engine = Engine {
            driver: if v.mariadb { MARIADB_ID } else { ID }.to_owned(),
            name: v.engine().to_owned(),
            version: Some(v.text()),
            version_num: Some(v.number),
        };
        p.depth = options.depth;
        p.collected = database::ProfileCaps {
            estimated: true,
            actual: runs,
            node_timing: runs && p.nodes.iter().any(|n| n.actuals.total_ms.is_some()),
            engine_counters: p.statement.counters.is_some(),
            undo_writes: runs && writes,
            ..database::ProfileCaps::NONE
        };
        if runs && writes {
            p.note(ProfileNote::RolledBack);
        }
        if options.depth == (ProfileDepth::Actual { timing: true }) {
            p.note(ProfileNote::InstrumentationOverhead);
        }
        if runs && !writes {
            p.statement.rows_returned =
                p.roots.first().and_then(|r| p.node(*r).total_rows()).map(|r| r.round() as u64);
        }
    }

    /// The first column of every row `sql` gives, as one text (an `EXPLAIN`'s answer).
    fn single_text(
        &mut self,
        sql: &str,
        cancel: &Cancel,
        deadline: Option<Instant>,
    ) -> Result<String, DbError> {
        let mut rows = database::Collect::default();
        self.stream(sql, &mut rows, cancel, deadline, OnStop::Drain)?;
        Ok(rows
            .rows
            .iter()
            .filter_map(|r| r.first())
            .map(Value::display)
            .collect::<Vec<_>>()
            .join("\n"))
    }

    /// The session's counters ([`profile::COUNTERS`]); none if they can't be read.
    fn status(&mut self) -> Vec<(String, u64)> {
        match self.catalog(&profile::status_sql()) {
            Ok(rows) => profile::status_values(&rows),
            Err(e) => {
                tracing::debug!(error = %e, "the session's counters weren't read");
                Vec::new()
            }
        }
    }

    /// Refused when a table `statement` writes is of an engine without transactions
    /// (MyISAM, MEMORY…): its changes couldn't be undone.
    fn check_undoable(&mut self, statement: &str) -> Result<(), DbError> {
        use database::sql::{self, Flavor, SplitOptions};
        let parsed = sql::parse(statement, Flavor::MySql, &SplitOptions::default());
        let Some(first) = parsed.statements.first() else { return Ok(()) };
        let tables: Vec<TableRef> =
            sql::written_tables(statement, parsed.tokens_of(first), Flavor::MySql)
                .into_iter()
                .map(|(t, _)| t)
                .collect();
        if tables.is_empty() {
            return Ok(());
        }
        let query = format!(
            "SELECT t.TABLE_NAME, t.ENGINE, e.TRANSACTIONS FROM information_schema.TABLES t \
             LEFT JOIN information_schema.ENGINES e ON e.ENGINE = t.ENGINE WHERE {}",
            stats::wanted("t.TABLE_SCHEMA", "t.TABLE_NAME", &tables)
        );
        for r in self.catalog(&query)? {
            let text = |i: usize| r.get(i).cloned().flatten().unwrap_or_default();
            if !text(1).is_empty() && text(2) != "YES" {
                return Err(DbError::Unsupported(format!(
                    "profiling a write to `{}` as it runs: its engine ({}) can't undo it",
                    text(0),
                    text(1)
                )));
            }
        }
        Ok(())
    }
}
