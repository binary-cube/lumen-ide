//! A commit to a CSV file: the file streamed into a temporary one in its folder, the
//! changes made on the way (each row changed found by its place and checked by its
//! fingerprint), then — the original still what the session read, the copy on disk — the
//! copy renamed over it. All or nothing: anything failing leaves the original as it was and
//! the copy removed. What isn't changed is copied byte for byte.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use database::csv::{cell_text, encode_field, encode_record, is_blank, split_fields};
use database::transaction::ordered;
use database::{
    Applied, Change, ChangeSet, DbError, DefaultValue, RowKey, SchemaChange, TypeClass, fingerprint,
};

use crate::index::{HASH_START, OffsetIndex, RecordReader, Stamp, hash};
use crate::{Layout, io_error};

/// How many times a rename held up by another program is tried.
const RENAME_TRIES: u32 = 6;
/// The first pause between two tries (doubled each time: 1.5 s in all).
const RENAME_PAUSE: Duration = Duration::from_millis(50);

/// The error says another program holds the file open (Windows: a sharing or lock
/// violation, or access denied while it's open — a spreadsheet keeps it so).
#[cfg(windows)]
pub(crate) fn held(e: &std::io::Error) -> bool {
    // ERROR_ACCESS_DENIED, ERROR_SHARING_VIOLATION, ERROR_LOCK_VIOLATION.
    matches!(e.raw_os_error(), Some(5 | 32 | 33))
}

/// The error says another program holds the file open: never on Unix (a file open
/// elsewhere is still renamed over).
#[cfg(not(windows))]
pub(crate) fn held(_e: &std::io::Error) -> bool {
    false
}

/// Renames `from` over `to`, tried again a few times, with a growing pause, while another
/// program holds `to` (on Windows).
pub fn replace(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut pause = RENAME_PAUSE;
    let mut tries = 1;
    loop {
        match std::fs::rename(from, to) {
            Err(e) if held(&e) && tries < RENAME_TRIES => {
                std::thread::sleep(pause);
                pause *= 2;
                tries += 1;
            }
            other => return other,
        }
    }
}

/// A record met while the file is copied (its line ending trimmed).
#[derive(Debug, Clone, Copy)]
pub enum Unit<'a> {
    Header(&'a [u8]),
    /// A data record, and its place among them (from 0).
    Data {
        line: u64,
        record: &'a [u8],
    },
}

/// What becomes of a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Out {
    Keep,
    Drop,
    Replace(Vec<u8>),
}

/// The changes a copy makes.
pub trait Edits {
    /// What becomes of `unit` (an error stops the commit).
    fn record(&mut self, layout: &Layout, unit: Unit<'_>) -> Result<Out, DbError>;
    /// Once the file is copied (`seen` data records), before the copy replaces it.
    fn check(&self, seen: u64) -> Result<(), DbError> {
        let _ = seen;
        Ok(())
    }
    /// The records added at the end (written, without their line ending).
    fn appended(&self) -> &[Vec<u8>] {
        &[]
    }
}

/// What a copy did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rewritten {
    /// The original's data records.
    pub seen: u64,
    /// Those of them written (kept or changed).
    pub written: u64,
}

/// The temporary copy, removed unless kept.
struct Temp(Option<PathBuf>);

impl Drop for Temp {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// The line ending as written: none, `\n` or `\r\n`.
fn ending_of(ending: &[u8]) -> &'static [u8] {
    match ending {
        b"\r\n" => b"\r\n",
        b"\n" => b"\n",
        _ => b"",
    }
}

/// Writes records, each line ending held until the next one: a last record dropped
/// doesn't leave a line ending the file didn't end with.
struct Writer<W: Write> {
    out: W,
    pending: &'static [u8],
    wrote: bool,
    /// The file's own line ending (a record added after one that had none).
    ending: &'static [u8],
}

