//! A table as the grid reads it: [`TableSource`] adapts a [`TableRepository`] (whatever
//! the driver) to the grid's [`GridSource`] port. The rows come in blocks (a repository
//! page each, aligned), asked one at a time — the worker keeps only a table's latest page
//! request, so asking two at once would drop the first — and only those the grid shows
//! are kept here (the repository's cache, within its memory limit, holds the rest). The
//! total is counted in the background (a huge table's estimate shown until asked), the
//! columns come from the structure, the grid's order and filter become the domain's.

use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::Arc;

use database::{
    Applied, ChangeSet, CompareOp, DbError, Fetch, Filter, KeyKind, Order, Page, RepositoryEvent,
    RowKey, Rows, Sorting, Structure, TableRef, TableRepository, TypeClass, Value,
};
use ui_components::data_grid::{
    CellClass, CellMark, CellRef, CellValue, Compare, Editing, GridCaps, GridColumn, GridFilter,
    GridQuery, GridSource, NEW_ROWS, RowState, SourceStatus, Total,
};

use crate::edits::Edits;

/// Rows per block when every row is shown (scrolled to).
pub const ALL_BLOCK: u32 = 500;

/// Past this many estimated rows, a table isn't counted until asked (`COUNT(*)` of a
/// huge table takes long); an exact total shown otherwise.
pub const COUNT_ON_REQUEST: u64 = 1_000_000;

/// What can be done with the table, as the source says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Abilities {
    pub sorting: Sorting,
    /// Rows can be inserted, updated and deleted (the driver), and the source isn't
    /// read-only, and the table isn't a view.
    pub edit_rows: bool,
    pub read_only: bool,
    pub view: bool,
    /// The source's size (a file's bytes), for [`Sorting::UpTo`].
    pub bytes: Option<u64>,
    pub cancel: bool,
}

/// A block of rows, and where each grid column is among its values.
#[derive(Debug)]
struct Block {
    rows: Arc<Rows>,
    at: Vec<Option<usize>>,
}

/// The table, for the grid (see the module's doc).
pub struct TableSource {
    repo: Box<dyn TableRepository>,
    table: TableRef,
    abilities: Abilities,
    columns: Vec<GridColumn>,
    structure: Option<Arc<Structure>>,
    /// The engine's estimate of the rows (listed with the table).
    estimate: Option<u64>,
    block: u32,
    order: Vec<Order>,
    filter: Option<Filter>,
    query: GridQuery,
    blocks: BTreeMap<u64, Block>,
    /// The block asked and not come yet.
    asked: Option<u64>,
    failed: Option<(u64, DbError)>,
    /// The rows the grid wants now.
    wanted: Range<u64>,
    total: Total,
    count_asked: bool,
    /// The last thing that failed (structure, total, page), for the user.
    error: Option<String>,
    /// Reading was stopped (asked to): nothing more is asked until the rows wanted
    /// change, the order or filter does, or it's refreshed.
    cancelled: bool,
    /// Something it was told came since the grid last asked (drawn again).
    news: bool,
    /// What the view needs to know of (commits, refreshes), taken by it.
    events: Vec<RepositoryEvent>,
    /// The changes waiting to be submitted.
    edits: Edits,
    /// Why editing is stopped for now (the text has unsaved changes), if it is.
    blocked: Option<String>,
    /// A submit is running: nothing is changed until it ends.
    submitting: bool,
    /// How the last submit ended (for the view to show).
    outcome: Option<Result<Applied, DbError>>,
    /// How the source's SQL is written (what the SQL extractors copy).
    sql: Option<Arc<dyn extractors::SqlStyle>>,
}

impl std::fmt::Debug for TableSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TableSource").field("table", &self.table).finish_non_exhaustive()
    }
}

