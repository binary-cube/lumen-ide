//! A table's structure, as the engine describes it: its columns (each with a class of type
//! the grid edits it by), its indexes and its foreign keys.

use std::fmt;

/// A table, by name (in a schema, for the engines that have them).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct TableRef {
    pub schema: Option<String>,
    pub name: String,
}

impl TableRef {
    pub fn new(name: impl Into<String>) -> Self {
        Self { schema: None, name: name.into() }
    }

    pub fn in_schema(schema: impl Into<String>, name: impl Into<String>) -> Self {
        Self { schema: Some(schema.into()), name: name.into() }
    }
}

impl fmt::Display for TableRef {
    /// `schema.name` (for messages and titles: SQL quotes it through a dialect).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.schema {
            Some(schema) => write!(f, "{schema}.{}", self.name),
            None => f.write_str(&self.name),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TableKind {
    Table,
    View,
}

/// A table as listed (its structure is read when it opens).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableInfo {
    pub table: TableRef,
    pub kind: TableKind,
    /// The engine's estimate of its rows, when it keeps one.
    pub rows: Option<u64>,
}

/// What kind of value a column holds: how it's shown and which editor changes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TypeClass {
    #[default]
    Text,
    Integer,
    Decimal,
    Bool,
    /// A date, a time or both.
    DateTime,
    Binary,
    Json,
    /// Anything else (arrays, geometries, ranges…): shown and edited as written.
    Other,
}

impl TypeClass {
    /// The class of a declared type (`varchar(255)`, `BIGINT`, `timestamp with time zone`,
    /// SQLite's free-form names by its affinity rules); unknown names are `Other`.
    pub fn of(declared: &str) -> Self {
        let t = declared.trim().to_ascii_lowercase();
        let has = |part: &str| t.contains(part);
        if t.is_empty() {
            // SQLite: a column without a type holds anything.
            Self::Other
        } else if t == "bool" || t == "boolean" || t == "bit" || t == "tinyint(1)" {
            Self::Bool
        } else if has("json") {
            Self::Json
        } else if has("range") || has("[]") || has("point") || t.starts_with('_') {
            // Ranges, arrays (`int4[]`, `_int4`), geometries: not what their names hold.
            Self::Other
        } else if has("date") || has("time") || has("interval") {
            Self::DateTime
        } else if has("int") || t == "serial" || t == "bigserial" || t == "smallserial" {
            Self::Integer
        } else if has("char") || has("text") || has("clob") || t == "uuid" || t == "name" {
            Self::Text
        } else if has("blob") || has("bytea") || has("binary") || t == "image" {
            Self::Binary
        } else if has("real")
            || has("floa")
            || has("doub")
            || has("dec")
            || has("numeric")
            || has("money")
        {
            Self::Decimal
        } else {
            Self::Other
        }
    }
}

/// A column of a table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnInfo {
    pub name: String,
    /// The type as declared (`varchar(255)`, `INTEGER`).
    pub declared_type: String,
    pub class: TypeClass,
    pub nullable: bool,
    /// Its default, as the engine's catalog writes it (an SQL expression).
    pub default: Option<String>,
    /// Part of the primary key.
    pub in_key: bool,
    /// Its place in the primary key (1-based), when the engine says: a key's order may
    /// not be its columns' (`PRIMARY KEY (b, a)`).
    pub key_position: Option<u16>,
    pub auto_increment: bool,
    /// Its collation, when declared (`COLLATE NOCASE`): how its text compares and sorts.
    pub collation: Option<String>,
    /// Computed from the row's other columns (`GENERATED ALWAYS AS (…)`): never written.
    pub generated: Option<Generated>,
    /// The values its type allows, when the engine lists them apart from the type (a
    /// PostgreSQL enum's labels, in their order); MySQL's are in its type.
    pub values: Option<Vec<String>>,
}

/// A column computed from the row's other columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generated {
    /// Its expression, as the engine's catalog wrote it.
    pub expression: String,
    /// Kept in the row (`STORED`), or computed when read (`VIRTUAL`).
    pub stored: bool,
}

impl ColumnInfo {
    /// A nullable column of `declared_type` (its class from the type).
    pub fn new(name: impl Into<String>, declared_type: impl Into<String>) -> Self {
        let declared_type = declared_type.into();
        Self {
            name: name.into(),
            class: TypeClass::of(&declared_type),
            declared_type,
            nullable: true,
            default: None,
            in_key: false,
            key_position: None,
            auto_increment: false,
            collation: None,
            generated: None,
            values: None,
        }
    }
}

