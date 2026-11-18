//! What the server knows of a profile's tables, pure (tested without a server): one query
//! for the tables (`information_schema.TABLES`: the engine's row estimate, the data's and
//! the indexes' sizes, the engine, its last change), one for their indexes (`STATISTICS`:
//! columns in order, unique, cardinality), and, when the user may read it,
//! `performance_schema`'s reads per index (`table_io_waits_summary_by_index_usage`: the
//! rows each index fetched, and those read with none). MySQL keeps no time of a table's
//! last `ANALYZE` the user may see: that stays unknown.

use database::TableRef;
use database::mysql::mysql_text;
use database::profile::{Extra, ExtraValue, IndexStats, Quantity, TableStats, Unit};

/// `(schema, name) IN (…)` for `tables` (a table without a schema: the session's database).
pub(crate) fn wanted(schema: &str, name: &str, tables: &[TableRef]) -> String {
    let pairs: Vec<String> = tables
        .iter()
        .map(|t| {
            let s = t.schema.as_deref().map_or_else(|| "DATABASE()".to_owned(), mysql_text);
            format!("({s}, {})", mysql_text(&t.name))
        })
        .collect();
    format!("({schema}, {name}) IN ({})", pairs.join(", "))
}

/// The tables' rows: schema, name, engine, rows, data, indexes, free, next number, last
/// change, made, collation, row format, average row.
pub fn tables_sql(tables: &[TableRef]) -> String {
    format!(
        "SELECT TABLE_SCHEMA, TABLE_NAME, ENGINE, TABLE_ROWS, DATA_LENGTH, INDEX_LENGTH, \
         DATA_FREE, AUTO_INCREMENT, UPDATE_TIME, CREATE_TIME, TABLE_COLLATION, ROW_FORMAT, \
         AVG_ROW_LENGTH FROM information_schema.TABLES WHERE {}",
        wanted("TABLE_SCHEMA", "TABLE_NAME", tables)
    )
}

/// Their indexes, a row per column in order: schema, table, index, non-unique, column,
/// cardinality.
pub fn indexes_sql(tables: &[TableRef]) -> String {
    format!(
        "SELECT TABLE_SCHEMA, TABLE_NAME, INDEX_NAME, NON_UNIQUE, COLUMN_NAME, CARDINALITY \
         FROM information_schema.STATISTICS WHERE {} \
         ORDER BY TABLE_SCHEMA, TABLE_NAME, INDEX_NAME <> 'PRIMARY', INDEX_NAME, SEQ_IN_INDEX",
        wanted("TABLE_SCHEMA", "TABLE_NAME", tables)
    )
}

/// `performance_schema`'s reads per index (none: without one): schema, table, index, rows
/// fetched, read.
pub fn io_sql(tables: &[TableRef]) -> String {
    format!(
        "SELECT OBJECT_SCHEMA, OBJECT_NAME, INDEX_NAME, COUNT_FETCH, COUNT_READ \
         FROM performance_schema.table_io_waits_summary_by_index_usage WHERE {}",
        wanted("OBJECT_SCHEMA", "OBJECT_NAME", tables)
    )
}

fn text(row: &[Option<String>], i: usize) -> Option<String> {
    row.get(i).cloned().flatten()
}

fn count(row: &[Option<String>], i: usize) -> Option<u64> {
    text(row, i).and_then(|t| t.trim().parse().ok())
}

fn quantity(key: &str, value: Option<u64>, unit: Unit) -> Option<Extra> {
    value.map(|v| Extra::new(key, ExtraValue::Quantity(Quantity::new(v as f64, unit))))
}

/// Whether a row of `schema.name` is `table`'s (asked without a schema: by its name).
fn is(table: &TableRef, schema: Option<&str>, name: Option<&str>) -> bool {
    name.is_some_and(|n| n.eq_ignore_ascii_case(&table.name))
        && table.schema.as_deref().is_none_or(|s| schema.is_some_and(|r| r.eq_ignore_ascii_case(s)))
}

/// `tables`' statistics (each as it was asked: a profile's names) from the queries' rows.
pub fn assemble(
    tables: &[TableRef],
    rows: &[Vec<Option<String>>],
    indexes: &[Vec<Option<String>>],
    io: &[Vec<Option<String>>],
) -> Vec<TableStats> {
    let mut out = Vec::new();
    for table in tables {
        let Some(row) =
            rows.iter().find(|r| is(table, text(r, 0).as_deref(), text(r, 1).as_deref()))
        else {
            continue;
        };
        let mut stats = TableStats::new(table.clone());
        stats.live_rows = count(row, 3);
        stats.table_bytes = count(row, 4);
        stats.index_bytes = count(row, 5);
        stats.total_bytes = match (stats.table_bytes, stats.index_bytes) {
            (None, None) => None,
            (a, b) => Some(a.unwrap_or(0) + b.unwrap_or(0)),
        };
        stats.stats_present = Some(stats.live_rows.is_some());
        let mut extra: Vec<Extra> = Vec::new();
        for (i, key) in [
            (2, "engine"),
            (8, "update_time"),
            (9, "create_time"),
            (10, "collation"),
            (11, "row_format"),
        ] {
            if let Some(value) = text(row, i) {
                extra.push(Extra::new(key, ExtraValue::Text(value)));
            }
        }
        extra.extend(quantity("data_free", count(row, 6), Unit::Bytes));
        extra.extend(quantity("auto_increment", count(row, 7), Unit::Count));
        extra.extend(quantity("avg_row_length", count(row, 12), Unit::Bytes));
        let (schema, name) = (text(row, 0), text(row, 1));
        let mine = |r: &&Vec<Option<String>>| text(r, 0) == schema && text(r, 1) == name;
        for r in indexes.iter().filter(mine) {
            let index = text(r, 2).unwrap_or_default();
            let column = text(r, 4).unwrap_or_default();
            match stats.indexes.iter_mut().find(|i| i.name == index) {
                Some(found) => found.columns.push(column),
                None => {
                    stats.indexes.push(IndexStats {
                        name: index.clone(),
                        columns: vec![column],
                        unique: text(r, 3).as_deref() == Some("0"),
                        bytes: None,
                        scans: None,
                    });
                    extra.extend(quantity(
                        &format!("cardinality.{index}"),
                        count(r, 5),
                        Unit::Rows,
                    ));
                }
            }
        }
        for r in io.iter().filter(mine) {
            match text(r, 2) {
                Some(index) => extra.extend(quantity(
                    &format!("rows_fetched.{index}"),
                    count(r, 3),
                    Unit::Rows,
                )),
                None => extra.extend(quantity("rows_read_without_index", count(r, 4), Unit::Rows)),
            }
        }
        stats.extra = extra;
        out.push(stats);
    }
    out
}

#[cfg(test)]
#[path = "../tests/unit/stats.rs"]
mod tests;
