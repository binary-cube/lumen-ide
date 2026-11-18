//! The workspace context's domain events.

use std::path::PathBuf;

use kernel::DomainEvent;

/// A project was opened (everything project-scoped starts from scratch).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectOpened {
    pub root: PathBuf,
}

impl DomainEvent for ProjectOpened {}

/// The project's window came to the front of the app's windows: the others' projects are
/// now behind it. (Published only on the project coming forward; another program taking the
/// focus is no such thing.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectBroughtForward;

impl DomainEvent for ProjectBroughtForward {}

/// The user left the project's window: it lost the keyboard (another program in front,
/// another of the app's windows, minimized). What the project holds only for its use may go
/// after a long while unused; [`ProjectBroughtForward`] when it's back. (A project opened
/// without coming forward — the app behind — is left from the start.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectLeft;

impl DomainEvent for ProjectLeft {}

/// The user's marks on the project's paths changed: what's indexed is looked at again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathMarksChanged;

impl DomainEvent for PathMarksChanged {}

/// A file of the project was renamed or moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRenamed {
    pub from: PathBuf,
    pub to: PathBuf,
}

impl DomainEvent for FileRenamed {}

/// Files / folders of the project were deleted (moved to the system's trash).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntriesDeleted {
    pub paths: Vec<PathBuf>,
}

impl DomainEvent for EntriesDeleted {}

/// Entries of the project were made, removed or renamed on disk (by anyone: the system's
/// watcher saw them), or what a module knows of a folder changed (listed again): each path
/// is the entry itself — its folder changed, and, for a folder, what it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntriesChanged {
    pub paths: Vec<PathBuf>,
}

impl DomainEvent for EntriesChanged {}

/// Files right inside the project's own folder were written (by us or by hand): what shows
/// or holds them follows. Not the project's entries: nothing is indexed again for them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnFilesWritten {
    pub paths: Vec<PathBuf>,
}

impl DomainEvent for OwnFilesWritten {}

/// Files' contents were written on disk by others (a download, another program, a formatter;
/// our own saves too): whoever reads them reads them again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilesWritten {
    pub paths: Vec<PathBuf>,
}

impl DomainEvent for FilesWritten {}

/// Changes to the project's entries may have been missed (the watcher just started, after
/// the project opened): whoever follows them looks at the whole project again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntriesRescan {
    pub root: PathBuf,
}

impl DomainEvent for EntriesRescan {}

/// The project's indexes are asked to be built again from scratch ("Reindex Project",
/// "Clear and Rebuild Caches").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReindexRequested {
    pub root: PathBuf,
}

impl DomainEvent for ReindexRequested {}

/// "Clear and Rebuild Caches" was asked for the project at `root`: what modules keep for it
/// only to avoid recomputing (on disk too — the language servers' own folders) goes, to be
/// made again as needed. Announced before [`ReindexRequested`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachesCleared {
    pub root: PathBuf,
}

impl DomainEvent for CachesCleared {}

/// Many entries start changing together (a paste of many files, a folder moved): whoever
/// follows the entries may wait for [`BulkChangeEnded`] and take the changes as one.
/// `id`: the batch's identity (a UUID v4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BulkChangeStarted {
    pub id: u128,
}

impl DomainEvent for BulkChangeStarted {}

/// The batch `id` ended (whether it succeeded or not).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BulkChangeEnded {
    pub id: u128,
}

impl DomainEvent for BulkChangeEnded {}

/// One of the project's indexes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IndexKind {
    /// The files' and folders' paths (Go to File).
    Paths,
    /// The declarations and the text (Go to Declaration, Search in Project).
    Symbols,
}

/// An index started working (built or brought up to date).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexingStarted {
    pub index: IndexKind,
}

impl DomainEvent for IndexingStarted {}

/// An index is up to date.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexingFinished {
    pub index: IndexKind,
}

impl DomainEvent for IndexingFinished {}

/// How much memory the symbol index takes per folder, measured after a build (relative paths,
/// with `/`, a few levels deep).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexMemoryMeasured {
    pub folders: std::sync::Arc<std::collections::HashMap<String, u64>>,
}

impl DomainEvent for IndexMemoryMeasured {}

/// An index brought these files' entries up to date one by one (a save, a change on disk,
/// a rename; a build is [`IndexingFinished`]): what's checked against the rest of the
/// project (an open file's calls into them) is checked again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexUpdated {
    pub index: IndexKind,
    pub paths: Vec<std::path::PathBuf>,
}

impl DomainEvent for IndexUpdated {}
