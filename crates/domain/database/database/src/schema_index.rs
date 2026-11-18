//! A data source's schema as the console's completion knows it, for one database: its
//! schemas, tables and views (kind, comment, estimated rows), their columns (type,
//! nullable, default, comment, place in the key), the relations between them — the
//! engine's foreign keys, the keys the user accepted ([`RelationNotes`]) and those found
//! from names ([`crate::relations::infer`]), set aside ones left out — and its routines.
//!
//! Pure and immutable once built (shared as an `Arc`, built in the background from what
//! the driver read: [`IndexParts`]); a newer one replaces it whole, with another
//! [`SchemaIndex::generation`]. Names are kept lowercase in sorted vectors: a prefix is a
//! binary search, a fuzzy match looks at a few hundred names at most.
//!
//! Two tiers, for huge schemas: the tables' names always; their columns all at once when
//! there aren't too many ([`IndexParts::complete`]), else only those a statement names,
//! read when asked and added ([`SchemaIndex::with_columns`]).

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use crate::diagram::{Origin, SchemaGraph};
use crate::relations::RelationNotes;
use crate::{
    ColumnBrief, ColumnInfo, DataSourceId, ForeignKey, Routine, RoutineKind, Structure,
    TableColumns, TableInfo, TableKeys, TableKind, TableRef,
};

/// A column, compact (5 000 tables × 30 columns fit in well under 20 MB).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexColumn {
    pub name: Box<str>,
    /// As declared (types are shared between columns).
    pub type_name: Arc<str>,
    pub nullable: bool,
    pub default: Option<Box<str>>,
    pub comment: Option<Box<str>>,
    /// Its place in the primary key (from 1).
    pub key_position: Option<u16>,
    /// The values its type allows (an enum's: PostgreSQL's labels, MySQL's `enum(…)`), in
    /// their order.
    pub values: Option<Arc<[Box<str>]>>,
}

/// A table or a view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexTable {
    pub table: TableRef,
    pub kind: TableKind,
    pub rows: Option<u64>,
    pub comment: Option<String>,
    /// `None`: not read yet (a huge schema's, read when a statement names it).
    pub columns: Option<Arc<[IndexColumn]>>,
    /// Its foreign keys (as read; their targets resolved in [`SchemaIndex::relations`]).
    pub foreign_keys: Vec<ForeignKey>,
}

/// Where a relation comes from (the surest first).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RelationOrigin {
    /// The engine's foreign key.
    Real,
    /// A key the user accepted.
    Virtual,
    /// Found from the columns' names, not decided (shown last, marked).
    Inferred,
}

/// `columns` of table `from` point to `references` of table `to` (indexes in
/// [`SchemaIndex::tables`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexRelation {
    pub from: u32,
    pub columns: Vec<String>,
    pub to: u32,
    pub references: Vec<String>,
    pub origin: RelationOrigin,
}

/// A routine (the server's, or the engine's own).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexRoutine {
    pub schema: Option<String>,
    pub name: String,
    /// Its arguments as the engine writes them (empty when it doesn't say).
    pub arguments: String,
    pub result: Option<String>,
    pub kind: RoutineKind,
    pub arity: Option<u16>,
    pub builtin: bool,
}

/// What an index is built from (what the driver read, in the background).
#[derive(Debug, Clone, Default)]
pub struct IndexParts {
    pub source: DataSourceId,
    pub database: Option<String>,
    /// The connection's schema (`public`, `main`; `None`: the engine has none).
    pub default_schema: Option<String>,
    pub schemas: Vec<String>,
    pub tables: Vec<TableInfo>,
    /// Their columns (all of them when [`Self::complete`], else those read so far).
    pub columns: Vec<TableColumns>,
    pub keys: Vec<TableKeys>,
    pub routines: Vec<Routine>,
    /// What else a query may read from (PostgreSQL's partitions and sequences): never
    /// called unknown.
    pub readable: Vec<TableRef>,
    pub notes: RelationNotes,
    /// Every table's columns were read.
    pub complete: bool,
    pub generation: u64,
}

