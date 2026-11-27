//! CSV files for the `database` domain: a file is a source with one table (named after the
//! file), read and changed through the same [`Session`] contract as a database — with the
//! domain's own CSV rules ([`database::csv`]), no other parser:
//!
//! - **opened** by its first bytes only: the dialect guessed from a sample (delimiter —
//!   a tab for `.tsv` / `.tab` —, quotes, header, byte order mark), the style it writes
//!   (line endings, every field quoted or only those that must be), the columns named by
//!   the header (`column_1`… without one) and typed by [`database::csv::infer_types`].
//!   The target's options can say `delimiter` (`,` `;` `|` `tab`) and `header`;
//! - **large files** are never read whole: an [`OffsetIndex`] of where the records start
//!   (a mark every thousand) is built in chunks as far as a page needs, to the end for the
//!   total, cancelled between chunks and resumed by the next request; a page is a `seek` to
//!   the nearest mark. The file is opened per request, never held between them;
//! - **sorting and filtering** run here (by [`database::eval`]'s rules) for files up to
//!   [`SORT_LIMIT`]: one pass keeping, of the rows passing, their place and their sort
//!   keys — the rows are read again a page at a time. A larger file refuses
//!   ([`Capabilities::sorting`] says so);
//! - **rows are found** by their place among the data records ([`RowKey::Line`]) and their
//!   fingerprint as read: a row changed since is a conflict;
//! - **a commit** ([`commit`]) streams the file into a temporary one beside it, with the
//!   changes, checks the original is still what was read (its size, time and the hash of
//!   what was scanned) and renames the copy over it — all or nothing; the bytes of what
//!   wasn't changed are copied as they were (quotes, blank lines, line endings, the byte
//!   order mark, a missing last line ending). A file held by another program (Windows) is
//!   retried a few times, then [`DbError::InUse`]. Columns are added, dropped and renamed
//!   the same way.

pub mod commit;
pub mod index;

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use database::csv::{self, CsvDialect, CsvStyle, cell_value, field_text, split_fields};
use database::{
    Applied, Cancel, Capabilities, ChangeSet, Column, ColumnInfo, DbError, Driver, DriverFactory,
    Filter, Kind, Language, Node, Order, Outcome, Page, RowKey, RowSink, SchemaChange,
    SchemaChanges, Secret, Session, Sorting, Structure, TableInfo, TableKind, TableRef, Target,
    TypeClass, Value,
};

pub use index::{OffsetIndex, RecordReader, Stamp, Tuning};

/// The extensions of the files it opens (lowercase).
pub const EXTENSIONS: &[&str] = &["csv", "tsv", "tab"];
/// The extensions whose delimiter is a tab.
const TAB_EXTENSIONS: &[&str] = &["tsv", "tab"];
/// How many first bytes tell a CSV file from a binary one.
pub const HEAD: usize = 1024;
/// The bytes the dialect and the types are guessed from.
pub const SAMPLE_BYTES: usize = 256 << 10;
/// The records the types are inferred from.
pub const SAMPLE_ROWS: usize = 1000;

/// The target's option giving how many records the types are guessed from
/// (`sample_rows`; [`SAMPLE_ROWS`] by default): the bytes read grow with it.
pub const SAMPLE_OPTION: &str = "sample_rows";

/// The largest sample read, whatever the option asks (8 MB).
const MAX_SAMPLE_BYTES: usize = 8 << 20;

/// The bytes read to guess from `rows` records.
fn sample_bytes(rows: usize) -> usize {
    SAMPLE_BYTES.saturating_mul(rows.div_ceil(SAMPLE_ROWS).max(1)).min(MAX_SAMPLE_BYTES)
}
/// The largest file sorted or filtered (one pass over it, its sort keys in memory).
pub const SORT_LIMIT: u64 = 64 << 20;

