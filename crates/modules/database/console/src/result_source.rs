//! A console's result as the grid reads it: [`ResultSource`] adapts a result the runner keeps
//! ([`crate::results::ResultSet`]) to the grid's [`GridSource`] port.
//!
//! A result of one table's rows (plan 39l: its query a `SELECT` of the table alone, its key
//! among its columns — [`database::edits::editable_result`], the table's structure read on
//! the console's connection once its tab is shown) is edited as the data view's grid is: its
//! cells, new rows (cloned or not), rows deleted, all kept pending here ([`Edits`], by each
//! row's key) until submitted ([`crate::edit`]); a big cell kept cut is read whole from its
//! table before its editor opens. Anything else stays read-only, the reason said.
//!
//! Its rows are the runner's chunks, shared (taken again only when the result's generation
//! changes). Past the rows here, the grid's rows are asked from the runner: the next page,
//! from the session's cursor or by running a pure query again from the next row; a write's
//! rest is never read (it's never run again). Sorting and filtering are done here, over every
//! row, once they're all here (in the background: the order worked out on the pool, the rows
//! never copied) — or by the engine, running a pure query again wrapped in the order and
//! filter, while more are left. Refreshing runs a pure query again; a write, never.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, Mutex, PoisonError};

use database::edits::{EditableResult, Edits, NotEditable};
use database::{ChangeSet, CompareOp, Filter, Order, RowKey, Value};
use ui_components::ServiceId;
use ui_components::data_grid::{
    CellMark, CellRef, CellValue, Compare, Editing, GridCaps, GridColumn, GridFilter, GridQuery,
    GridSource, NEW_ROWS, RowState, SourceStatus, Total, Whole,
};

use crate::results::{Editability, Row, ServerView, cell_ref, grid_columns, row_in};
use crate::runner::{ResultView, Runner};
use crate::runtime::Runtime;

/// The order (and filter) worked out over every row: the rows' places, in the order shown.
type Order32 = Arc<Vec<u32>>;

/// A local order on its way: the query it's for, and what came.
type Sorting = Arc<Mutex<Option<(GridQuery, Order32)>>>;

/// A result for the grid (see the module's doc).
pub struct ResultSource {
    runner: Runner,
    id: ServiceId,
    runtime: Runtime,
    /// The result as last read (its chunks shared).
    view: Option<ResultView>,
    columns: Vec<GridColumn>,
    query: GridQuery,
    /// The order shown, over every row here (`None`: theirs).
    local: Option<Order32>,
    /// The query [`Self::local`] was worked out for, and one on its way.
    local_for: Option<GridQuery>,
    sorting: Sorting,
    sorting_for: Option<GridQuery>,
    /// The next page was asked (and not come yet).
    asked: bool,
    /// The rows let go for memory were asked again.
    refilled: bool,
    /// Something it did was refused (a write isn't refreshed): taken by its view.
    notice: Option<String>,
    /// Not polled yet: the first poll says it changed (the grid takes its columns then).
    fresh: bool,
    /// What its source allows (a read-only one, a driver that doesn't edit rows: never).
    rights: Rights,
    /// The changes waiting to be submitted, by each row's key.
    edits: Edits,
    /// The rows with changes, by their place among the chunks: their keys.
    edited: HashMap<u64, RowKey>,
    /// The rows' epoch [`Self::edited`]'s places are of.
    epoch: u64,
    /// The whole values of big cells read for their editors, by their place and column.
    wholes: HashMap<(u64, usize), Arc<Value>>,
    /// A big cell asked whole (its place, its column, its row's key).
    asked_whole: Option<(u64, usize, RowKey)>,
    /// Its columns were made for its table's structure (key, nullable, writable).
    columns_editable: bool,
}

/// What a result's source allows its rows (as the console's source is now).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rights {
    pub read_only: bool,
    /// The driver edits rows.
    pub edit_rows: bool,
}