/// A source's schema for completion (see the module's doc).
#[derive(Debug, Clone, Default)]
pub struct SchemaIndex {
    pub source: DataSourceId,
    pub database: Option<String>,
    generation: u64,
    default_schema: Option<String>,
    schemas: Vec<String>,
    /// By lowercase name, then schema.
    tables: Vec<IndexTable>,
    lower: Vec<Box<str>>,
    relations: Vec<IndexRelation>,
    /// Each table's relations (indexes in `relations`), both ways.
    by_table: Vec<Vec<u32>>,
    /// By lowercase name.
    routines: Vec<IndexRoutine>,
    routine_lower: Vec<Box<str>>,
    /// What else may be read from, as matched (lowercase, the default schema filled in),
    /// sorted.
    readable: Vec<TableRef>,
    complete: bool,
    notes: RelationNotes,
}

impl SchemaIndex {
    /// The index of `parts` (see [`IndexParts`]).
    pub fn build(parts: IndexParts) -> Self {
        let mut types: HashMap<String, Arc<str>> = HashMap::new();
        let default = parts.default_schema.clone();
        // A table's columns and keys, by its name as the engine lists it.
        let mut columns: HashMap<TableRef, Vec<ColumnBrief>> =
            parts.columns.into_iter().map(|(t, c)| (normal(t, &default), c)).collect();
        let mut keys: HashMap<TableRef, Vec<ForeignKey>> =
            parts.keys.into_iter().map(|(t, k)| (normal(t, &default), k)).collect();
        let mut tables: Vec<IndexTable> = parts
            .tables
            .into_iter()
            .map(|info| {
                let key = normal(info.table.clone(), &default);
                let read = columns.remove(&key);
                IndexTable {
                    table: info.table,
                    kind: info.kind,
                    rows: info.rows,
                    comment: None,
                    columns: read.map(|c| compact(c, &mut types)),
                    foreign_keys: keys.remove(&key).unwrap_or_default(),
                }
            })
            .collect();
        // Tables the listing missed but whose columns came (a race with a change): kept.
        for (table, read) in columns {
            tables.push(IndexTable {
                foreign_keys: keys.remove(&table).unwrap_or_default(),
                table,
                kind: TableKind::Table,
                rows: None,
                comment: None,
                columns: Some(compact(read, &mut types)),
            });
        }
        let mut routines: Vec<IndexRoutine> = parts
            .routines
            .into_iter()
            .map(|r| IndexRoutine {
                schema: r.schema,
                name: r.name,
                arguments: r.arguments,
                result: r.result,
                kind: r.kind,
                arity: r.arity,
                builtin: r.builtin,
            })
            .collect();
        routines.sort_by_cached_key(|r| (r.name.to_lowercase(), r.builtin, r.arity));
        let routine_lower = routines.iter().map(|r| r.name.to_lowercase().into()).collect();
        let mut schemas = parts.schemas;
        for t in &tables {
            if let Some(s) = &t.table.schema
                && !schemas.contains(s)
            {
                schemas.push(s.clone());
            }
        }
        let mut index = Self {
            source: parts.source,
            database: parts.database,
            generation: parts.generation,
            default_schema: parts.default_schema,
            schemas,
            tables,
            lower: Vec::new(),
            relations: Vec::new(),
            by_table: Vec::new(),
            routines,
            routine_lower,
            readable: Vec::new(),
            complete: parts.complete,
            notes: parts.notes,
        };
        let mut readable: Vec<TableRef> =
            parts.readable.into_iter().map(|t| normal(t, &default)).collect();
        readable.sort_by(|a, b| (&a.name, &a.schema).cmp(&(&b.name, &b.schema)));
        readable.dedup();
        index.readable = readable;
        index.sort();
        index.relate();
        index
    }