/// What a CSV session can do: its rows edited, inserted and deleted, columns added, dropped
/// and renamed (no other change: a file has no indexes, no types of its own, one table),
/// all in one commit; reading, counting and sorting cancelled; sorted and filtered up to
/// [`SORT_LIMIT`].
pub const CAPABILITIES: Capabilities = Capabilities {
    transactions: true,
    edit_rows: true,
    schema_changes: SchemaChanges {
        tables: false,
        columns: true,
        alter_columns: false,
        indexes: false,
        renames: true,
        foreign_keys: false,
        reorder: true,
        databases: false,
        schemas: false,
    },
    schemas: false,
    file_based: true,
    cancel: true,
    returning: true,
    sorting: Sorting::UpTo(SORT_LIMIT),
    // No SQL of its own (plan 39: no console for a CSV file).
    console: None,
};

/// `head` (a file's first bytes) is text: no NUL byte (a binary file, or UTF-16).
pub fn is_text(head: &[u8]) -> bool {
    !head.contains(&0)
}

/// The driver as the app lists it (created only when a source opens).
pub fn factory() -> DriverFactory {
    DriverFactory::new("csv", "CSV", Kind::Relational, None, || Arc::new(CsvDriver))
}

#[derive(Debug, Default)]
pub struct CsvDriver;

impl Driver for CsvDriver {
    fn kind(&self) -> Kind {
        Kind::Relational
    }

    /// Its rows are a table, shown as an SQL engine's are (it runs no query of its own).
    fn language(&self) -> Language {
        Language::Sql
    }

    fn capabilities(&self) -> Capabilities {
        CAPABILITIES
    }

    /// Its file, and how it's read when guessing isn't wanted: the separator, the header.
    fn connection(&self) -> database::ConnectionSpec {
        use database::{Binding, FieldKind, FieldSpec, FieldTab};
        let choice = |pairs: &[(&str, &str)]| {
            FieldKind::Choice(
                pairs.iter().map(|(v, l)| ((*v).to_owned(), (*l).to_owned())).collect(),
            )
        };
        database::ConnectionSpec::file(&["csv", "tsv", "tab"], "csv")
            .with(FieldSpec::new(
                "delimiter",
                "datasources.field_delimiter",
                choice(&[
                    (",", "datasources.delimiter_comma"),
                    (";", "datasources.delimiter_semicolon"),
                    ("tab", "datasources.delimiter_tab"),
                    ("|", "datasources.delimiter_pipe"),
                ]),
                Binding::Option("delimiter".into()),
                FieldTab::Options,
            ))
            .with(FieldSpec::new(
                "header",
                "datasources.field_header",
                FieldKind::Flag,
                Binding::Option("header".into()),
                FieldTab::Options,
            ))
    }

    /// Opens `target.database` (the file); no password.
    fn connect(&self, target: &Target, _secret: &Secret) -> Result<Box<dyn Session>, DbError> {
        CsvSession::open(target).map(|s| Box::new(s) as Box<dyn Session>)
    }
}

/// An I/O failure, as the domain says it: a file another program holds is
/// [`DbError::InUse`].
pub(crate) fn io_error(e: &std::io::Error, what: &str) -> DbError {
    if commit::held(e) {
        return DbError::InUse(format!("{what}: the file is open in another program ({e})"));
    }
    DbError::Query(format!("{what}: {e}"))
}

/// How a file is written, and its columns: guessed when it opens, kept by the session's
/// own commits (guessed again only when another program changed the file).
#[derive(Debug, Clone)]
pub struct Layout {
    pub dialect: CsvDialect,
    pub style: CsvStyle,
    pub columns: Vec<ColumnInfo>,
    /// The rows estimated from the sample (before the index knows them).
    pub estimate: Option<u64>,
}

impl Layout {
    /// A data record's values (its line ending trimmed), by its columns' classes: a cell
    /// missing is `NULL`, one more than the columns isn't read.
    pub fn values(&self, record: &[u8]) -> Vec<Value> {
        let record = csv::trim_line_ending(record);
        let fields = split_fields(record, self.dialect.delimiter, self.dialect.quote);
        self.columns
            .iter()
            .enumerate()
            .map(|(at, column)| match fields.get(at) {
                Some(range) => cell_value(
                    &field_text(&record[range.clone()], self.dialect.quote),
                    column.class,
                ),
                None => Value::Null,
            })
            .collect()
    }

