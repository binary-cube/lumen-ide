//! A table's pending changes, as a grid makes them (a table's data view, a console's result): cells changed on rows found by their
//! key (the values read kept, a change since being a conflict), new rows (their columns
//! given or left to the engine), rows to delete — and the [`ChangeSet`] they become when
//! submitted (deletions, updates, insertions: the order [`crate::transaction`] runs them
//! in). Pure: nothing here reads or writes.

use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash as _, Hasher as _};

use crate::sql::editable::{QueryShape, SelectItem};
use crate::structure::ROW_ID_NAMES;
use crate::{
    CellChange, Change, ChangeSet, Column, ColumnInfo, Generated, RowKey, Structure, TableRef,
    Value,
};

/// A row read from the table, changed: its key, its values as read (by column), the new
/// ones.
#[derive(Debug, Clone, PartialEq)]
pub struct Updated {
    pub key: RowKey,
    pub original: Vec<Value>,
    pub cells: BTreeMap<usize, Value>,
}

/// A new row: its id in the grid, and each column's value (`None`: the engine's default).
#[derive(Debug, Clone, PartialEq)]
pub struct Inserted {
    pub id: u64,
    pub values: Vec<Option<Value>>,
}

/// The pending changes of one table (see the module's doc). The rows changed and deleted
/// are found by their key's hash (a grid asks for each cell it draws, each frame; a
/// thousand rows deleted at once), kept in order for what's submitted.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Edits {
    updated: Vec<Updated>,
    inserted: Vec<Inserted>,
    deleted: Vec<RowKey>,
    next: u64,
    /// Where each key's row is in `updated` and in `deleted`, by the key's hash.
    updated_at: HashMap<u64, Vec<usize>>,
    deleted_at: HashMap<u64, Vec<usize>>,
}

/// A row key's hash (a float's bits: keys compare by `==` once found).
fn key_hash(key: &RowKey) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    match key {
        RowKey::Primary(columns) => {
            0u8.hash(&mut h);
            for (name, value) in columns {
                name.hash(&mut h);
                hash_value(value, &mut h);
            }
        }
        RowKey::RowId(id) => (1u8, id).hash(&mut h),
        RowKey::Line { line, fingerprint } => (2u8, line, fingerprint).hash(&mut h),
    }
    h.finish()
}

fn hash_value(value: &Value, h: &mut std::collections::hash_map::DefaultHasher) {
    match value {
        Value::Null => 0u8.hash(h),
        Value::Bool(b) => (1u8, b).hash(h),
        Value::Int(n) => (2u8, n).hash(h),
        Value::Float(f) => (3u8, f.to_bits()).hash(h),
        Value::Text(t) => (4u8, t).hash(h),
        Value::Bytes(b) => (5u8, b).hash(h),
        Value::Json(t) => (6u8, t).hash(h),
        Value::Other(t) => (7u8, t).hash(h),
    }
}

/// Where in `list` the row with `key` is, through `at` (its places by hash).
fn place<T>(
    at: &HashMap<u64, Vec<usize>>,
    list: &[T],
    key: &RowKey,
    key_of: impl Fn(&T) -> &RowKey,
) -> Option<usize> {
    at.get(&key_hash(key))?.iter().copied().find(|&i| list.get(i).is_some_and(|t| key_of(t) == key))
}

/// The places by hash of `list`'s keys, made again (after a row left it).
fn places<T>(list: &[T], key_of: impl Fn(&T) -> &RowKey) -> HashMap<u64, Vec<usize>> {
    let mut at: HashMap<u64, Vec<usize>> = HashMap::new();
    for (i, t) in list.iter().enumerate() {
        at.entry(key_hash(key_of(t))).or_default().push(i);
    }
    at
}

impl Edits {
    /// The rows read and changed, in the order they were.
    pub fn updated(&self) -> &[Updated] {
        &self.updated
    }

    /// The new rows.
    pub fn inserted(&self) -> &[Inserted] {
        &self.inserted
    }

    /// The rows to delete, in the order they were marked.
    pub fn deleted(&self) -> &[RowKey] {
        &self.deleted
    }

    fn updated_place(&self, key: &RowKey) -> Option<usize> {
        place(&self.updated_at, &self.updated, key, |u| &u.key)
    }

    fn remove_updated(&mut self, i: usize) {
        self.updated.remove(i);
        self.updated_at = places(&self.updated, |u| &u.key);
    }

