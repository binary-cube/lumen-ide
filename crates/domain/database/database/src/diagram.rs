//! The relations between a source's tables, as a diagram shows them: each table with its
//! columns and keys ([`GraphTable`]), and the links between them ([`Link`]) — from their
//! foreign keys, with the columns that join them and how many rows meet on each side
//! ([`Cardinality`]). A table a key names but that wasn't read (another schema's) is there
//! too, marked [`GraphTable::external`], with the columns named.
//!
//! The diagram is a view of its own module; the explorer, which owns the sessions, opens it
//! with a [`DiagramRequest`]: the source's structure read through [`SchemaAccess`] (the
//! port it implements), the project's bus to follow, where to start. Neither module knows
//! the other: this is the whole of what they share.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use kernel::EventBus;

use crate::relations::{Candidate, Confidence, RelationKinds, RelationNotes, VirtualKey};
use crate::{Cancel, ColumnInfo, DataSourceId, DbError, Structure, TableInfo, TableKind, TableRef};

/// The kind of view the diagram opens as (the modules' shared name for it).
pub const VIEW_EXTENSION: &str = "view-data-diagram";

/// How many rows of each side meet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cardinality {
    /// A row of the target has at most one row pointing to it (the key's columns are
    /// unique: the table's primary key, a unique index).
    OneToOne,
    /// A row of the target has any number pointing to it.
    OneToMany,
}

/// Where a link comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Origin {
    /// A foreign key of the engine.
    Real,
    /// A key the user accepted (kept with the data source, not by the engine).
    Virtual,
    /// Found from the columns' names ([`crate::relations`]), this sure.
    Inferred(Confidence),
}

impl Origin {
    /// Shown when the diagram shows `kinds`.
    pub fn shown(self, kinds: RelationKinds) -> bool {
        matches!(
            (self, kinds),
            (_, RelationKinds::Both)
                | (Self::Virtual, _)
                | (Self::Real, RelationKinds::Real)
                | (Self::Inferred(_), RelationKinds::Inferred)
        )
    }
}

/// `columns` of table `from` point to `references` of table `to` (indexes in
/// [`SchemaGraph::tables`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub from: usize,
    pub columns: Vec<String>,
    pub to: usize,
    pub references: Vec<String>,
    pub cardinality: Cardinality,
    pub origin: Origin,
    /// The key's name, when it has one.
    pub name: Option<String>,
}

impl Link {
    /// A table pointing to itself (a tree: `parent_id`).
    pub fn is_self(&self) -> bool {
        self.from == self.to
    }
}

/// A table of the diagram.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphTable {
    pub structure: Structure,
    pub kind: TableKind,
    /// The engine's estimate of its rows.
    pub rows: Option<u64>,
    /// Only named by another table's key (not read: another schema's, gone): its columns
    /// are those named.
    pub external: bool,
}

impl GraphTable {
    pub fn table(&self) -> &TableRef {
        &self.structure.table
    }

    /// `columns` (in any order) are unique: its primary key, or a unique index's.
    pub fn is_unique(&self, columns: &[String]) -> bool {
        let same = |set: &[String]| {
            !set.is_empty()
                && set.len() == columns.len()
                && set.iter().all(|c| columns.iter().any(|d| d.eq_ignore_ascii_case(c)))
        };
        same(&self.structure.primary_key())
            || self
                .structure
                .indexes
                .iter()
                .any(|i| i.unique && i.predicate.is_none() && same(&i.columns))
    }
}

/// A source's tables and the links between them (see the module's doc).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SchemaGraph {
    /// By schema, then name.
    pub tables: Vec<GraphTable>,
    /// By table, then the order of their keys.
    pub links: Vec<Link>,
}

