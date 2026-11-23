//! Tables in a tree of values (JSON, YAML: read into one [`Node`] tree, so both are walked
//! the same): a list of records (its columns the union of their keys, nested records
//! flattened as `customer.city`), a list of lists (its first row a header when it's all
//! text and the rest isn't), a record of records (its keys a column of their own). Every
//! one found, wherever it is, with its path (`$.orders`, `$.data[0].rows`).

use crate::{Cell, DataTable, Limits};

/// A value of a tree.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Null,
    Bool(bool),
    Number(f64),
    Text(String),
    List(Vec<Node>),
    Map(Vec<(String, Node)>),
}

/// The tables of a JSON document (a JSON Lines one too: an object a line); its keys in the
/// order written (read straight into a [`Node`], never through a map that sorts them).
pub fn json(text: &str, limits: Limits) -> Vec<DataTable> {
    let root = match serde_json::from_str::<Node>(text) {
        Ok(node) => node,
        Err(_) => {
            // JSON Lines: each line a value, together a list.
            let lines: Option<Vec<Node>> = text
                .lines()
                .filter(|l| !l.trim().is_empty())
                .take(limits.rows)
                .map(|l| serde_json::from_str::<Node>(l).ok())
                .collect();
            match lines {
                Some(items) if !items.is_empty() => Node::List(items),
                _ => return Vec::new(),
            }
        }
    };
    found(&root, limits)
}

/// The tables of a YAML document (its first document; several: each one's).
pub fn yaml(text: &str, limits: Limits) -> Vec<DataTable> {
    // Aliases copy what they name: a small file nesting them would grow without bound
    // while loaded. Its size once expanded is counted first, by its events (linear).
    if expanded_yaml_nodes(text) > MOST_YAML_NODES {
        return Vec::new();
    }
    let Ok(docs) = yaml_rust2::YamlLoader::load_from_str(text) else { return Vec::new() };
    let nodes: Vec<Node> = docs.iter().map(|d| from_yaml(d, limits.depth)).collect();
    let root = if nodes.len() == 1 {
        nodes.into_iter().next().unwrap_or(Node::Null)
    } else {
        Node::List(nodes)
    };
    found(&root, limits)
}

/// The most nodes a YAML document may hold once its aliases are expanded (a table of rows
/// by columns fits well under it).
const MOST_YAML_NODES: u64 = 4_000_000;

/// How many nodes a YAML text holds once its aliases are expanded (each alias counting
/// what it names), counted from its events without expanding anything; past
/// [`MOST_YAML_NODES`] the count stops growing exactly.
fn expanded_yaml_nodes(text: &str) -> u64 {
    use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser};
    use yaml_rust2::scanner::Marker;

    /// The nodes counted: each open collection's (its anchor, its count so far), each
    /// anchor's size, the document's whole.
    #[derive(Default)]
    struct Count {
        open: Vec<(usize, u64)>,
        anchors: std::collections::HashMap<usize, u64>,
        total: u64,
    }

    impl Count {
        /// A node of `size` done (anchored as `anchor`, 0 for none), added to its parent.
        fn done(&mut self, anchor: usize, size: u64) {
            if anchor > 0 {
                self.anchors.insert(anchor, size);
            }
            match self.open.last_mut() {
                Some((_, count)) => *count = count.saturating_add(size),
                None => self.total = self.total.saturating_add(size),
            }
        }
    }

    impl MarkedEventReceiver for Count {
        fn on_event(&mut self, event: Event, _: Marker) {
            match event {
                Event::Scalar(_, _, anchor, _) => self.done(anchor, 1),
                Event::Alias(id) => {
                    let size = self.anchors.get(&id).copied().unwrap_or(1);
                    self.done(0, size);
                }
                Event::SequenceStart(anchor, _) | Event::MappingStart(anchor, _) => {
                    self.open.push((anchor, 1));
                }
                Event::SequenceEnd | Event::MappingEnd => {
                    if let Some((anchor, size)) = self.open.pop() {
                        self.done(anchor, size);
                    }
                }
                _ => {}
            }
        }
    }

    let mut count = Count::default();
    // A text that doesn't parse is refused by the loader after.
    let _ = Parser::new_from_str(text).load(&mut count, true);
    count.total
}

impl<'de> serde::Deserialize<'de> for Node {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_any(NodeVisitor)
    }
}

