//! A console's results: each result set a run brings ([`ResultSet`]: its columns, its rows
//! in a [`ResultBuffer`], where the rest is read from — [`More`] — and how long it took),
//! kept by the console's runner while their tabs are in the Services window.
//!
//! The rows come in chunks, shared with the views (an `Arc` each: a view takes the chunks it
//! knows without copying a row), their bytes counted; a cell bigger than [`CELL_LIMIT`] is
//! kept cut, with its size said. A console holds at most its share of memory
//! ([`Results::trim`]): past it, the oldest unpinned results let their rows go (a pure query's
//! are read again when its tab is shown; a write's are gone, it's never run again).

use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use database::edits::{EditableResult, NotEditable};
use database::run::CursorToken;
use database::sql::QueryShape;
use database::{Applied, Column, DbError, Filter, Order, PlanNode, RowKey, Value};
use ui_components::ServiceId;
use ui_components::data_grid::{CellClass, CellRef, GridColumn};

/// Rows a chunk holds (the last one may hold fewer).
pub const CHUNK_ROWS: usize = 1000;

/// A text or bytes cell past this size is kept cut (its size said).
pub const CELL_LIMIT: usize = 64 * 1024;

/// A row of a result.
pub type Row = Box<[Value]>;

/// A result's rows, in chunks (see the module's doc).
#[derive(Debug, Clone, Default)]
pub struct ResultBuffer {
    chunks: Vec<Arc<[Row]>>,
    /// The first row of each chunk.
    starts: Vec<u64>,
    /// The rows of the chunk being filled (sealed every [`CHUNK_ROWS`], and at a page's end).
    open: Vec<Row>,
    len: u64,
    bytes: usize,
    /// Cells kept cut.
    cut: u64,
    /// Where they are (row, column), in the order the rows came.
    cut_at: Vec<(u64, u32)>,
    /// Bumped whenever its rows are let go (read again: a row's place isn't the same).
    epoch: u64,
}

impl ResultBuffer {
    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The bytes its rows hold (an estimate: the values and their text).
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Cells kept cut (bigger than [`CELL_LIMIT`]).
    pub fn cut(&self) -> u64 {
        self.cut
    }

    /// Where its cut cells are (row, column), in order.
    pub fn cut_at(&self) -> &[(u64, u32)] {
        &self.cut_at
    }

    /// Changes whenever its rows are let go.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// A row added (its big cells cut).
    pub fn push(&mut self, row: Vec<Value>) {
        let at = self.len;
        let row: Row = row
            .into_iter()
            .enumerate()
            .map(|(column, v)| {
                let (v, cut) = cut_cell(v);
                if cut {
                    self.cut += 1;
                    self.cut_at.push((at, column as u32));
                }
                v
            })
            .collect();
        self.bytes += row_bytes(&row);
        self.open.push(row);
        self.len += 1;
        if self.open.len() >= CHUNK_ROWS {
            self.seal();
        }
    }

    /// The rows being filled made a chunk (seen by the views from now).
    pub fn seal(&mut self) {
        if self.open.is_empty() {
            return;
        }
        let rows: Vec<Row> = std::mem::take(&mut self.open);
        let start = self.len - rows.len() as u64;
        self.starts.push(start);
        self.chunks.push(rows.into());
    }

    /// The sealed chunks (what a view reads), and the rows they hold.
    pub fn chunks(&self) -> (Vec<Arc<[Row]>>, Vec<u64>, u64) {
        let sealed = self.len - self.open.len() as u64;
        (self.chunks.clone(), self.starts.clone(), sealed)
    }

    /// Every row let go.
    pub fn clear(&mut self) {
        let epoch = self.epoch + 1;
        *self = Self::default();
        self.epoch = epoch;
    }

    /// Row `i` (sealed or not).
    pub fn row(&self, i: u64) -> Option<&[Value]> {
        let sealed = self.len - self.open.len() as u64;
        if i >= sealed {
            return self.open.get((i - sealed) as usize).map(|r| &r[..]);
        }
        row_in(&self.chunks, &self.starts, i)
    }
}

/// Row `i` of `chunks` (their first rows `starts`).
pub fn row_in<'a>(chunks: &'a [Arc<[Row]>], starts: &[u64], i: u64) -> Option<&'a [Value]> {
    let at = starts.partition_point(|&s| s <= i).checked_sub(1)?;
    let chunk = chunks.get(at)?;
    chunk.get((i - starts[at]) as usize).map(|r| &r[..])
}