/// A type's class as the grid knows it.
pub fn cell_class(class: TypeClass) -> CellClass {
    match class {
        TypeClass::Text => CellClass::Text,
        TypeClass::Integer => CellClass::Integer,
        TypeClass::Decimal => CellClass::Decimal,
        TypeClass::Bool => CellClass::Bool,
        TypeClass::DateTime => CellClass::DateTime,
        TypeClass::Binary => CellClass::Binary,
        TypeClass::Json => CellClass::Json,
        TypeClass::Other => CellClass::Other,
    }
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

/// The grid's value as the domain's.
pub fn value(cell: &CellValue) -> Value {
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

/// The grid's filter as the domain's (built from the model: never text).
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

/// The grid's columns of a structure (each column's key, type and nullability).
pub fn grid_columns(structure: &Structure) -> Vec<GridColumn> {
    let key = structure.primary_key();
    structure
        .columns
        .iter()
        .map(|c| GridColumn {
            name: c.name.clone(),
            type_name: c.declared_type.clone(),
            class: cell_class(c.class),
            key: key.contains(&c.name),
            nullable: c.nullable,
            editable: c.generated.is_none(),
            computed: c.generated.is_some(),
            generated: c.auto_increment,
        })
        .collect()
}

/// A failure as the user reads it (a file held by another program, an external driver's
/// program failing: the locales' text).
pub fn message(e: &DbError) -> String {
    use database::DriverFailure as F;
    match e {
        DbError::InUse(_) => i18n::t("database.file_in_use"),
        DbError::Cancelled => i18n::t("database.cancelled"),
        DbError::Driver(failure @ F::Protocol { driver, theirs, ours }) => {
            let range = |(min, max): (u32, u32)| {
                if min == max { min.to_string() } else { format!("{min}–{max}") }
            };
            let key = if failure.driver_too_old() {
                "database.driver_too_old"
            } else {
                "database.driver_too_new"
            };
            i18n::tr!(key, driver = driver.as_str(), theirs = range(*theirs), ours = range(*ours))
        }
        DbError::Driver(F::NotStarted { driver, why }) => {
            i18n::tr!("database.driver_not_started", driver = driver.as_str(), why = why.as_str())
        }
        DbError::Driver(F::Stopped { driver, why }) => {
            i18n::tr!("database.driver_stopped", driver = driver.as_str(), why = why.as_str())
        }
        DbError::Driver(F::TimedOut { driver, what }) => {
            i18n::tr!("database.driver_timed_out", driver = driver.as_str(), what = what.as_str())
        }
        // Said in the interface's language (`Display` stays English: the logs').
        DbError::Connect(why) => i18n::tr!("database.err_connect", why = why.as_str()),
        DbError::Auth(why) => i18n::tr!("database.err_auth", why = why.as_str()),
        DbError::Unsupported(what) => i18n::tr!("database.err_unsupported", what = what.as_str()),
        DbError::Conflict(why) => i18n::tr!("database.err_conflict", why = why.as_str()),
        DbError::ReadOnly(why) => i18n::tr!("database.err_read_only", why = why.as_str()),
        other => other.to_string(),
    }
}

impl TableSource {
    /// `table` through `repo`, `page` rows a page (`None`: all), its listed `estimate`.
    pub fn new(
        repo: Box<dyn TableRepository>,
        table: TableRef,
        abilities: Abilities,
        page: Option<u32>,
        estimate: Option<u64>,
    ) -> Self {
        let mut source = Self {
            repo,
            table,
            abilities,
            columns: Vec::new(),
            structure: None,
            estimate,
            block: block_size(page),
            order: Vec::new(),
            filter: None,
            query: GridQuery::default(),
            blocks: BTreeMap::new(),
            asked: None,
            failed: None,
            wanted: 0..0,
            total: Total::Pending,
            count_asked: false,
            error: None,
            cancelled: false,
            news: false,
            events: Vec::new(),
            edits: Edits::default(),
            blocked: None,
            submitting: false,
            outcome: None,
            sql: None,
        };
        source.ask_structure();
        source
    }

    /// Its rows copied as SQL in `dialect`'s way (`None`: standard SQL).
    pub fn with_dialect(mut self, dialect: Option<Arc<dyn database::Dialect>>) -> Self {
        self.sql = dialect.map(database::extract::sql_style);
        self
    }

    pub fn table(&self) -> &TableRef {
        &self.table
    }

    pub fn structure(&self) -> Option<&Arc<Structure>> {
        self.structure.as_ref()
    }

    /// The order and filter the rows are read in now (what an export of every row uses).
    pub fn query(&self) -> (Vec<Order>, Option<Filter>) {
        (self.order.clone(), self.filter.clone())
    }

    /// The memory the rows it and its repository keep hold (bytes, estimated).
    pub fn memory(&self) -> usize {
        self.repo.memory() + self.blocks.values().map(|b| b.at.len() * 8).sum::<usize>()
    }

    pub fn repository(&mut self) -> &mut dyn TableRepository {
        self.repo.as_mut()
    }

    /// Editing stopped for `why` (`None`: allowed again, as far as the table goes).
    pub fn block(&mut self, why: Option<String>) {
        self.blocked = why;
    }

    /// The pending changes, as submitted (`None`: nothing pending, or the structure
    /// isn't known).
    pub fn change_set(&self) -> Option<ChangeSet> {
        let structure = self.structure.as_ref()?;
        let set = self.edits.change_set(self.table.clone(), &structure.columns);
        (!set.is_empty()).then_some(set)
    }

    /// What `set` would run (SQL; none for a source that writes none).
    pub fn preview(&self, set: &ChangeSet) -> Result<Vec<String>, DbError> {
        self.repo.preview(set)
    }

    /// `set` applied in one transaction; editing waits until it ends.
    pub fn submit(&mut self, set: ChangeSet) {
        self.submitting = true;
        self.outcome = None;
        self.repo.apply(set);
    }

    pub fn submitting(&self) -> bool {
        self.submitting
    }

    /// How the last submit ended, once (the view says it).
    pub fn take_outcome(&mut self) -> Option<Result<Applied, DbError>> {
        self.outcome.take()
    }

    pub fn edits(&self) -> &Edits {
        &self.edits
    }

    /// Row `row`'s key and its values as read (in the grid's columns), when it came.
    fn read_row(&self, row: u64) -> Option<(&RowKey, Vec<Value>)> {
        let (block, at) = self.block_of(row);
        let b = self.blocks.get(&block)?;
        let key = b.rows.keys.get(at)?;
        let values = b.rows.rows.get(at)?;
        let original =
            b.at.iter()
                .map(|i| i.and_then(|i| values.get(i).cloned()).unwrap_or(Value::Null))
                .collect();
        Some((key, original))
    }

    /// Row `row`'s key, when it came.
    fn key_of(&self, row: u64) -> Option<&RowKey> {
        let (block, at) = self.block_of(row);
        self.blocks.get(&block)?.rows.keys.get(at)
    }

    /// What came for the view (commits, refreshes) since it last looked.
    pub fn take_events(&mut self) -> Vec<RepositoryEvent> {
        std::mem::take(&mut self.events)
    }

    /// Another page size: blocks of its size (the cached ones go).
    pub fn set_page_size(&mut self, page: Option<u32>) {
        let block = block_size(page);
        if block != self.block {
            self.block = block;
            self.blocks.clear();
            self.asked = None;
            self.failed = None;
        }
    }

    /// The rows sorted and filtered at the source, for this one's size.
    fn sorts(&self) -> bool {
        match self.abilities.sorting {
            Sorting::UpTo(_) => {
                self.abilities.bytes.is_some_and(|b| self.abilities.sorting.allows(b))
            }
            other => other.allows(0),
        }
    }

    fn ask_structure(&mut self) {
        match self.repo.structure() {
            Fetch::Ready(structure) => self.take_structure(structure),
            Fetch::Pending => {}
            Fetch::Failed(e) => self.error = Some(message(&e)),
        }
    }

    fn take_structure(&mut self, structure: Arc<Structure>) {
        let columns = grid_columns(&structure);
        if columns != self.columns {
            self.columns = columns;
            for block in self.blocks.values_mut() {
                block.at = positions(&self.columns, &block.rows);
            }
        }
        self.structure = Some(structure);
        self.news = true;
    }

    /// The total, asked unless the table is huge (then its estimate, until asked).
    fn ask_count(&mut self, forced: bool) {
        if (self.count_asked || self.cancelled) && !forced {
            return;
        }
        let huge = self.filter.is_none() && self.estimate.is_some_and(|n| n > COUNT_ON_REQUEST);
        if huge && !forced {
            self.total = Total::Estimate(self.estimate.unwrap_or(0));
            return;
        }
        self.count_asked = true;
        let fetch = self.repo.count(self.filter.as_ref());
        self.take_count(fetch);
    }

    fn take_count(&mut self, fetch: Fetch<u64>) {
        match fetch {
            Fetch::Ready(n) => self.total = Total::Known(n),
            Fetch::Pending => {
                if !matches!(self.total, Total::Known(_)) {
                    self.total = match self.estimate.filter(|_| self.filter.is_none()) {
                        Some(n) => Total::Estimate(n),
                        None => Total::Pending,
                    };
                }
            }
            Fetch::Failed(e) => {
                self.total = Total::Unknown;
                if e != DbError::Cancelled {
                    self.error = Some(message(&e));
                }
            }
        }
    }

    fn page(&self, block: u64) -> Page {
        Page {
            offset: block * u64::from(self.block),
            limit: self.block,
            order: self.order.clone(),
            filter: self.filter.clone(),
        }
    }

    /// The first block of the wanted rows not here: asked (one at a time); those the
    /// repository has cached come at once, and the next is looked at.
    fn ask_rows(&mut self) {
        if self.asked.is_some() || self.wanted.is_empty() || self.cancelled {
            return;
        }
        let size = u64::from(self.block);
        let (first, last) = (self.wanted.start / size, (self.wanted.end - 1) / size);
        for block in first..=last {
            if self.blocks.contains_key(&block)
                || self.failed.as_ref().is_some_and(|(b, _)| *b == block)
            {
                continue;
            }
            // Past the end: nothing to ask.
            if self.total.known().is_some_and(|t| block * size >= t && t > 0) {
                break;
            }
            let page = self.page(block);
            match self.repo.page(&page) {
                Fetch::Ready(rows) => self.take_block(block, rows),
                Fetch::Pending => {
                    self.asked = Some(block);
                    return;
                }
                Fetch::Failed(e) => {
                    if e != DbError::Cancelled {
                        self.error = Some(message(&e));
                        self.failed = Some((block, e));
                    }
                    return;
                }
            }
        }
    }

    fn take_block(&mut self, block: u64, rows: Arc<Rows>) {
        let size = u64::from(self.block);
        // A short block: the rows end there (the total is known without counting).
        if (rows.len() as u64) < size && !matches!(self.total, Total::Known(_)) {
            self.total = Total::Known(block * size + rows.len() as u64);
        }
        let at = positions(&self.columns, &rows);
        self.blocks.insert(block, Block { rows, at });
        // Only what's near the rows wanted is kept (the repository has the rest).
        let (first, last) = (self.wanted.start / size, self.wanted.end.saturating_sub(1) / size);
        self.blocks.retain(|b, _| *b + 1 >= first && *b <= last + 1);
        self.news = true;
    }

    /// Everything known let go (a refresh, a commit): read again.
    fn reset(&mut self) {
        self.blocks.clear();
        self.asked = None;
        self.failed = None;
        self.count_asked = false;
        if !matches!(self.total, Total::Estimate(_)) {
            self.total = Total::Pending;
        }
        self.error = None;
        self.news = true;
    }

    fn block_of(&self, row: u64) -> (u64, usize) {
        let size = u64::from(self.block);
        (row / size, (row % size) as usize)
    }
}

/// Rows per block for a page size (`None`: all rows).
fn block_size(page: Option<u32>) -> u32 {
    page.unwrap_or(ALL_BLOCK).clamp(10, 1000)
}

/// Where each grid column is among a block's values (by name).
fn positions(columns: &[GridColumn], rows: &Rows) -> Vec<Option<usize>> {
    if columns.is_empty() {
        return Vec::new();
    }
    columns.iter().map(|c| rows.columns.iter().position(|r| r.name == c.name)).collect()
}

impl GridSource for TableSource {
    fn columns(&self) -> &[GridColumn] {
        &self.columns
    }

    fn total(&self) -> Total {
        self.total
    }

    fn row_state(&self, row: u64) -> RowState {
        if row >= NEW_ROWS {
            return if self.edits.new_row(row).is_some() {
                RowState::Ready
            } else {
                RowState::Missing
            };
        }
        if self.total.known().is_some_and(|t| row >= t) {
            return RowState::Missing;
        }
        let (block, at) = self.block_of(row);
        match self.blocks.get(&block) {
            Some(b) if at < b.rows.len() => RowState::Ready,
            Some(_) => RowState::Missing,
            None if self.failed.as_ref().is_some_and(|(b, _)| *b == block) => RowState::Failed,
            None => RowState::Pending,
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
        if let Some(value) = self.key_of(row).and_then(|k| self.edits.value(k, column)) {
            return cell_ref(value);
        }
        let (block, at) = self.block_of(row);
        let Some(b) = self.blocks.get(&block) else { return CellRef::Null };
        let Some(Some(index)) = b.at.get(column) else { return CellRef::Null };
        b.rows.rows.get(at).and_then(|r| r.get(*index)).map_or(CellRef::Null, cell_ref)
    }

    fn request(&mut self, query: &GridQuery, rows: Range<u64>) {
        if *query != self.query {
            let sorts = self.sorts();
            self.order = if sorts {
                query
                    .sort
                    .iter()
                    .map(|k| Order { column: k.column.clone(), descending: k.descending })
                    .collect()
            } else {
                Vec::new()
            };
            let new_filter = if sorts { query.filter.as_ref().map(filter) } else { None };
            if new_filter != self.filter {
                self.filter = new_filter;
                self.count_asked = false;
                self.total = Total::Pending;
            }
            self.query = query.clone();
            self.cancelled = false;
            self.blocks.clear();
            self.asked = None;
            self.failed = None;
            self.error = None;
        }
        if rows != self.wanted {
            self.cancelled = false;
        }
        self.wanted = rows;
        if self.structure.is_some() {
            self.ask_count(false);
            self.ask_rows();
        }
    }

    fn poll(&mut self) -> bool {
        let events = self.repo.poll();
        for event in &events {
            match event {
                RepositoryEvent::Structure => self.ask_structure(),
                RepositoryEvent::Count => {
                    let fetch = self.repo.count(self.filter.as_ref());
                    self.take_count(fetch);
                    self.news = true;
                }
                RepositoryEvent::Page(page) => {
                    if page.order != self.order
                        || page.filter != self.filter
                        || page.limit != self.block
                    {
                        continue;
                    }
                    let block = page.offset / u64::from(self.block);
                    if self.asked == Some(block) {
                        self.asked = None;
                    }
                    match self.repo.page(page) {
                        Fetch::Ready(rows) => self.take_block(block, rows),
                        Fetch::Failed(e) if e != DbError::Cancelled => {
                            self.error = Some(message(&e));
                            self.failed = Some((block, e));
                            self.news = true;
                        }
                        // Asked again (a refresh came between): it comes later.
                        Fetch::Failed(_) | Fetch::Pending => {}
                    }
                }
                RepositoryEvent::Refreshed => self.reset(),
                RepositoryEvent::Applied(result) => {
                    // Applied: nothing waits any more (the rows are read again). Refused:
                    // every change stays, nothing lost.
                    if result.is_ok() {
                        self.edits.clear();
                    }
                    self.submitting = false;
                    self.outcome = Some(result.clone());
                    self.news = true;
                }
            }
        }
        self.events.extend(
            events
                .into_iter()
                .filter(|e| matches!(e, RepositoryEvent::Applied(_) | RepositoryEvent::Refreshed)),
        );
        if self.structure.is_some() {
            self.ask_count(false);
            self.ask_rows();
        } else if !self.columns.is_empty() || self.error.is_none() {
            // Refreshed: the structure asked again.
            self.ask_structure();
        }
        std::mem::take(&mut self.news)
    }

    fn status(&self) -> SourceStatus {
        if self.cancelled {
            return SourceStatus::Failed(message(&DbError::Cancelled));
        }
        if let Some(error) = &self.error {
            return SourceStatus::Failed(error.clone());
        }
        let counting =
            self.count_asked && matches!(self.total, Total::Pending | Total::Estimate(_));
        if self.structure.is_none() || self.asked.is_some() || counting {
            SourceStatus::Loading
        } else {
            SourceStatus::Ready
        }
    }

    fn capabilities(&self) -> GridCaps {
        let sorts = self.sorts();
        let limit = match (sorts, self.abilities.sorting) {
            (false, Sorting::UpTo(max)) => {
                Some(i18n::tr!("database.sort_limit", size = i18n::bytes(max)))
            }
            (false, _) => Some(i18n::t("database.sort_none")),
            _ => None,
        };
        GridCaps { sort: sorts, filter: sorts, limit, cancel: self.abilities.cancel }
    }

    fn name(&self) -> String {
        self.table.name.clone()
    }

    fn sql(&self) -> Option<Arc<dyn extractors::SqlStyle>> {
        self.sql.clone()
    }

    fn cancel(&mut self) {
        // What's asked is let go: asked again when the grid wants it.
        self.repo.refresh();
        self.cancelled = true;
        self.asked = None;
        self.news = true;
    }

    fn refresh(&mut self) {
        self.cancelled = false;
        self.repo.refresh();
        self.reset();
        self.structure = None;
        self.ask_structure();
    }

    fn count(&mut self) {
        self.ask_count(true);
    }

    fn editing(&self) -> Editing {
        let why = if let Some(why) = &self.blocked {
            Some(why.clone())
        } else if self.submitting {
            Some(i18n::t("database.ro_submitting"))
        } else if self.abilities.read_only {
            Some(i18n::t("database.ro_source"))
        } else if self.abilities.view {
            Some(i18n::t("database.ro_view"))
        } else if !self.abilities.edit_rows {
            Some(i18n::t("database.ro_driver"))
        } else {
            match self.structure.as_ref().map(|s| s.key_kind()) {
                None => Some(i18n::t("grid.loading")),
                Some(KeyKind::None) => Some(i18n::t("database.ro_no_key")),
                Some(_) => None,
            }
        };
        match why {
            Some(why) => Editing::ReadOnly(why),
            None => Editing::Allowed,
        }
    }

    fn is_default(&self, row: u64, column: usize) -> bool {
        row >= NEW_ROWS
            && self
                .edits
                .new_row(row)
                .is_some_and(|r| r.values.get(column).is_some_and(Option::is_none))
    }

    fn mark(&self, row: u64, column: usize) -> CellMark {
        if row >= NEW_ROWS {
            return if self.edits.new_row(row).is_some() {
                CellMark::Inserted
            } else {
                CellMark::None
            };
        }
        let Some(key) = self.key_of(row) else { return CellMark::None };
        if self.edits.is_deleted(key) {
            CellMark::Deleted
        } else if self.edits.value(key, column).is_some() {
            CellMark::Modified
        } else {
            CellMark::None
        }
    }

    fn set(&mut self, row: u64, column: usize, cell: CellValue) -> Result<(), String> {
        if let Editing::ReadOnly(why) = self.editing() {
            return Err(why);
        }
        let new = value(&cell);
        if row >= NEW_ROWS {
            self.edits.set_new(row, column, new);
            return Ok(());
        }
        let (key, original) = self.read_row(row).ok_or_else(|| i18n::t("grid.loading"))?;
        if self.edits.is_deleted(key) {
            return Err(i18n::t("database.row_deleted"));
        }
        let key = key.clone();
        self.edits.set(&key, &original, column, new);
        Ok(())
    }

    fn insert(&mut self, like: Option<u64>) -> Option<u64> {
        if !self.editing().allowed() {
            return None;
        }
        let columns = self.structure.as_ref()?.columns.clone();
        // A clone takes the row's values, but its key and generated columns (left to the
        // engine: a key copied would be the same row's).
        let values = like.map(|row| {
            (0..columns.len())
                .map(|c| {
                    let column = &columns[c];
                    if column.in_key || column.generated.is_some() {
                        return None;
                    }
                    Some(match self.cell(row, c) {
                        CellRef::Null => Value::Null,
                        other => value(&other.to_value()),
                    })
                })
                .collect()
        });
        Some(self.edits.insert(NEW_ROWS, columns.len(), values))
    }

    fn delete(&mut self, rows: &[u64]) {
        if !self.editing().allowed() {
            return;
        }
        for &row in rows {
            if row >= NEW_ROWS {
                self.edits.drop_new(row);
            } else if let Some(key) = self.key_of(row).cloned() {
                self.edits.toggle_delete(&key);
            }
        }
    }

    fn revert(&mut self, rows: Option<&[u64]>) {
        let Some(rows) = rows else {
            self.edits.clear();
            return;
        };
        for &row in rows {
            if row >= NEW_ROWS {
                self.edits.drop_new(row);
            } else if let Some(key) = self.key_of(row).cloned() {
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
}

#[cfg(test)]
#[path = "../tests/unit/grid_source.rs"]
mod tests;