/// Reads any JSON value as a [`Node`], a record's keys in their order (the parser's own
/// nesting limit bounds how deep).
struct NodeVisitor;

impl<'de> serde::de::Visitor<'de> for NodeVisitor {
    type Value = Node;

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a JSON value")
    }

    fn visit_unit<E>(self) -> Result<Node, E> {
        Ok(Node::Null)
    }

    fn visit_bool<E>(self, b: bool) -> Result<Node, E> {
        Ok(Node::Bool(b))
    }

    fn visit_i64<E>(self, n: i64) -> Result<Node, E> {
        Ok(Node::Number(n as f64))
    }

    fn visit_u64<E>(self, n: u64) -> Result<Node, E> {
        Ok(Node::Number(n as f64))
    }

    fn visit_f64<E>(self, n: f64) -> Result<Node, E> {
        Ok(if n.is_finite() { Node::Number(n) } else { Node::Null })
    }

    fn visit_str<E>(self, s: &str) -> Result<Node, E> {
        Ok(Node::Text(s.to_owned()))
    }

    fn visit_string<E>(self, s: String) -> Result<Node, E> {
        Ok(Node::Text(s))
    }

    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Node, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element::<Node>()? {
            items.push(item);
        }
        Ok(Node::List(items))
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Node, A::Error> {
        let mut entries = Vec::new();
        while let Some((k, v)) = map.next_entry::<String, Node>()? {
            entries.push((k, v));
        }
        Ok(Node::Map(entries))
    }
}

fn from_yaml(v: &yaml_rust2::Yaml, depth: usize) -> Node {
    use yaml_rust2::Yaml;
    if depth == 0 {
        return Node::Null;
    }
    match v {
        Yaml::Real(r) => r.parse().map_or_else(|_| Node::Text(r.clone()), Node::Number),
        Yaml::Integer(i) => Node::Number(*i as f64),
        Yaml::String(s) => Node::Text(s.clone()),
        Yaml::Boolean(b) => Node::Bool(*b),
        Yaml::Array(a) => Node::List(a.iter().map(|x| from_yaml(x, depth - 1)).collect()),
        Yaml::Hash(h) => Node::Map(
            h.iter()
                .map(|(k, x)| {
                    let key = match k {
                        Yaml::String(s) | Yaml::Real(s) => s.clone(),
                        Yaml::Integer(i) => i.to_string(),
                        Yaml::Boolean(b) => b.to_string(),
                        _ => String::new(),
                    };
                    (key, from_yaml(x, depth - 1))
                })
                .collect(),
        ),
        _ => Node::Null,
    }
}

/// Every table under `root`.
pub fn found(root: &Node, limits: Limits) -> Vec<DataTable> {
    let mut out = Vec::new();
    walk(root, "$".to_owned(), limits, 0, &mut out);
    out
}

fn walk(node: &Node, path: String, limits: Limits, depth: usize, out: &mut Vec<DataTable>) {
    if depth > limits.depth || out.len() >= limits.tables {
        return;
    }
    if let Some(table) = as_table(node, &path, limits) {
        out.push(table);
    }
    match node {
        Node::List(items) => {
            // A list of records' records aren't walked one by one (their lists are, through
            // the first that has one).
            for (i, item) in items.iter().enumerate().take(limits.tables) {
                walk(item, format!("{path}[{i}]"), limits, depth + 1, out);
            }
        }
        Node::Map(entries) => {
            for (k, v) in entries {
                walk(v, format!("{path}.{k}"), limits, depth + 1, out);
            }
        }
        _ => {}
    }
}

/// `node` as a table, if it's one.
fn as_table(node: &Node, path: &str, limits: Limits) -> Option<DataTable> {
    match node {
        Node::List(items)
            if items.len() >= 2 || items.iter().any(|i| matches!(i, Node::Map(_))) =>
        {
            let records = items.iter().filter(|i| matches!(i, Node::Map(_))).count();
            if records * 2 >= items.len() && records > 0 {
                return Some(records_table(path, items.iter().map(|i| (None, i)), limits));
            }
            let lists = items.iter().filter(|i| matches!(i, Node::List(_))).count();
            if lists == items.len() {
                return Some(rows_table(path, items, limits));
            }
            None
        }
        // A record of records (`{"ro": {...}, "de": {...}}`): its keys a column.
        Node::Map(entries)
            if entries.len() >= 2 && entries.iter().all(|(_, v)| matches!(v, Node::Map(_))) =>
        {
            Some(records_table(path, entries.iter().map(|(k, v)| (Some(k.as_str()), v)), limits))
        }
        _ => None,
    }
}

