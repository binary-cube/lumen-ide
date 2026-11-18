//! How changes run, as pure rules: the order a set of row changes is applied in, the
//! statements of one transaction (each with what it must do: an update or a delete changes
//! exactly one row, an insert gives back its row's key, a check finds no row it didn't find
//! before), and a structure change as plain DDL or, when the engine can't alter a table, as
//! its rebuild. The same [`Script`] is what the preview shows and what [`run`] runs.

use std::collections::HashMap;

use crate::sqlite::{
    FOREIGN_KEYS_OFF, FOREIGN_KEYS_ON, RebuildSource, Rewritten, Sqlite, Stale, foreign_key_check,
    rebuild, rewritten,
};
use crate::{
    Applied, Cancel, Capabilities, Change, ChangeSet, Column, DbError, Dialect, Flow, KeyKind,
    Outcome, RowKey, RowSink, SchemaChange, Session, Structure, TableRef, Value,
};

/// What a statement must do for its transaction to go on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expect {
    /// Anything (an insert, DDL).
    Any,
    /// Change exactly one row (an update, a delete): none means the row changed or went
    /// since it was read, more means its key didn't find one row — a conflict either way.
    OneRow,
    /// Return no row (a check).
    NoRows,
    /// An insert giving back its row's key (`RETURNING`), its values as `KeyKind` names them.
    Inserted(KeyKind),
    /// A foreign key check before the changes: what a later step of the same SQL
    /// ([`Expect::NoNewViolations`]) compares with.
    Baseline,
    /// A foreign key check after the changes (`PRAGMA foreign_key_check`'s rows: table,
    /// rowid, parent, key): no row breaking a key of `tables` (theirs, or pointing to one)
    /// that the baseline didn't have — what was broken before isn't the change's.
    NoNewViolations { tables: Vec<String> },
}

/// A statement of a transaction, with what it must do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub sql: String,
    pub expect: Expect,
}

/// How many rows break a foreign key in `after` and not in `before` (counted by child and
/// parent table: a row's rowid may change with a rebuild), of those whose table or parent
/// is one of `tables` (any case).
pub fn new_violations(before: &[Vec<Value>], after: &[Vec<Value>], tables: &[String]) -> u64 {
    let group = |row: &Vec<Value>| {
        let name = |at: usize| row.get(at).map(|v| v.display().to_lowercase()).unwrap_or_default();
        (name(0), name(2))
    };
    let mut counts: HashMap<(String, String), i64> = HashMap::new();
    for row in before {
        *counts.entry(group(row)).or_default() -= 1;
    }
    for row in after {
        *counts.entry(group(row)).or_default() += 1;
    }
    let mine = |t: &str| tables.iter().any(|m| m.eq_ignore_ascii_case(t));
    counts
        .into_iter()
        .filter(|((child, parent), n)| *n > 0 && (mine(child) || mine(parent)))
        .map(|(_, n)| n as u64)
        .sum()
}

impl Step {
    pub fn any(sql: impl Into<String>) -> Self {
        Self { sql: sql.into(), expect: Expect::Any }
    }

    /// The step keeps the rows it returns (to read a key, or to compare).
    pub fn keeps_rows(&self) -> bool {
        matches!(
            self.expect,
            Expect::Inserted(_) | Expect::Baseline | Expect::NoNewViolations { .. }
        )
    }

    /// Whether `outcome` is what the step must do: otherwise why the transaction is
    /// rolled back.
    pub fn check(&self, outcome: &Outcome) -> Result<(), DbError> {
        self.verify(outcome, &[], None).map(|_| ())
    }

    /// Whether the step did what it must, from its `outcome` and the `rows` it returned
    /// (kept when [`Self::keeps_rows`]), `baseline` being the rows of the same check before
    /// the changes. An insert gives back its row's key.
    pub fn verify(
        &self,
        outcome: &Outcome,
        rows: &[Vec<Value>],
        baseline: Option<&[Vec<Value>]>,
    ) -> Result<Option<RowKey>, DbError> {
        match &self.expect {
            Expect::Any | Expect::Baseline => Ok(None),
            Expect::OneRow => match outcome.affected {
                Some(1) => Ok(None),
                Some(0) => Err(DbError::Conflict("the row changed or was deleted".to_owned())),
                Some(n) => Err(DbError::Conflict(format!("{n} rows matched one row's key"))),
                None => {
                    Err(DbError::Conflict("the engine didn't say which rows changed".to_owned()))
                }
            },
            Expect::NoRows if outcome.rows == 0 => Ok(None),
            Expect::NoRows => {
                Err(DbError::Query(format!("{} rows break a foreign key", outcome.rows)))
            }
            Expect::Inserted(key) => {
                let Some(row) = rows.first() else { return Ok(None) };
                Ok(match key {
                    KeyKind::Primary(columns) if row.len() == columns.len() => Some(
                        RowKey::Primary(columns.iter().cloned().zip(row.iter().cloned()).collect()),
                    ),
                    KeyKind::RowId => match row.first() {
                        Some(Value::Int(id)) => Some(RowKey::RowId(*id)),
                        _ => None,
                    },
                    _ => None,
                })
            }
            Expect::NoNewViolations { tables } => {
                match new_violations(baseline.unwrap_or_default(), rows, tables) {
                    0 => Ok(None),
                    n => Err(DbError::Query(format!(
                        "{n} rows would break a foreign key (rolled back)"
                    ))),
                }
            }
        }
    }
}

