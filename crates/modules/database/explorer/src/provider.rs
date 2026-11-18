//! Which files open as data, and how: a file a registered driver opens by itself
//! ([`database::drivers::for_file`]: its extension, then its first bytes) — a binary one
//! (SQLite) instead of its bytes ([`ViewerKind::Replace`], asked before the binary files'
//! notice, which is the fallback), a text one (CSV) beside its editable text
//! ([`ViewerKind::Preview`], the data shown first). A text file too large for the editor
//! opens as data alone. Only the file's first bytes are read to tell.

use std::path::Path;

use database::DataFiles;
use document::{DocumentStorage, TextCheck, probe_text};
use fs_local::LocalFileSystem;
use ui_components::{Viewer, ViewerKind, ViewerProvider};

use crate::view::{DataView, VIEW};

/// The data view's provider.
#[derive(Debug, Default)]
pub struct DataViewProvider;

/// The driver that opens `path` by itself, if any (its first bytes read through
/// `storage`).
pub fn data_files(path: &Path, storage: &dyn DocumentStorage) -> Option<DataFiles> {
    database::drivers::for_file(path, |n| storage.head(path, n).ok().map(|(_, head)| head))
}

/// How a file opened by `files` is shown: a binary one, or a text one too large for the
/// editor, alone; a text one beside its text.
pub fn kind_of(files: &DataFiles, check: Option<TextCheck>) -> ViewerKind {
    match check {
        _ if !files.text => ViewerKind::Replace,
        Some(TextCheck::TooLarge | TextCheck::Binary) => ViewerKind::Replace,
        _ => ViewerKind::Preview,
    }
}

/// The virtual extension of a configured source's view (opened by the "Database" window:
/// [`crate::view::OpenSource`]), the same view as a file's.
pub const SOURCE_EXTENSION: &str = "view-data-source";

fn is_source_view(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()) == Some(SOURCE_EXTENSION)
}

impl ViewerProvider for DataViewProvider {
    fn id(&self) -> &'static str {
        VIEW
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        if is_source_view(path) {
            return Some(ViewerKind::Replace);
        }
        let files = data_files(path, &LocalFileSystem)?;
        let check = files.text.then(|| probe_text(path, &LocalFileSystem).ok()).flatten();
        Some(kind_of(&files, check))
    }

    fn open(&self, path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        if is_source_view(path) {
            return Ok(Box::new(DataView::for_source()));
        }
        let files = data_files(path, &LocalFileSystem)
            .ok_or_else(|| std::io::Error::other("not a data file"))?;
        Ok(Box::new(DataView::new(path.to_path_buf(), files)))
    }
}

#[cfg(test)]
#[path = "../tests/unit/provider.rs"]
mod tests;