    /// This index with `loaded` tables' columns and keys (a huge schema's, read when named),
    /// as generation `generation`.
    pub fn with_columns(
        &self,
        loaded: Vec<(TableRef, Vec<ColumnBrief>, Vec<ForeignKey>)>,
        generation: u64,
    ) -> Self {
        let mut next = self.clone();
        let mut types: HashMap<String, Arc<str>> = HashMap::new();
        for t in next.tables.iter().filter_map(|t| t.columns.as_ref()).flat_map(|c| c.iter()) {
            types.entry(t.type_name.to_string()).or_insert_with(|| Arc::clone(&t.type_name));
        }
        for (table, columns, keys) in loaded {
            match next.find(&table) {
                Some(i) => {
                    next.tables[i].columns = Some(compact(columns, &mut types));
                    next.tables[i].foreign_keys = keys;
                }
                None => next.tables.push(IndexTable {
                    table,
                    kind: TableKind::Table,
                    rows: None,
                    comment: None,
                    columns: Some(compact(columns, &mut types)),
                    foreign_keys: keys,
                }),
            }
        }
        next.generation = generation;
        next.sort();
        next.relate();
        next
    }

    /// Changes with every build (a newer index of the same source has another).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn default_schema(&self) -> Option<&str> {
        self.default_schema.as_deref()
    }

    pub fn schemas(&self) -> &[String] {
        &self.schemas
    }

    /// Every table's columns were read (the first tier is the whole schema).
    pub fn is_complete(&self) -> bool {
        self.complete
    }

    /// The relations decided for the source when it was built.
    pub fn notes(&self) -> &RelationNotes {
        &self.notes
    }

    pub fn tables(&self) -> &[IndexTable] {
        &self.tables
    }

    pub fn table(&self, i: usize) -> &IndexTable {
        &self.tables[i]
    }

    pub fn relations(&self) -> &[IndexRelation] {
        &self.relations
    }

    /// Table `i`'s relations, both ways.
    pub fn relations_of(&self, i: usize) -> impl Iterator<Item = &IndexRelation> {
        self.by_table.get(i).into_iter().flatten().map(|&r| &self.relations[r as usize])
    }

    pub fn routines(&self) -> &[IndexRoutine] {
        &self.routines
    }

    /// The table `table` names: by schema and name (exactly, else in any case); without a
    /// schema, the default schema's first, else the only one so named.
    pub fn find(&self, table: &TableRef) -> Option<usize> {
        let lower = table.name.to_lowercase();
        let range = self.range(&self.lower, &lower);
        let candidates = || range.clone().filter(|&i| *self.lower[i] == *lower);
        let schema_is = |i: usize, schema: &str| {
            self.schema_of(i).is_some_and(|s| s.eq_ignore_ascii_case(schema))
        };
        if let Some(schema) = &table.schema {
            let in_schema: Vec<usize> = candidates().filter(|&i| schema_is(i, schema)).collect();
            return in_schema
                .iter()
                .copied()
                .find(|&i| self.tables[i].table.name == table.name)
                .or_else(|| in_schema.first().copied());
        }
        let all: Vec<usize> = candidates().collect();
        let exact = |set: &[usize]| {
            set.iter()
                .copied()
                .find(|&i| self.tables[i].table.name == table.name)
                .or(set.first().copied())
        };
        if let Some(default) = &self.default_schema {
            let own: Vec<usize> = all.iter().copied().filter(|&i| schema_is(i, default)).collect();
            if let Some(found) = exact(&own) {
                return Some(found);
            }
        }
        let unqualified: Vec<usize> =
            all.iter().copied().filter(|&i| self.tables[i].table.schema.is_none()).collect();
        if let Some(found) = exact(&unqualified) {
            return Some(found);
        }
        exact(&all)
    }

    /// A query may read from `table`: a table or view listed (its schema's, or, without
    /// one, any schema's), or another relation of [`IndexParts::readable`].
    pub fn exists(&self, table: &TableRef) -> bool {
        if self.find(table).is_some() {
            return true;
        }
        let lower = table.name.to_lowercase();
        let start = self.readable.partition_point(|t| t.name < lower);
        self.readable[start..].iter().take_while(|t| t.name == lower).any(|t| {
            table.schema.as_ref().is_none_or(|s| t.schema.as_deref() == Some(&s.to_lowercase()))
        })
    }

    /// `schema` was read (a table of it is listed, or it's among the schemas).
    pub fn knows_schema(&self, schema: &str) -> bool {
        self.schemas.iter().any(|s| s.eq_ignore_ascii_case(schema))
            || self.default_schema.as_deref().is_some_and(|s| s.eq_ignore_ascii_case(schema))
    }

    /// Table `i`'s column `name` (exactly, else in any case), when its columns were read.
    pub fn column(&self, i: usize, name: &str) -> Option<usize> {
        let columns = self.tables.get(i)?.columns.as_ref()?;
        columns
            .iter()
            .position(|c| &*c.name == name)
            .or_else(|| columns.iter().position(|c| c.name.eq_ignore_ascii_case(name)))
    }

    /// Where column `column` of table `i` points (a foreign key's, real or accepted): the
    /// table and its column.
    pub fn points_to(&self, i: usize, column: &str) -> Option<(usize, String)> {
        self.relations_of(i)
            .filter(|r| r.from as usize == i && r.origin != RelationOrigin::Inferred)
            .find_map(|r| {
                let at = r.columns.iter().position(|c| c.eq_ignore_ascii_case(column))?;
                Some((r.to as usize, r.references.get(at)?.clone()))
            })
    }

    /// Table `i`'s schema (the default one for a table listed without).
    pub fn schema_of(&self, i: usize) -> Option<&str> {
        self.tables[i].table.schema.as_deref().or(self.default_schema.as_deref())
    }

    /// Table `i` is in `schema` (`None`: the default one).
    pub fn in_schema(&self, i: usize, schema: Option<&str>) -> bool {
        match (self.schema_of(i), schema.or(self.default_schema.as_deref())) {
            (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
            (None, None) => true,
            _ => false,
        }
    }

    /// The tables whose names match `query` (lowercase, may be empty), at most `limit`:
    /// those starting with it first (a binary search), then fuzzy matches among the names
    /// starting with its first letter.
    pub fn candidates(&self, query: &str, limit: usize) -> Vec<usize> {
        candidates(&self.lower, query, limit)
    }

    /// The routines whose names match `query` (as [`Self::candidates`]).
    pub fn routine_candidates(&self, query: &str, limit: usize) -> Vec<usize> {
        candidates(&self.routine_lower, query, limit)
    }

    /// Of `tables`, those named but whose columns weren't read yet.
    pub fn missing(&self, tables: &[TableRef]) -> Vec<TableRef> {
        tables
            .iter()
            .filter(|t| self.find(t).is_some_and(|i| self.tables[i].columns.is_none()))
            .cloned()
            .collect()
    }

    /// Roughly what it holds in memory (bytes).
    pub fn memory(&self) -> usize {
        use std::mem::size_of;
        let mut bytes = size_of::<Self>();
        let mut types: Vec<*const u8> = Vec::new();
        for t in &self.tables {
            bytes += size_of::<IndexTable>() + t.table.name.len() + 8;
            bytes += t.table.schema.as_ref().map_or(0, String::len);
            bytes += t.comment.as_ref().map_or(0, String::len);
            if let Some(columns) = &t.columns {
                for c in columns.iter() {
                    bytes += size_of::<IndexColumn>() + c.name.len();
                    bytes += c.default.as_ref().map_or(0, |d| d.len());
                    bytes += c.comment.as_ref().map_or(0, |d| d.len());
                    types.push(c.type_name.as_ptr());
                }
            }
            bytes += t.foreign_keys.len() * (size_of::<ForeignKey>() + 32);
        }
        types.sort_unstable();
        types.dedup();
        bytes += types.len() * 32;
        bytes += self.lower.iter().map(|l| l.len() + 16).sum::<usize>();
        bytes += self.relations.len() * (size_of::<IndexRelation>() + 32);
        bytes += self.by_table.iter().map(|b| 24 + b.len() * 4).sum::<usize>();
        bytes += self
            .routines
            .iter()
            .map(|r| size_of::<IndexRoutine>() + r.name.len() * 2 + r.arguments.len() + 32)
            .sum::<usize>();
        bytes
    }

    fn sort(&mut self) {
        self.tables.sort_by_cached_key(|t| {
            (t.table.name.to_lowercase(), t.table.schema.as_deref().map(str::to_lowercase))
        });
        self.tables.dedup_by(|a, b| a.table == b.table);
        self.lower = self.tables.iter().map(|t| t.table.name.to_lowercase().into()).collect();
    }

    /// The range of `names` (sorted) starting with `prefix`.
    fn range(&self, names: &[Box<str>], prefix: &str) -> Range<usize> {
        prefix_range(names, prefix)
    }

    /// The relations of the tables whose columns were read: their keys, those accepted,
    /// those found from names (neither accepted nor set aside).
    fn relate(&mut self) {
        let read: Vec<usize> =
            (0..self.tables.len()).filter(|&i| self.tables[i].columns.is_some()).collect();
        let graph_tables: Vec<(TableInfo, Structure)> = read
            .iter()
            .map(|&i| {
                let t = &self.tables[i];
                let table = self.qualified(i);
                let columns = t
                    .columns
                    .iter()
                    .flat_map(|c| c.iter())
                    .map(|c| {
                        let mut info = ColumnInfo::new(c.name.to_string(), c.type_name.to_string());
                        info.nullable = c.nullable;
                        info.key_position = c.key_position;
                        info.in_key = c.key_position.is_some();
                        info
                    })
                    .collect();
                let foreign_keys = t
                    .foreign_keys
                    .iter()
                    .map(|k| {
                        let mut k = k.clone();
                        if k.table.schema.is_none() {
                            k.table.schema.clone_from(&table.schema);
                        }
                        k
                    })
                    .collect();
                (
                    TableInfo { table: table.clone(), kind: t.kind, rows: t.rows },
                    Structure { table, columns, foreign_keys, ..Structure::default() },
                )
            })
            .collect();
        let graph = SchemaGraph::new(graph_tables);
        let found = crate::relations::infer(&graph);
        let graph = graph.with_notes(&found, &self.notes);
        // The graph's tables back to the index's.
        let mut back: HashMap<TableRef, usize> = HashMap::new();
        for &i in &read {
            back.insert(self.qualified(i), i);
        }
        let mut relations = Vec::new();
        for link in &graph.links {
            let (Some(&from), Some(&to)) = (
                back.get(graph.tables[link.from].table()),
                back.get(graph.tables[link.to].table()),
            ) else {
                continue;
            };
            let origin = match link.origin {
                Origin::Real => RelationOrigin::Real,
                Origin::Virtual => RelationOrigin::Virtual,
                Origin::Inferred(_) => RelationOrigin::Inferred,
            };
            // A key without its columns named (SQLite's `REFERENCES t`): the target's key.
            let mut references = link.references.clone();
            if references.is_empty() {
                references = self.primary_key(to);
            }
            if references.len() != link.columns.len() {
                continue;
            }
            relations.push(IndexRelation {
                from: from as u32,
                columns: link.columns.clone(),
                to: to as u32,
                references,
                origin,
            });
        }
        relations.sort_by_key(|r| r.origin);
        let mut by_table = vec![Vec::new(); self.tables.len()];
        for (k, r) in relations.iter().enumerate() {
            by_table[r.from as usize].push(k as u32);
            if r.to != r.from {
                by_table[r.to as usize].push(k as u32);
            }
        }
        self.relations = relations;
        self.by_table = by_table;
    }

    /// Table `i` with its schema written (the default one when listed without).
    fn qualified(&self, i: usize) -> TableRef {
        let t = &self.tables[i].table;
        TableRef {
            schema: t.schema.clone().or_else(|| self.default_schema.clone()),
            name: t.name.clone(),
        }
    }

    /// Table `i`'s primary key, in order.
    pub fn primary_key(&self, i: usize) -> Vec<String> {
        let mut key: Vec<(u16, String)> = self.tables[i]
            .columns
            .iter()
            .flat_map(|c| c.iter())
            .filter_map(|c| Some((c.key_position?, c.name.to_string())))
            .collect();
        key.sort();
        key.into_iter().map(|(_, n)| n).collect()
    }
}