/// One transaction, as it runs: what goes before it (outside: `PRAGMA foreign_keys`), its
/// statements between `begin` and `commit` (any failing: `rollback`), and what goes after
/// it, committed or not. `stale`: what it drops and doesn't make again (the views,
/// triggers and indexes naming a column dropped) — the user confirms losing them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Script {
    pub before: Vec<String>,
    pub begin: String,
    pub steps: Vec<Step>,
    pub commit: String,
    pub rollback: String,
    pub after: Vec<String>,
    pub stale: Vec<Stale>,
}

impl Script {
    /// A transaction of `steps` in `dialect`'s words.
    pub fn transaction(dialect: &dyn Dialect, steps: Vec<Step>) -> Self {
        Self {
            before: Vec::new(),
            begin: dialect.begin().to_owned(),
            steps,
            commit: dialect.commit().to_owned(),
            rollback: dialect.rollback().to_owned(),
            after: Vec::new(),
            stale: Vec::new(),
        }
    }

    /// Every statement, in the order they run when nothing fails (what the preview shows);
    /// an empty one (a driver's transaction with no words to begin it) runs nothing and
    /// isn't shown.
    pub fn statements(&self) -> Vec<String> {
        let mut all = self.before.clone();
        all.push(self.begin.clone());
        all.extend(self.steps.iter().map(|s| s.sql.clone()));
        all.push(self.commit.clone());
        all.extend(self.after.iter().cloned());
        all.retain(|s| !s.is_empty());
        all
    }
}

/// `script`'s steps run inside a transaction already open (a console's, in `Tx: Manual` or
/// after a `BEGIN` of its own): between a savepoint and its release, nothing before or after
/// them. One failing rolls back to the savepoint — only these changes undone, the
/// transaction kept and never abandoned (PostgreSQL's) —; the rest waits for the
/// transaction's Commit.
pub fn nested(script: &Script, savepoint: &str) -> Script {
    Script {
        before: Vec::new(),
        begin: format!("SAVEPOINT {savepoint}"),
        steps: script.steps.clone(),
        commit: format!("RELEASE SAVEPOINT {savepoint}"),
        rollback: format!("ROLLBACK TO SAVEPOINT {savepoint}"),
        after: Vec::new(),
        stale: script.stale.clone(),
    }
}

/// The order a set's changes are applied in: deletes, then updates, then inserts (each in
/// the order made). A row deleted frees its key and its unique values for a row changed or
/// added in the same set; an update moving a key away frees it for an insert.
pub fn ordered(set: &ChangeSet) -> ChangeSet {
    let rank = |c: &Change| match c {
        Change::Delete { .. } => 0,
        Change::Update { .. } => 1,
        Change::Insert { .. } => 2,
    };
    let mut changes = set.changes.clone();
    // Stable: the user's order within each kind.
    changes.sort_by_key(rank);
    ChangeSet { table: set.table.clone(), changes }
}

/// Whether `structure`'s rows can be changed: the source isn't read-only, the driver edits
/// rows, the table has a key to find them by.
pub fn check_editable(
    structure: &Structure,
    capabilities: &Capabilities,
    read_only: bool,
) -> Result<(), DbError> {
    if read_only {
        return Err(DbError::ReadOnly("the data source is read-only".to_owned()));
    }
    if !capabilities.edit_rows {
        return Err(DbError::ReadOnly("the driver doesn't edit rows".to_owned()));
    }
    if structure.key_kind() == KeyKind::None {
        return Err(DbError::ReadOnly(format!(
            "{} has no key to find its rows by",
            structure.table
        )));
    }
    Ok(())
}