impl Default for Rights {
    fn default() -> Self {
        Self { read_only: false, edit_rows: true }
    }
}

impl std::fmt::Debug for ResultSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResultSource").field("id", &self.id).finish_non_exhaustive()
    }
}

impl ResultSource {
    /// Result `id` of `runner`; its local order worked out on `runtime`.
    pub fn new(runner: Runner, id: ServiceId, runtime: Runtime) -> Self {
        let mut source = Self {
            runner,
            id,
            runtime,
            view: None,
            columns: Vec::new(),
            query: GridQuery::default(),
            local: None,
            local_for: None,
            sorting: Arc::default(),
            sorting_for: None,
            asked: false,
            refilled: false,
            notice: None,
            fresh: true,
            rights: Rights::default(),
            edits: Edits::default(),
            edited: HashMap::new(),
            epoch: 0,
            wholes: HashMap::new(),
            asked_whole: None,
            columns_editable: false,
        };
        source.read();
        source
    }

    /// What its source allows now (read-only, its driver editing rows or not).
    pub fn set_rights(&mut self, rights: Rights) {
        self.rights = rights;
    }

    /// The result as it's edited, once its table's structure says it can be.
    pub fn editable(&self) -> Option<&Arc<EditableResult>> {
        match &self.view.as_ref()?.editing {
            Editability::Ready(e) => Some(e),
            _ => None,
        }
    }

    pub fn edits(&self) -> &Edits {
        &self.edits
    }

    /// The pending changes as submitted (`None`: none, or it isn't edited).
    pub fn change_set(&self) -> Option<ChangeSet> {
        let editable = self.editable()?;
        let view = self.view.as_ref()?;
        let set =
            self.edits.change_set(editable.table.clone(), &editable.column_infos(&view.columns));
        (!set.is_empty()).then_some(set)
    }

    /// Its changes are being submitted.
    pub fn submitting(&self) -> bool {
        self.view.as_ref().is_some_and(|v| v.submitting)
    }

    /// How the last submit ended, once (applied: its changes are gone, the rows read again).
    pub fn take_submitted(&mut self) -> Option<Result<database::Applied, database::DbError>> {
        let outcome = self.runner.take_submitted(self.id)?;
        if outcome.is_ok() {
            self.edits.clear();
            self.edited.clear();
            self.wholes.clear();
            self.asked_whole = None;
        }
        Some(outcome)
    }

    /// The table and the result column a cell of `row` points to by a foreign key of its
    /// table, with the cell's value (`None`: not a key's column, or no value).
    pub fn reference(
        &self,
        row: u64,
        column: usize,
    ) -> Option<(database::TableRef, String, Value)> {
        let editable = self.editable()?;
        let name = &editable.column(column)?.name;
        let (table, target) = editable.structure.foreign_keys.iter().find_map(|k| {
            let at = k.columns.iter().position(|c| c == name)?;
            Some((k.table.clone(), k.references.get(at)?.clone()))
        })?;
        let value = self.whole_or_cell(row, column)?;
        (value != Value::Null).then_some((table, target, value))
    }

    /// Why its rows can't be edited, or `None`: they can.
    fn refusal(&self) -> Option<String> {
        let Some(view) = &self.view else { return Some(i18n::t("console.result_gone")) };
        if self.rights.read_only {
            return Some(i18n::t("console.edit_ro_source"));
        }
        if !self.rights.edit_rows {
            return Some(i18n::t("database.ro_driver"));
        }
        if let Some(why) = &view.refused {
            return Some(refused_text(why));
        }
        if view.submitting {
            return Some(i18n::t("database.ro_submitting"));
        }
        match &view.editing {
            Editability::Ready(_) => None,
            Editability::Unknown | Editability::Reading => Some(i18n::t("console.edit_reading")),
            Editability::Refused(why) => Some(refused_text(why)),
            Editability::Failed(why) => Some(i18n::tr!("console.edit_failed", why = why.as_str())),
        }
    }

