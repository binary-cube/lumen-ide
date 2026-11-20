//! A request, run on a session (on the pool's thread). An SQL source's changes run as its
//! dialect writes them ([`database::transaction`]: the same statements the preview
//! showed), one transaction each: an update or a delete that doesn't change exactly one row
//! is a conflict, and everything is rolled back; a table SQLite can't alter is rebuilt by
//! its procedure's steps, in order (several tables in one transaction). A source that writes no SQL (a CSV file) applies the
//! changes itself, by the same contract.

use database::transaction::{self, AlterPreview, Script, alter_plan, alter_preview, apply_script};
use database::{
    Applied, Cancel, ChangeSet, Collect, Column, DbError, Filter, Flow, RowSink, SchemaChange,
    Session, TableRef, Value,
};

use crate::worker::{Request, Response, Source};

/// Runs `request` on `session`.
pub(crate) fn run(
    session: &mut dyn Session,
    request: &Request,
    source: &Source,
    cancel: &Cancel,
) -> Result<Response, DbError> {
    match request {
        Request::Tables(path) => session.tables(path).map(Response::Tables),
        Request::Databases => {
            let names = session.databases()?;
            let current = session.current()?.database;
            Ok(Response::Databases { names, current })
        }
        Request::Structure(table) => session.structure(table).map(Response::Structure),
        Request::Count { table, filter } => {
            count(session, source, table, filter.as_ref(), cancel).map(Response::Count)
        }
        Request::RowCount(table) => {
            count(session, source, table, None, cancel).map(Response::Count)
        }
        Request::ReadPage { table, page, .. } => {
            let mut rows = Collect::default();
            session.read(table, page, &mut rows, cancel)?;
            if cancel.is_cancelled() {
                return Err(DbError::Cancelled);
            }
            Ok(Response::Page(rows.into()))
        }
        Request::Apply(set) => apply(session, source, set, cancel).map(Response::Applied),
        Request::Alter(changes) => {
            alter(session, source, changes, None, cancel).map(Response::Applied)
        }
        Request::PlanAlter(changes) => plan(session, source, changes).map(Response::Plan),
        Request::AlterAsShown { changes, shown } => {
            alter(session, source, changes, Some(shown.as_ref()), cancel).map(Response::Applied)
        }
        Request::Close => Ok(Response::Closed),
    }
}

/// The tables a request changes when it commits, and whether their structure changed.
pub(crate) fn changed_tables(request: &Request) -> Vec<(TableRef, bool)> {
    match request {
        Request::Apply(set) => vec![(set.table.clone(), false)],
        Request::Alter(changes) | Request::AlterAsShown { changes, .. } => {
            let mut tables: Vec<(TableRef, bool)> = Vec::new();
            let mut add = |table: TableRef| {
                if !tables.iter().any(|(t, _)| *t == table) {
                    tables.push((table, true));
                }
            };
            for change in changes {
                add(change.table().clone());
                if let SchemaChange::RenameTable { table, to } = change {
                    add(TableRef { schema: table.schema.clone(), name: to.clone() });
                }
            }
            tables
        }
        _ => Vec::new(),
    }
}

fn count(
    session: &mut dyn Session,
    source: &Source,
    table: &TableRef,
    filter: Option<&Filter>,
    cancel: &Cancel,
) -> Result<u64, DbError> {
    let Some(dialect) = dialect(session, source) else {
        return session.count(table, filter, cancel);
    };
    let mut first = First::default();
    session.execute(&dialect.count(table, filter)?, &mut first, cancel)?;
    match first.value {
        Some(Value::Int(n)) => Ok(n.max(0) as u64),
        Some(Value::Other(n) | Value::Text(n)) => {
            n.trim().parse().map_err(|_| DbError::Query(format!("a count of `{n}`")))
        }
        other => Err(DbError::Query(format!("a count of {other:?}"))),
    }
}

/// How `session` writes its SQL: its server's own dialect (a version's rules), else its
/// driver's.
fn dialect(
    session: &dyn Session,
    source: &Source,
) -> Option<std::sync::Arc<dyn database::Dialect>> {
    session.dialect().or_else(|| source.dialect.clone())
}

