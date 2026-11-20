//! The grid's port: what it reads its rows through ([`GridSource`]), in plain types — no
//! database, no file: a table of a database, a CSV file, a query's result or any tabular
//! list adapts itself to it.

use std::fmt::Write as _;
use std::ops::Range;

use super::filter::GridFilter;

/// Where a column's values sit in their cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
    Center,
}

/// What kind of values a column holds: how they're shown, aligned and edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CellClass {
    #[default]
    Text,
    Integer,
    Decimal,
    Bool,
    DateTime,
    Binary,
    Json,
    Other,
}

impl CellClass {
    pub fn is_number(self) -> bool {
        matches!(self, Self::Integer | Self::Decimal)
    }
}

/// A column as the grid shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridColumn {
    pub name: String,
    /// The type shown under its name (`INTEGER`, `varchar(255)`).
    pub type_name: String,
    pub class: CellClass,
    /// Part of the key its rows are found by (a key drawn beside its name).
    pub key: bool,
    pub nullable: bool,
    /// Its values can be written (not a generated column); the source may still be
    /// read-only as a whole.
    pub editable: bool,
    /// Its values are worked out by the engine (`GENERATED ALWAYS AS`, a query's
    /// expression): "Skip Computed Columns" leaves it out of the SQL copied.
    pub computed: bool,
    /// Its values are made by the engine (an auto-increment, an identity): "Skip Generated
    /// Columns" leaves it out.
    pub generated: bool,
}

impl GridColumn {
    /// A nullable, editable column of `class`.
    pub fn new(name: impl Into<String>, type_name: impl Into<String>, class: CellClass) -> Self {
        Self {
            name: name.into(),
            type_name: type_name.into(),
            class,
            key: false,
            nullable: true,
            editable: true,
            computed: false,
            generated: false,
        }
    }

    pub fn align(&self) -> Align {
        match self.class {
            CellClass::Integer | CellClass::Decimal => Align::Right,
            CellClass::Bool => Align::Center,
            _ => Align::Left,
        }
    }
}

/// A cell's value as the source holds it (borrowed: nothing is copied to draw it).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CellRef<'a> {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(&'a str),
    Bytes(&'a [u8]),
    /// JSON, as the source gave it.
    Json(&'a str),
    /// Anything else, as text (dates, decimals, UUIDs).
    Other(&'a str),
}

impl CellRef<'_> {
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Its text as shown (bytes: their size; null: empty), written into `out`.
    pub fn write_display(&self, out: &mut String) {
        match self {
            Self::Null => {}
            Self::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Self::Int(i) => {
                let _ = write!(out, "{i}");
            }
            Self::Float(f) => {
                let _ = write!(out, "{f}");
            }
            Self::Text(t) | Self::Json(t) | Self::Other(t) => out.push_str(t),
            Self::Bytes(b) => out.push_str(&i18n::tr!("grid.bytes", count = b.len())),
        }
    }

    /// Its text as shown.
    pub fn display(&self) -> String {
        let mut out = String::new();
        self.write_display(&mut out);
        out
    }

    /// Its value as a number, when it's one (a decimal kept as text counts too).
    pub fn number(&self) -> Option<f64> {
        match self {
            Self::Int(i) => Some(*i as f64),
            Self::Float(f) if f.is_finite() => Some(*f),
            Self::Other(t) => t.trim().parse::<f64>().ok().filter(|f| f.is_finite()),
            _ => None,
        }
    }

    pub fn to_value(&self) -> CellValue {
        match *self {
            Self::Null => CellValue::Null,
            Self::Bool(b) => CellValue::Bool(b),
            Self::Int(i) => CellValue::Int(i),
            Self::Float(f) => CellValue::Float(f),
            Self::Text(t) => CellValue::Text(t.to_owned()),
            Self::Bytes(b) => CellValue::Bytes(b.to_vec()),
            Self::Json(t) => CellValue::Json(t.to_owned()),
            Self::Other(t) => CellValue::Other(t.to_owned()),
        }
    }
}

/// A cell's value, owned (an edit, a filter's operand).
#[derive(Debug, Clone, PartialEq, Default)]
pub enum CellValue {
    #[default]
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Bytes(Vec<u8>),
    Json(String),
    Other(String),
}

impl CellValue {
    pub fn as_ref(&self) -> CellRef<'_> {
        match self {
            Self::Null => CellRef::Null,
            Self::Bool(b) => CellRef::Bool(*b),
            Self::Int(i) => CellRef::Int(*i),
            Self::Float(f) => CellRef::Float(*f),
            Self::Text(t) => CellRef::Text(t),
            Self::Bytes(b) => CellRef::Bytes(b),
            Self::Json(t) => CellRef::Json(t),
            Self::Other(t) => CellRef::Other(t),
        }
    }
}

