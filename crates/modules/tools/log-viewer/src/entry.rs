//! One line of the log as the grid shows it: its level, its time (in the user's zone, to
//! the millisecond), module, thread, message, and its fields (with the spans it ran in)
//! as `name=value` pairs. A line that isn't the log's JSON (a line of the file of before) is
//! shown whole as its message. And whether a line passes the grid's filter, by the same
//! cells.

use serde_json::Value;
use ui_components::data_grid::filter::{Compare, GridFilter};
use ui_components::data_grid::source::CellValue;
use ui_components::{Block, BlockKind};

/// The columns, by the names the filter writes them with (the JSON's own keys): the level
/// first (what's looked for first), then when, where from, what.
pub const COLUMNS: [&str; 6] = ["level", "ts", "target", "thread", "msg", "fields"];

/// The level's column.
pub const LEVEL: usize = 0;
/// The message's column.
pub const MSG: usize = 4;

/// The longest a cell is shown in the grid (characters): the rest is in the row's details.
pub const CELL_MAX: usize = 300;

/// A line's cells, in [`COLUMNS`]' order, and the line itself (its details).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entry {
    pub cells: [String; 6],
    pub raw: String,
}

impl Entry {
    pub fn cell(&self, column: usize) -> &str {
        self.cells.get(column).map_or("", String::as_str)
    }

    /// Its cells as the grid shows them: none longer than [`CELL_MAX`] characters (cut at a
    /// character, with `…`); the line kept whole for its details.
    #[must_use]
    pub fn shortened(mut self) -> Self {
        for cell in &mut self.cells {
            if let Some((end, _)) = cell.char_indices().nth(CELL_MAX) {
                cell.truncate(end);
                cell.push('…');
            }
        }
        self
    }
}

/// What a line holds, for its details: a heading (its message whole), what it is (level,
/// time, module, thread, run), its fields each on its own, the spans it ran in. A line that
/// isn't the log's JSON: itself.
pub fn details(raw: &str) -> Vec<Block> {
    let Ok(Value::Object(object)) = serde_json::from_str::<Value>(raw) else {
        return vec![Block::new(BlockKind::Paragraph, raw)];
    };
    let text = |key: &str| object.get(key).and_then(Value::as_str).unwrap_or_default().to_owned();
    let mut blocks = vec![Block::new(BlockKind::Heading, text("msg"))];
    let about: Vec<String> = [
        text("level"),
        local_time(&text("ts")),
        text("target"),
        text("thread"),
        format!("run {}", text("run")),
    ]
    .into_iter()
    .filter(|s| !s.is_empty() && s != "run ")
    .collect();
    blocks.push(Block::new(BlockKind::Paragraph, about.join("  ·  ")));
    if let Some(Value::Object(fields)) = object.get("fields")
        && !fields.is_empty()
    {
        blocks.push(Block::new(BlockKind::Gap, ""));
        for (name, value) in fields {
            let value = match value {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            blocks.push(Block::new(BlockKind::Point { depth: 0 }, format!("{name}: {value}")));
        }
    }
    if let Some(Value::Array(spans)) = object.get("span")
        && !spans.is_empty()
    {
        blocks.push(Block::new(BlockKind::Gap, ""));
        for span in spans {
            let name = span.get("name").and_then(Value::as_str).unwrap_or_default();
            let mut line = format!("in {name}");
            if let Some(Value::Object(fields)) = span.get("fields") {
                for (k, v) in fields {
                    let v = v.as_str().map_or_else(|| v.to_string(), str::to_owned);
                    line.push_str(&format!("  {k}={v}"));
                }
            }
            blocks.push(Block::new(BlockKind::Point { depth: 0 }, line));
        }
    }
    blocks
}

/// `line` (a line of the file, without its newline) as its cells.
pub fn parse(line: &str) -> Entry {
    let Ok(Value::Object(object)) = serde_json::from_str::<Value>(line) else {
        let mut entry = Entry { raw: line.to_owned(), ..Entry::default() };
        entry.cells[MSG] = line.to_owned();
        return entry;
    };
    let text = |key: &str| object.get(key).and_then(Value::as_str).unwrap_or_default().to_owned();
    let mut fields = String::new();
    if let Some(Value::Object(map)) = object.get("fields") {
        for (name, value) in map {
            push_pair(&mut fields, name, value);
        }
    }
    if let Some(Value::Array(spans)) = object.get("span") {
        let names: Vec<&str> = spans.iter().filter_map(|s| s.get("name")?.as_str()).collect();
        if !names.is_empty() {
            push_pair(&mut fields, "in", &Value::String(names.join(" › ")));
        }
    }
    Entry {
        cells: [
            text("level"),
            local_time(&text("ts")),
            text("target"),
            text("thread"),
            text("msg"),
            fields,
        ],
        raw: line.to_owned(),
    }
}

/// `name=value` after what's there (a text as it is, anything else as JSON).
fn push_pair(out: &mut String, name: &str, value: &Value) {
    if !out.is_empty() {
        out.push_str("  ");
    }
    out.push_str(name);
    out.push('=');
    match value {
        Value::String(s) => out.push_str(s),
        other => out.push_str(&other.to_string()),
    }
}

/// An RFC 3339 moment in the user's zone, to the millisecond (`2026-11-20 10:35:21.388`);
/// as written when it can't be read.
pub fn local_time(ts: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(ts).map_or_else(
        |_| ts.to_owned(),
        |at| at.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
    )
}

/// Whether `entry` passes `filter` (`None`: every one does): its cells as text, a text
/// compared as written, `LIKE`'s "holds" in any case.
pub fn passes(entry: &Entry, filter: Option<&GridFilter>) -> bool {
    filter.is_none_or(|f| holds(entry, f))
}

fn holds(entry: &Entry, filter: &GridFilter) -> bool {
    let column = |name: &str| COLUMNS.iter().position(|c| c.eq_ignore_ascii_case(name));
    match filter {
        GridFilter::Compare { column: name, op, value } => {
            let Some(i) = column(name) else { return false };
            let cell = entry.cell(i);
            let value = text_of(value);
            let order = cell.cmp(value.as_str());
            match op {
                Compare::Eq => cell.eq_ignore_ascii_case(&value),
                Compare::NotEq => !cell.eq_ignore_ascii_case(&value),
                Compare::Less => order.is_lt(),
                Compare::LessEq => order.is_le(),
                Compare::Greater => order.is_gt(),
                Compare::GreaterEq => order.is_ge(),
            }
        }
        GridFilter::IsNull(name) => column(name).is_some_and(|i| entry.cell(i).is_empty()),
        GridFilter::NotNull(name) => column(name).is_some_and(|i| !entry.cell(i).is_empty()),
        GridFilter::Contains { column: name, text } => column(name)
            .is_some_and(|i| entry.cell(i).to_lowercase().contains(&text.to_lowercase())),
        GridFilter::And(parts) => parts.iter().all(|p| holds(entry, p)),
        GridFilter::Or(parts) => parts.iter().any(|p| holds(entry, p)),
        GridFilter::Not(part) => !holds(entry, part),
    }
}

/// A filter's value as text.
fn text_of(value: &CellValue) -> String {
    match value {
        CellValue::Null => String::new(),
        CellValue::Bool(b) => b.to_string(),
        CellValue::Int(i) => i.to_string(),
        CellValue::Float(f) => f.to_string(),
        CellValue::Text(s) | CellValue::Json(s) | CellValue::Other(s) => s.clone(),
        CellValue::Bytes(b) => String::from_utf8_lossy(b).into_owned(),
    }
}

#[cfg(test)]
#[path = "../tests/unit/entry.rs"]
mod tests;
