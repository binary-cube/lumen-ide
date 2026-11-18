//! What every export of a grid's rows shares, whoever reads them (a table's worker, a query
//! console's result): the format ([`ExportFormat`]: one of the data extractors — the same
//! text ⌘C and "Copy As" write — or an Excel workbook, only here), the one a file's name asks
//! for, where the text goes ([`Target`]), what writes it a block at a time ([`ExportWriter`]),
//! the file written beside its target as a hidden temporary one and renamed over it once
//! complete — removed, the target left as it was, if it's cancelled or fails
//! ([`ExportSink`]) —, the clipboard's limits, and how it ended as the user is told it
//! ([`ExportOutcome::told`]).

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use kernel::ProjectId;
use workbench_model::Notification;

use super::copy::{CopyStream, Extractor, Options};
use super::source::{CellRef, GridColumn};
use super::xlsx::XlsxWriter;

/// At most this many rows are copied to the clipboard (more: to a file).
pub const CLIPBOARD_ROWS: u64 = 10_000;

/// At most this much text goes to the clipboard.
pub const CLIPBOARD_BYTES: usize = 16 << 20;

/// The id of the Excel workbook among the export's formats.
pub const XLSX: &str = "xlsx";

/// What an export writes.
#[derive(Debug, Clone)]
pub enum ExportFormat {
    /// An extractor's text.
    Text(Arc<dyn Extractor>),
    /// An Excel workbook (only an export writes one: the clipboard holds text).
    Xlsx,
}

impl ExportFormat {
    pub fn id(&self) -> &str {
        match self {
            Self::Text(e) => e.id(),
            Self::Xlsx => XLSX,
        }
    }

    pub fn name(&self) -> String {
        match self {
            Self::Text(e) => e.name(),
            Self::Xlsx => i18n::t("extract.xlsx"),
        }
    }

    /// Its usual extension.
    pub fn extension(&self) -> &str {
        match self {
            Self::Text(e) => e.extension(),
            Self::Xlsx => XLSX,
        }
    }

    pub fn is_text(&self) -> bool {
        matches!(self, Self::Text(_))
    }

    /// Format `id` of those offered to `project` (`None`: not one of them, or one that can't
    /// be used now — a template with an error).
    pub fn find(project: Option<ProjectId>, id: &str) -> Option<Self> {
        if id == XLSX {
            return Some(Self::Xlsx);
        }
        extractors::catalog().find(project, id).filter(|e| e.problem().is_none()).map(Self::Text)
    }

    /// The format `project` chose (⌘C's).
    pub fn chosen(project: Option<ProjectId>) -> Self {
        Self::Text(extractors::catalog().chosen(project))
    }

    /// The formats an export offers (every extractor that can be used, the workbook last;
    /// `text`: only those writing text — for the clipboard).
    pub fn offered(project: Option<ProjectId>, text: bool) -> Vec<Self> {
        let mut all: Vec<Self> = extractors::catalog()
            .all(project)
            .into_iter()
            .filter(|e| e.problem().is_none())
            .map(Self::Text)
            .collect();
        if !text {
            all.push(Self::Xlsx);
        }
        all
    }
}

/// The format a file's name asks for: `chosen` when its extension is the chosen one's, else
/// the one its extension names (`None`: one it doesn't name).
pub fn format_of(
    path: &Path,
    project: Option<ProjectId>,
    chosen: &ExportFormat,
) -> Option<ExportFormat> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if ext == chosen.extension() {
        return Some(chosen.clone());
    }
    let id = match ext.as_str() {
        "csv" => "csv",
        "tsv" | "tab" => "tsv-header",
        "json" => "json",
        "sql" => "sql",
        "md" | "markdown" => "markdown",
        "html" | "htm" => "html",
        "xml" => "xml",
        "xlsx" => XLSX,
        "txt" => "pretty",
        _ => return None,
    };
    ExportFormat::find(project, id)
}

/// What writes an export's bytes, a block of rows at a time: an extractor's text or a
/// workbook.
pub enum ExportWriter {
    Text(CopyStream),
    Xlsx(Box<XlsxWriter>),
}

impl std::fmt::Debug for ExportWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExportWriter").field("written", &self.written()).finish()
    }
}

impl ExportWriter {
    /// `format` writing `columns` of `table` (`sql`: the source's dialect, `options`: the
    /// columns its SQL skips).
    pub fn new(
        format: &ExportFormat,
        table: &str,
        columns: &[GridColumn],
        sql: Option<Arc<dyn extractors::SqlStyle>>,
        options: Options,
    ) -> Self {
        match format {
            ExportFormat::Text(e) => {
                Self::Text(CopyStream::new(e.as_ref(), table, columns, sql, options))
            }
            ExportFormat::Xlsx => Self::Xlsx(Box::new(XlsxWriter::new(table, columns))),
        }
    }

    /// Rows written so far.
    pub fn written(&self) -> u64 {
        match self {
            Self::Text(s) => s.written(),
            Self::Xlsx(x) => x.written(),
        }
    }

