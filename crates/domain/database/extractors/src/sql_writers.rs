//! The SQL extractors, through the source's [`crate::SqlStyle`]: `INSERT`s a row a statement
//! or many rows a statement (a new statement every [`MULTIROW_ROWS`]), `UPDATE`s by the key
//! (every column when there's none among them) and a `WHERE` clause matching the rows (`id IN
//! (…)` for one column, the rows' conditions joined by `OR` for more).

use std::sync::Arc;

use crate::cell::{Cell, Column};
use crate::extractor::{Extractor, Group, Input, Lines, Stream};
use crate::sql::SqlStyle;

/// Rows a multi-row `INSERT` holds before another starts (an engine's limits stay far).
pub const MULTIROW_ROWS: u64 = 1000;

/// `INSERT INTO t (a, b) VALUES (…);` — a row a statement, or many (`multirow`).
#[derive(Debug, Clone, Copy)]
pub struct SqlInserts {
    pub multirow: bool,
}

impl Extractor for SqlInserts {
    fn id(&self) -> &str {
        if self.multirow { "sql-multirow" } else { "sql" }
    }

    fn name(&self) -> String {
        i18n::t(if self.multirow { "extract.sql_multirow" } else { "extract.sql_inserts" })
    }

    fn group(&self) -> Group {
        Group::Sql
    }

    fn extension(&self) -> &str {
        "sql"
    }

    fn skips_columns(&self) -> bool {
        true
    }

    fn open(&self, input: &Input) -> Box<dyn Stream> {
        let names: Vec<String> = input.columns.iter().map(|c| input.sql.name(&c.name)).collect();
        Box::new(InsertStream {
            head: format!("INSERT INTO {} ({})", input.sql.name(&input.table), names.join(", ")),
            sql: Arc::clone(&input.sql),
            multirow: self.multirow,
            lines: Lines::new("\n"),
            in_statement: 0,
        })
    }
}

struct InsertStream {
    head: String,
    sql: Arc<dyn SqlStyle>,
    multirow: bool,
    lines: Lines,
    /// Rows of the multi-row statement open now.
    in_statement: u64,
}