/// Records as rows: their flattened keys the columns (in the order first met).
fn records_table<'a>(
    path: &str,
    records: impl Iterator<Item = (Option<&'a str>, &'a Node)>,
    limits: Limits,
) -> DataTable {
    let mut names: Vec<String> = Vec::new();
    let mut rows: Vec<Vec<(usize, Cell)>> = Vec::new();
    let mut keyed = false;
    let mut left_out = 0;
    for (key, record) in records {
        if rows.len() >= limits.rows {
            left_out += 1;
            continue;
        }
        let mut cells = Vec::new();
        if let Some(key) = key {
            keyed = true;
            cells.push((usize::MAX, Cell::text(key)));
        }
        let mut flat = Vec::new();
        flatten(record, String::new(), &mut flat, 4);
        for (name, cell) in flat {
            let at = match names.iter().position(|n| *n == name) {
                Some(i) => i,
                None if names.len() < limits.columns => {
                    names.push(name);
                    names.len() - 1
                }
                None => continue,
            };
            cells.push((at, cell));
        }
        rows.push(cells);
    }
    let offset = usize::from(keyed);
    let mut all_names = Vec::with_capacity(names.len() + offset);
    if keyed {
        all_names.push(crate::key_named());
    }
    all_names.extend(names);
    let width = all_names.len();
    let rows = rows
        .into_iter()
        .map(|cells| {
            let mut row = vec![Cell::Empty; width];
            for (at, cell) in cells {
                let i = if at == usize::MAX { 0 } else { at + offset };
                if let Some(slot) = row.get_mut(i) {
                    *slot = cell;
                }
            }
            row
        })
        .collect();
    let mut table = DataTable::new(path, all_names, rows);
    table.left_out = left_out;
    table
}

/// A record's values by their flattened names (`customer.city`); a list inside is said by
/// its length, a leaf as itself.
fn flatten(node: &Node, prefix: String, out: &mut Vec<(String, Cell)>, depth: usize) {
    match node {
        Node::Map(entries) if depth > 0 => {
            for (k, v) in entries {
                let name = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                flatten(v, name, out, depth - 1);
            }
        }
        Node::Map(_) => {}
        leaf => out
            .push((if prefix.is_empty() { crate::value_named() } else { prefix }, leaf_cell(leaf))),
    }
}

fn leaf_cell(node: &Node) -> Cell {
    match node {
        Node::Null => Cell::Empty,
        Node::Bool(b) => Cell::Number(f64::from(u8::from(*b))),
        Node::Number(n) => Cell::Number(*n),
        Node::Text(t) => Cell::text(t),
        Node::List(items) => Cell::Number(items.len() as f64),
        Node::Map(_) => Cell::Empty,
    }
}

/// Lists as rows: the first a header when it's all text and the next isn't.
fn rows_table(path: &str, items: &[Node], limits: Limits) -> DataTable {
    let rows: Vec<Vec<Cell>> = items
        .iter()
        .take(limits.rows + 1)
        .filter_map(|i| {
            if let Node::List(cells) = i {
                Some(cells.iter().take(limits.columns).map(leaf_cell).collect())
            } else {
                None
            }
        })
        .collect();
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    let header = rows.first().is_some_and(|r| r.iter().all(|c| matches!(c, Cell::Text(_))))
        && rows.get(1).is_some_and(|r| r.iter().any(|c| matches!(c, Cell::Number(_))));
    let (names, body) = if header {
        let names = rows[0].iter().map(|c| match c {
            Cell::Text(t) => t.clone(),
            _ => String::new(),
        });
        (names.chain((rows[0].len()..width).map(crate::column_named)).collect(), rows[1..].to_vec())
    } else {
        ((0..width).map(crate::column_named).collect(), rows)
    };
    let mut body = body;
    body.truncate(limits.rows);
    DataTable::new(path, names, body)
}

#[cfg(test)]
#[path = "../tests/unit/tree.rs"]
mod tests;