impl<W: Write> Writer<W> {
    fn emit(&mut self, content: &[u8], ending: &[u8]) -> std::io::Result<()> {
        if self.wrote {
            let separator = if self.pending.is_empty() { self.ending } else { self.pending };
            self.out.write_all(separator)?;
        }
        self.out.write_all(content)?;
        self.pending = ending_of(ending);
        self.wrote = true;
        Ok(())
    }

    /// The last line ending, if the file ended with one.
    fn finish(&mut self, final_newline: bool) -> std::io::Result<()> {
        if final_newline && self.wrote {
            let ending = if self.pending.is_empty() { self.ending } else { self.pending };
            self.out.write_all(ending)?;
        }
        Ok(())
    }
}

/// Copies `path` into a temporary file beside it, `edits` deciding each record's fate,
/// then renames it over `path` — if `path` is still what `index` read (its size and time;
/// the hash of what it scanned). Anything failing: the original untouched, the copy gone.
pub fn rewrite(
    path: &Path,
    layout: &Layout,
    index: &OffsetIndex,
    edits: &mut dyn Edits,
) -> Result<Rewritten, DbError> {
    let changed = || DbError::Conflict("the file changed on disk since it was read".to_owned());
    let stamp = Stamp::of(path).map_err(|e| io_error(&e, "reading the file"))?;
    if stamp != index.stamp {
        return Err(changed());
    }
    let dir = path.parent().unwrap_or(Path::new("."));
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let temp_path =
        dir.join(format!(".{name}.{}.tmp", kernel::id::uuid_text(kernel::id::new_uuid())));
    let write_error = |e: std::io::Error| io_error(&e, "writing the file's copy");
    let read_error = |e: std::io::Error| io_error(&e, "reading the file");
    let file = OpenOptions::new().write(true).create_new(true).open(&temp_path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            DbError::ReadOnly(format!("its folder can't be written: {e}"))
        } else {
            write_error(e)
        }
    })?;
    let mut temp = Temp(Some(temp_path.clone()));
    let mut writer = Writer {
        out: BufWriter::with_capacity(256 << 10, file),
        pending: b"",
        wrote: false,
        ending: layout.style.line_ending.bytes(),
    };

    let rewritten = {
        let mut original = File::open(path).map_err(read_error)?;
        // The byte order mark, as it was.
        let start = layout.start();
        let mut bom = vec![0; start as usize];
        original.read_exact(&mut bom).map_err(read_error)?;
        writer.out.write_all(&bom).map_err(write_error)?;
        let (scanned, expected) = index.scanned();
        let mut checked = scanned <= start;
        let mut running = HASH_START;
        let dialect = &layout.dialect;
        let mut reader =
            RecordReader::at(original, start, dialect.delimiter, dialect.quote, 256 << 10)
                .map_err(read_error)?;
        let mut unit = Vec::new();
        let mut header = dialect.header;
        let (mut seen, mut written) = (0u64, 0u64);
        let mut final_newline = true;
        while reader.next_into(&mut unit).map_err(read_error)?.is_some() {
            if !checked {
                running = hash(running, &unit);
                match reader.offset() {
                    at if at == scanned && running == expected => checked = true,
                    at if at >= scanned => return Err(changed()),
                    _ => {}
                }
            }
            let content = database::csv::trim_line_ending(&unit);
            let ending = &unit[content.len()..];
            final_newline = !ending.is_empty();
            if is_blank(&unit) {
                writer.emit(b"", ending).map_err(write_error)?;
                continue;
            }
            let met = if std::mem::take(&mut header) {
                Unit::Header(content)
            } else {
                seen += 1;
                Unit::Data { line: seen - 1, record: content }
            };
            let is_data = matches!(met, Unit::Data { .. });
            match edits.record(layout, met)? {
                Out::Keep => writer.emit(content, ending).map_err(write_error)?,
                Out::Drop => continue,
                Out::Replace(bytes) => writer.emit(&bytes, ending).map_err(write_error)?,
            }
            written += u64::from(is_data);
        }
        if !checked {
            // Shorter than what was read.
            return Err(changed());
        }
        for record in edits.appended() {
            writer.emit(record, layout.style.line_ending.bytes()).map_err(write_error)?;
        }
        writer.finish(final_newline).map_err(write_error)?;
        edits.check(seen)?;
        Rewritten { seen, written }
        // The original closed here: Windows renames nothing over an open file.
    };
    let file = writer.out.into_inner().map_err(|e| write_error(e.into_error()))?;
    file.sync_all().map_err(write_error)?;
    drop(file);
    if let Ok(meta) = std::fs::metadata(path) {
        // Its permissions, not the copy's defaults.
        let _ = std::fs::set_permissions(&temp_path, meta.permissions());
    }
    if Stamp::of(path).map_err(read_error)? != index.stamp {
        return Err(changed());
    }
    replace(&temp_path, path).map_err(|e| io_error(&e, "replacing the file"))?;
    temp.0 = None;
    Ok(rewritten)
}