    /// Row `row`'s values as read (`None`: not here), and its place among the chunks.
    fn read_row(&self, row: u64) -> Option<(u64, &[Value])> {
        let at = self.place(row)?;
        let view = self.view.as_ref()?;
        Some((at, row_in(&view.chunks, &view.starts, at)?))
    }

    /// The cell at the chunks' `at`, column `column`, is kept cut.
    fn is_cut(&self, at: u64, column: usize) -> bool {
        let Some(view) = &self.view else { return false };
        view.cut_at.binary_search(&(at, column as u32)).is_ok()
    }

    /// A cell's value whole: the one read for a big cell, else as read (`None`: a big cell
    /// not read whole, or no row).
    fn whole_or_cell(&self, row: u64, column: usize) -> Option<Value> {
        let (at, values) = self.read_row(row)?;
        if let Some(whole) = self.wholes.get(&(at, column)) {
            return Some((**whole).clone());
        }
        if self.is_cut(at, column) {
            return None;
        }
        values.get(column).cloned()
    }

    /// Row `row`'s key and its values as read, its big cells whole where they were read.
    fn original(&self, row: u64) -> Option<(u64, RowKey, Vec<Value>)> {
        let editable = self.editable()?;
        let (at, values) = self.read_row(row)?;
        let key = editable.key_of(values)?;
        let original = values
            .iter()
            .enumerate()
            .map(|(c, v)| self.wholes.get(&(at, c)).map_or_else(|| v.clone(), |w| (**w).clone()))
            .collect();
        Some((at, key, original))
    }

    /// The key of the row at the chunks' `at`, when it has changes.
    fn edited_key(&self, row: u64) -> Option<&RowKey> {
        if self.edited.is_empty() {
            return None;
        }
        self.edited.get(&self.place(row)?)
    }

    /// The rows read again (their places changed): the changed rows found again by key, the
    /// whole values let go.
    fn rows_moved(&mut self) {
        self.wholes.clear();
        self.asked_whole = None;
        self.edited.clear();
        if self.edits.is_empty() {
            return;
        }
        let (Some(editable), Some(view)) = (self.editable().cloned(), &self.view) else { return };
        let mut wanted: Vec<RowKey> = self.edits.updated().iter().map(|u| u.key.clone()).collect();
        wanted.extend(self.edits.deleted().iter().cloned());
        for at in 0..view.len {
            if wanted.is_empty() {
                break;
            }
            let Some(key) = row_in(&view.chunks, &view.starts, at).and_then(|r| editable.key_of(r))
            else {
                continue;
            };
            if let Some(i) = wanted.iter().position(|k| *k == key) {
                wanted.swap_remove(i);
                self.edited.insert(at, key);
            }
        }
    }

    /// The grid's columns made for the table's structure: its key, nullable and writable
    /// columns (the others read only).
    fn edit_columns(&mut self) {
        let Some(editable) = self.editable().cloned() else { return };
        for (i, column) in self.columns.iter_mut().enumerate() {
            let info = editable.column(i);
            column.key = editable.in_key(i);
            column.nullable = info.is_none_or(|c| c.nullable);
            column.editable = editable.writable(i);
            // An expression (or the row id) is the engine's too: computed.
            column.computed = info.is_none_or(|c| c.generated.is_some());
            column.generated = info.is_some_and(|c| c.auto_increment);
        }
        self.columns_editable = true;
    }

    pub fn id(&self) -> ServiceId {
        self.id
    }

    /// The result as last read.
    pub fn result(&self) -> Option<&ResultView> {
        self.view.as_ref()
    }

    pub fn runner(&self) -> &Runner {
        &self.runner
    }

    /// The rows here (all of them, or the first pages).
    pub fn loaded(&self) -> u64 {
        self.view.as_ref().map_or(0, |v| v.len)
    }

    /// Every row is here (nothing more to read, nothing let go).
    pub fn complete(&self) -> bool {
        self.view.as_ref().is_some_and(|v| v.more == crate::results::More::None && !v.dropped)
    }