/// A value's bytes, as counted.
fn value_bytes(value: &Value) -> usize {
    std::mem::size_of::<Value>()
        + match value {
            Value::Text(t) | Value::Json(t) | Value::Other(t) => t.len(),
            Value::Bytes(b) => b.len(),
            _ => 0,
        }
}

fn row_bytes(row: &[Value]) -> usize {
    std::mem::size_of::<Row>() + row.iter().map(value_bytes).sum::<usize>()
}

/// A big cell cut to [`CELL_LIMIT`] (text on a character's boundary, its size said after it).
fn cut_cell(value: Value) -> (Value, bool) {
    let size = |n: usize| i18n::tr!("console.cell_cut", size = i18n::bytes(n as u64));
    match value {
        Value::Text(t) if t.len() > CELL_LIMIT => {
            let mut end = CELL_LIMIT;
            while !t.is_char_boundary(end) {
                end -= 1;
            }
            (Value::Text(format!("{}… {}", &t[..end], size(t.len()))), true)
        }
        Value::Json(t) if t.len() > CELL_LIMIT => {
            let mut end = CELL_LIMIT;
            while !t.is_char_boundary(end) {
                end -= 1;
            }
            (Value::Text(format!("{}… {}", &t[..end], size(t.len()))), true)
        }
        Value::Bytes(b) if b.len() > CELL_LIMIT => (Value::Bytes(b[..CELL_LIMIT].to_vec()), true),
        other => (other, false),
    }
}

/// Where the rest of a result is read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum More {
    /// Nothing left: every row is here.
    None,
    /// The rest waits in the session's cursor (closed by the next statement it runs).
    Cursor(CursorToken),
    /// A pure query: the rest is read by running it again, wrapped, from the next row.
    Rerun,
    /// A statement that wrote: the rest wasn't brought, and won't be (it's never run again).
    Lost,
}

impl More {
    /// More rows can be read.
    pub fn readable(&self) -> bool {
        matches!(self, Self::Cursor(_) | Self::Rerun)
    }
}

/// Whether a result's rows can be edited, as far as the runner knows (plan 39l).
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Editability {
    /// Not asked yet (its tab not shown).
    #[default]
    Unknown,
    /// Its table's structure is being read.
    Reading,
    /// They can: its table, the key its rows are found by.
    Ready(Arc<EditableResult>),
    /// They can't, by its query or its table: why.
    Refused(NotEditable),
    /// Its table's structure couldn't be read (why, for the user).
    Failed(String),
}

/// A big cell's whole value read for its editor: its row's key, its column, the value (or
/// why it couldn't be read).
pub type WholeValue = (RowKey, usize, Result<Arc<Value>, String>);

/// How the engine filters and sorts a pure query's result (run again over it).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ServerView {
    pub filter: Option<Filter>,
    pub order: Vec<Order>,
}

impl ServerView {
    pub fn is_empty(&self) -> bool {
        self.filter.is_none() && self.order.is_empty()
    }
}

/// One result set (see the module's doc).
#[derive(Debug, Clone)]
pub struct ResultSet {
    pub id: ServiceId,
    /// `Result N` of its console (counted over its runs).
    pub number: u64,
    /// Its tab's title: `database.schema.table` for a query of one table, else `Result N`.
    pub title: String,
    /// The statement as run (passwords hidden), and its place in the console's text.
    pub statement: String,
    pub range: Range<usize>,
    /// It only reads: it can be run again (next pages, sorting, refreshing).
    pub rerunnable: bool,
    pub columns: Vec<Column>,
    pub rows: ResultBuffer,
    pub more: More,
    /// The engine's filter and order it was last read with.
    pub view: ServerView,
    /// Until the first row came, and the rest.
    pub execution: Duration,
    pub fetching: Duration,
    pub pinned: bool,
    /// Its rows were let go for memory (read again when shown, a pure query's).
    pub dropped: bool,
    /// Rows are being read for it.
    pub loading: bool,
    /// Why the last read for it failed.
    pub error: Option<String>,
    /// Bumped with each change (its view reads it again then).
    pub generation: u64,
    /// What its query is made of, for editing its rows (or why they can't be).
    pub shape: Result<QueryShape, NotEditable>,
    /// Whether its rows can be edited (once its tab asked).
    pub editing: Editability,
    /// Its changes are being submitted.
    pub submitting: bool,
    /// How the last submit of its changes ended (taken by its view).
    pub submitted: Option<Result<Applied, DbError>>,
    /// The whole values of big cells read for their editors.
    pub wholes: Vec<WholeValue>,
}