/// `t` with `default` as its schema when it has none (how tables are matched).
fn normal(mut t: TableRef, default: &Option<String>) -> TableRef {
    if t.schema.is_none() {
        t.schema.clone_from(default);
    }
    t.schema = t.schema.map(|s| s.to_lowercase());
    t.name = t.name.to_lowercase();
    t
}

fn compact(columns: Vec<ColumnBrief>, types: &mut HashMap<String, Arc<str>>) -> Arc<[IndexColumn]> {
    columns
        .into_iter()
        .map(|c| IndexColumn {
            type_name: Arc::clone(
                types.entry(c.type_name.clone()).or_insert_with(|| c.type_name.as_str().into()),
            ),
            name: c.name.into(),
            nullable: c.nullable,
            default: c.default.map(Into::into),
            comment: c.comment.map(Into::into),
            key_position: c.key_position,
            values: c
                .values
                .or_else(|| crate::sql::value::enum_values(&c.type_name))
                .map(|v| v.into_iter().map(Into::into).collect()),
        })
        .collect()
}

/// The range of sorted `names` starting with `prefix`.
fn prefix_range(names: &[Box<str>], prefix: &str) -> Range<usize> {
    let start = names.partition_point(|n| &**n < prefix);
    let end = start + names[start..].partition_point(|n| n.starts_with(prefix));
    start..end
}