    /// Sorted and filtered by the engine (its query run again): said in the view's bar.
    pub fn by_server(&self) -> bool {
        self.view.as_ref().is_some_and(|v| !v.view.is_empty()) && self.local.is_none()
    }

    /// What it refused, for its view to tell (taken once).
    pub fn take_notice(&mut self) -> Option<String> {
        self.notice.take()
    }

    /// The order and filter the grid asks, as the engine reads them.
    pub fn server_view(query: &GridQuery) -> ServerView {
        ServerView {
            filter: query.filter.as_ref().map(filter),
            order: query
                .sort
                .iter()
                .map(|k| Order { column: k.column.clone(), descending: k.descending })
                .collect(),
        }
    }

    /// The result read again from the runner when it changed. `true` if it did.
    fn read(&mut self) -> bool {
        let generation = self.runner.result_generation(self.id);
        if generation.is_none() {
            let gone = self.view.is_some();
            self.view = None;
            return gone;
        }
        if self.view.as_ref().map(|v| v.generation) == generation {
            return false;
        }
        let Some(view) = self.runner.result(self.id) else { return false };
        let first = row_in(&view.chunks, &view.starts, 0);
        if self.columns.len() != view.columns.len()
            || self.columns.iter().zip(&view.columns).any(|(a, b)| a.name != b.name)
            || (self.columns.is_empty() && first.is_some())
        {
            self.columns = grid_columns(&view.columns, first);
            self.columns_editable = false;
        }
        if !view.loading {
            self.asked = false;
        }
        if !view.dropped {
            self.refilled = false;
        }
        // The rows changed under a local order: worked out again.
        let rows_changed = self.view.as_ref().is_none_or(|v| v.len != view.len);
        let moved = self.epoch != view.epoch;
        self.epoch = view.epoch;
        let editable_now = matches!(view.editing, Editability::Ready(_));
        // A big cell asked whole came (or couldn't be read).
        if let Some((at, column, key)) = &self.asked_whole
            && let Some((_, _, value)) =
                view.wholes.iter().find(|(k, c, _)| k == key && c == column)
            && let Ok(value) = value
        {
            self.wholes.insert((*at, *column), Arc::clone(value));
        }
        self.view = Some(view);
        if moved {
            self.rows_moved();
        }
        if editable_now && !self.columns_editable {
            self.edit_columns();
        } else if !editable_now && self.columns_editable {
            let first = self.view.as_ref().and_then(|v| row_in(&v.chunks, &v.starts, 0));
            self.columns = grid_columns(
                &self.view.as_ref().map(|v| v.columns.clone()).unwrap_or_default(),
                first,
            );
            self.columns_editable = false;
        }
        if rows_changed && self.local_for.is_some() {
            self.local = None;
            self.local_for = None;
            self.sort_locally();
        }
        true
    }

    /// Whether this query is worked out here (every row here, or a write's rows: never
    /// read again) rather than by the engine.
    fn local_query(&self) -> bool {
        let Some(view) = &self.view else { return true };
        !view.rerunnable || (!view.more.readable() && !view.dropped && view.view.is_empty())
    }

    /// The order of [`Self::query`] worked out over the rows here, in the background.
    fn sort_locally(&mut self) {
        let query = self.query.clone();
        if query.sort.is_empty() && query.filter.is_none() {
            self.local = None;
            self.local_for = None;
            self.sorting_for = None;
            return;
        }
        if self.sorting_for.as_ref() == Some(&query) || self.local_for.as_ref() == Some(&query) {
            return;
        }
        let Some(view) = self.view.clone() else { return };
        self.sorting_for = Some(query.clone());
        let slot = Arc::clone(&self.sorting);
        let wake = self.runner.clone();
        self.runtime.spawn(move || {
            let order = local_order(&view, &query);
            *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some((query, Arc::new(order)));
            wake.wake_up();
        });
    }

