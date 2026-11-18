//! What a run says: its messages, its titles, its plan written out.

use super::*;

/// The query reading result column `column`'s whole value of the row with `key` from its
/// table (by the dialect: names quoted, the key's values written as literals).
pub fn whole_query(
    dialect: &dyn Dialect,
    editable: &database::edits::EditableResult,
    key: &RowKey,
    column: usize,
) -> Result<String, DbError> {
    let name = editable
        .column(column)
        .map(|c| c.name.clone())
        .ok_or_else(|| DbError::Unsupported(i18n::t("database.why_not_a_column")))?;
    let condition = dialect.key_condition_by(key, editable.structure.row_id_name())?;
    Ok(format!(
        "SELECT {} FROM {} WHERE {condition}",
        dialect.quote_identifier(&name)?,
        dialect.table_name(&editable.table)?
    ))
}

/// A table a statement names, as the grids name it: the schema the connection is in when it
/// names none (SQLite's `main`: none).
pub(super) fn resolved(table: TableRef, place: &Place, flavor: sql::Flavor) -> TableRef {
    let schema = table.schema.or_else(|| place.schema.clone());
    match schema {
        Some(s) if flavor == sql::Flavor::Sqlite && s.eq_ignore_ascii_case("main") => {
            TableRef::new(table.name)
        }
        Some(s) => TableRef::in_schema(s, table.name),
        None => TableRef::new(table.name),
    }
}

/// The Output's `database.schema>` before a statement.
pub(super) fn prefix(place: &Place) -> String {
    match (&place.database, &place.schema) {
        (Some(d), Some(s)) => format!("{d}.{s}>"),
        (Some(one), None) | (None, Some(one)) => format!("{one}>"),
        (None, None) => String::new(),
    }
}

pub(super) fn failed(why: String, total: usize) -> LastRun {
    LastRun {
        ok: false,
        done: 0,
        total,
        rows: None,
        truncated: false,
        affected: None,
        elapsed: Duration::ZERO,
        error: Some(why),
        cancelled: false,
        plan_steps: None,
    }
}

/// `elapsed` as the Output says it: milliseconds and seconds (`411 ms`, `1 s 204 ms`).
pub fn millis(elapsed: Duration) -> String {
    let ms = elapsed.as_millis();
    if ms < 1000 { format!("{ms} ms") } else { format!("{} s {} ms", ms / 1000, ms % 1000) }
}

/// A time as the Output says it (`0.120 ms`, `2.48 s`).
pub(super) fn time_text(ms: f64) -> String {
    if ms >= 1000.0 { format!("{:.2} s", ms / 1000.0) } else { format!("{ms:.3} ms") }
}

/// A profile's tab's title: `Profile · table` for a query of one table, else `Profile`.
pub fn profile_title(sent: &str, flavor: sql::Flavor) -> String {
    match profile_table(sent, flavor) {
        Some(table) => i18n::tr!("profile.view.title_of", table = table.as_str()),
        None => i18n::t("profile.view.title"),
    }
}

/// The one table `sent` queries (`None`: more, or none).
pub fn profile_table(sent: &str, flavor: sql::Flavor) -> Option<String> {
    let tokens = sql::lex(sent, flavor);
    sql::query_table(sent, &tokens, flavor).map(|t| t.name)
}