impl SchemaGraph {
    /// The graph of `tables` (each as listed, with its structure): their foreign keys as
    /// links, a target not among them added as an external table.
    pub fn new(tables: Vec<(TableInfo, Structure)>) -> Self {
        let mut tables: Vec<GraphTable> = tables
            .into_iter()
            .map(|(info, mut structure)| {
                structure.table = info.table;
                GraphTable { structure, kind: info.kind, rows: info.rows, external: false }
            })
            .collect();
        tables.sort_by(|a, b| a.table().cmp(b.table()));
        tables.dedup_by(|a, b| a.table() == b.table());
        let mut graph = Self { tables, links: Vec::new() };
        // The tables by name (lowercased), in their order: each key looks only at those of
        // its name, not at every table (thousands of keys, thousands of tables).
        let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, t) in graph.tables.iter().enumerate() {
            by_name.entry(t.table().name.to_ascii_lowercase()).or_default().push(i);
        }
        let read = graph.tables.len();
        for from in 0..read {
            let keys = graph.tables[from].structure.foreign_keys.clone();
            for key in keys {
                let name = key.table.name.to_ascii_lowercase();
                let named = by_name.get(&name).map_or(&[][..], Vec::as_slice);
                let to = match graph.resolve_among(named, &key.table, from) {
                    Some(to) => to,
                    None => {
                        let named = by_name.get(&name).map_or(&[][..], Vec::as_slice).to_vec();
                        let to = graph.add_external(
                            &named,
                            &key.table,
                            from,
                            &key.columns,
                            &key.references,
                        );
                        if !named.contains(&to) {
                            by_name.entry(name).or_default().push(to);
                        }
                        to
                    }
                };
                let unique = graph.tables[from].is_unique(&key.columns);
                graph.links.push(Link {
                    from,
                    columns: key.columns,
                    to,
                    references: key.references,
                    cardinality: if unique {
                        Cardinality::OneToOne
                    } else {
                        Cardinality::OneToMany
                    },
                    origin: Origin::Real,
                    name: key.name,
                });
            }
        }
        graph
    }

    /// The table a key of table `from` names: by schema and name, else in `from`'s schema,
    /// else the only one of that name (as the engine compares names: any case).
    pub fn resolve(&self, target: &TableRef, from: usize) -> Option<usize> {
        let all: Vec<usize> = (0..self.tables.len()).collect();
        self.resolve_among(&all, target, from)
    }

    /// [`Self::resolve`] among tables `named` (those that may bear its name, in order).
    fn resolve_among(&self, named_ones: &[usize], target: &TableRef, from: usize) -> Option<usize> {
        let own = self.tables.get(from).and_then(|t| t.table().schema.clone());
        let named = |t: &GraphTable, schema: &Option<String>| {
            t.table().name.eq_ignore_ascii_case(&target.name)
                && match (schema, &t.table().schema) {
                    (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
                    (None, None) => true,
                    _ => false,
                }
        };
        let tables = || named_ones.iter().map(|&i| (i, &self.tables[i]));
        if let Some((i, _)) = tables().find(|(_, t)| named(t, &target.schema)) {
            return Some(i);
        }
        if target.schema.is_none()
            && let Some((i, _)) = tables().find(|(_, t)| named(t, &own))
        {
            return Some(i);
        }
        let mut same = tables().filter(|(_, t)| {
            t.table().name.eq_ignore_ascii_case(&target.name)
                && (target.schema.is_none() || t.table().schema.is_none())
        });
        match (same.next(), same.next()) {
            (Some((i, _)), None) => Some(i),
            _ => None,
        }
    }

    /// A table only named (by `from`'s key `columns` → `references`): kept once, its
    /// columns those named (typed as the columns pointing to them).
    fn add_external(
        &mut self,
        named: &[usize],
        target: &TableRef,
        from: usize,
        columns: &[String],
        references: &[String],
    ) -> usize {
        let mut table = target.clone();
        if table.schema.is_none() {
            table.schema.clone_from(&self.tables[from].table().schema);
        }
        let found = named.iter().copied().find(|&i| {
            let t = &self.tables[i];
            t.external && *t.table() == table
        });
        if let Some(i) = found {
            let known = &mut self.tables[i].structure.columns;
            for name in references {
                if !known.iter().any(|c| c.name == *name) {
                    known.push(ColumnInfo::new(name.clone(), ""));
                }
            }
            return i;
        }
        let source = &self.tables[from].structure;
        let columns = references
            .iter()
            .zip(columns)
            .map(|(name, own)| {
                let declared =
                    source.column(own).map(|c| c.declared_type.clone()).unwrap_or_default();
                ColumnInfo::new(name.clone(), declared)
            })
            .collect();
        self.tables.push(GraphTable {
            structure: Structure { table, columns, ..Structure::default() },
            kind: TableKind::Table,
            rows: None,
            external: true,
        });
        self.tables.len() - 1
    }

    /// The index of `table` (any case).
    pub fn find(&self, table: &TableRef) -> Option<usize> {
        self.tables
            .iter()
            .position(|t| t.table() == table)
            .or_else(|| self.resolve(table, usize::MAX))
    }

    /// The links touching table `i`.
    pub fn links_of(&self, i: usize) -> impl Iterator<Item = (usize, &Link)> {
        self.links.iter().enumerate().filter(move |(_, l)| l.from == i || l.to == i)
    }

    /// Table `focus` and those up to `depth` links away (either way), through the links
    /// `keep` accepts; in the tables' order.
    pub fn neighborhood(
        &self,
        focus: usize,
        depth: usize,
        keep: impl Fn(&Link) -> bool,
    ) -> Vec<usize> {
        if focus >= self.tables.len() {
            return Vec::new();
        }
        let mut around: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for link in self.links.iter().filter(|l| keep(l)) {
            around.entry(link.from).or_default().push(link.to);
            around.entry(link.to).or_default().push(link.from);
        }
        let mut distance = vec![usize::MAX; self.tables.len()];
        distance[focus] = 0;
        let mut queue = VecDeque::from([focus]);
        while let Some(i) = queue.pop_front() {
            if distance[i] == depth {
                continue;
            }
            for &next in around.get(&i).into_iter().flatten() {
                if distance[next] == usize::MAX {
                    distance[next] = distance[i] + 1;
                    queue.push_back(next);
                }
            }
        }
        (0..self.tables.len()).filter(|&i| distance[i] != usize::MAX).collect()
    }

    /// The tables whose name (or `schema.name`) has `filter` in it, any case; all of them
    /// for an empty one.
    pub fn matching(&self, filter: &str) -> Vec<usize> {
        let filter = filter.trim().to_lowercase();
        (0..self.tables.len())
            .filter(|&i| {
                filter.is_empty()
                    || self.tables[i].table().to_string().to_lowercase().contains(&filter)
            })
            .collect()
    }

    /// `link` as a key (to accept it, set it aside, find it among those decided).
    pub fn key_of(&self, link: &Link) -> VirtualKey {
        VirtualKey {
            table: self.tables[link.from].table().clone(),
            columns: link.columns.clone(),
            target: self.tables[link.to].table().clone(),
            references: link.references.clone(),
        }
    }

    /// This graph (its engine's keys) with what the user decided and what names say: the
    /// keys `notes` accepted as [`Origin::Virtual`] links (one the engine has too left
    /// out), then `found` ([`crate::relations::infer`] on this graph) as
    /// [`Origin::Inferred`], but those accepted or set aside.
    pub fn with_notes(&self, found: &[Candidate], notes: &RelationNotes) -> Self {
        let mut graph = self.clone();
        for key in &notes.virtual_keys {
            let (Some(from), Some(to)) = (graph.find(&key.table), graph.find(&key.target)) else {
                continue;
            };
            let known = graph.links.iter().any(|l| {
                l.from == from
                    && l.columns.len() == key.columns.len()
                    && l.columns.iter().zip(&key.columns).all(|(a, b)| a.eq_ignore_ascii_case(b))
            });
            if known {
                continue;
            }
            let unique = graph.tables[from].is_unique(&key.columns);
            graph.links.push(Link {
                from,
                columns: key.columns.clone(),
                to,
                references: key.references.clone(),
                cardinality: if unique { Cardinality::OneToOne } else { Cardinality::OneToMany },
                origin: Origin::Virtual,
                name: None,
            });
        }
        for found in found {
            if notes.is_accepted(&found.key) || notes.is_ignored(&found.key) {
                continue;
            }
            let unique = graph.tables[found.from].is_unique(&found.key.columns);
            graph.links.push(Link {
                from: found.from,
                columns: found.key.columns.clone(),
                to: found.to,
                references: found.key.references.clone(),
                cardinality: if unique { Cardinality::OneToOne } else { Cardinality::OneToMany },
                origin: Origin::Inferred(found.confidence),
                name: None,
            });
        }
        graph
    }

    /// How a link reads: `orders.user_id → users.id` (`t.(a, b) → u.(x, y)` for a
    /// composite key).
    pub fn label(&self, link: &Link) -> String {
        let side = |i: usize, columns: &[String]| {
            let name = self.tables.get(i).map_or("?", |t| t.table().name.as_str());
            match columns {
                [one] => format!("{name}.{one}"),
                many => format!("{name}.({})", many.join(", ")),
            }
        };
        format!("{} → {}", side(link.from, &link.columns), side(link.to, &link.references))
    }
}