    /// The row's place among the chunks (through the local order).
    fn place(&self, row: u64) -> Option<u64> {
        match &self.local {
            Some(order) => order.get(row as usize).map(|&i| u64::from(i)),
            None => Some(row),
        }
    }

    fn row(&self, row: u64) -> Option<&[Value]> {
        let view = self.view.as_ref()?;
        let at = self.place(row)?;
        row_in(&view.chunks, &view.starts, at)
    }
}

/// The places of `view`'s rows passing `query`'s filter, in its order (stable: equal rows
/// keep theirs).
pub fn local_order(view: &ResultView, query: &GridQuery) -> Vec<u32> {
    let names: Vec<&str> = view.columns.iter().map(|c| c.name.as_str()).collect();
    let filter = query.filter.as_ref().map(filter);
    let order: Vec<Order> = query
        .sort
        .iter()
        .map(|k| Order { column: k.column.clone(), descending: k.descending })
        .collect();
    let at: Vec<Option<usize>> =
        order.iter().map(|o| names.iter().position(|n| *n == o.column)).collect();
    let mut kept: Vec<u32> = (0..view.len as u32)
        .filter(|&i| {
            let Some(filter) = &filter else { return true };
            let Some(row) = row_in(&view.chunks, &view.starts, u64::from(i)) else {
                return false;
            };
            database::eval::matches(filter, &|column| {
                names.iter().position(|n| *n == column).and_then(|c| row.get(c).cloned())
            })
        })
        .collect();
    if !order.is_empty() {
        let key = |i: u32| -> Vec<Value> {
            let row = row_in(&view.chunks, &view.starts, u64::from(i)).unwrap_or(&[]);
            at.iter().map(|c| c.and_then(|c| row.get(c).cloned()).unwrap_or(Value::Null)).collect()
        };
        let mut keyed: Vec<(Vec<Value>, u32)> = kept.iter().map(|&i| (key(i), i)).collect();
        keyed.sort_by(|a, b| database::eval::compare_rows(&order, &a.0, &b.0));
        kept = keyed.into_iter().map(|(_, i)| i).collect();
    }
    kept
}

/// The grid's filter as the domain's.
pub fn filter(f: &GridFilter) -> Filter {
    match f {
        GridFilter::Compare { column, op, value: v } => Filter::Compare {
            column: column.clone(),
            op: match op {
                Compare::Eq => CompareOp::Eq,
                Compare::NotEq => CompareOp::NotEq,
                Compare::Less => CompareOp::Less,
                Compare::LessEq => CompareOp::LessEq,
                Compare::Greater => CompareOp::Greater,
                Compare::GreaterEq => CompareOp::GreaterEq,
            },
            value: value(v),
        },
        GridFilter::IsNull(c) => Filter::IsNull(c.clone()),
        GridFilter::NotNull(c) => Filter::NotNull(c.clone()),
        GridFilter::Contains { column, text } => {
            Filter::Contains { column: column.clone(), text: text.clone() }
        }
        GridFilter::And(parts) => Filter::And(parts.iter().map(filter).collect()),
        GridFilter::Or(parts) => Filter::Or(parts.iter().map(filter).collect()),
        GridFilter::Not(inner) => Filter::Not(Box::new(filter(inner))),
    }
}

fn value(cell: &ui_components::data_grid::CellValue) -> Value {
    use ui_components::data_grid::CellValue;
    match cell {
        CellValue::Null => Value::Null,
        CellValue::Bool(b) => Value::Bool(*b),
        CellValue::Int(i) => Value::Int(*i),
        CellValue::Float(f) => Value::Float(*f),
        CellValue::Text(t) => Value::Text(t.clone()),
        CellValue::Bytes(b) => Value::Bytes(b.clone()),
        CellValue::Json(t) => Value::Json(t.clone()),
        CellValue::Other(t) => Value::Other(t.clone()),
    }
}