/// An index of a table.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IndexInfo {
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
    /// Made by a constraint (a primary key, a `UNIQUE` column), not by `CREATE INDEX`:
    /// it goes with the table's definition.
    pub implicit: bool,
    /// Which of its columns are in descending order, by place (missing ones: ascending).
    pub descending: Vec<bool>,
    /// A partial index's condition (`WHERE …`): the rows it holds.
    pub predicate: Option<String>,
}

impl IndexInfo {
    /// An index on `columns`, all ascending, of every row.
    pub fn new(name: impl Into<String>, columns: Vec<String>, unique: bool) -> Self {
        Self { name: name.into(), columns, unique, ..Self::default() }
    }

    /// Its column at `at` is in descending order.
    pub fn is_descending(&self, at: usize) -> bool {
        self.descending.get(at).copied().unwrap_or(false)
    }
}

/// What a foreign key does when the row it points to changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FkAction {
    #[default]
    NoAction,
    Restrict,
    Cascade,
    SetNull,
    SetDefault,
}

impl FkAction {
    pub fn sql(self) -> &'static str {
        match self {
            Self::NoAction => "NO ACTION",
            Self::Restrict => "RESTRICT",
            Self::Cascade => "CASCADE",
            Self::SetNull => "SET NULL",
            Self::SetDefault => "SET DEFAULT",
        }
    }
}

/// A foreign key: `columns` point to `references` of `table`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignKey {
    pub name: Option<String>,
    pub columns: Vec<String>,
    pub table: TableRef,
    pub references: Vec<String>,
    pub on_update: FkAction,
    pub on_delete: FkAction,
}

/// An object of the engine's schema kept as its own SQL (a trigger, a view), from its
/// catalog: rerun as written when a table is rebuilt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaObject {
    pub name: String,
    pub sql: String,
}

/// A table's structure.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Structure {
    pub table: TableRef,
    pub columns: Vec<ColumnInfo>,
    pub indexes: Vec<IndexInfo>,
    pub foreign_keys: Vec<ForeignKey>,
    /// Its rows have a `rowid` (SQLite, but `WITHOUT ROWID` tables): found by it without a
    /// primary key.
    pub rowid: bool,
    /// Its rows are found by their place in a file, checked with their fingerprint (a CSV
    /// file: [`crate::RowKey::Line`]).
    pub lines: bool,
}

/// The names a SQLite row id answers to, in the order they're tried (a column of the same
/// name hides one).
pub const ROW_ID_NAMES: [&str; 3] = ["rowid", "_rowid_", "oid"];

/// How a table's rows are found to be edited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyKind {
    /// By these columns (the primary key).
    Primary(Vec<String>),
    /// By the engine's row id.
    RowId,
    /// By the line in a file, checked with the row's fingerprint (CSV).
    Line,
    /// They can't be: the table is only read.
    None,
}

impl Structure {
    pub fn column(&self, name: &str) -> Option<&ColumnInfo> {
        self.columns.iter().find(|c| c.name == name)
    }

    /// The primary key's columns, in the key's order (the columns' where it isn't known).
    pub fn primary_key(&self) -> Vec<String> {
        let mut key: Vec<&ColumnInfo> = self.columns.iter().filter(|c| c.in_key).collect();
        // Stable: the columns without a position keep their order, after the others.
        key.sort_by_key(|c| c.key_position.unwrap_or(u16::MAX));
        key.into_iter().map(|c| c.name.clone()).collect()
    }

    /// The name its row id is read by: the first of SQLite's three (`rowid`, `_rowid_`,
    /// `oid`) no column of its own hides. None without a row id, or with all three hidden.
    pub fn row_id_name(&self) -> Option<&'static str> {
        if !self.rowid {
            return None;
        }
        ROW_ID_NAMES
            .into_iter()
            .find(|name| !self.columns.iter().any(|c| c.name.eq_ignore_ascii_case(name)))
    }

    /// How its rows are found (a primary key first, then the row id, when a name of it
    /// isn't hidden by a column, then the line in its file).
    /// Its rows can be found to be edited ([`Self::key_kind`] isn't `None`), nothing made
    /// to say how.
    pub fn has_key(&self) -> bool {
        self.columns.iter().any(|c| c.in_key) || self.row_id_name().is_some() || self.lines
    }

    pub fn key_kind(&self) -> KeyKind {
        let key = self.primary_key();
        if !key.is_empty() {
            KeyKind::Primary(key)
        } else if self.row_id_name().is_some() {
            KeyKind::RowId
        } else if self.lines {
            KeyKind::Line
        } else {
            KeyKind::None
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/structure.rs"]
mod tests;
