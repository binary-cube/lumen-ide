//! A source's tables as rows: by schema and group, each table and, opened, its columns,
//! indexes and foreign keys.

use std::collections::{HashMap, HashSet};

use database::{DataSourceId, Structure, TableInfo, TableKind};
use ui_kit::ListFilter;

use super::rows::{push_word, status};
use super::{Group, Load, NodeKey, Part, Row, RowKind, RowMarks, SourceTree, TreeSource, Words};

impl SourceTree {
    /// A source's tables, by schema (a source without schemas: its groups right under it),
    /// under `parent` (the source's row, or its database's).
    pub(super) fn schema_rows(
        &self,
        source: &TreeSource,
        parent: &NodeKey,
        tables: &[TableInfo],
        filter: &ListFilter,
        words: &Words,
        out: &mut Vec<Row>,
    ) {
        let id = source.id;
        let filtering = !filter.is_empty();
        if tables.is_empty() {
            out.push(status(parent, 1, RowKind::Hint, &words.no_tables));
            return;
        }
        // The tables by schema, in the order the schemas first come, grouped in one pass
        // (thousands of tables in tens of schemas: not a pass over all of them per schema).
        let mut at: HashMap<Option<&str>, usize> = HashMap::new();
        let mut schemas: Vec<(Option<String>, Vec<&TableInfo>)> = Vec::new();
        for info in tables {
            let i = *at.entry(info.table.schema.as_deref()).or_insert_with(|| {
                schemas.push((info.table.schema.clone(), Vec::new()));
                schemas.len() - 1
            });
            schemas[i].1.push(info);
        }
        // Only those the source shows (none named: all).
        schemas.retain(|(s, _)| {
            source.schemas.is_empty() || s.as_ref().is_none_or(|s| source.schemas.contains(s))
        });
        let none = ListFilter::new("");
        for (schema, mine) in schemas {
            let mut children = Vec::new();
            // Named like the filter: its tables as without one.
            let named = filtering && schema.as_ref().is_some_and(|n| filter.matches(n));
            let (filter, filtering) = if named { (&none, false) } else { (filter, filtering) };
            let (depth, open) = match &schema {
                Some(name) => {
                    let key = NodeKey::Schema(id, name.clone());
                    (2, filtering || self.expanded.contains(&key))
                }
                None => (1, true),
            };
            if open {
                for (group, kind, label) in [
                    (Group::Tables, TableKind::Table, &words.tables),
                    (Group::Views, TableKind::View, &words.views),
                ] {
                    let listed: Vec<&TableInfo> =
                        mine.iter().copied().filter(|t| t.kind == kind).collect();
                    if listed.is_empty() {
                        continue;
                    }
                    let key = NodeKey::Group(id, schema.clone(), group);
                    let group_open = filtering || self.expanded.contains(&key);
                    let mut rows = Vec::new();
                    if group_open {
                        for info in &listed {
                            self.table_rows(id, info, depth + 1, filter, words, &mut rows);
                        }
                    }
                    if filtering && rows.is_empty() && !filter.matches(label) {
                        continue;
                    }
                    children.push(Row {
                        key,
                        depth,
                        kind: RowKind::Group(group),
                        label: label.clone(),
                        detail: String::new(),
                        count: Some(listed.len()),
                        expandable: true,
                        expanded: group_open,
                        color: None,
                        marks: RowMarks::default(),
                    });
                    children.extend(rows);
                }
            }
            match schema {
                Some(name) => {
                    if filtering && children.is_empty() && !filter.matches(&name) {
                        continue;
                    }
                    out.push(Row {
                        key: NodeKey::Schema(id, name.clone()),
                        depth: 1,
                        kind: RowKind::Schema,
                        label: name,
                        detail: String::new(),
                        count: Some(mine.len()),
                        expandable: true,
                        expanded: open,
                        color: None,
                        marks: RowMarks::default(),
                    });
                    out.extend(children);
                }
                None => out.extend(children),
            }
        }
    }

