//! Tables in an XML document: an element repeated under one parent (`<book>` after
//! `<book>` in `<catalog>`) is a row each; its columns are its attributes (`@id`) and its
//! children holding only text (`title`), in the order first met; one holding only text is
//! its own `value`. Every such group, wherever it is, with its path (`/catalog/book`).

use roxmltree::{Document, Node, ParsingOptions};

use crate::{Cell, DataTable, Limits};

/// The tables of an XML document (none when it can't be read).
pub fn tables(text: &str, limits: Limits) -> Vec<DataTable> {
    let options = ParsingOptions { allow_dtd: false, ..ParsingOptions::default() };
    let Ok(doc) = Document::parse_with_options(text, options) else { return Vec::new() };
    let mut out = Vec::new();
    walk(
        doc.root_element(),
        format!("/{}", doc.root_element().tag_name().name()),
        limits,
        0,
        &mut out,
    );
    out
}

fn walk(node: Node<'_, '_>, path: String, limits: Limits, depth: usize, out: &mut Vec<DataTable>) {
    if depth > limits.depth || out.len() >= limits.tables {
        return;
    }
    // The children by name, in the order the names were first met (one pass: a hostile
    // file's many names never searched again and again).
    let mut places: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut named: Vec<(&str, Vec<Node<'_, '_>>)> = Vec::new();
    for child in node.children().filter(Node::is_element) {
        let name = child.tag_name().name();
        let at = *places.entry(name).or_insert_with(|| {
            named.push((name, Vec::new()));
            named.len() - 1
        });
        if let Some((_, children)) = named.get_mut(at) {
            children.push(child);
        }
    }
    for (name, children) in named {
        let child_path = format!("{path}/{name}");
        if children.len() >= 2
            && let Some(table) = table_of(&child_path, children.iter().copied(), limits)
        {
            out.push(table);
        }
        // Deeper: through the first of them (its siblings are alike).
        if let Some(first) = children.first() {
            walk(*first, child_path, limits, depth + 1, out);
        }
    }
}

/// Elements as rows: their attributes and text children the columns.
fn table_of<'a, 'i: 'a>(
    path: &str,
    rows: impl Iterator<Item = Node<'a, 'i>>,
    limits: Limits,
) -> Option<DataTable> {
    let mut names: Vec<String> = Vec::new();
    let mut cells: Vec<Vec<(usize, Cell)>> = Vec::new();
    let mut left_out = 0;
    let column = |names: &mut Vec<String>, name: String| -> Option<usize> {
        if let Some(i) = names.iter().position(|n| *n == name) {
            return Some(i);
        }
        (names.len() < limits.columns).then(|| {
            names.push(name);
            names.len() - 1
        })
    };
    for element in rows {
        if cells.len() >= limits.rows {
            left_out += 1;
            continue;
        }
        let mut row = Vec::new();
        for a in element.attributes() {
            if let Some(i) = column(&mut names, format!("@{}", a.name())) {
                row.push((i, Cell::text(a.value())));
            }
        }
        let children: Vec<Node<'_, '_>> = element.children().filter(Node::is_element).collect();
        if children.is_empty()
            && let Some(i) = column(&mut names, crate::value_named())
        {
            row.push((i, Cell::text(&text_of(element))));
        }
        for child in children {
            // A child with children of its own isn't a cell.
            if child.children().any(|c| c.is_element()) {
                continue;
            }
            if let Some(i) = column(&mut names, child.tag_name().name().to_owned()) {
                row.push((i, Cell::text(&text_of(child))));
            }
        }
        cells.push(row);
    }
    if names.is_empty() {
        return None;
    }
    let width = names.len();
    let rows = cells
        .into_iter()
        .map(|row| {
            let mut out = vec![Cell::Empty; width];
            for (i, cell) in row {
                out[i] = cell;
            }
            out
        })
        .collect();
    let mut table = DataTable::new(path, names, rows);
    table.left_out = left_out;
    Some(table)
}

fn text_of(node: Node<'_, '_>) -> String {
    node.children().filter(Node::is_text).filter_map(|t| t.text()).collect::<String>()
}