    pub fn is_text(&self) -> bool {
        matches!(self, Self::Text(_))
    }

    pub fn begin(&mut self) -> Result<Vec<u8>, String> {
        match self {
            Self::Text(s) => Ok(s.begin().into_bytes()),
            Self::Xlsx(x) => x.begin(),
        }
    }

    pub fn rows(&mut self, rows: &[Vec<CellRef<'_>>]) -> Result<Vec<u8>, String> {
        match self {
            Self::Text(s) => Ok(s.rows(rows).into_bytes()),
            Self::Xlsx(x) => x.rows(rows),
        }
    }

    pub fn finish(&mut self) -> Result<Vec<u8>, String> {
        match self {
            Self::Text(s) => Ok(s.finish().into_bytes()),
            Self::Xlsx(x) => x.finish(),
        }
    }
}

/// Where the rows go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// This file (replaced once the export is complete).
    File(PathBuf),
    /// The clipboard (the text handed back).
    Clipboard,
}

/// How an export ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportOutcome {
    Written { rows: u64, path: PathBuf },
    Copied { rows: u64, text: String },
    Cancelled,
    Failed(String),
}

impl ExportOutcome {
    /// What the user is told, and the text to put on the clipboard (copied rows).
    pub fn told(self) -> (Notification, Option<String>) {
        match self {
            Self::Written { rows, path } => (
                Notification::success(i18n::tr!(
                    "database.exported",
                    count = rows,
                    file = kernel::paths::file_name(&path).as_str()
                )),
                None,
            ),
            Self::Copied { rows, text } => (
                Notification::success(i18n::tr!("database.export_copied", count = rows)),
                Some(text),
            ),
            Self::Cancelled => (Notification::info(i18n::t("database.export_cancelled")), None),
            Self::Failed(why) => {
                (Notification::error(i18n::tr!("database.export_failed", why = why.as_str())), None)
            }
        }
    }
}

/// Where an export's text is written: a temporary file beside its target, or memory (the
/// clipboard's, within [`CLIPBOARD_BYTES`]).
#[derive(Debug)]
pub enum ExportSink {
    File { temp: PathBuf, target: PathBuf, out: BufWriter<File> },
    Memory(String),
}

impl ExportSink {
    /// Opens `target`: its temporary file made (beside it, hidden), or an empty text.
    pub fn open(target: &Target) -> std::io::Result<Self> {
        match target {
            Target::Clipboard => Ok(Self::Memory(String::new())),
            Target::File(target) => {
                let temp = temporary(target);
                File::create(&temp).map(|file| Self::File {
                    temp,
                    target: target.clone(),
                    out: BufWriter::with_capacity(256 << 10, file),
                })
            }
        }
    }

    pub fn write(&mut self, text: &str) -> std::io::Result<()> {
        self.write_bytes(text.as_bytes())
    }

    /// Bytes (the clipboard takes only text).
    pub fn write_bytes(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        match self {
            Self::File { out, .. } => out.write_all(bytes),
            Self::Memory(buffer) => {
                if buffer.len() + bytes.len() > CLIPBOARD_BYTES {
                    return Err(std::io::Error::other(i18n::t("database.export_too_large")));
                }
                let text = std::str::from_utf8(bytes).map_err(std::io::Error::other)?;
                buffer.push_str(text);
                Ok(())
            }
        }
    }

    /// Every row written: `finish` (the format's end) added, the file completed (a line
    /// break at the end of a text) and renamed over its target; `rows` were written.
    pub fn complete(self, finish: &[u8], rows: u64, text: bool) -> ExportOutcome {
        match self {
            Self::Memory(mut copied) => {
                copied.push_str(&String::from_utf8_lossy(finish));
                ExportOutcome::Copied { rows, text: copied }
            }
            Self::File { temp, target, mut out } => {
                let end: &[u8] = if text { b"\n" } else { b"" };
                let written = out
                    .write_all(finish)
                    .and_then(|()| out.write_all(end))
                    .and_then(|()| out.flush())
                    .and_then(|()| out.get_ref().sync_all());
                drop(out);
                match written.and_then(|()| std::fs::rename(&temp, &target)) {
                    Ok(()) => ExportOutcome::Written { rows, path: target },
                    Err(e) => {
                        let _ = std::fs::remove_file(&temp);
                        ExportOutcome::Failed(e.to_string())
                    }
                }
            }
        }
    }

    /// Given up: a temporary file removed (the target untouched).
    pub fn discard(self) {
        if let Self::File { temp, out, .. } = self {
            drop(out);
            let _ = std::fs::remove_file(&temp);
        }
    }
}

/// The temporary file written beside `target` (hidden, a new name each time).
pub fn temporary(target: &Path) -> PathBuf {
    let name = kernel::paths::file_name(target);
    let id = kernel::id::new_uuid();
    target.with_file_name(format!(".{name}.{id:032x}.part"))
}

#[cfg(test)]
#[path = "../../tests/unit/data_grid/export.rs"]
mod tests;