    fn retain_deleted(&mut self, keep: impl Fn(&RowKey) -> bool) {
        self.deleted.retain(keep);
        self.deleted_at = places(&self.deleted, |k| k);
    }

    pub fn is_empty(&self) -> bool {
        self.updated.is_empty() && self.inserted.is_empty() && self.deleted.is_empty()
    }

    /// How many changes: each cell changed, each row added or deleted.
    pub fn count(&self) -> usize {
        let cells: usize =
            self.updated.iter().filter(|u| !self.is_deleted(&u.key)).map(|u| u.cells.len()).sum();
        cells + self.inserted.len() + self.deleted.len()
    }

    pub fn clear(&mut self) {
        self.updated.clear();
        self.inserted.clear();
        self.deleted.clear();
        self.updated_at.clear();
        self.deleted_at.clear();
    }

    /// The changed value of column `col` of the row with `key`, if changed.
    pub fn value(&self, key: &RowKey, col: usize) -> Option<&Value> {
        self.updated[self.updated_place(key)?].cells.get(&col)
    }

    pub fn is_deleted(&self, key: &RowKey) -> bool {
        place(&self.deleted_at, &self.deleted, key, |k| k).is_some()
    }

    /// Column `col` of the row read with `key` and values `original` set to `value` (the
    /// value read again: no change).
    pub fn set(&mut self, key: &RowKey, original: &[Value], col: usize, value: Value) {
        let at = self.updated_place(key);
        let same = original.get(col) == Some(&value);
        match at {
            Some(i) => {
                if same {
                    self.updated[i].cells.remove(&col);
                    if self.updated[i].cells.is_empty() {
                        self.remove_updated(i);
                    }
                } else {
                    self.updated[i].cells.insert(col, value);
                }
            }
            None if same => {}
            None => {
                self.updated_at.entry(key_hash(key)).or_default().push(self.updated.len());
                self.updated.push(Updated {
                    key: key.clone(),
                    original: original.to_vec(),
                    cells: BTreeMap::from([(col, value)]),
                });
            }
        }
    }

    /// A new row of `columns` columns (with `values` when cloned): its id (`base` and up).
    pub fn insert(&mut self, base: u64, columns: usize, values: Option<Vec<Option<Value>>>) -> u64 {
        let id = base + self.next;
        self.next += 1;
        let mut values = values.unwrap_or_default();
        values.resize(columns, None);
        self.inserted.push(Inserted { id, values });
        id
    }

    pub fn new_row(&self, id: u64) -> Option<&Inserted> {
        self.inserted.iter().find(|i| i.id == id)
    }

    /// Column `col` of new row `id` set.
    pub fn set_new(&mut self, id: u64, col: usize, value: Value) {
        if let Some(row) = self.inserted.iter_mut().find(|i| i.id == id)
            && let Some(slot) = row.values.get_mut(col)
        {
            *slot = Some(value);
        }
    }

    /// The row with `key` marked for deletion, or unmarked when it was.
    pub fn toggle_delete(&mut self, key: &RowKey) {
        if self.is_deleted(key) {
            self.retain_deleted(|k| k != key);
        } else {
            self.deleted_at.entry(key_hash(key)).or_default().push(self.deleted.len());
            self.deleted.push(key.clone());
        }
    }

    pub fn drop_new(&mut self, id: u64) {
        self.inserted.retain(|i| i.id != id);
    }

    /// Every change of the row with `key` dropped.
    pub fn revert_row(&mut self, key: &RowKey) {
        if let Some(i) = self.updated_place(key) {
            self.remove_updated(i);
        }
        if self.is_deleted(key) {
            self.retain_deleted(|k| k != key);
        }
    }

    /// The changes to submit, in the order they run: deletions, updates (of rows not
    /// deleted), insertions (the columns given, generated ones never).
    pub fn change_set(&self, table: TableRef, columns: &[ColumnInfo]) -> ChangeSet {
        let mut set = ChangeSet::new(table);
        for key in &self.deleted {
            set.changes.push(Change::Delete { key: key.clone() });
        }
        for row in self.updated.iter().filter(|u| !self.is_deleted(&u.key)) {
            let cells: Vec<CellChange> = row
                .cells
                .iter()
                .filter_map(|(&col, new)| {
                    let column = columns.get(col)?;
                    Some(CellChange {
                        column: column.name.clone(),
                        old: row.original.get(col).cloned().unwrap_or(Value::Null),
                        new: new.clone(),
                    })
                })
                .collect();
            if !cells.is_empty() {
                set.changes.push(Change::Update { key: row.key.clone(), cells });
            }
        }
        for row in &self.inserted {
            let values = row
                .values
                .iter()
                .enumerate()
                .filter_map(|(col, value)| {
                    let column = columns.get(col).filter(|c| c.generated.is_none())?;
                    Some((column.name.clone(), value.clone()?))
                })
                .collect();
            set.changes.push(Change::Insert { values });
        }
        set
    }
}