fn values(sql: &dyn SqlStyle, row: &[Cell<'_>]) -> String {
    let values: Vec<String> = row.iter().map(|v| sql.literal(*v)).collect();
    format!("({})", values.join(", "))
}

impl Stream for InsertStream {
    fn rows(&mut self, rows: &[Vec<Cell<'_>>], out: &mut String) {
        for row in rows {
            let values = values(self.sql.as_ref(), row);
            if !self.multirow {
                self.lines.push(out, &format!("{} VALUES {values};", self.head));
                continue;
            }
            if self.in_statement == MULTIROW_ROWS {
                out.push(';');
                self.in_statement = 0;
            }
            if self.in_statement == 0 {
                self.lines.push(out, &self.head.clone());
                self.lines.push(out, &format!("VALUES {values}"));
            } else {
                out.push(',');
                self.lines.push(out, &format!("       {values}"));
            }
            self.in_statement += 1;
        }
    }

    fn finish(&mut self, out: &mut String) {
        if self.multirow && self.in_statement > 0 {
            out.push(';');
        }
    }
}

/// `name = literal`, or `name IS NULL`.
fn condition(sql: &dyn SqlStyle, column: &Column, value: Cell<'_>) -> String {
    let name = sql.name(&column.name);
    if value.is_null() {
        format!("{name} IS NULL")
    } else {
        format!("{name} = {}", sql.literal(value))
    }
}

/// The columns rows are found by: the key's among them, else all of them.
fn finding(columns: &[Column]) -> Vec<usize> {
    let keyed: Vec<usize> = (0..columns.len()).filter(|&i| columns[i].key).collect();
    if keyed.is_empty() { (0..columns.len()).collect() } else { keyed }
}

/// `UPDATE t SET b = … WHERE id = …;` for each row: the key's columns found by, the others
/// set (all of them when every column is the key's, or there's no key).
#[derive(Debug, Clone, Copy)]
pub struct SqlUpdates;

impl Extractor for SqlUpdates {
    fn id(&self) -> &str {
        "sql-update"
    }

    fn name(&self) -> String {
        i18n::t("extract.sql_updates")
    }

    fn group(&self) -> Group {
        Group::Sql
    }

    fn extension(&self) -> &str {
        "sql"
    }

    fn skips_columns(&self) -> bool {
        true
    }

    fn open(&self, input: &Input) -> Box<dyn Stream> {
        let found_by = finding(&input.columns);
        let mut set: Vec<usize> =
            (0..input.columns.len()).filter(|&i| !input.columns[i].key).collect();
        if set.is_empty() {
            set = (0..input.columns.len()).collect();
        }
        Box::new(UpdateStream {
            table: input.sql.name(&input.table),
            columns: input.columns.clone(),
            sql: Arc::clone(&input.sql),
            found_by,
            set,
            lines: Lines::new("\n"),
        })
    }
}

struct UpdateStream {
    table: String,
    columns: Vec<Column>,
    sql: Arc<dyn SqlStyle>,
    found_by: Vec<usize>,
    set: Vec<usize>,
    lines: Lines,
}

impl Stream for UpdateStream {
    fn rows(&mut self, rows: &[Vec<Cell<'_>>], out: &mut String) {
        let sql = self.sql.as_ref();
        for row in rows {
            let value = |i: usize| row.get(i).copied().unwrap_or(Cell::Null);
            let set: Vec<String> = self
                .set
                .iter()
                .map(|&i| {
                    format!("{} = {}", sql.name(&self.columns[i].name), sql.literal(value(i)))
                })
                .collect();
            let found: Vec<String> =
                self.found_by.iter().map(|&i| condition(sql, &self.columns[i], value(i))).collect();
            self.lines.push(
                out,
                &format!(
                    "UPDATE {} SET {} WHERE {};",
                    self.table,
                    set.join(", "),
                    found.join(" AND ")
                ),
            );
        }
    }
}

/// A `WHERE` clause's condition matching the rows (without the word `WHERE`): `id IN (1, 2)`
/// for one column found by (`OR id IS NULL` with a `NULL` among them), else each row's
/// conditions, a row a line, joined by `OR`.
#[derive(Debug, Clone, Copy)]
pub struct WhereClause;

impl Extractor for WhereClause {
    fn id(&self) -> &str {
        "sql-where"
    }

    fn name(&self) -> String {
        i18n::t("extract.sql_where")
    }

    fn group(&self) -> Group {
        Group::Sql
    }

    fn extension(&self) -> &str {
        "sql"
    }

    fn open(&self, input: &Input) -> Box<dyn Stream> {
        Box::new(WhereStream {
            columns: input.columns.clone(),
            found_by: finding(&input.columns),
            sql: Arc::clone(&input.sql),
            values: 0,
            null: false,
            rows: 0,
        })
    }
}

struct WhereStream {
    columns: Vec<Column>,
    found_by: Vec<usize>,
    sql: Arc<dyn SqlStyle>,
    /// One column: the values written into `IN (…)` so far, and whether a `NULL` came.
    values: u64,
    null: bool,
    /// More columns: the rows written so far.
    rows: u64,
}

impl Stream for WhereStream {
    fn rows(&mut self, rows: &[Vec<Cell<'_>>], out: &mut String) {
        let sql = self.sql.as_ref();
        if let [only] = self.found_by[..] {
            for row in rows {
                let value = row.get(only).copied().unwrap_or(Cell::Null);
                if value.is_null() {
                    self.null = true;
                    continue;
                }
                if self.values == 0 {
                    out.push_str(&sql.name(&self.columns[only].name));
                    out.push_str(" IN (");
                } else {
                    out.push_str(", ");
                }
                out.push_str(&sql.literal(value));
                self.values += 1;
            }
            return;
        }
        for row in rows {
            let parts: Vec<String> = self
                .found_by
                .iter()
                .map(|&i| {
                    condition(sql, &self.columns[i], row.get(i).copied().unwrap_or(Cell::Null))
                })
                .collect();
            if self.rows > 0 {
                out.push_str("\nOR ");
            }
            out.push('(');
            out.push_str(&parts.join(" AND "));
            out.push(')');
            self.rows += 1;
        }
    }

    fn finish(&mut self, out: &mut String) {
        let [only] = self.found_by[..] else { return };
        let name = self.sql.name(&self.columns[only].name);
        match (self.values, self.null) {
            (0, true) => out.push_str(&format!("{name} IS NULL")),
            (0, false) => {}
            (_, true) => out.push_str(&format!(") OR {name} IS NULL")),
            (_, false) => out.push(')'),
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/sql_writers.rs"]
mod tests;