impl ResultSet {
    pub fn new(number: u64, title: String, statement: String, range: Range<usize>) -> Self {
        Self {
            id: ServiceId::new(),
            number,
            title,
            statement,
            range,
            rerunnable: false,
            columns: Vec::new(),
            rows: ResultBuffer::default(),
            more: More::None,
            view: ServerView::default(),
            execution: Duration::ZERO,
            fetching: Duration::ZERO,
            pinned: false,
            dropped: false,
            loading: false,
            error: None,
            generation: 0,
            shape: Err(NotEditable::NotQuery),
            editing: Editability::Unknown,
            submitting: false,
            submitted: None,
            wholes: Vec::new(),
        }
    }

    pub fn touch(&mut self) {
        self.generation += 1;
    }
}

/// A statement's profile (Explain, Explain Analyze), shown in a Profile tab of its own: its nodes
/// (shared with the view, never copied), the statement as sent (passwords hidden), how long
/// it took. As a result's, an unpinned one is replaced by the next plan or run.
#[derive(Debug, Clone)]
pub struct PlanSet {
    pub id: ServiceId,
    /// `Profile`, or `Profile · table` for a query of one table.
    pub title: String,
    pub statement: String,
    pub range: Range<usize>,
    pub nodes: Arc<Vec<PlanNode>>,
    /// It ran (`ANALYZE`): its rows and times are real.
    pub analyze: bool,
    /// It wrote, and was rolled back (an analyzed write).
    pub rolled_back: bool,
    pub elapsed: Duration,
    pub pinned: bool,
    pub generation: u64,
    /// The profile it shows (kept by the console's runner).
    pub profile: Option<database::profile::ProfileId>,
    /// The one table it queries (its editor tab's title says it).
    pub table: Option<String>,
    /// The profile itself (shared with the console's profiles: kept while its tab is).
    pub report: Option<Arc<database::profile::ProfileReport>>,
    /// The hash of the statement's text in the console when it was profiled (what a
    /// finding lights there only while it's the same).
    pub document_hash: Option<u64>,
    /// What was known of its tables when it was made (the console's profiles keep the
    /// latest, their statistics read later included).
    pub facts: Option<Arc<database::profile::rules::Facts>>,
}

impl PlanSet {
    pub fn new(
        title: String,
        statement: String,
        range: Range<usize>,
        nodes: Vec<PlanNode>,
    ) -> Self {
        Self {
            id: ServiceId::new(),
            title,
            statement,
            range,
            nodes: Arc::new(nodes),
            analyze: false,
            rolled_back: false,
            profile: None,
            report: None,
            table: None,
            document_hash: None,
            facts: None,
            elapsed: Duration::ZERO,
            pinned: false,
            generation: 0,
        }
    }

    /// The bytes it holds (an estimate: its nodes' texts).
    pub fn bytes(&self) -> usize {
        self.nodes
            .iter()
            .flat_map(PlanNode::walk)
            .map(|n| {
                std::mem::size_of::<PlanNode>()
                    + n.operation.len()
                    + n.object.as_ref().map_or(0, String::len)
                    + n.detail.as_ref().map_or(0, String::len)
            })
            .sum::<usize>()
            + self.statement.len()
    }
}

/// A console's results, oldest first, and their generation.
#[derive(Debug, Default)]
pub struct Results {
    pub sets: Vec<ResultSet>,
    /// The plans explained, oldest first (their tabs after the results').
    pub plans: Vec<PlanSet>,
    /// `Result N`'s next number.
    pub next: u64,
    /// Bumped when a set comes or goes (or is pinned): the window's tabs.
    pub generation: u64,
}

impl Results {
    pub fn get(&self, id: ServiceId) -> Option<&ResultSet> {
        self.sets.iter().find(|s| s.id == id)
    }

    pub fn get_mut(&mut self, id: ServiceId) -> Option<&mut ResultSet> {
        self.sets.iter_mut().find(|s| s.id == id)
    }

    pub fn plan(&self, id: ServiceId) -> Option<&PlanSet> {
        self.plans.iter().find(|p| p.id == id)
    }

    pub fn plan_mut(&mut self, id: ServiceId) -> Option<&mut PlanSet> {
        self.plans.iter_mut().find(|p| p.id == id)
    }

    /// A new plan comes: the unpinned plans go (the pinned ones stay).
    pub fn replace_unpinned_plans(&mut self) {
        self.plans.retain(|p| p.pinned);
        self.generation += 1;
    }

