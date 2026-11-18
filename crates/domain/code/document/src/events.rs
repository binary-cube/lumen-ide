//! The document context's domain events.

use std::path::PathBuf;

use kernel::DomainEvent;

/// A document was written to its file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSaved {
    pub path: PathBuf,
}

impl DomainEvent for FileSaved {}

/// A document is about to be written to its (existing) file: what's on disk is still the
/// previous version (the local history keeps it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileWillSave {
    pub path: PathBuf,
}

impl DomainEvent for FileWillSave {}

/// A file's document is open in the editor (a project's tab): its text as it is now. What
/// follows the documents (a language server) takes them from here, never from the editor.
#[derive(Debug, Clone)]
pub struct DocumentOpened {
    pub document: kernel::DocumentId,
    pub path: PathBuf,
    /// Its language (its id: `typescript`).
    pub language: String,
    pub version: u64,
    pub text: crate::TextSnapshot,
}

impl DomainEvent for DocumentOpened {}

/// An open document's text changed (typed, undone, reloaded, formatted): its new version.
/// Announced once per change seen, never per keystroke's parts.
#[derive(Debug, Clone)]
pub struct DocumentChanged {
    pub document: kernel::DocumentId,
    pub path: PathBuf,
    pub version: u64,
    pub text: crate::TextSnapshot,
}

impl DomainEvent for DocumentChanged {}

/// A document isn't open any more (its tab closed, the file renamed: opened again under the
/// new name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentClosed {
    pub document: kernel::DocumentId,
    pub path: PathBuf,
}

impl DomainEvent for DocumentClosed {}
