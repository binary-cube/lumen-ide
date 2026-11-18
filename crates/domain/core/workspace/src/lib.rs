//! Workspace bounded context: the open project, its files and its tree.

pub mod application;
pub mod clipboard;
pub mod delete;
pub mod domain;
pub mod entries;
pub mod events;
pub mod ignore;
pub mod marks;
pub mod mask;
pub mod recent;
pub mod search;
pub mod sessions;
pub mod watch;

/// Larger files aren't read as code (generated, minified, data): not indexed, not searched
/// for a symbol's usages, not scanned for a refactoring or the dependency graph.
pub const MAX_SOURCE_FILE: u64 = 2 * 1024 * 1024;

pub use application::{
    FileSystem, Stamp, expand_more, merge_listing, open_project, refresh_dir, refresh_path,
    refresh_paths, reveal_path, toggle_directory,
};
pub use domain::{DirEntry, FileTree, Mark, NodeId, NodeKind, Note, VisibleRow};
pub use mask::FileMask;
pub use recent::{RecentFiles, RecentStore, record as record_recent};
pub use search::{
    FileFilter, FileMatch, PathIndex, PathQuery, PathResults, complete_folders, index_files,
    index_project, search_files, search_folders, search_paths, search_paths_with, split_location,
};
pub use watch::{FileWatcher, FileWatching, WatchedDirs};