/// The transaction applying `set` in `dialect`: its changes [`ordered`], each update and
/// delete changing exactly one row. With the table's `structure`, rows are found by the
/// row id's name no column hides, generated columns are refused, and an insert gives back
/// its row's key when the dialect can ([`Dialect::returning`]).
pub fn apply_script(
    dialect: &dyn Dialect,
    set: &ChangeSet,
    structure: Option<&Structure>,
) -> Result<Script, DbError> {
    let set = ordered(set);
    let statements = dialect.changes_of(&set, structure)?;
    let inserted = match structure {
        Some(s) => {
            let key = s.key_kind();
            dialect.returning(&key, s.row_id_name())?.map(|_| key)
        }
        None => None,
    };
    let steps = set
        .changes
        .iter()
        .zip(statements)
        .map(|(change, sql)| Step {
            sql,
            expect: match change {
                Change::Insert { .. } => inserted.clone().map_or(Expect::Any, Expect::Inserted),
                Change::Update { .. } | Change::Delete { .. } => Expect::OneRow,
            },
        })
        .collect();
    Ok(Script::transaction(dialect, steps))
}

/// The one existing table all of `changes` act on, if there is one (a table being created
/// has nothing to rebuild).
pub fn alter_target(changes: &[SchemaChange]) -> Option<&TableRef> {
    let first = changes.first()?.table();
    let one = changes
        .iter()
        .all(|c| c.table() == first && !matches!(c, SchemaChange::CreateTable { .. }));
    one.then_some(first)
}

/// Whether `dialect` must rebuild `table` for any of `changes`.
pub fn needs_rebuild(dialect: &dyn Dialect, changes: &[SchemaChange], table: &Structure) -> bool {
    changes.iter().any(|c| dialect.needs_rebuild(c, table))
}

/// The transaction making `changes` with `dialect`'s DDL, in order.
pub fn alter_script(dialect: &dyn Dialect, changes: &[SchemaChange]) -> Result<Script, DbError> {
    let mut steps = Vec::new();
    for change in changes {
        steps.extend(dialect.schema(change)?.into_iter().map(Step::any));
    }
    Ok(Script::transaction(dialect, steps))
}

/// A part of a structure change: one table's changes, as plain DDL or as its rebuild.
#[derive(Debug, Clone)]
pub enum AlterPart {
    Plain(Vec<SchemaChange>),
    Rebuild(Box<RebuildSource>, Vec<SchemaChange>),
}

impl AlterPart {
    fn tables(&self) -> Vec<TableRef> {
        let changes = match self {
            Self::Plain(changes) | Self::Rebuild(_, changes) => changes,
        };
        let mut tables: Vec<TableRef> = Vec::new();
        for change in changes {
            let mut add = |t: TableRef| {
                if !tables.contains(&t) {
                    tables.push(t);
                }
            };
            add(change.table().clone());
            if let SchemaChange::RenameTable { table, to } = change {
                add(TableRef { schema: table.schema.clone(), name: to.clone() });
            }
        }
        tables
    }
}

/// The rebuild of `source`'s table making `changes`: SQLite's 12 steps (foreign keys off
/// before, checked against what was broken before the commit, on again after, when they
/// were on).
pub fn rebuild_script(source: &RebuildSource, changes: &[SchemaChange]) -> Result<Script, DbError> {
    parts_script(&Sqlite, vec![AlterPart::Rebuild(Box::new(source.clone()), changes.to_vec())])
}

/// One transaction making `parts` in order (plain DDL in `dialect`'s words, rebuilds by
/// SQLite's procedure). With a rebuild among them and foreign keys on: off before, the
/// rows breaking one read before the changes and again before the commit (only a new one
/// rolls it back, in a table changed or pointing to one), on again after.
pub fn parts_script(dialect: &dyn Dialect, parts: Vec<AlterPart>) -> Result<Script, DbError> {
    let foreign_keys =
        parts.iter().any(|p| matches!(p, AlterPart::Rebuild(source, _) if source.foreign_keys));
    let mut steps = Vec::new();
    let mut stale = Vec::new();
    // The tables changed, by schema (in the order met).
    let mut schemas: Vec<(Option<String>, Vec<String>)> = Vec::new();
    for part in &parts {
        for table in part.tables() {
            match schemas.iter_mut().find(|(s, _)| *s == table.schema) {
                Some((_, names)) => names.push(table.name),
                None => schemas.push((table.schema, vec![table.name])),
            }
        }
        match part {
            AlterPart::Plain(changes) => {
                for change in changes {
                    steps.extend(dialect.schema(change)?.into_iter().map(Step::any));
                }
            }
            AlterPart::Rebuild(source, changes) => {
                let plan = rebuild(&source.table, changes)?;
                steps.extend(plan.statements.into_iter().map(Step::any));
                stale.extend(plan.stale);
            }
        }
    }
    let mut script = Script::transaction(dialect, steps);
    script.stale = stale;
    if foreign_keys {
        let mut baselines = Vec::new();
        for (schema, tables) in schemas {
            let sql = foreign_key_check(schema.as_deref())?;
            baselines.push(Step { sql: sql.clone(), expect: Expect::Baseline });
            script.steps.push(Step { sql, expect: Expect::NoNewViolations { tables } });
        }
        script.steps.splice(0..0, baselines);
        script.before.push(FOREIGN_KEYS_OFF.to_owned());
        script.after.push(FOREIGN_KEYS_ON.to_owned());
    }
    Ok(script)
}