    /// The column named `name`.
    pub fn column(&self, name: &str) -> Result<usize, DbError> {
        self.columns
            .iter()
            .position(|c| c.name == name)
            .ok_or_else(|| DbError::Query(format!("no column named `{name}`")))
    }

    /// The byte order mark's length.
    pub fn start(&self) -> u64 {
        if self.dialect.bom { 3 } else { 0 }
    }
}

/// The type a class is declared as (a CSV file has none: what was inferred).
pub fn declared(class: TypeClass) -> &'static str {
    match class {
        TypeClass::Integer => "INTEGER",
        TypeClass::Decimal => "DECIMAL",
        TypeClass::Bool => "BOOLEAN",
        TypeClass::DateTime => "DATETIME",
        TypeClass::Json => "JSON",
        TypeClass::Binary => "BLOB",
        TypeClass::Text | TypeClass::Other => "TEXT",
    }
}

/// A column of `class`.
pub fn column(name: String, class: TypeClass) -> ColumnInfo {
    let mut column = ColumnInfo::new(name, declared(class));
    column.class = class;
    column
}

/// The rows a sort or a filter keeps, in their order: where each starts and its place
/// among the data records (its key).
#[derive(Debug, Clone)]
struct View {
    order: Vec<Order>,
    filter: Option<Filter>,
    rows: Vec<(u64, u64)>,
}

/// One CSV file, open (see the crate's doc).
pub struct CsvSession {
    path: PathBuf,
    table: TableRef,
    read_only: bool,
    tuning: Tuning,
    /// The delimiter and header the target asks for (guessed when not).
    delimiter: Option<u8>,
    header: Option<bool>,
    /// The records the types are guessed from.
    sample_rows: usize,
    layout: Layout,
    index: OffsetIndex,
    view: Option<View>,
}

impl std::fmt::Debug for CsvSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CsvSession").field("path", &self.path).finish_non_exhaustive()
    }
}

/// The delimiter an option names.
fn delimiter_option(text: &str) -> Option<u8> {
    match text.trim() {
        "tab" | "\\t" | "\t" => Some(b'\t'),
        t if t.len() == 1 && t.is_ascii() && t != "\"" => t.bytes().next(),
        _ => None,
    }
}

impl CsvSession {
    /// Opens `target`'s file (see the crate's doc).
    pub fn open(target: &Target) -> Result<Self, DbError> {
        Self::open_tuned(target, Tuning::default())
    }