impl GridSource for ResultSource {
    fn columns(&self) -> &[GridColumn] {
        &self.columns
    }

    fn total(&self) -> Total {
        let Some(view) = &self.view else { return Total::Unknown };
        if let Some(order) = &self.local {
            return Total::Known(order.len() as u64);
        }
        if self.sorting_for.is_some() || (view.dropped && view.rerunnable) {
            return Total::Pending;
        }
        if view.more.readable() { Total::AtLeast(view.len) } else { Total::Known(view.len) }
    }

    fn row_state(&self, row: u64) -> RowState {
        if row >= NEW_ROWS {
            return if self.edits.new_row(row).is_some() {
                RowState::Ready
            } else {
                RowState::Missing
            };
        }
        let Some(view) = &self.view else { return RowState::Missing };
        if self.local.is_some() {
            return if self.row(row).is_some() { RowState::Ready } else { RowState::Missing };
        }
        if self.sorting_for.is_some() {
            return RowState::Pending;
        }
        if row < view.len {
            return if self.row(row).is_some() { RowState::Ready } else { RowState::Pending };
        }
        if view.error.is_some() {
            return RowState::Failed;
        }
        if view.more.readable() || view.loading || (view.dropped && view.rerunnable) {
            RowState::Pending
        } else {
            RowState::Missing
        }
    }