/// How many rows there are, as far as the source knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Total {
    /// Being counted.
    Pending,
    Known(u64),
    /// The source's estimate (a huge table: counted on request, [`GridSource::count`]).
    Estimate(u64),
    /// It can't be told (counting failed).
    Unknown,
    /// At least this many: those read so far, more left to read (a query's result brought a
    /// page at a time: `500+`).
    AtLeast(u64),
}

impl Total {
    /// The rows there are, when counted.
    pub fn known(self) -> Option<u64> {
        match self {
            Self::Known(n) => Some(n),
            _ => None,
        }
    }

    /// The rows there are or are estimated to be.
    pub fn approximate(self) -> Option<u64> {
        match self {
            Self::Known(n) | Self::Estimate(n) => Some(n),
            _ => None,
        }
    }

    /// The rows read so far when more are left to read ([`Total::AtLeast`]).
    pub fn at_least(self) -> Option<u64> {
        match self {
            Self::AtLeast(n) => Some(n),
            _ => None,
        }
    }
}

/// Whether a row's values are there to draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowState {
    Ready,
    /// Asked for, still coming.
    Pending,
    /// Its page failed (the source's status says why).
    Failed,
    /// Past the last row.
    Missing,
}

/// One column of an order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortKey {
    pub column: String,
    pub descending: bool,
}

/// What the grid shows of the source: in which order, which rows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GridQuery {
    pub sort: Vec<SortKey>,
    pub filter: Option<GridFilter>,
}

/// How the source is doing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SourceStatus {
    #[default]
    Ready,
    /// Something it was asked is still coming.
    Loading,
    /// The last thing it was asked failed (the message for the user).
    Failed(String),
}

/// What the source can do (the grid offers only that).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GridCaps {
    /// Rows sorted at the source.
    pub sort: bool,
    /// Rows filtered at the source.
    pub filter: bool,
    /// Why it can't sort or filter (shown where the fields would be), when it can't.
    pub limit: Option<String>,
    /// A slow page can be cancelled.
    pub cancel: bool,
}

/// What can be edited, and why not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Editing {
    /// Its rows are changed through [`GridSource::submit`].
    Allowed,
    /// Not now, or never: the reason, for the user.
    ReadOnly(String),
}

impl Editing {
    pub fn allowed(&self) -> bool {
        matches!(self, Self::Allowed)
    }
}

/// What's pending on a cell or its row (drawn in its color until submitted or reverted).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CellMark {
    #[default]
    None,
    /// Its value was changed.
    Modified,
    /// Its row is new.
    Inserted,
    /// Its row will be deleted.
    Deleted,
}

/// Whether a cell's value is here whole, for its editor (a source may keep big values cut).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Whole {
    Here,
    /// Asked for: its editor opens once it comes.
    Coming,
    /// It couldn't be read (why, for the user).
    Failed(String),
}

/// Rows added in the grid (not submitted yet) are known by ids from here up, shown after
/// the page's rows.
pub const NEW_ROWS: u64 = 1 << 62;