fn writable(source: &Source) -> Result<(), DbError> {
    if source.read_only {
        return Err(DbError::ReadOnly(i18n::t("database.why_source_read_only")));
    }
    Ok(())
}

fn apply(
    session: &mut dyn Session,
    source: &Source,
    set: &ChangeSet,
    cancel: &Cancel,
) -> Result<Applied, DbError> {
    writable(source)?;
    if !source.capabilities.edit_rows {
        return Err(DbError::ReadOnly(i18n::t("database.why_driver_no_edits")));
    }
    if set.is_empty() {
        return Ok(Applied::default());
    }
    match dialect(session, source) {
        Some(dialect) => {
            // Its structure: the row id's name, the generated columns, the keys given back.
            let structure = session.structure(&set.table).ok();
            let script = apply_script(dialect.as_ref(), set, structure.as_ref())?;
            run_script(session, &script, cancel)
        }
        None => session.apply(set),
    }
}

/// Whether `source` may make `changes`: writable, and each one among what its driver does.
fn alterable(source: &Source, changes: &[SchemaChange]) -> Result<(), DbError> {
    writable(source)?;
    let can = source.capabilities.schema_changes;
    if !can.any() {
        return Err(DbError::Unsupported(i18n::t("database.why_structure")));
    }
    if changes.iter().any(|c| !can.allows(c)) {
        return Err(DbError::Unsupported(i18n::t("database.why_change")));
    }
    Ok(())
}

/// What `changes` would run on an SQL source, or the statements a source without the
/// editor's SQL writes itself (an external driver's); none for one that makes them itself
/// (its view sums them up).
fn plan(
    session: &mut dyn Session,
    source: &Source,
    changes: &[SchemaChange],
) -> Result<Option<AlterPreview>, DbError> {
    alterable(source, changes)?;
    match dialect(session, source) {
        Some(dialect) => alter_preview(dialect.as_ref(), session, changes).map(Some),
        None => Ok(session
            .plan_alter(changes)?
            .map(|script| AlterPreview { script, rewritten: Vec::new() })),
    }
}

/// Makes `changes`: with `shown` (what the user saw), only if planning them again gives
/// exactly that script.
fn alter(
    session: &mut dyn Session,
    source: &Source,
    changes: &[SchemaChange],
    shown: Option<Option<&Script>>,
    cancel: &Cancel,
) -> Result<Applied, DbError> {
    alterable(source, changes)?;
    if changes.is_empty() {
        return Ok(Applied::default());
    }
    let script = match dialect(session, source) {
        Some(dialect) => alter_plan(dialect.as_ref(), session, changes)?,
        // A source without the editor's SQL: the statements it writes itself, run as
        // shown, or the changes it makes itself.
        None => match session.plan_alter(changes)? {
            Some(script) => script,
            None if matches!(shown, Some(Some(_))) => {
                return Err(DbError::Conflict(i18n::t("database.why_script_without_sql")));
            }
            None => return session.alter(changes),
        },
    };
    if let Some(shown) = shown
        && shown != Some(&script)
    {
        return Err(DbError::Conflict(i18n::t("database.why_structure_changed")));
    }
    // For structure changes: the statements run.
    run_script(session, &script, cancel)
        .map(|applied| Applied { affected: script.steps.len() as u64, ..applied })
}

/// Runs `script` on `session` ([`database::transaction::run`]): what goes before, the
/// transaction (each step checked; one failing, or a cancel, rolls it all back), what goes
/// after — whatever happened. The rows changed, as the engine counted them, and the keys
/// of the rows inserted.
pub fn run_script(
    session: &mut dyn Session,
    script: &Script,
    cancel: &Cancel,
) -> Result<Applied, DbError> {
    transaction::run(session, script, cancel, &mut |sql, e| {
        tracing::warn!(%sql, error = %e, "a statement around a transaction failed");
    })
}

/// The first value of the first row (a count).
#[derive(Default)]
struct First {
    value: Option<Value>,
}

impl RowSink for First {
    fn columns(&mut self, _columns: &[Column]) {}

    fn row(&mut self, values: Vec<Value>) -> Flow {
        self.value = values.into_iter().next();
        Flow::Stop
    }
}

#[cfg(test)]
#[path = "../tests/unit/execute.rs"]
mod tests;