    /// [`Self::open`], its index built as `tuning` says.
    pub fn open_tuned(target: &Target, tuning: Tuning) -> Result<Self, DbError> {
        let file = target.database.trim();
        if file.is_empty() {
            return Err(DbError::Connect("no CSV file given".to_owned()));
        }
        let path = PathBuf::from(file);
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let tab = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| TAB_EXTENSIONS.iter().any(|t| t.eq_ignore_ascii_case(e)));
        let delimiter = target
            .options
            .get("delimiter")
            .and_then(|d| delimiter_option(d))
            .or(tab.then_some(b'\t'));
        let header = target.flag("header");
        let sample_rows = target
            .options
            .get(SAMPLE_OPTION)
            .and_then(|n| n.trim().parse::<usize>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(SAMPLE_ROWS);
        let (layout, index) = load(&path, delimiter, header, sample_rows, tuning)?;
        Ok(Self {
            path,
            table: TableRef::new(stem),
            read_only: target.read_only(),
            tuning,
            delimiter,
            header,
            sample_rows,
            layout,
            index,
            view: None,
        })
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    pub fn index(&self) -> &OffsetIndex {
        &self.index
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The file as it is now: guessed again (and the index started over) if another
    /// program changed it since it was read; whether it had.
    fn refresh(&mut self) -> Result<bool, DbError> {
        let stamp =
            Stamp::of(&self.path).map_err(|e| DbError::Connect(format!("the CSV file: {e}")))?;
        if stamp == self.index.stamp {
            return Ok(false);
        }
        let (layout, index) =
            load(&self.path, self.delimiter, self.header, self.sample_rows, self.tuning)?;
        (self.layout, self.index, self.view) = (layout, index, None);
        Ok(true)
    }

    fn check_table(&self, table: &TableRef) -> Result<(), DbError> {
        if table.schema.is_none() && table.name == self.table.name {
            Ok(())
        } else {
            Err(DbError::Query(format!("no table named `{table}`")))
        }
    }

    fn writable(&self) -> Result<(), DbError> {
        if self.read_only {
            return Err(DbError::ReadOnly("the data source is read-only".to_owned()));
        }
        Ok(())
    }

    fn open_file(&self) -> Result<File, DbError> {
        File::open(&self.path).map_err(|e| io_error(&e, "reading the file"))
    }

    fn reader(&self, offset: u64, buffer: usize) -> Result<RecordReader, DbError> {
        let dialect = &self.layout.dialect;
        RecordReader::at(self.open_file()?, offset, dialect.delimiter, dialect.quote, buffer)
            .map_err(|e| io_error(&e, "reading the file"))
    }

    fn columns(&self) -> Vec<Column> {
        self.layout
            .columns
            .iter()
            .map(|c| Column { name: c.name.clone(), type_name: c.declared_type.clone() })
            .collect()
    }

    /// A row to `sink`, with its key.
    fn emit(&self, sink: &mut dyn RowSink, line: u64, record: &[u8]) -> database::Flow {
        let values = self.layout.values(record);
        sink.row_key(RowKey::Line { line, fingerprint: database::fingerprint(&values) });
        sink.row(values)
    }

    /// A page in the file's order: from the nearest mark, the records before it skipped.
    fn read_plain(
        &mut self,
        page: &Page,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
    ) -> Result<Outcome, DbError> {
        let mut outcome = Outcome::default();
        // The page scanned too: a commit then checks the rows shown are still there.
        let end = page.offset.saturating_add(u64::from(page.limit));
        self.index.cover(&self.path, end, cancel)?;
        let Some((offset, mut line)) = self.index.mark_for(page.offset) else {
            return Ok(outcome);
        };
        let mut reader = self.reader(offset, self.tuning.buffer)?;
        let mut record = Vec::new();
        let io = |e: std::io::Error| io_error(&e, "reading the file");
        while line < page.offset {
            if reader.next_record(&mut record).map_err(io)?.is_none() {
                return Ok(outcome);
            }
            line += 1;
        }
        while outcome.rows < u64::from(page.limit) {
            if cancel.is_cancelled() {
                return Err(DbError::Cancelled);
            }
            if reader.next_record(&mut record).map_err(io)?.is_none() {
                break;
            }
            outcome.rows += 1;
            if self.emit(sink, line, &record) == database::Flow::Stop {
                outcome.stopped = true;
                break;
            }
            line += 1;
        }
        Ok(outcome)
    }

    /// The rows `filter` keeps in `order` (made once per order and filter: one pass over
    /// the file, cancelled between chunks).
    fn view(
        &mut self,
        order: &[Order],
        filter: Option<&Filter>,
        cancel: &Cancel,
    ) -> Result<&View, DbError> {
        let cached = self
            .view
            .as_ref()
            .is_some_and(|v| v.order.as_slice() == order && v.filter.as_ref() == filter);
        if !cached {
            self.view = None;
            let rows = self.sorted(order, filter, cancel)?;
            self.view = Some(View { order: order.to_vec(), filter: filter.cloned(), rows });
        }
        self.view.as_ref().ok_or_else(|| DbError::Query("no rows".to_owned()))
    }

    fn sorted(
        &mut self,
        order: &[Order],
        filter: Option<&Filter>,
        cancel: &Cancel,
    ) -> Result<Vec<(u64, u64)>, DbError> {
        if !CAPABILITIES.sorting.allows(self.index.stamp.len) {
            return Err(DbError::Unsupported(format!(
                "sorting or filtering a CSV file over {} MB",
                SORT_LIMIT >> 20
            )));
        }
        // The whole file scanned (its total known, all of it hashed for a commit).
        self.index.complete(&self.path, cancel)?;
        let sort: Vec<usize> =
            order.iter().map(|o| self.layout.column(&o.column)).collect::<Result<_, _>>()?;
        if let Some(filter) = filter {
            for name in database::eval::filter_columns(filter) {
                self.layout.column(&name)?;
            }
        }
        let mut reader = self.reader(self.layout.start(), self.tuning.buffer)?;
        let mut record = Vec::new();
        let mut kept: Vec<(Vec<Value>, u64, u64)> = Vec::new();
        let (mut line, mut chunk) = (0u64, 0u64);
        let mut header = self.layout.dialect.header;
        let io = |e: std::io::Error| io_error(&e, "reading the file");
        while let Some(start) = reader.next_record(&mut record).map_err(io)? {
            chunk += record.len() as u64;
            if chunk >= self.tuning.chunk {
                chunk = 0;
                if cancel.is_cancelled() {
                    return Err(DbError::Cancelled);
                }
            }
            if std::mem::take(&mut header) {
                continue;
            }
            let values = self.layout.values(&record);
            let passes = filter.is_none_or(|f| {
                database::eval::matches(f, &|name| {
                    self.layout.column(name).ok().and_then(|at| values.get(at).cloned())
                })
            });
            if passes {
                let keys = sort.iter().map(|&at| values[at].clone()).collect();
                kept.push((keys, start, line));
            }
            line += 1;
        }
        // Stable: rows sorting alike stay in the file's order.
        kept.sort_by(|a, b| database::eval::compare_rows(order, &a.0, &b.0));
        Ok(kept.into_iter().map(|(_, start, line)| (start, line)).collect())
    }

    /// A page of a sort or a filter: each row read again where it starts.
    fn read_view(
        &mut self,
        page: &Page,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
    ) -> Result<Outcome, DbError> {
        let rows: Vec<(u64, u64)> = {
            let view = self.view(&page.order, page.filter.as_ref(), cancel)?;
            let from = usize::try_from(page.offset).unwrap_or(usize::MAX).min(view.rows.len());
            let to = from.saturating_add(page.limit as usize).min(view.rows.len());
            view.rows[from..to].to_vec()
        };
        let mut outcome = Outcome::default();
        let mut record = Vec::new();
        for (start, line) in rows {
            if cancel.is_cancelled() {
                return Err(DbError::Cancelled);
            }
            // A row is short: a small buffer, not a chunk's.
            let mut reader = self.reader(start, 8 << 10)?;
            if reader
                .next_record(&mut record)
                .map_err(|e| io_error(&e, "reading the file"))?
                .is_none()
            {
                break;
            }
            outcome.rows += 1;
            if self.emit(sink, line, &record) == database::Flow::Stop {
                outcome.stopped = true;
                break;
            }
        }
        Ok(outcome)
    }

    /// The rows: known once the index is complete, estimated from the sample before.
    fn rows(&self) -> Option<u64> {
        if self.index.done() { Some(self.index.records()) } else { self.layout.estimate }
    }

    /// After a commit of ours: the file as written is the one known (its layout kept, its
    /// index started over).
    fn committed(&mut self, layout: Layout) {
        // Not known (can't be read now): read again, guessed anew, by the next request.
        let stamp = Stamp::of(&self.path).unwrap_or(Stamp { len: u64::MAX, modified: None });
        self.index = OffsetIndex::new(
            stamp,
            self.tuning,
            layout.dialect.delimiter,
            layout.dialect.quote,
            layout.dialect.header,
            layout.start(),
        );
        self.layout = layout;
        self.view = None;
    }

    /// The file changed since it was read (by another program): nothing can be written
    /// over it; it's read again for what comes next.
    fn check_unchanged(&mut self) -> Result<(), DbError> {
        if self.refresh()? {
            return Err(DbError::Conflict("the file changed on disk since it was read".to_owned()));
        }
        Ok(())
    }
}

/// Reads `path`'s start: its layout, and an empty index of it.
fn load(
    path: &Path,
    delimiter: Option<u8>,
    header: Option<bool>,
    sample_rows: usize,
    tuning: Tuning,
) -> Result<(Layout, OffsetIndex), DbError> {
    use std::io::Read;
    let stamp = Stamp::of(path)
        .map_err(|e| DbError::Connect(format!("no such CSV file: {} ({e})", path.display())))?;
    let file = File::open(path).map_err(|e| match io_error(&e, "opening the file") {
        DbError::Query(why) => DbError::Connect(why),
        other => other,
    })?;
    let bytes = sample_bytes(sample_rows);
    let mut sample = Vec::with_capacity(bytes.min(stamp.len as usize));
    file.take(bytes as u64)
        .read_to_end(&mut sample)
        .map_err(|e| io_error(&e, "reading the file"))?;
    let complete = (sample.len() as u64) >= stamp.len;
    let mut guess = csv::guess_with(&sample, sample_rows, complete, delimiter);
    let (body, _) = csv::strip_bom(&sample);
    if let Some(header) = header
        && header != guess.dialect.header
    {
        // Told: the types and names again, the first record counted as asked.
        let text = String::from_utf8_lossy(body);
        let d = guess.dialect;
        let records =
            csv::parse(&text, d.delimiter, d.quote, sample_rows.saturating_add(1), complete);
        let rows = if header { records.get(1..).unwrap_or(&[]) } else { &records[..] };
        let classes = csv::infer_types(rows, d.delimiter);
        let width = classes.len().max(records.first().map_or(0, Vec::len));
        guess.dialect.header = header;
        guess.columns = (0..width)
            .map(|i| csv::CsvColumn {
                name: records
                    .first()
                    .and_then(|r| r.get(i))
                    .filter(|n| header && !n.trim().is_empty())
                    .map(|n| n.trim().to_owned()),
                class: classes.get(i).copied().unwrap_or(TypeClass::Text),
            })
            .collect();
    }
    let dialect = guess.dialect;
    let style = csv::guess_style(body, &dialect);
    let mut names: Vec<String> = Vec::new();
    let columns = guess
        .columns
        .into_iter()
        .enumerate()
        .map(|(at, c)| {
            let mut name = c.name.unwrap_or_else(|| format!("column_{}", at + 1));
            // Two columns of one name: the second numbered (a cell is found by its name).
            if names.contains(&name) {
                let base = name.clone();
                let mut n = 2;
                while names.contains(&name) {
                    name = format!("{base}_{n}");
                    n += 1;
                }
            }
            names.push(name.clone());
            column(name, c.class)
        })
        .collect();
    let estimate = estimate(body, &dialect, stamp.len, complete);
    let layout = Layout { dialect, style, columns, estimate };
    let index = OffsetIndex::new(
        stamp,
        tuning,
        dialect.delimiter,
        dialect.quote,
        dialect.header,
        layout.start(),
    );
    Ok((layout, index))
}

/// The rows of a file of `len` bytes starting with `sample`: counted when it's all there,
/// from the sample's average record otherwise.
fn estimate(sample: &[u8], dialect: &CsvDialect, len: u64, complete: bool) -> Option<u64> {
    let mut scanner = csv::RecordScanner::new(dialect.delimiter, dialect.quote);
    let (mut records, mut start, mut bytes) = (0u64, 0usize, 0usize);
    while let Some(n) = scanner.end(&sample[start..]) {
        if !csv::is_blank(&sample[start..=start + n]) {
            records += 1;
        }
        start += n + 1;
        bytes = start;
    }
    if complete && !csv::is_blank(&sample[start..]) {
        records += 1;
        bytes = sample.len();
    }
    let header = u64::from(dialect.header && records > 0);
    if complete {
        return Some(records - header);
    }
    if records <= header || bytes == 0 {
        return None;
    }
    let average = bytes as f64 / records as f64;
    Some(((len as f64 / average) as u64).saturating_sub(header))
}

impl Session for CsvSession {
    fn server(&self) -> String {
        "CSV".to_owned()
    }

    /// The table, then its columns.
    fn children(&mut self, path: &[String]) -> Result<Vec<Node>, DbError> {
        self.refresh()?;
        let name = self.table.name.clone();
        match path {
            [] => Ok(vec![Node {
                kind: "table",
                name: name.clone(),
                detail: self.rows().map(|n| n.to_string()).unwrap_or_default(),
                path: vec![name],
                expandable: !self.layout.columns.is_empty(),
            }]),
            [table] if *table == name => Ok(self
                .layout
                .columns
                .iter()
                .map(|c| Node {
                    kind: "column",
                    name: c.name.clone(),
                    detail: c.declared_type.clone(),
                    path: vec![name.clone(), c.name.clone()],
                    expandable: false,
                })
                .collect()),
            _ => Ok(Vec::new()),
        }
    }

    fn execute(
        &mut self,
        _query: &str,
        _sink: &mut dyn RowSink,
        _cancel: &Cancel,
    ) -> Result<Outcome, DbError> {
        Err(DbError::Unsupported("queries on a CSV file".to_owned()))
    }

    fn tables(&mut self, path: &[String]) -> Result<Vec<TableInfo>, DbError> {
        self.refresh()?;
        if !path.is_empty() {
            return Ok(Vec::new());
        }
        Ok(vec![TableInfo { table: self.table.clone(), kind: TableKind::Table, rows: self.rows() }])
    }

    fn structure(&mut self, table: &TableRef) -> Result<Structure, DbError> {
        self.check_table(table)?;
        self.refresh()?;
        Ok(Structure {
            table: self.table.clone(),
            columns: self.layout.columns.clone(),
            lines: true,
            ..Structure::default()
        })
    }

    fn read(
        &mut self,
        table: &TableRef,
        page: &Page,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
    ) -> Result<Outcome, DbError> {
        self.check_table(table)?;
        self.refresh()?;
        sink.columns(&self.columns());
        if page.order.is_empty() && page.filter.is_none() {
            self.read_plain(page, sink, cancel)
        } else {
            self.read_view(page, sink, cancel)
        }
    }

    fn count(
        &mut self,
        table: &TableRef,
        filter: Option<&Filter>,
        cancel: &Cancel,
    ) -> Result<u64, DbError> {
        self.check_table(table)?;
        self.refresh()?;
        match filter {
            None => self.index.complete(&self.path, cancel),
            Some(filter) => Ok(self.view(&[], Some(filter), cancel)?.rows.len() as u64),
        }
    }

    fn apply(&mut self, changes: &ChangeSet) -> Result<Applied, DbError> {
        self.check_table(&changes.table)?;
        self.writable()?;
        self.check_unchanged()?;
        if changes.is_empty() {
            return Ok(Applied::default());
        }
        let mut plan = commit::RowPlan::new(&self.layout, changes)?;
        let rewritten = commit::rewrite(&self.path, &self.layout, &self.index, &mut plan)?;
        self.committed(self.layout.clone());
        Ok(plan.applied(rewritten))
    }

    fn alter(&mut self, changes: &[SchemaChange]) -> Result<Applied, DbError> {
        for change in changes {
            self.check_table(change.table())?;
        }
        self.writable()?;
        self.check_unchanged()?;
        if changes.is_empty() {
            return Ok(Applied::default());
        }
        let mut plan = commit::ColumnPlan::new(&self.layout, changes)?;
        commit::rewrite(&self.path, &self.layout, &self.index, &mut plan)?;
        self.committed(plan.layout(&self.layout));
        Ok(Applied { affected: changes.len() as u64, inserted: Vec::new() })
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