/// Whether `changes` drop a column that something kept as its own SQL names (a view, a
/// trigger, an index on an expression or with a `WHERE`, a `CHECK`).
pub fn drops_named(source: &RebuildSource, changes: &[SchemaChange]) -> bool {
    let t = &source.table;
    let objects = t.views.iter().chain(&t.triggers).chain(&t.written_indexes);
    let sql: Vec<&str> =
        objects.map(|o| o.sql.as_str()).chain(t.checks.iter().map(String::as_str)).collect();
    changes.iter().any(|c| match c {
        SchemaChange::DropColumn { column, .. } => {
            sql.iter().any(|s| crate::sqlite::names(s, column))
        }
        _ => false,
    })
}

/// The transaction making `changes` on `session` in `dialect`: plain DDL in their order
/// when `ALTER TABLE` can make them all; otherwise each table's changes in turn (in the
/// order the tables are first met), rebuilt where it must be, in one transaction. A
/// column "changed" only in its name is renamed ([`Dialect::simplified`]).
pub fn alter_plan(
    dialect: &dyn Dialect,
    session: &mut dyn Session,
    changes: &[SchemaChange],
) -> Result<Script, DbError> {
    let mut groups: Vec<(TableRef, Vec<SchemaChange>)> = Vec::new();
    for change in changes {
        match groups.iter_mut().find(|(t, _)| t == change.table()) {
            Some((_, mine)) => mine.push(change.clone()),
            None => groups.push((change.table().clone(), vec![change.clone()])),
        }
    }
    let mut structures: Vec<(TableRef, Structure)> = Vec::new();
    let mut parts = Vec::new();
    let mut rebuilds = false;
    for (table, mine) in groups {
        let made_or_moved = mine.iter().any(|c| {
            matches!(
                c,
                SchemaChange::CreateTable { .. }
                    | SchemaChange::DropTable { .. }
                    | SchemaChange::RenameTable { .. }
            )
        });
        // A table made, dropped or renamed here, or not there yet: plain DDL.
        let structure = if made_or_moved { None } else { session.structure(&table).ok() };
        let Some(structure) = structure else {
            parts.push(AlterPart::Plain(mine));
            continue;
        };
        let mine: Vec<SchemaChange> =
            mine.iter().map(|c| dialect.simplified(c, &structure)).collect();
        // A column dropped that a view, a trigger, a partial index or a check names: its
        // engine's own `DROP COLUMN` refuses it, a rebuild leaves those out (said).
        let named = if needs_rebuild(dialect, &mine, &structure) {
            None
        } else if mine.iter().any(|c| matches!(c, SchemaChange::DropColumn { .. })) {
            session.rebuild_source(&table).ok().filter(|source| drops_named(source, &mine))
        } else {
            None
        };
        if let Some(source) = named {
            rebuilds = true;
            parts.push(AlterPart::Rebuild(Box::new(source), mine));
        } else if needs_rebuild(dialect, &mine, &structure) {
            rebuilds = true;
            parts.push(AlterPart::Rebuild(Box::new(session.rebuild_source(&table)?), mine));
        } else {
            parts.push(AlterPart::Plain(mine));
        }
        structures.push((table, structure));
    }
    if rebuilds {
        return parts_script(dialect, parts);
    }
    // Nothing rebuilt: the changes as given, in their order.
    let simple: Vec<SchemaChange> = changes
        .iter()
        .map(|c| match structures.iter().find(|(t, _)| t == c.table()) {
            Some((_, s)) => dialect.simplified(c, s),
            None => c.clone(),
        })
        .collect();
    alter_script(dialect, &simple)
}

/// A structure change as it's shown before it runs: the [`Script`] (exactly what runs) and
/// what SQLite rewrites by itself for a rename (the views, triggers and indexes naming the
/// old name), for information.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlterPreview {
    pub script: Script,
    pub rewritten: Vec<Rewritten>,
}

