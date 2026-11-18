//! The project's queries, as one port: paths, declarations, text and filtered file
//! listings — everything a feature (a module, a refactoring, an agent) asks about the
//! project. An implementation is a snapshot, usable from any thread.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use code_intelligence::{ClassDiagram, SymbolHit, SymbolIndex, SymbolQuery};
use text_search::{Hit, SearchQuery};
use workspace::{FileFilter, PathQuery, PathResults};

/// What the app gives a panel (`set_data`) at its opening and whenever the project or its
/// files change: the project's queries (`None`: no project open), and the events it may
/// follow (the project's entries changing).
#[derive(Clone)]
pub struct ProjectView {
    /// The project's identity (`None`: no project open).
    pub project: Option<kernel::ProjectId>,
    pub queries: Option<Arc<dyn ProjectQueries>>,
    pub events: kernel::EventBus,
    /// The project's files are being indexed for the first time (searches find little yet).
    pub indexing: bool,
}

impl std::fmt::Debug for ProjectView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectView")
            .field("root", &self.queries.as_ref().map(|q| q.root()))
            .finish_non_exhaustive()
    }
}

/// What a panel showing the project's entries (the project tree) is told (`tell_panel`): by
/// the app (Locate, Go to File) or by a module (a new entry, a pasted one shown).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntriesRequest {
    /// Show `path` (its folders opened), selected; a folder opened too if `expand`.
    Reveal { path: PathBuf, expand: bool },
    /// Its search for files takes the keyboard (its text selected).
    FocusSearch,
    /// A new file / folder typed in place, in the selected folder.
    NewEntry { is_dir: bool },
    /// Entry `path`'s new name typed in place, in its row (Rename…).
    Rename { path: PathBuf },
}

pub trait ProjectQueries: Send + Sync {
    /// Folders and files by (fuzzy) name.
    fn paths(&self, query: &PathQuery<'_>) -> PathResults;
    /// The folders a path typed so far goes on to (`src/Co` → `src/Controller`), relative
    /// to the root, at most `limit` (see [`workspace::complete_folders`]). By default none.
    fn complete_folder(&self, typed: &str, limit: usize) -> Vec<String> {
        let _ = (typed, limit);
        Vec::new()
    }
    /// Declarations by (fuzzy) name, filtered by kind / language / dependencies.
    fn symbols(&self, query: &SymbolQuery<'_>) -> Vec<SymbolHit>;
    /// Text and meaning (lexical, semantic when the model is loaded).
    fn text(&self, query: &SearchQuery) -> Vec<Hit>;
    /// The project's files `filter` accepts (absolute paths).
    fn files(&self, filter: &FileFilter) -> Vec<PathBuf>;
    /// Type `fqn`'s diagram: its ancestors, children and constructor dependencies.
    fn class_diagram(&self, fqn: &str) -> Option<ClassDiagram>;
    /// The project's root.
    fn root(&self) -> PathBuf;
    /// The project's files (relative to the root, with `/`), one shared list: the same
    /// `Arc` while they don't change.
    fn file_list(&self) -> Arc<Vec<String>>;
    /// A copy of the declarations' index (its tables shared, copied on write: cheap), read
    /// without holding it (a long job doesn't block the indexer).
    fn symbol_index(&self) -> SymbolIndex;
    /// A file's language (its id: `php`, `javascript`…).
    fn language_of(&self, path: &Path) -> String;
    /// Changes when the project's files do (a module's caches go by it).
    fn generation(&self) -> u64;
    /// Folder `dir`'s entries as the project shows them: the disk's, plus what modules know
    /// is there without being on disk (a remote project's files: absent), with their marks.
    fn list_dir(&self, dir: &Path) -> std::io::Result<Vec<workspace::DirEntry>>;
    /// A file's icon (its name in the app's set) and its color's language, by the languages'
    /// rules (`file.php` → `file-code`, `php`).
    fn file_icon(&self, name: &str) -> (&'static str, String);
    /// Every language's rules for the dependency graph ([`code_intelligence::SourceRules`]).
    fn source_rules(&self) -> Vec<code_intelligence::SourceRules> {
        Vec::new()
    }
    /// A folder's color key by its name (`src` → `source`), by the modules' rules; `None`:
    /// a plain folder.
    /// How a declaration of `language` (of `kind`) is searched for ("Find Usages"): the
    /// extensions of the files it can be used in and its language's rules; `None`: nowhere.
    fn usage_search(
        &self,
        language: &str,
        kind: code_intelligence::SymbolKind,
    ) -> Option<(&'static [&'static str], code_intelligence::SourceRules)> {
        let _ = (language, kind);
        None
    }

    /// A project file's text (`relative` to the root) as on disk; too big to be code, or
    /// unreadable: `None`.
    fn read_source(&self, relative: &Path) -> Option<String> {
        let _ = relative;
        None
    }

    /// What the user marked in the project (`None`: nothing).
    fn path_marks(&self) -> Option<std::sync::Arc<workspace::marks::PathMarks>> {
        None
    }

    fn folder_color(&self, name: &str) -> Option<&'static str> {
        let _ = name;
        None
    }
}