/// What a data file's own view is asked to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileTable {
    pub table: TableRef,
    /// Its structure, not its rows.
    pub structure: bool,
    /// The column pointed at in the structure.
    pub column: Option<String>,
    /// The condition its rows are filtered by.
    pub filter: Option<String>,
}

/// How the explorer opens a table's data for the diagram (a double click on its card).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Opening {
    /// The file's own view, asked to show the table.
    File(PathBuf, FileTable),
    /// The table's view.
    View(TableView),
}

/// A table's view to open: what finds its viewer and what it's given. Compared by its title
/// and kind (its data is the explorer's).
#[derive(Clone)]
pub struct TableView {
    pub title: String,
    pub extension: &'static str,
    pub data: std::sync::Arc<dyn std::any::Any + Send + Sync>,
}

impl std::fmt::Debug for TableView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TableView").field("title", &self.title).finish_non_exhaustive()
    }
}

impl PartialEq for TableView {
    fn eq(&self, other: &Self) -> bool {
        (&self.title, self.extension) == (&other.title, other.extension)
    }
}

impl Eq for TableView {}

/// What the diagram asks of the source it shows (implemented by the explorer, over the
/// source's session).
pub trait SchemaAccess: Send + Sync {
    /// The source.
    fn source(&self) -> DataSourceId;
    /// Its name (the diagram's title).
    fn name(&self) -> String;
    /// Every table of `schema` (none: of the source) with its structure, in the background
    /// (it waits for the session: never called on the UI thread). `progress` is told how
    /// many structures were read, of how many; `cancel` stops it.
    fn read(
        &self,
        schema: Option<&str>,
        cancel: &Cancel,
        progress: &(dyn Fn(usize, usize) + Sync),
    ) -> Result<Vec<(TableInfo, Structure)>, DbError>;
    /// Opens `table`'s data; `None`: it can't be.
    fn open(&self, table: &TableRef) -> Option<Opening>;
    /// What the user decided about the source's relations (the keys accepted, those set
    /// aside, which are shown): the defaults until something was.
    fn notes(&self) -> RelationNotes {
        RelationNotes::default()
    }
    /// Keeps `notes` with the source (so the grid follows the keys accepted too).
    fn set_notes(&self, notes: RelationNotes) {
        let _ = notes;
    }
    /// The color the source is shown in (its own, else its folder's): the diagram's tab
    /// carries it. Cheap (asked at each paint).
    fn color(&self) -> Option<crate::folders::SourceColor> {
        None
    }
}

/// What the diagram's view is opened with.
#[derive(Clone)]
pub struct DiagramRequest {
    pub access: Arc<dyn SchemaAccess>,
    /// The project's bus: a structure changed by a commit is read again.
    pub events: Option<EventBus>,
    /// The schema shown (none: the whole source).
    pub schema: Option<String>,
    /// The table it starts from, with its neighbors (none: every table).
    pub focus: Option<TableRef>,
}

impl std::fmt::Debug for DiagramRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiagramRequest")
            .field("source", &self.access.source())
            .field("schema", &self.schema)
            .field("focus", &self.focus)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
#[path = "../tests/unit/diagram.rs"]
mod tests;
