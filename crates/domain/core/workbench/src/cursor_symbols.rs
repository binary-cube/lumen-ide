//! A module's own names at the cursor — a `.env` key, in its file or where code reads it —
//! as Rename and Find Usages see them: the module says what's renamed and where it's used
//! (a [`RenameContext`]: the name, the files that may use it, what a found name is); the
//! app asks the modules before the language servers and the built-in engine, and the
//! refactoring module's preview, applying and undo do the rest, as for any rename.

use std::collections::HashMap;
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use refactor::application::RenameContext;

use crate::ProjectQueries;

/// Where the cursor is, and the project around it.
#[derive(Clone)]
pub struct CursorContext<'a> {
    /// The open project's identity (`None`: a file opened alone).
    pub project: Option<kernel::ProjectId>,
    pub root: Option<&'a Path>,
    /// The project's files (`None`: no project open) — a snapshot, for a background thread.
    pub queries: Option<Arc<dyn ProjectQueries>>,
    /// The open documents' text, unsaved changes included (by absolute path).
    pub open: Arc<HashMap<PathBuf, String>>,
    /// The document's file (absolute), its language at the cursor, its text, the cursor.
    pub path: &'a Path,
    pub language: &'a str,
    pub text: &'a str,
    pub offset: usize,
}

impl Debug for CursorContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CursorContext")
            .field("path", &self.path)
            .field("language", &self.language)
            .field("offset", &self.offset)
            .finish_non_exhaustive()
    }
}

/// A module's names at the cursor.
pub trait CursorSymbols: Send + Sync + Debug {
    /// What a rename at the cursor renames, and where it's used — `None`: none of its names
    /// is there (the app asks on). Cheap: the places are found later, on a background
    /// thread, through the context's ports.
    fn rename_context(&self, at: &CursorContext<'_>) -> Option<RenameContext>;
}