/// Identifiers: words start after `_`, `$`, `.` (and in camelCase).
const IDENTIFIERS: kernel::FuzzyStyle = kernel::FuzzyStyle {
    separators: &['_', '$', '.', ' '],
    consecutive_bonus: 8,
    boundary_bonus: 6,
    gap_penalty: true,
    prefer_boundaries: true,
};

/// The score of `name` for `query` (lowercase), or `None`: a prefix scores the most.
pub fn score(query: &str, name: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let chars: Vec<char> = query.chars().collect();
    let (mut score, _) = kernel::fuzzy_match(name, &chars, &IDENTIFIERS)?;
    let lower = name.to_lowercase();
    if lower.starts_with(query) {
        score += 60;
    }
    if lower == query {
        score += 30;
    }
    Some(score - (name.chars().count() as i32 - chars.len() as i32).max(0) / 4)
}

/// [`SchemaIndex::candidates`] over any sorted list of lowercase names.
fn candidates(names: &[Box<str>], query: &str, limit: usize) -> Vec<usize> {
    let exact = prefix_range(names, query);
    let mut out: Vec<usize> = exact.clone().take(limit).collect();
    if out.len() >= limit || query.is_empty() {
        return out;
    }
    // Fuzzy among those starting with the query's first letter (and `_x`-style names'
    // words), a few hundred at most.
    let first: String = query.chars().take(1).collect();
    let around = prefix_range(names, &first);
    let mut fuzzy: Vec<(i32, usize)> = around
        .filter(|i| !exact.contains(i))
        .take(600)
        .filter_map(|i| Some((score(query, &names[i])?, i)))
        .collect();
    // And the names with a word starting so (`order_items` for `it`).
    if exact.len() + fuzzy.len() < limit {
        let word: String = format!("_{first}");
        let seen = prefix_range(names, &first);
        fuzzy.extend(
            names
                .iter()
                .enumerate()
                .filter(|(i, n)| !seen.contains(i) && n.contains(&word))
                .filter_map(|(i, n)| Some((score(query, n)? - 10, i)))
                .take(limit),
        );
    }
    fuzzy.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    out.extend(fuzzy.into_iter().map(|(_, i)| i).take(limit - out.len()));
    out
}

#[cfg(test)]
#[path = "../tests/unit/schema_index.rs"]
pub(crate) mod tests;