    /// A new run starts: the unpinned results go (the next run replaces them), the
    /// pinned ones stay. What's gone holding a cursor, said.
    pub fn replace_unpinned(&mut self) -> Vec<CursorToken> {
        let mut cursors = Vec::new();
        // And the unpinned plans: a run replaces what the console showed.
        self.plans.retain(|p| p.pinned);
        self.sets.retain(|s| {
            if s.pinned {
                return true;
            }
            if let More::Cursor(token) = &s.more {
                cursors.push(token.clone());
            }
            false
        });
        self.generation += 1;
        cursors
    }

    /// The session runs another statement: what was left in its cursor can't be read from
    /// it any more (a pure query's rest is read by running it again; a write's is lost).
    pub fn cursors_closed(&mut self) {
        for set in &mut self.sets {
            if matches!(set.more, More::Cursor(_)) {
                set.more = if set.rerunnable { More::Rerun } else { More::Lost };
                set.touch();
            }
        }
    }

    /// A result keeps a cursor open on the session (it's never closed for idleness then).
    pub fn has_cursor(&self) -> bool {
        self.sets.iter().any(|s| matches!(s.more, More::Cursor(_)))
    }

    /// The bytes every result holds (their rows; the plans' nodes are small).
    pub fn bytes(&self) -> usize {
        self.sets.iter().map(|s| s.rows.bytes()).sum()
    }

    /// The bytes the plans hold.
    pub fn plan_bytes(&self) -> usize {
        self.plans.iter().map(PlanSet::bytes).sum()
    }

    /// Kept within `limit` bytes: the oldest unpinned results let their rows go first, then
    /// the oldest pinned ones; never `keep` (the one being read). How many let go.
    pub fn trim(&mut self, limit: usize, keep: Option<ServiceId>) -> usize {
        let mut dropped = 0;
        for pinned in [false, true] {
            for i in 0..self.sets.len() {
                if self.bytes() <= limit {
                    return dropped;
                }
                let set = &mut self.sets[i];
                if set.pinned != pinned || Some(set.id) == keep || set.rows.is_empty() {
                    continue;
                }
                set.rows.clear();
                set.dropped = true;
                if let More::Cursor(_) = set.more {
                    set.more = if set.rerunnable { More::Rerun } else { More::Lost };
                }
                set.touch();
                dropped += 1;
            }
        }
        dropped
    }
}

/// A column's class, from its type's name and, when it says nothing, the values it holds.
pub fn class_of(column: &Column, sample: Option<&Value>) -> CellClass {
    let t = column.type_name.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| t.contains(w));
    if has(&["bool"]) {
        CellClass::Bool
    } else if has(&["int", "serial", "oid"]) {
        CellClass::Integer
    } else if has(&["real", "float", "double", "numeric", "decimal", "money"]) {
        CellClass::Decimal
    } else if has(&["date", "time"]) {
        CellClass::DateTime
    } else if has(&["json"]) {
        CellClass::Json
    } else if has(&["blob", "bytea", "binary"]) {
        CellClass::Binary
    } else if has(&["char", "text", "clob", "uuid", "name", "enum"]) {
        CellClass::Text
    } else {
        match sample {
            Some(Value::Int(_)) => CellClass::Integer,
            Some(Value::Float(_)) => CellClass::Decimal,
            Some(Value::Bool(_)) => CellClass::Bool,
            Some(Value::Bytes(_)) => CellClass::Binary,
            Some(Value::Json(_)) => CellClass::Json,
            _ => CellClass::Text,
        }
    }
}

/// A result's columns as the grid shows them (read only until the table's structure says
/// which can be edited: [`crate::result_source`]).
pub fn grid_columns(columns: &[Column], first: Option<&[Value]>) -> Vec<GridColumn> {
    columns
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let sample = first.and_then(|r| r.get(i)).filter(|v| **v != Value::Null);
            let mut column =
                GridColumn::new(c.name.clone(), c.type_name.clone(), class_of(c, sample));
            column.editable = false;
            column
        })
        .collect()
}

/// A value as the grid draws it (borrowed).
pub fn cell_ref(value: &Value) -> CellRef<'_> {
    match value {
        Value::Null => CellRef::Null,
        Value::Bool(b) => CellRef::Bool(*b),
        Value::Int(i) => CellRef::Int(*i),
        Value::Float(f) => CellRef::Float(*f),
        Value::Text(t) => CellRef::Text(t),
        Value::Bytes(b) => CellRef::Bytes(b),
        Value::Json(t) => CellRef::Json(t),
        Value::Other(t) => CellRef::Other(t),
    }
}

#[cfg(test)]
#[path = "../tests/unit/results.rs"]
mod tests;
