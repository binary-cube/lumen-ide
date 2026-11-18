//! Reading a table a page at a time, and changing its rows: a set of changes kept pending,
//! shown, then applied together (all or none), each row found by its [`RowKey`].

use crate::{TableRef, Value};

/// How a row is found again to be changed.
#[derive(Debug, Clone, PartialEq)]
pub enum RowKey {
    /// Its primary key's columns and values.
    Primary(Vec<(String, Value)>),
    /// The engine's row id (SQLite).
    RowId(i64),
    /// Its line in a file (0-based, the header not counted), and the fingerprint of the
    /// row as it was read ([`fingerprint`]): a file changed since is a conflict.
    Line { line: u64, fingerprint: u64 },
}

/// A row's fingerprint (FNV-1a over its values, each tagged by its kind): the same values
/// give the same one in every run and on every platform.
pub fn fingerprint(values: &[Value]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    let mut eat = |bytes: &[u8]| {
        for &b in bytes {
            hash ^= u64::from(b);
            hash = hash.wrapping_mul(PRIME);
        }
    };
    for value in values {
        match value {
            Value::Null => eat(&[0]),
            Value::Bool(b) => eat(&[1, u8::from(*b)]),
            Value::Int(i) => {
                eat(&[2]);
                eat(&i.to_le_bytes());
            }
            Value::Float(f) => {
                eat(&[3]);
                eat(&f.to_bits().to_le_bytes());
            }
            Value::Text(t) => {
                eat(&[4]);
                eat(&(t.len() as u64).to_le_bytes());
                eat(t.as_bytes());
            }
            Value::Bytes(b) => {
                eat(&[5]);
                eat(&(b.len() as u64).to_le_bytes());
                eat(b);
            }
            Value::Json(t) => {
                eat(&[6]);
                eat(&(t.len() as u64).to_le_bytes());
                eat(t.as_bytes());
            }
            Value::Other(t) => {
                eat(&[7]);
                eat(&(t.len() as u64).to_le_bytes());
                eat(t.as_bytes());
            }
        }
    }
    hash
}

/// One column of a page's order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Order {
    pub column: String,
    pub descending: bool,
}

/// How a filter compares a column with a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    Eq,
    NotEq,
    Less,
    LessEq,
    Greater,
    GreaterEq,
}

impl CompareOp {
    pub fn sql(self) -> &'static str {
        match self {
            Self::Eq => "=",
            Self::NotEq => "<>",
            Self::Less => "<",
            Self::LessEq => "<=",
            Self::Greater => ">",
            Self::GreaterEq => ">=",
        }
    }
}

/// Which rows a page shows: built from the model (columns by name, values as values), never
/// from text typed as SQL.
#[derive(Debug, Clone, PartialEq)]
pub enum Filter {
    Compare {
        column: String,
        op: CompareOp,
        value: Value,
    },
    IsNull(String),
    NotNull(String),
    /// The column's text holds `text` (any case).
    Contains {
        column: String,
        text: String,
    },
    And(Vec<Filter>),
    Or(Vec<Filter>),
    Not(Box<Filter>),
}

/// A page of a table's rows.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub offset: u64,
    pub limit: u32,
    pub order: Vec<Order>,
    pub filter: Option<Filter>,
}

impl Page {
    /// The first `limit` rows, in the engine's order.
    pub fn first(limit: u32) -> Self {
        Self { offset: 0, limit, order: Vec::new(), filter: None }
    }
}

/// A cell changed: its column, the value read (a row changed since is a conflict) and the
/// new one.
#[derive(Debug, Clone, PartialEq)]
pub struct CellChange {
    pub column: String,
    pub old: Value,
    pub new: Value,
}

/// A row's change.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    /// A new row: the columns given (the others take their defaults).
    Insert {
        values: Vec<(String, Value)>,
    },
    Update {
        key: RowKey,
        cells: Vec<CellChange>,
    },
    Delete {
        key: RowKey,
    },
}

/// The pending changes of one table, applied together.
#[derive(Debug, Clone, PartialEq)]
pub struct ChangeSet {
    pub table: TableRef,
    pub changes: Vec<Change>,
}

impl ChangeSet {
    pub fn new(table: TableRef) -> Self {
        Self { table, changes: Vec::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// How many rows it deletes.
    pub fn deletes(&self) -> usize {
        self.changes.iter().filter(|c| matches!(c, Change::Delete { .. })).count()
    }
}

/// What applying changes did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Applied {
    /// Rows inserted, updated or deleted (for schema changes: the statements run).
    pub affected: u64,
    /// The keys of the rows inserted, in order, when the engine gives them back.
    pub inserted: Vec<RowKey>,
}
