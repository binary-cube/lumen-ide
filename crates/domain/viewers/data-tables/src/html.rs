//! Tables in an HTML page: each `<table>` (a table inside a cell is a table of its own, not
//! part of its cell), its rows' `<th>` / `<td>` cells as text (tags left out, entities read,
//! blanks folded), a cell spanning `colspan` columns repeated in each; its header the `<th>`
//! row, else its first row when that's all text. Read leniently: real pages don't close what
//! they open.

use crate::{Cell, DataTable, Limits};

/// A table being read: its rows (as many as a table keeps, its header among them; the
/// others counted), the row and the cell open.
struct Open {
    rows: Vec<Vec<(String, bool)>>,
    left_out: usize,
    row: Option<Vec<(String, bool)>>,
    cell: Option<(String, bool, usize)>,
    limits: Limits,
}

impl Open {
    fn new(limits: Limits) -> Self {
        Self { rows: Vec::new(), left_out: 0, row: None, cell: None, limits }
    }

    /// The cell closed into its row: its text in each column it spans, as many as a table
    /// keeps (a hostile page's thousands of cells are left out as they come).
    fn close_cell(&mut self) {
        if let Some((text, head, span)) = self.cell.take() {
            let row = self.row.get_or_insert_with(Vec::new);
            let room = self.limits.columns.saturating_sub(row.len());
            if room == 0 {
                return;
            }
            let text = fold(&text);
            for _ in 0..span.clamp(1, room) {
                row.push((text.clone(), head));
            }
        }
    }

    fn close_row(&mut self) {
        self.close_cell();
        if let Some(row) = self.row.take().filter(|r| !r.is_empty()) {
            // The rows kept and one more (it may be the header).
            if self.rows.len() <= self.limits.rows {
                self.rows.push(row);
            } else {
                self.left_out = self.left_out.saturating_add(1);
            }
        }
    }
}

/// A table's rows as read: each cell's text and whether it's a header's.
type Rows = Vec<Vec<(String, bool)>>;

/// The most columns a cell spans.
const MAX_SPAN: usize = 64;

/// The tables of an HTML page, in the order they close.
pub fn tables(text: &str, limits: Limits) -> Vec<DataTable> {
    let mut stack: Vec<Open> = Vec::new();
    let mut done: Vec<(Rows, usize)> = Vec::new();
    // The tables opened past the depth, left out: their ends close nothing kept.
    let mut skipped: usize = 0;
    let mut rest = text;
    while let Some(lt) = rest.find('<') {
        let before = &rest[..lt];
        if let Some(cell) = stack.last_mut().and_then(|t| t.cell.as_mut()) {
            cell.0.push_str(before);
        }
        let after = &rest[lt + 1..];
        // A comment, a script, a style: skipped whole.
        if after.starts_with("!--") {
            rest = after.find("-->").map_or("", |e| &after[e + 3..]);
            continue;
        }
        let Some(gt) = after.find('>') else { break };
        let tag = &after[..gt];
        rest = &after[gt + 1..];
        let lower = tag.trim().to_ascii_lowercase();
        let closing = lower.starts_with('/');
        let name: String = lower
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        if !closing && (name == "script" || name == "style") {
            let end = format!("</{name}");
            // Found in the text as it is, its case ignored (never a copy of the rest).
            rest = find_ignoring_case(rest, &end).map_or("", |e| &rest[e..]);
            continue;
        }
        // Inside a table left out: its rows and cells aren't the table's around it (their
        // text reads into its cell).
        if skipped > 0 && matches!(name.as_str(), "tr" | "td" | "th") {
            continue;
        }
        match (name.as_str(), closing) {
            ("table", false) if stack.len() < limits.depth && skipped == 0 => {
                stack.push(Open::new(limits));
            }
            ("table", false) => skipped = skipped.saturating_add(1),
            ("table", true) if skipped > 0 => skipped -= 1,
            ("table", true) => {
                if let Some(mut t) = stack.pop() {
                    t.close_row();
                    if done.len() < limits.tables {
                        done.push((t.rows, t.left_out));
                    }
                }
            }
            ("tr", false) => {
                if let Some(t) = stack.last_mut() {
                    t.close_row();
                    t.row = Some(Vec::new());
                }
            }
            ("tr", true) => {
                if let Some(t) = stack.last_mut() {
                    t.close_row();
                }
            }
            ("td" | "th", false) => {
                if let Some(t) = stack.last_mut() {
                    t.close_cell();
                    let span = attribute(tag, "colspan")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(1)
                        .clamp(1, MAX_SPAN);
                    t.cell = Some((String::new(), name == "th", span));
                }
            }
            ("td" | "th", true) => {
                if let Some(t) = stack.last_mut() {
                    t.close_cell();
                }
            }
            // A line break inside a cell: a space.
            ("br" | "p" | "div", _) => {
                if let Some(cell) = stack.last_mut().and_then(|t| t.cell.as_mut()) {
                    cell.0.push(' ');
                }
            }
            _ => {}
        }
    }
    // What a page left open closes with it.
    while let Some(mut t) = stack.pop() {
        t.close_row();
        if done.len() < limits.tables {
            done.push((t.rows, t.left_out));
        }
    }
    done.into_iter()
        .enumerate()
        .filter_map(|(i, (rows, left_out))| {
            let mut table = table_of(&crate::table_named(i), rows, limits)?;
            table.left_out = table.left_out.saturating_add(left_out);
            Some(table)
        })
        .collect()
}