/// A record's fields as written, `edit` changing them, joined again (a record left empty
/// written as one empty quoted field: not a blank line).
fn splice(layout: &Layout, record: &[u8], edit: impl FnOnce(&mut Vec<Vec<u8>>)) -> Vec<u8> {
    let dialect = &layout.dialect;
    let mut fields: Vec<Vec<u8>> = split_fields(record, dialect.delimiter, dialect.quote)
        .into_iter()
        .map(|range| record[range].to_vec())
        .collect();
    edit(&mut fields);
    let mut out = fields.join(&dialect.delimiter);
    if out.is_empty() {
        out = vec![dialect.quote, dialect.quote];
    }
    out
}

/// A value as a cell's text; bytes refused.
fn text_of(value: &database::Value, column: &str) -> Result<String, DbError> {
    cell_text(value)
        .ok_or_else(|| DbError::Query(format!("`{column}`: a CSV file holds text, not bytes")))
}

/// A row changed or deleted, and its fingerprint as read.
#[derive(Debug, Clone)]
enum Edit {
    Delete { fingerprint: u64 },
    Update { fingerprint: u64, cells: Vec<(usize, String)> },
}

/// A set of row changes, as a copy makes them.
#[derive(Debug)]
pub struct RowPlan {
    edits: HashMap<u64, Edit>,
    met: usize,
    /// The rows added, written.
    inserts: Vec<Vec<u8>>,
    /// Their keys, as they'll be read: (place among the rows added, fingerprint).
    inserted: Vec<u64>,
}

impl RowPlan {
    /// The plan of `set` (in [`ordered`]'s order); a row changed and deleted, or deleted
    /// twice, is a conflict, as an engine would find.
    pub fn new(layout: &Layout, set: &ChangeSet) -> Result<Self, DbError> {
        let mut edits: HashMap<u64, Edit> = HashMap::new();
        let mut inserts = Vec::new();
        let mut inserted = Vec::new();
        let line_of = |key: &RowKey| match key {
            RowKey::Line { line, fingerprint } => Ok((*line, *fingerprint)),
            other => Err(DbError::Query(format!("a CSV row is found by its line, not {other:?}"))),
        };
        let gone =
            |line: u64| DbError::Conflict(format!("the row on line {} was deleted", line + 1));
        for change in ordered(set).changes {
            match change {
                Change::Delete { key } => {
                    let (line, fingerprint) = line_of(&key)?;
                    if edits.insert(line, Edit::Delete { fingerprint }).is_some() {
                        return Err(gone(line));
                    }
                }
                Change::Update { key, cells } => {
                    let (line, fingerprint) = line_of(&key)?;
                    let mut new = Vec::with_capacity(cells.len());
                    for cell in &cells {
                        new.push((layout.column(&cell.column)?, text_of(&cell.new, &cell.column)?));
                    }
                    match edits.get_mut(&line) {
                        None => {
                            edits.insert(line, Edit::Update { fingerprint, cells: new });
                        }
                        Some(Edit::Update { cells, .. }) => cells.extend(new),
                        Some(Edit::Delete { .. }) => return Err(gone(line)),
                    }
                }
                Change::Insert { values } => {
                    let mut fields = vec![String::new(); layout.columns.len()];
                    for (column, value) in &values {
                        fields[layout.column(column)?] = text_of(value, column)?;
                    }
                    let record =
                        encode_record(&fields, &layout.dialect, &layout.style).into_bytes();
                    inserted.push(fingerprint(&layout.values(&record)));
                    inserts.push(record);
                }
            }
        }
        Ok(Self { edits, met: 0, inserts, inserted })
    }