/// Why a query's result can't be edited (each told to the user in their words).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotEditable {
    /// It isn't a `SELECT`.
    NotQuery,
    /// It reads more than one table, or no table (a join, a `UNION`, a `WITH`, a subquery
    /// or a function in its `FROM`).
    NotOneTable,
    /// `DISTINCT`: a row may stand for several.
    Distinct,
    /// `GROUP BY` / `HAVING`: its rows are groups.
    Grouped,
    /// An aggregate (or a window, `over`) in its select list: its rows are groups.
    Aggregate(String),
    /// It locks the rows it reads (`FOR UPDATE`): it isn't run again to read them back once
    /// they're changed.
    Locking,
    /// The table has no key its rows are found by (no primary key, no row id, no unique key
    /// of `NOT NULL` columns): a view, say.
    NoKey,
    /// The table's key isn't among the result's columns (its columns named).
    KeyMissing(Vec<String>),
    /// Its columns aren't the table's as the query names them.
    Unmatched,
}

/// How a result's rows are found in their table: by these result columns' values (the
/// table's primary key, or a unique key of `NOT NULL` columns), or by its row id's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultKey {
    /// The key's columns, each with its place among the result's.
    Columns(Vec<(String, usize)>),
    /// The row id's place among the result's (SQLite: `rowid` selected).
    RowId(usize),
}

/// A query's result as it's edited (see [`editable_result`]): its table, the table's
/// structure, which table column each result column is, the key its rows are found by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditableResult {
    pub table: TableRef,
    pub structure: Structure,
    /// For each result column, its column in [`Self::structure`] (`None`: an expression, the
    /// row id, or a column shown twice — read only).
    pub columns: Vec<Option<usize>>,
    pub key: ResultKey,
}

/// How a result of `shape`'s query (its `result` columns, as the engine gave them) is edited,
/// by `structure` (its table's): each result column taken back to the table's, the key its
/// rows are found by among them. Why not, otherwise.
pub fn editable_result(
    shape: &QueryShape,
    result: &[Column],
    structure: Structure,
) -> Result<EditableResult, NotEditable> {
    let find =
        |name: &str| {
            structure.columns.iter().position(|c| c.name == name).or_else(|| {
                structure.columns.iter().position(|c| c.name.eq_ignore_ascii_case(name))
            })
        };
    let row_id = structure.row_id_name();
    // Each item as the result's columns it makes (`*`: the table's, in order).
    let mut made: Vec<Item> = Vec::new();
    for item in &shape.items {
        match item {
            SelectItem::All => made.extend((0..structure.columns.len()).map(Item::Column)),
            SelectItem::Column(name) => match find(name) {
                Some(at) => made.push(Item::Column(at)),
                // A name of the row id no column hides (`rowid`, `_rowid_`, `oid`).
                None if structure.rowid
                    && ROW_ID_NAMES.iter().any(|n| n.eq_ignore_ascii_case(name)) =>
                {
                    made.push(Item::RowId);
                }
                None => made.push(Item::Expression),
            },
            SelectItem::Expression => made.push(Item::Expression),
        }
    }
    if made.len() != result.len() {
        return Err(NotEditable::Unmatched);
    }
    // A `*`'s columns are the table's by name too (a structure read since may differ).
    for ((item, column), shape_item) in
        made.iter().zip(result).zip(expanded(&shape.items, &structure))
    {
        if let (Item::Column(at), true) = (item, shape_item)
            && !structure.columns[*at].name.eq_ignore_ascii_case(&column.name)
        {
            return Err(NotEditable::Unmatched);
        }
    }
    let mut columns: Vec<Option<usize>> = Vec::with_capacity(made.len());
    let mut row_id_at = None;
    for (i, item) in made.iter().enumerate() {
        match item {
            // Shown twice: the first one is edited.
            Item::Column(at) if !columns.contains(&Some(*at)) => columns.push(Some(*at)),
            Item::RowId => {
                row_id_at.get_or_insert(i);
                columns.push(None);
            }
            _ => columns.push(None),
        }
    }
    let place = |name: &str| {
        let at = structure.columns.iter().position(|c| c.name == name)?;
        columns.iter().position(|c| *c == Some(at))
    };
    let primary = structure.primary_key();
    let by = |names: &[String]| -> Option<Vec<(String, usize)>> {
        names.iter().map(|n| place(n).map(|i| (n.clone(), i))).collect()
    };
    let key = if let Some(found) = (!primary.is_empty()).then(|| by(&primary)).flatten() {
        ResultKey::Columns(found)
    } else if let Some(at) = row_id_at {
        ResultKey::RowId(at)
    } else if let Some(found) = unique_keys(&structure).find_map(|names| by(&names)) {
        ResultKey::Columns(found)
    } else if !primary.is_empty() {
        return Err(NotEditable::KeyMissing(primary));
    } else if let Some(r) = row_id {
        return Err(NotEditable::KeyMissing(vec![r.to_owned()]));
    } else if let Some(names) = unique_keys(&structure).next() {
        return Err(NotEditable::KeyMissing(names));
    } else {
        return Err(NotEditable::NoKey);
    };
    Ok(EditableResult { table: shape.table.clone(), structure, columns, key })
}