/// A profile's summary line in the Output: the engine's planning and execution, the
/// runner's time, the rows, a write undone, the engine's counters, the blocks, JIT, the
/// costliest step — what's known —, and how many problems its analysis found.
pub fn profile_summary(report: &ProfileReport) -> String {
    let p = &report.profile;
    let s = &p.statement;
    let mut parts = Vec::new();
    if let Some(ms) = s.planning_ms {
        parts.push(i18n::tr!("console.profile_planning", time = time_text(ms).as_str()));
    }
    if let Some(ms) = s.execution_ms {
        parts.push(i18n::tr!("console.profile_execution", time = time_text(ms).as_str()));
    }
    if let Some(ms) = s.wall_ms {
        parts.push(i18n::tr!("console.profile_wall", time = time_text(ms).as_str()));
    }
    if let Some(rows) = s.rows_affected.or(s.rows_returned) {
        parts.push(i18n::tr!("console.profile_rows", rows = rows as i64));
    }
    if p.has_note(ProfileNote::RolledBack) {
        parts.push(i18n::t("console.profile_rolled_back"));
    }
    if let Some(c) = s.counters.filter(|c| c.fullscan_steps.is_some() || c.vm_steps.is_some()) {
        let n = |v: Option<u64>| v.unwrap_or(0).to_string();
        parts.push(i18n::tr!(
            "console.profile_counters",
            fullscan = n(c.fullscan_steps).as_str(),
            sorts = n(c.sorts).as_str(),
            autoindex = n(c.autoindex).as_str(),
            vm = n(c.vm_steps).as_str()
        ));
    }
    if let Some(io) = s.io_total.filter(|io| io.shared_hit.is_some() || io.shared_read.is_some()) {
        let n = |v: Option<u64>| v.unwrap_or(0).to_string();
        parts.push(i18n::tr!(
            "console.profile_blocks",
            hit = n(io.shared_hit).as_str(),
            read = n(io.shared_read).as_str()
        ));
    }
    if let Some(ms) = s.jit.and_then(|j| j.total_ms) {
        parts.push(i18n::tr!("console.profile_jit", time = time_text(ms).as_str()));
    }
    // The costliest step: the most of the statement's time (or cost) of its own.
    let costliest = report
        .order
        .iter()
        .filter_map(|id| {
            let d = report.derived(*id);
            d.self_pct.or(d.cost_pct).map(|share| (*id, share))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((id, share)) = costliest.filter(|(_, share)| *share > 0.0) {
        let step = database::profile::export::head(p.node(id));
        let share = format!("{:.0}%", share * 100.0);
        parts.push(i18n::tr!(
            "console.profile_costliest",
            step = step.as_str(),
            share = share.as_str()
        ));
    }
    // What the analysis found (39h3).
    parts.push(crate::profile::analysis::problems_text(&report.findings));
    i18n::tr!("console.profile_summary", parts = parts.join(" · ").as_str())
}

/// A plan's node and its children, indented, in the Output; how many there were.
pub(super) fn write_plan(
    log: &Log,
    runtime: &Runtime,
    node: &PlanNode,
    depth: usize,
    flavor: sql::Flavor,
) -> usize {
    let mut text = format!("{}{}", "  ".repeat(depth), node.operation);
    if let Some(object) = &node.object {
        text.push_str(" · ");
        text.push_str(object);
    }
    if let Some(detail) = node.detail.as_ref().filter(|d| !d.is_empty()) {
        text.push_str(" · ");
        // A condition's literals may be passwords: hidden as in the statement.
        text.push_str(&sql::mask_secrets(detail, flavor));
    }
    log.push(LogLine::new(runtime.clock(), LogKind::Result, text));
    1 + node.children.iter().map(|c| write_plan(log, runtime, c, depth + 1, flavor)).sum::<usize>()
}

/// Where in the document an error is, when the engine said (line and column from 0): its
/// position in the text sent taken back through the statement's map (its parameters'
/// values written in), so it lands in the user's own text.
pub fn error_place(
    e: &DbError,
    document: &str,
    statement: &PlannedStatement,
) -> Option<TextPosition> {
    let DbError::Sql(error) = e else { return None };
    error
        .position
        .and_then(|position| sql::locate(document, statement.sent(), &statement.map(), position))
}

/// What failed, as told: the engine's words (its code, detail and hint) and the place in
/// the document (line and column, from 1) when it said one.
pub fn error_text(e: &DbError, place: Option<TextPosition>) -> String {
    let DbError::Sql(error) = e else { return message(e) };
    let mut text = match &error.sqlstate {
        Some(code) => format!("[{code}] {}", error.message),
        None => error.message.clone(),
    };
    if let Some(detail) = &error.detail {
        text.push_str(&format!(" ({detail})"));
    }
    if let Some(hint) = &error.hint {
        text.push_str(&format!(" — {hint}"));
    }
    match place {
        Some(at) => i18n::tr!(
            "console.error_at",
            message = text,
            line = at.line + 1,
            column = at.column + 1
        ),
        None => text,
    }
}

/// An error as the user is told it.
pub fn message(e: &DbError) -> String {
    match e {
        DbError::Cancelled => i18n::t("console.cancelled"),
        DbError::InUse(_) => i18n::t("console.in_use"),
        other => reason(other),
    }
}

/// A failure's kind said in the interface's language, with what the engine or the driver
/// said of it (`Display` stays English: the logs').
pub(super) fn reason(e: &DbError) -> String {
    match e {
        DbError::Connect(why) => i18n::tr!("database.err_connect", why = why.as_str()),
        DbError::Auth(why) => i18n::tr!("database.err_auth", why = why.as_str()),
        DbError::Unsupported(what) => i18n::tr!("database.err_unsupported", what = what.as_str()),
        DbError::Conflict(why) => i18n::tr!("database.err_conflict", why = why.as_str()),
        DbError::ReadOnly(why) => i18n::tr!("database.err_read_only", why = why.as_str()),
        other => other.to_string(),
    }
}