/// [`alter_plan`]'s script for `changes`, with what its renames make the engine rewrite
/// (read from each renamed table's own SQL, when the session gives it).
pub fn alter_preview(
    dialect: &dyn Dialect,
    session: &mut dyn Session,
    changes: &[SchemaChange],
) -> Result<AlterPreview, DbError> {
    let script = alter_plan(dialect, session, changes)?;
    let mut found = Vec::new();
    let mut seen: Vec<&TableRef> = Vec::new();
    for change in changes {
        let renames = matches!(
            change,
            SchemaChange::RenameColumn { .. }
                | SchemaChange::RenameTable { .. }
                | SchemaChange::AlterColumn { .. }
        );
        if !renames || seen.contains(&change.table()) {
            continue;
        }
        seen.push(change.table());
        if let Ok(source) = session.rebuild_source(change.table()) {
            found.extend(rewritten(&source.table, changes, &script.stale));
        }
    }
    Ok(AlterPreview { script, rewritten: found })
}

/// What a statement run gave: its outcome and the rows kept.
type Ran = Result<(Outcome, Vec<Vec<Value>>), DbError>;

/// The rows a step returned: counted, and kept when it needs them.
struct Returned {
    keep: bool,
    count: u64,
    rows: Vec<Vec<Value>>,
}

impl RowSink for Returned {
    fn columns(&mut self, _columns: &[Column]) {}

    fn row(&mut self, values: Vec<Value>) -> Flow {
        self.count += 1;
        if self.keep {
            self.rows.push(values);
        }
        Flow::Continue
    }
}

/// Runs `script` on `session`: what goes before, the transaction (each step checked; one
/// failing, or a cancel, rolls it all back), what goes after — whatever happened. The
/// rows changed, as the engine counted them, and the keys of the rows inserted. A statement
/// before or after failing, or a rollback, is told to `warn`.
pub fn run(
    session: &mut dyn Session,
    script: &Script,
    cancel: &Cancel,
    warn: &mut dyn FnMut(&str, &DbError),
) -> Result<Applied, DbError> {
    // Its own statements aren't cancelled half-way: a cancel is seen between them.
    let never = Cancel::default();
    let mut exec = |sql: &str, keep: bool| -> Ran {
        // No words to say it (a driver's transaction begun by its first step): nothing runs.
        if sql.is_empty() {
            return Ok((Outcome::default(), Vec::new()));
        }
        let mut rows = Returned { keep, count: 0, rows: Vec::new() };
        let outcome = session.execute(sql, &mut rows, &never)?;
        Ok((Outcome { rows: outcome.rows.max(rows.count), ..outcome }, rows.rows))
    };
    let after = |exec: &mut dyn FnMut(&str, bool) -> Ran, warn: &mut dyn FnMut(&str, &DbError)| {
        for sql in &script.after {
            if let Err(e) = exec(sql, false) {
                warn(sql, &e);
            }
        }
    };
    for sql in &script.before {
        if let Err(e) = exec(sql, false) {
            after(&mut exec, warn);
            return Err(e);
        }
    }
    if let Err(e) = exec(&script.begin, false) {
        after(&mut exec, warn);
        return Err(e);
    }
    let mut applied = Applied::default();
    let mut baselines: HashMap<&str, Vec<Vec<Value>>> = HashMap::new();
    let mut failed = None;
    for step in &script.steps {
        if cancel.is_cancelled() {
            failed = Some(DbError::Cancelled);
            break;
        }
        let ran = exec(&step.sql, step.keeps_rows()).and_then(|(outcome, rows)| {
            let baseline = baselines.get(step.sql.as_str()).map(Vec::as_slice);
            let key = step.verify(&outcome, &rows, baseline)?;
            Ok((outcome, rows, key))
        });
        match ran {
            Ok((outcome, rows, key)) => {
                applied.affected += outcome.affected.unwrap_or(0);
                applied.inserted.extend(key);
                if step.expect == Expect::Baseline {
                    baselines.insert(step.sql.as_str(), rows);
                }
            }
            Err(e) => {
                failed = Some(e);
                break;
            }
        }
    }
    let result = match failed {
        Some(e) => Err(e),
        None => exec(&script.commit, false).map(|_| applied),
    };
    if result.is_err()
        && let Err(e) = exec(&script.rollback, false)
    {
        warn(&script.rollback, &e);
    }
    after(&mut exec, warn);
    result
}

#[cfg(test)]
#[path = "../tests/unit/transaction.rs"]
mod tests;
