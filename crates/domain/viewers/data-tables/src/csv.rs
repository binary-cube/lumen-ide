//! A delimited text as a table (RFC 4180: a field in quotes may hold the delimiter, a line
//! break, a doubled quote): its first line the header when it's all text and the next
//! isn't, else columns numbered.

use crate::{Cell, DataTable, Limits};

/// `text` split by `delimiter` (none when it holds no line).
pub fn table(text: &str, delimiter: u8, limits: Limits) -> Option<DataTable> {
    let (mut records, skipped) =
        records(text.trim_start_matches('\u{feff}'), delimiter as char, limits);
    if records.is_empty() {
        return None;
    }
    let width = records.iter().map(Vec::len).max().unwrap_or(0).min(limits.columns);
    let is_number = |f: &String| crate::infer::number(f, crate::infer::Marks::Plain).is_some();
    let header = records.len() > 1
        && !records[0].iter().any(is_number)
        && (records[1].iter().any(is_number) || records[0].iter().all(|f| !f.trim().is_empty()));
    let names: Vec<String> = if header {
        let first = records.remove(0);
        (0..width)
            .map(|i| first.get(i).map_or_else(|| crate::column_named(i), |f| f.trim().to_owned()))
            .collect()
    } else {
        (0..width).map(crate::column_named).collect()
    };
    let left_out = records.len().saturating_sub(limits.rows) + skipped;
    records.truncate(limits.rows);
    let rows = records
        .into_iter()
        .map(|r| r.iter().take(width).map(|f| Cell::text(f)).collect())
        .collect();
    let mut table = DataTable::new("table", names, rows);
    table.left_out = left_out;
    Some(table)
}

/// The most bytes a field keeps (a cell shows far less; a hostile field can't grow).
const MOST_FIELD: usize = 64 * 1024;

/// Its records, each a list of fields (blank lines left out; past the columns a table
/// keeps, fields read but not kept), and how many past the limit were only counted.
fn records(text: &str, d: char, limits: Limits) -> (Vec<Vec<String>>, usize) {
    let mut out = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    let mut skipped = 0;
    // A header and the rows kept; past them, records only counted (nothing kept).
    let keep = limits.rows.saturating_add(1);
    while let Some(c) = chars.next() {
        if out.len() >= keep {
            match c {
                '"' => quoted = !quoted,
                '\n' if !quoted => skipped += 1,
                _ => {}
            }
            continue;
        }
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                chars.next();
                field.push('"');
            }
            '"' if quoted => quoted = false,
            '"' if field.trim().is_empty() => {
                field.clear();
                quoted = true;
            }
            c if c == d && !quoted => {
                if record.len() < limits.columns {
                    record.push(std::mem::take(&mut field));
                } else {
                    field.clear();
                }
            }
            '\r' if !quoted => {}
            '\n' if !quoted => {
                if record.len() < limits.columns {
                    record.push(std::mem::take(&mut field));
                } else {
                    field.clear();
                }
                if !(record.len() == 1 && record[0].trim().is_empty()) {
                    out.push(std::mem::take(&mut record));
                } else {
                    record.clear();
                }
            }
            c if field.len() < MOST_FIELD => field.push(c),
            _ => {}
        }
    }
    if out.len() < keep && (!field.is_empty() || !record.is_empty()) {
        if record.len() < limits.columns {
            record.push(field);
        }
        out.push(record);
    }
    (out, skipped)
}

#[cfg(test)]
#[path = "../tests/unit/csv.rs"]
mod tests;