    /// A table and, opened, its structure's parts.
    pub(super) fn table_rows(
        &self,
        id: DataSourceId,
        info: &TableInfo,
        depth: u16,
        filter: &ListFilter,
        words: &Words,
        out: &mut Vec<Row>,
    ) {
        // Named like the filter: its parts as without one.
        let none = ListFilter::new("");
        let filter =
            if !filter.is_empty() && filter.matches(&info.table.name) { &none } else { filter };
        let filtering = !filter.is_empty();
        let key = NodeKey::Table(id, info.table.clone());
        let load = self.known.get(&id).and_then(|k| k.structures.get(&info.table));
        let structure = load.and_then(Load::ready);
        let open = self.expanded.contains(&key) || (filtering && structure.is_some());
        let mut children = Vec::new();
        if open {
            match load {
                Some(Load::Ready(s)) => {
                    self.part_rows(id, info, s, depth + 1, (filter, words), &mut children)
                }
                Some(Load::Failed(why)) => {
                    children.push(status(&key, depth + 1, RowKind::Error, why))
                }
                _ => children.push(status(&key, depth + 1, RowKind::Loading, &words.loading)),
            }
        }
        if filtering {
            children.retain(|r| !matches!(r.kind, RowKind::Loading | RowKind::Error));
            if children.is_empty() && !filter.matches(&info.table.name) {
                return;
            }
        }
        out.push(Row {
            key,
            depth,
            kind: RowKind::Table(info.kind),
            label: info.table.name.clone(),
            detail: info.rows.map(words.rows).unwrap_or_default(),
            count: structure.map(|s| s.columns.len()),
            expandable: true,
            expanded: open,
            color: None,
            marks: RowMarks::default(),
        });
        out.extend(children);
    }

    /// A table's columns, indexes and foreign keys (each part only when it has some).
    fn part_rows(
        &self,
        id: DataSourceId,
        info: &TableInfo,
        s: &Structure,
        depth: u16,
        (filter, words): (&ListFilter, &Words),
        out: &mut Vec<Row>,
    ) {
        let filtering = !filter.is_empty();
        let table = &info.table;
        let foreign: HashSet<&str> =
            s.foreign_keys.iter().flat_map(|k| k.columns.iter().map(String::as_str)).collect();
        let columns: Vec<Row> = s
            .columns
            .iter()
            .map(|c| {
                let mut detail = c.declared_type.clone();
                if !c.nullable && !c.in_key {
                    push_word(&mut detail, "NOT NULL");
                }
                Row {
                    key: NodeKey::Column(id, table.clone(), c.name.clone()),
                    depth: depth + 1,
                    kind: RowKind::Column {
                        key: c.in_key,
                        foreign: foreign.contains(c.name.as_str()),
                    },
                    label: c.name.clone(),
                    detail,
                    count: None,
                    expandable: false,
                    expanded: false,
                    color: None,
                    marks: RowMarks::default(),
                }
            })
            .collect();
        let indexes: Vec<Row> = s
            .indexes
            .iter()
            .map(|i| {
                let mut detail = format!("({})", i.columns.join(", "));
                if i.unique {
                    push_word(&mut detail, &words.unique);
                }
                Row {
                    key: NodeKey::Index(id, table.clone(), i.name.clone()),
                    depth: depth + 1,
                    kind: RowKind::Index { unique: i.unique },
                    label: i.name.clone(),
                    detail,
                    count: None,
                    expandable: false,
                    expanded: false,
                    color: None,
                    marks: RowMarks::default(),
                }
            })
            .collect();
        let keys: Vec<Row> = s
            .foreign_keys
            .iter()
            .enumerate()
            .map(|(n, k)| Row {
                key: NodeKey::Key(id, table.clone(), n),
                depth: depth + 1,
                kind: RowKind::Key,
                label: k.name.clone().unwrap_or_else(|| k.columns.join(", ")),
                detail: format!("→ {} ({})", k.table.name, k.references.join(", ")),
                count: None,
                expandable: false,
                expanded: false,
                color: None,
                marks: RowMarks::default(),
            })
            .collect();
        for (part, label, rows) in [
            (Part::Columns, &words.columns, columns),
            (Part::Indexes, &words.indexes, indexes),
            (Part::Keys, &words.keys, keys),
        ] {
            if rows.is_empty() {
                continue;
            }
            let key = NodeKey::Part(id, table.clone(), part);
            let total = rows.len();
            let shown: Vec<Row> = if filtering {
                rows.into_iter().filter(|r| filter.matches(&r.label)).collect()
            } else {
                rows
            };
            if filtering && shown.is_empty() {
                continue;
            }
            let open = filtering || self.expanded.contains(&key);
            out.push(Row {
                key,
                depth,
                kind: RowKind::Part(part),
                label: label.clone(),
                detail: String::new(),
                count: Some(total),
                expandable: true,
                expanded: open,
                color: None,
                marks: RowMarks::default(),
            });
            if open {
                out.extend(shown);
            }
        }
    }
}