    fn cell(&self, row: u64, column: usize) -> CellRef<'_> {
        if row >= NEW_ROWS {
            return self
                .edits
                .new_row(row)
                .and_then(|r| r.values.get(column)?.as_ref())
                .map_or(CellRef::Null, cell_ref);
        }
        if let Some(value) = self.edited_key(row).and_then(|k| self.edits.value(k, column)) {
            return cell_ref(value);
        }
        if !self.wholes.is_empty()
            && let Some(whole) = self.place(row).and_then(|at| self.wholes.get(&(at, column)))
        {
            return cell_ref(whole);
        }
        self.row(row).and_then(|r: &[Value]| r.get(column)).map_or(CellRef::Null, cell_ref)
    }

    fn request(&mut self, query: &GridQuery, rows: Range<u64>) {
        self.read();
        // Shown: whether its rows can be edited is found out (once).
        if let Some(view) = &self.view
            && view.editing == Editability::Unknown
            && view.refused.is_none()
            && !self.rights.read_only
            && self.rights.edit_rows
        {
            self.runner.describe(self.id);
        }
        if *query != self.query {
            self.query = query.clone();
            if self.local_query() {
                self.sort_locally();
            } else {
                // While more are left, the engine sorts and filters (its query run again).
                self.local = None;
                self.local_for = None;
                self.sorting_for = None;
                self.asked = true;
                self.runner.requery(self.id, Self::server_view(query));
                return;
            }
        }
        let Some(view) = &self.view else { return };
        if view.dropped && view.rerunnable && !self.refilled && !view.loading {
            // Let go for memory: read again (as it was last sorted and filtered).
            self.refilled = true;
            self.runner.requery(self.id, view.view.clone());
            return;
        }
        if self.local.is_none()
            && rows.end > view.len
            && view.more.readable()
            && !view.loading
            && !self.asked
        {
            self.asked = self.runner.fetch_more(self.id);
        }
    }

    fn poll(&mut self) -> bool {
        let mut changed = self.read() || std::mem::take(&mut self.fresh);
        let came = self.sorting.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some((query, order)) = came
            && self.sorting_for.as_ref() == Some(&query)
        {
            self.sorting_for = None;
            if query == self.query {
                self.local = Some(order);
                self.local_for = Some(query);
            }
            changed = true;
        }
        changed
    }

    fn status(&self) -> SourceStatus {
        let Some(view) = &self.view else {
            return SourceStatus::Failed(i18n::t("console.result_gone"));
        };
        if let Some(why) = &view.error {
            return SourceStatus::Failed(why.clone());
        }
        if view.dropped && !view.rerunnable {
            return SourceStatus::Failed(i18n::t("console.result_dropped"));
        }
        if view.loading || self.sorting_for.is_some() {
            SourceStatus::Loading
        } else {
            SourceStatus::Ready
        }
    }

    fn capabilities(&self) -> GridCaps {
        GridCaps { sort: true, filter: true, limit: None, cancel: true }
    }

    fn name(&self) -> String {
        let title = self.view.as_ref().map(|v| v.title.as_str()).unwrap_or_default();
        match title.rsplit_once('.') {
            Some((_, table)) => table.to_owned(),
            None => "result".to_owned(),
        }
    }

    fn sql(&self) -> Option<Arc<dyn extractors::SqlStyle>> {
        self.runner.dialect().map(database::extract::sql_style)
    }

    fn cancel(&mut self) {
        self.runner.stop();
    }

    fn editing(&self) -> Editing {
        match self.refusal() {
            Some(why) => Editing::ReadOnly(why),
            None => Editing::Allowed,
        }
    }

    fn mark(&self, row: u64, column: usize) -> CellMark {
        if row >= NEW_ROWS {
            return if self.edits.new_row(row).is_some() {
                CellMark::Inserted
            } else {
                CellMark::None
            };
        }
        let Some(key) = self.edited_key(row) else { return CellMark::None };
        if self.edits.is_deleted(key) {
            CellMark::Deleted
        } else if self.edits.value(key, column).is_some() {
            CellMark::Modified
        } else {
            CellMark::None
        }
    }

    fn is_default(&self, row: u64, column: usize) -> bool {
        row >= NEW_ROWS
            && self
                .edits
                .new_row(row)
                .is_some_and(|r| r.values.get(column).is_some_and(Option::is_none))
    }

    fn set(&mut self, row: u64, column: usize, cell: CellValue) -> Result<(), String> {
        if let Some(why) = self.refusal() {
            return Err(why);
        }
        if !self.editable().is_some_and(|e| e.writable(column)) {
            return Err(i18n::t("console.edit_not_column"));
        }
        let new = value(&cell);
        if row >= NEW_ROWS {
            self.edits.set_new(row, column, new);
            return Ok(());
        }
        let (at, key, original) =
            self.original(row).ok_or_else(|| i18n::t("console.edit_no_key"))?;
        if self.is_cut(at, column) && !self.wholes.contains_key(&(at, column)) {
            return Err(i18n::t("grid.reading_whole"));
        }
        if self.edits.is_deleted(&key) {
            return Err(i18n::t("database.row_deleted"));
        }
        self.edits.set(&key, &original, column, new);
        self.edited.insert(at, key);
        Ok(())
    }

    fn insert(&mut self, like: Option<u64>) -> Option<u64> {
        if self.refusal().is_some() {
            return None;
        }
        let editable = Arc::clone(self.editable()?);
        let count = self.columns.len();
        // A clone takes the row's values, but its key and the columns that aren't written
        // (left to the engine: a key copied would be the same row's); a big cell not read
        // whole is left to the engine too.
        let values = like.map(|row| {
            (0..count)
                .map(|c| {
                    if editable.in_key(c) || !editable.writable(c) {
                        return None;
                    }
                    if row >= NEW_ROWS {
                        return self.edits.new_row(row)?.values.get(c)?.clone();
                    }
                    self.whole_or_cell(row, c)
                })
                .collect()
        });
        Some(self.edits.insert(NEW_ROWS, count, values))
    }

    fn delete(&mut self, rows: &[u64]) {
        if self.refusal().is_some() {
            return;
        }
        for &row in rows {
            if row >= NEW_ROWS {
                self.edits.drop_new(row);
            } else if let Some((at, key, _)) = self.original(row) {
                self.edits.toggle_delete(&key);
                self.edited.insert(at, key);
            }
        }
    }

    fn revert(&mut self, rows: Option<&[u64]>) {
        let Some(rows) = rows else {
            self.edits.clear();
            self.edited.clear();
            return;
        };
        for &row in rows {
            if row >= NEW_ROWS {
                self.edits.drop_new(row);
            } else if let Some(key) = self.edited_key(row).cloned() {
                self.edits.revert_row(&key);
            }
        }
    }

    fn pending(&self) -> usize {
        self.edits.count()
    }

    fn new_rows(&self) -> Vec<u64> {
        self.edits.inserted().iter().map(|i| i.id).collect()
    }

    fn whole(&mut self, row: u64, column: usize) -> Whole {
        if row >= NEW_ROWS {
            return Whole::Here;
        }
        let Some(at) = self.place(row) else { return Whole::Here };
        if !self.is_cut(at, column) || self.wholes.contains_key(&(at, column)) {
            return Whole::Here;
        }
        // Asked already: how it went.
        if let Some((a, c, key)) = &self.asked_whole
            && (*a, *c) == (at, column)
        {
            let came = self.view.as_ref().and_then(|v| {
                v.wholes.iter().find(|(k, col, _)| k == key && col == c).map(|(_, _, v)| v.clone())
            });
            return match came {
                Some(Ok(_)) => Whole::Here,
                Some(Err(why)) => Whole::Failed(why),
                None => Whole::Coming,
            };
        }
        let Some((_, key, _)) = self.original(row) else {
            return Whole::Failed(i18n::t("console.edit_no_key"));
        };
        if !self.runner.read_whole(self.id, key.clone(), column) {
            return Whole::Failed(i18n::t("console.edit_reading"));
        }
        self.asked_whole = Some((at, column, key));
        Whole::Coming
    }

    /// F5: a pure query run again (as sorted and filtered now); a write, never.
    fn refresh(&mut self) {
        let Some(view) = &self.view else { return };
        if !view.rerunnable {
            self.notice = Some(i18n::t("console.write_not_rerun"));
            return;
        }
        let server =
            if self.local_query() { ServerView::default() } else { Self::server_view(&self.query) };
        self.local = None;
        self.local_for = None;
        self.sorting_for = None;
        if self.runner.requery(self.id, server) {
            self.asked = true;
        }
    }
}