    /// What the copy did, as the domain says it: the rows changed and the keys of those
    /// added (after every row kept).
    pub fn applied(&self, rewritten: Rewritten) -> Applied {
        Applied {
            affected: (self.edits.len() + self.inserts.len()) as u64,
            inserted: self
                .inserted
                .iter()
                .enumerate()
                .map(|(at, &fingerprint)| RowKey::Line {
                    line: rewritten.written + at as u64,
                    fingerprint,
                })
                .collect(),
        }
    }
}

impl Edits for RowPlan {
    fn record(&mut self, layout: &Layout, unit: Unit<'_>) -> Result<Out, DbError> {
        let Unit::Data { line, record } = unit else { return Ok(Out::Keep) };
        let Some(edit) = self.edits.get(&line) else { return Ok(Out::Keep) };
        let read = match edit {
            Edit::Delete { fingerprint } | Edit::Update { fingerprint, .. } => *fingerprint,
        };
        if fingerprint(&layout.values(record)) != read {
            return Err(DbError::Conflict(format!(
                "the row on line {} changed since it was read",
                line + 1
            )));
        }
        self.met += 1;
        Ok(match edit {
            Edit::Delete { .. } => Out::Drop,
            Edit::Update { cells, .. } => Out::Replace(splice(layout, record, |fields| {
                for (at, text) in cells {
                    if fields.len() <= *at {
                        fields.resize(at + 1, Vec::new());
                    }
                    fields[*at] = encode_field(text, &layout.dialect, &layout.style).into_bytes();
                }
            })),
        })
    }

    fn check(&self, _seen: u64) -> Result<(), DbError> {
        if self.met < self.edits.len() {
            return Err(DbError::Conflict(
                "a row changed was deleted since it was read".to_owned(),
            ));
        }
        Ok(())
    }

    fn appended(&self) -> &[Vec<u8>] {
        &self.inserts
    }
}

/// Where a column of the new layout comes from.
#[derive(Debug, Clone)]
enum Origin {
    /// The original's column at this place.
    Old(usize),
    /// Added, every row given this text.
    New(String),
}

#[derive(Debug, Clone)]
struct PlannedColumn {
    origin: Origin,
    name: String,
    class: TypeClass,
    renamed: bool,
}

/// Columns added, dropped and renamed, as a copy makes them (each record's fields as they
/// were, moved; the header's names changed).
#[derive(Debug)]
pub struct ColumnPlan {
    columns: Vec<PlannedColumn>,
    /// The original's columns (a record's fields past them are kept at its end).
    width: usize,
}

