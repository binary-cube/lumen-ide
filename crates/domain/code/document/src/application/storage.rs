//! Opening and saving documents, through the [`DocumentStorage`] port.

use std::io;
use std::path::{Path, PathBuf};

use crate::domain::document::Document;
use crate::domain::encoding::{SNIFF_LEN, TextCheck, check_text, decode};

/// The port to where documents are stored (implemented in `fs-local`).
pub trait DocumentStorage {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;
    /// The file's size and at most `max` bytes from its start (without reading the rest).
    fn head(&self, path: &Path, max: usize) -> io::Result<(u64, Vec<u8>)>;
    /// Atomic write: either the full new content appears, or the old one remains.
    fn write(&self, path: &Path, contents: &[u8]) -> io::Result<()>;
    /// The file can't be written (no permission, the read-only attribute, a read-only
    /// volume): its tab opens locked. By default, every file can.
    fn read_only(&self, path: &Path) -> bool {
        let _ = path;
        false
    }
}

/// Whether `path` can be opened as text (only its start is read).
pub fn probe_text(path: &Path, storage: &dyn DocumentStorage) -> io::Result<TextCheck> {
    let (len, head) = storage.head(path, SNIFF_LEN)?;
    Ok(check_text(len, &head))
}

/// Opens `path` as a document; a binary or too large file is refused (`InvalidData`)
/// before it's read.
pub fn open_document(path: &Path, storage: &dyn DocumentStorage) -> io::Result<Document> {
    match probe_text(path, storage)? {
        TextCheck::Text => {}
        TextCheck::Binary => {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "not a text file"));
        }
        TextCheck::TooLarge => {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "too large to open as text"));
        }
    }
    let bytes = storage.read(path)?;
    Ok(Document::from_decoded(Some(path.to_path_buf()), decode(&bytes)))
}

/// Saves to `path` or, if absent, to the document's path.
pub fn save_document(
    doc: &mut Document,
    path: Option<PathBuf>,
    storage: &dyn DocumentStorage,
) -> io::Result<()> {
    let save = prepare_save(doc, path)?;
    storage.write(&save.path, &save.bytes)?;
    doc.mark_saved(save.path);
    Ok(())
}

/// A save's first half, made where the document is: where it goes, its bytes, the version
/// and format they are of. Written elsewhere (a worker), then told to the document
/// ([`Document::mark_saved_version`]).
#[derive(Debug)]
pub struct PreparedSave {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
    pub version: u64,
    pub format: (crate::domain::encoding::Encoding, crate::domain::encoding::LineEnding),
}

/// The document cleaned up for saving (its trailing spaces, its final newline, as it says)
/// and encoded, for `path` or, if absent, its own.
pub fn prepare_save(doc: &mut Document, path: Option<PathBuf>) -> io::Result<PreparedSave> {
    let path = path
        .or_else(|| doc.path().map(Path::to_path_buf))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the document has no path"))?;
    crate::application::commands::cleanup_for_save(doc);
    // Characters the file's encoding can't hold (`ș` or `€` in a Latin-1 file) are never
    // replaced: the file becomes UTF-8 (the caller tells the user).
    if !crate::domain::encoding::represents_chunks(doc.buffer().chunks(), doc.encoding()) {
        doc.set_encoding(crate::domain::encoding::Encoding::Utf8);
    }
    Ok(PreparedSave {
        path,
        bytes: doc.encoded(),
        version: doc.version(),
        format: (doc.encoding(), doc.line_ending()),
    })
}

#[cfg(test)]
#[path = "../../tests/unit/application/storage.rs"]
mod tests;