/// The rows a local order shows, for an export (their places among the chunks).
pub fn rows_in_order<'a>(
    view: &'a ResultView,
    order: Option<&'a [u32]>,
) -> Box<dyn Iterator<Item = &'a [Value]> + 'a> {
    match order {
        Some(order) => Box::new(
            order.iter().filter_map(move |&i| row_in(&view.chunks, &view.starts, u64::from(i))),
        ),
        None => Box::new(view.chunks.iter().flat_map(|c| c.iter().map(|r: &Row| &r[..]))),
    }
}

/// Why a result can't be edited, in the user's words.
pub fn refused_text(why: &NotEditable) -> String {
    match why {
        NotEditable::NotQuery => i18n::t("console.edit_not_select"),
        NotEditable::NotOneTable => i18n::t("console.edit_not_one_table"),
        NotEditable::Distinct => i18n::t("console.edit_distinct"),
        NotEditable::Grouped => i18n::t("console.edit_grouped"),
        NotEditable::Aggregate(name) => {
            i18n::tr!("console.edit_aggregate", name = name.to_uppercase())
        }
        NotEditable::Locking => i18n::t("console.edit_locking"),
        NotEditable::NoKey => i18n::t("console.edit_no_table_key"),
        NotEditable::KeyMissing(columns) => {
            i18n::tr!("console.edit_key_missing", columns = columns.join(", "))
        }
        NotEditable::Unmatched => i18n::t("console.edit_unmatched"),
    }
}

impl ResultSource {
    /// The local order shown, if any (an export writes the rows in it).
    pub fn local(&self) -> Option<Order32> {
        self.local.clone()
    }
}

#[cfg(test)]
#[path = "../tests/unit/result_source.rs"]
mod tests;