/// Where `needle` (ASCII) first is in `text`, its letters' case ignored.
fn find_ignoring_case(text: &str, needle: &str) -> Option<usize> {
    let needle = needle.as_bytes();
    text.as_bytes().windows(needle.len()).position(|w| w.eq_ignore_ascii_case(needle))
}

fn table_of(path: &str, mut rows: Vec<Vec<(String, bool)>>, limits: Limits) -> Option<DataTable> {
    if rows.is_empty() {
        return None;
    }
    let width = rows.iter().map(Vec::len).max().unwrap_or(0).min(limits.columns);
    let is_number = |t: &String| crate::infer::number(t, crate::infer::Marks::Plain).is_some();
    let header = rows[0].iter().all(|(_, head)| *head)
        || (rows.len() > 1
            && !rows[0].iter().any(|(t, _)| is_number(t))
            && rows[1].iter().any(|(t, _)| is_number(t)));
    let names: Vec<String> = if header {
        let first = rows.remove(0);
        (0..width)
            .map(|i| first.get(i).map_or_else(|| crate::column_named(i), |(t, _)| t.clone()))
            .collect()
    } else {
        (0..width).map(crate::column_named).collect()
    };
    let left_out = rows.len().saturating_sub(limits.rows);
    rows.truncate(limits.rows);
    let body = rows
        .into_iter()
        .map(|r| r.into_iter().take(width).map(|(t, _)| Cell::text(&t)).collect())
        .collect();
    let mut table = DataTable::new(path, names, body);
    table.left_out = left_out;
    Some(table)
}

/// A tag's attribute's value (`colspan="2"`, `colspan=2`).
fn attribute(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let at = lower.find(&format!("{name}="))? + name.len() + 1;
    let value = &tag[at..];
    let value = value.trim_start_matches(['"', '\'']);
    Some(value.chars().take_while(|c| c.is_ascii_alphanumeric()).collect())
}

/// A cell's text: tags left out, entities read, blanks folded to one space.
fn fold(raw: &str) -> String {
    let text = entities(raw);
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The usual entities read (`&amp;`, `&lt;`, `&nbsp;`, `&#39;`, `&#x27;`).
fn entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        let end = after.find(';').filter(|&e| e <= 10);
        let Some(end) = end else {
            out.push('&');
            rest = after;
            continue;
        };
        let name = &after[..end];
        let c = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ if name.starts_with("#x") => {
                u32::from_str_radix(&name[2..], 16).ok().and_then(char::from_u32)
            }
            _ if name.starts_with('#') => name[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match c {
            Some(c) => {
                out.push(c);
                rest = &after[end + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
#[path = "../tests/unit/html.rs"]
mod tests;