/// The grid's source of rows (see the module's doc). Nothing blocks: what isn't there is
/// [`RowState::Pending`] (asked by [`GridSource::request`]) until [`GridSource::poll`]
/// says something came.
pub trait GridSource: Send {
    fn columns(&self) -> &[GridColumn];
    fn total(&self) -> Total;
    /// Row `row` (in the source's order for `query`, 0-based).
    fn row_state(&self, row: u64) -> RowState;
    /// A value of a [`RowState::Ready`] row (`Null` for any other).
    fn cell(&self, row: u64, column: usize) -> CellRef<'_>;
    /// The rows the grid shows now, in `query`'s order and filter: asked for if they
    /// aren't there (only what's missing; asked again with the same, nothing happens).
    fn request(&mut self, query: &GridQuery, rows: Range<u64>);
    /// Something came since the last call (rows, the total, the columns): the grid draws
    /// again.
    fn poll(&mut self) -> bool;
    fn status(&self) -> SourceStatus;
    fn capabilities(&self) -> GridCaps;
    /// The name rows copied as SQL `INSERT`s go into.
    fn name(&self) -> String;
    /// How its SQL writes names and values (its dialect: what the SQL extractors write);
    /// `None`: standard SQL.
    fn sql(&self) -> Option<std::sync::Arc<dyn extractors::SqlStyle>> {
        None
    }
    /// Stops what's being read.
    fn cancel(&mut self) {}
    /// Lets go of what's known: read again.
    fn refresh(&mut self) {}
    /// Counts the rows exactly (an estimate shown until then).
    fn count(&mut self) {}
    /// Whether its rows can be edited now.
    fn editing(&self) -> Editing {
        Editing::ReadOnly(i18n::t("grid.read_only"))
    }
    /// What's pending on a cell (a new or deleted row marks all of its cells).
    fn mark(&self, row: u64, column: usize) -> CellMark {
        let _ = (row, column);
        CellMark::None
    }
    /// An icon before a cell's text, in its tone (a log line's level): its name in the
    /// app's set. `None`: none.
    fn cell_icon(&self, row: u64, column: usize) -> Option<(&'static str, crate::ds::Tone)> {
        let _ = (row, column);
        None
    }
    /// What the WHERE field shows while it's empty, when the source has an example of its
    /// own (its columns' names); `None`: the grid's.
    fn filter_hint(&self) -> Option<String> {
        None
    }
    /// A new row's cell left to the engine (its default), drawn as such.
    fn is_default(&self, row: u64, column: usize) -> bool {
        let _ = (row, column);
        false
    }
    /// A cell's new value (kept pending). `Err`: why it can't be (for the user).
    fn set(&mut self, row: u64, column: usize, value: CellValue) -> Result<(), String> {
        let _ = (row, column, value);
        Err(i18n::t("grid.read_only"))
    }
    /// A new row (a copy of `like`'s values when given; its key and generated columns
    /// left to the engine): its id ([`NEW_ROWS`] and up).
    fn insert(&mut self, like: Option<u64>) -> Option<u64> {
        let _ = like;
        None
    }
    /// The rows marked for deletion (a new row is just dropped; marked again: unmarked).
    fn delete(&mut self, rows: &[u64]) {
        let _ = rows;
    }
    /// The changes of these rows dropped (`None`: all of them).
    fn revert(&mut self, rows: Option<&[u64]>) {
        let _ = rows;
    }
    /// How many changes wait to be submitted.
    fn pending(&self) -> usize {
        0
    }
    /// The new rows' ids, in the order added (shown after the page's rows).
    fn new_rows(&self) -> Vec<u64> {
        Vec::new()
    }
    /// Whether a cell's value is here whole before it's edited (one kept cut is asked for
    /// then, and comes later: [`Whole::Coming`]).
    fn whole(&mut self, row: u64, column: usize) -> Whole {
        let _ = (row, column);
        Whole::Here
    }
}