impl ColumnPlan {
    pub fn new(layout: &Layout, changes: &[SchemaChange]) -> Result<Self, DbError> {
        let mut columns: Vec<PlannedColumn> = layout
            .columns
            .iter()
            .enumerate()
            .map(|(at, c)| PlannedColumn {
                origin: Origin::Old(at),
                name: c.name.clone(),
                class: c.class,
                renamed: false,
            })
            .collect();
        let find = |columns: &[PlannedColumn], name: &str| {
            columns
                .iter()
                .position(|c| c.name == name)
                .ok_or_else(|| DbError::Query(format!("no column named `{name}`")))
        };
        let free = |columns: &[PlannedColumn], name: &str| {
            if columns.iter().any(|c| c.name == name) {
                Err(DbError::Query(format!("there is already a column named `{name}`")))
            } else {
                Ok(())
            }
        };
        let rename = |columns: &mut Vec<PlannedColumn>, from: &str, to: &str| {
            if !layout.dialect.header {
                return Err(DbError::Unsupported(
                    "renaming a column of a CSV file without a header".to_owned(),
                ));
            }
            let at = find(columns, from)?;
            free(columns, to)?;
            columns[at].name = to.to_owned();
            columns[at].renamed = true;
            Ok(())
        };
        for change in changes {
            match change {
                SchemaChange::AddColumn { column, .. } => {
                    free(&columns, &column.name)?;
                    let text = match &column.default {
                        Some(DefaultValue::Value(value)) => text_of(value, &column.name)?,
                        _ => String::new(),
                    };
                    let class = match column.declared_type.trim() {
                        "" => TypeClass::Text,
                        declared => TypeClass::of(declared),
                    };
                    columns.push(PlannedColumn {
                        origin: Origin::New(text),
                        name: column.name.clone(),
                        class,
                        renamed: true,
                    });
                }
                SchemaChange::DropColumn { column, .. } => {
                    let at = find(&columns, column)?;
                    columns.remove(at);
                }
                SchemaChange::RenameColumn { from, to, .. } => rename(&mut columns, from, to)?,
                SchemaChange::AlterColumn { column, to, .. } => {
                    let at = find(&columns, column)?;
                    let same_type = to.declared_type.trim().is_empty()
                        || TypeClass::of(&to.declared_type) == columns[at].class;
                    if !same_type || to.default.is_some() || !to.nullable {
                        return Err(DbError::Unsupported(
                            "changing a CSV column's type, default or nullability".to_owned(),
                        ));
                    }
                    if to.name != *column {
                        rename(&mut columns, column, &to.name)?;
                    }
                }
                SchemaChange::MoveColumn { column, to, .. } => {
                    let at = find(&columns, column)?;
                    let moved = columns.remove(at);
                    let to = (*to).min(columns.len());
                    columns.insert(to, moved);
                }
                other => {
                    return Err(DbError::Unsupported(format!(
                        "{} on a CSV file",
                        match other {
                            SchemaChange::CreateTable { .. } => "creating a table",
                            SchemaChange::RenameTable { .. } => "renaming the table",
                            SchemaChange::DropTable { .. } => "dropping the table",
                            SchemaChange::AddForeignKey { .. }
                            | SchemaChange::DropForeignKey { .. } => "foreign keys",
                            _ => "indexes",
                        }
                    )));
                }
            }
        }
        if columns.is_empty() {
            return Err(DbError::Query("a CSV file keeps at least one column".to_owned()));
        }
        Ok(Self { columns, width: layout.columns.len() })
    }

    /// The layout once made.
    pub fn layout(&self, old: &Layout) -> Layout {
        let columns = self
            .columns
            .iter()
            .map(|c| match c.origin {
                Origin::Old(at) => {
                    let mut column = old.columns[at].clone();
                    column.name.clone_from(&c.name);
                    column
                }
                Origin::New(_) => crate::column(c.name.clone(), c.class),
            })
            .collect();
        Layout { columns, ..old.clone() }
    }
}

impl Edits for ColumnPlan {
    fn record(&mut self, layout: &Layout, unit: Unit<'_>) -> Result<Out, DbError> {
        let (record, header) = match unit {
            Unit::Header(record) => (record, true),
            Unit::Data { record, .. } => (record, false),
        };
        let encode = |text: &str| encode_field(text, &layout.dialect, &layout.style).into_bytes();
        Ok(Out::Replace(splice(layout, record, |fields| {
            let extra = fields.split_off(self.width.min(fields.len()));
            let old = std::mem::take(fields);
            for column in &self.columns {
                fields.push(match (&column.origin, header) {
                    (Origin::Old(at), _) if !(header && column.renamed) => {
                        old.get(*at).cloned().unwrap_or_default()
                    }
                    (_, true) => encode(&column.name),
                    (Origin::New(text), false) => encode(text),
                    (Origin::Old(_), false) => Vec::new(),
                });
            }
            fields.extend(extra);
        })))
    }
}

#[cfg(test)]
#[path = "../tests/unit/commit.rs"]
mod tests;
