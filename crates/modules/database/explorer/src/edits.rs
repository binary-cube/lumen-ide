//! A table's pending changes ([`Edits`], the domain's: a console's result is edited by the
//! same rules) and, for a source that writes no SQL (a CSV file), what they'd do in words.

use database::{Change, ChangeSet, RowKey, Value};

pub use database::edits::{Edits, Inserted, Updated};

/// What `set` would do, for a source that writes no SQL (a CSV file): a line per change,
/// for the preview.
pub fn summary(set: &ChangeSet) -> Vec<String> {
    let key = |k: &RowKey| match k {
        RowKey::Line { line, .. } => i18n::tr!("database.summary_line", line = line + 1),
        RowKey::RowId(id) => format!("rowid {id}"),
        RowKey::Primary(columns) => columns
            .iter()
            .map(|(c, v)| format!("{c} = {}", v.display()))
            .collect::<Vec<_>>()
            .join(", "),
    };
    set.changes
        .iter()
        .map(|change| match change {
            Change::Delete { key: k } => i18n::tr!("database.summary_delete", row = key(k)),
            Change::Update { key: k, cells } => {
                let cells: Vec<String> = cells
                    .iter()
                    .map(|c| format!("{}: {} → {}", c.column, shown(&c.old), shown(&c.new)))
                    .collect();
                i18n::tr!("database.summary_update", row = key(k), cells = cells.join("; "))
            }
            Change::Insert { values } => {
                let values: Vec<String> =
                    values.iter().map(|(c, v)| format!("{c} = {}", shown(v))).collect();
                i18n::tr!("database.summary_insert", values = values.join(", "))
            }
        })
        .collect()
}

fn shown(v: &Value) -> String {
    match v {
        Value::Null => "NULL".to_owned(),
        Value::Text(t) => format!("'{t}'"),
        other => other.display(),
    }
}

#[cfg(test)]
#[path = "../tests/unit/edits.rs"]
mod tests;