/// What a select list's item makes, by the table's structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Item {
    Column(usize),
    RowId,
    Expression,
}

/// For each result column, whether it comes from a `*` (its name must be the table's).
fn expanded(items: &[SelectItem], structure: &Structure) -> Vec<bool> {
    items
        .iter()
        .flat_map(|item| match item {
            SelectItem::All => vec![true; structure.columns.len()],
            _ => vec![false],
        })
        .collect()
}

/// The table's unique keys of `NOT NULL` columns (an index of every row), their columns.
fn unique_keys(structure: &Structure) -> impl Iterator<Item = Vec<String>> + '_ {
    structure
        .indexes
        .iter()
        .filter(|i| i.unique && i.predicate.is_none() && !i.columns.is_empty())
        .filter(|i| i.columns.iter().all(|c| structure.column(c).is_some_and(|c| !c.nullable)))
        .map(|i| i.columns.clone())
}

impl EditableResult {
    /// The key of a result's row (`None`: a key's value is `NULL` — no row is found by it).
    pub fn key_of(&self, row: &[Value]) -> Option<RowKey> {
        match &self.key {
            ResultKey::Columns(columns) => {
                let values: Option<Vec<(String, Value)>> = columns
                    .iter()
                    .map(|(name, at)| match row.get(*at)? {
                        Value::Null => None,
                        v => Some((name.clone(), v.clone())),
                    })
                    .collect();
                values.map(RowKey::Primary)
            }
            ResultKey::RowId(at) => match row.get(*at)? {
                Value::Int(id) => Some(RowKey::RowId(*id)),
                _ => None,
            },
        }
    }

    /// Result column `at` is a column of the table that can be written (not generated).
    pub fn writable(&self, at: usize) -> bool {
        self.column(at).is_some_and(|c| c.generated.is_none())
    }

    /// Result column `at`'s column of the table.
    pub fn column(&self, at: usize) -> Option<&ColumnInfo> {
        self.structure.columns.get((*self.columns.get(at)?)?)
    }

    /// Result column `at` is part of the key the rows are found by.
    pub fn in_key(&self, at: usize) -> bool {
        match &self.key {
            ResultKey::Columns(columns) => columns.iter().any(|(_, i)| *i == at),
            ResultKey::RowId(i) => *i == at,
        }
    }

    /// The columns [`Edits::change_set`] writes, in the result's order (`result`'s names): a
    /// column that isn't the table's — an expression, the row id — is never written (it
    /// counts as generated).
    pub fn column_infos(&self, result: &[Column]) -> Vec<ColumnInfo> {
        result
            .iter()
            .enumerate()
            .map(|(i, column)| match self.column(i) {
                Some(info) => info.clone(),
                None => {
                    let mut info = ColumnInfo::new(column.name.clone(), column.type_name.clone());
                    info.generated =
                        Some(Generated { expression: column.name.clone(), stored: false });
                    info
                }
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "../tests/unit/edits.rs"]
mod tests;
