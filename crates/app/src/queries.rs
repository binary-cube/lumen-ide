//! The project's queries (the `ProjectQueries` port): one snapshot of the project files
//! service, the symbol index and the text search, handed to whoever asks (modules, the
//! refactorings, background jobs) — usable from any thread.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};

use code_intelligence::{SymbolHit, SymbolIndex, SymbolQuery};
use text_search::{CodeSearch, Hit, SearchQuery};
use workbench_model::ProjectQueries;
use workspace::{FileFilter, PathIndex, PathQuery, PathResults};

/// A folder's entries as the project shows them (the disk and the modules' listings).
pub type ListDir = dyn Fn(&Path) -> std::io::Result<Vec<workspace::DirEntry>> + Send + Sync;
/// A file's icon and its color's language.
pub type FileIcon = dyn Fn(&str) -> (&'static str, String) + Send + Sync;
pub type FolderColor = dyn Fn(&str) -> Option<&'static str> + Send + Sync;

pub struct QueriesSnapshot {
    pub root: PathBuf,
    /// The disk the project is walked on when there's no index yet (the workbench's port).
    pub disk: Arc<dyn workspace::FileSystem + Send + Sync>,
    /// The files service's index when the snapshot was taken (`None`: not built yet — the
    /// project is walked once, on the thread asking).
    pub files: Option<Arc<Vec<String>>>,
    pub folders: Option<Arc<Vec<String>>>,
    /// The files `.gitignore` leaves out: found by name (Go to File, the search's files by
    /// name), never searched in or listed as the project's files.
    pub ignored: Option<Arc<Vec<String>>>,
    pub symbols: Arc<RwLock<SymbolIndex>>,
    pub text: Option<Arc<dyn CodeSearch>>,
    pub walked: OnceLock<PathIndex>,
    /// The languages' registry's answer for a file.
    pub language_of: Arc<dyn Fn(&Path) -> String + Send + Sync>,
    /// The files' generation when the snapshot was taken.
    pub generation: u64,
    /// The modules' listings merged with the disk's (`ListedFs`).
    pub list_dir: Arc<ListDir>,
    /// The languages' icon rules.
    pub file_icon: Arc<FileIcon>,
    /// The modules' folder colors.
    pub folder_color: Arc<FolderColor>,
    /// What the user marked in the project.
    pub marks: workspace::marks::ProjectMarks,
    /// The languages' intelligence (their naming rules).
    pub intelligence: Vec<Arc<dyn code_intelligence::LanguageIntelligence>>,
}

impl std::fmt::Debug for QueriesSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QueriesSnapshot").field("root", &self.root).finish_non_exhaustive()
    }
}

impl QueriesSnapshot {
    /// The files and folders: the index, or the project walked once.
    fn index(&self) -> (&[String], &[String]) {
        if let Some(files) = &self.files {
            return (files, self.folders.as_deref().map_or(&[], Vec::as_slice));
        }
        let walked = self.walked.get_or_init(|| {
            workspace::index_project(&self.root, &*self.disk, workspace::search::FILE_LIMIT)
                .unwrap_or_default()
        });
        (&walked.files, &walked.folders)
    }
}

impl ProjectQueries for QueriesSnapshot {
    fn paths(&self, query: &PathQuery<'_>) -> PathResults {
        let (files, folders) = self.index();
        let ignored: &[String] = match (&self.files, &self.ignored) {
            (Some(_), Some(ignored)) => ignored,
            // Walked once (no index yet): its own.
            (None, _) => self.walked.get().map_or(&[], |w| w.ignored.as_slice()),
            _ => &[],
        };
        workspace::search_paths_with(files, ignored, folders, query)
    }

    fn complete_folder(&self, typed: &str, limit: usize) -> Vec<String> {
        let (_, folders) = self.index();
        workspace::complete_folders(folders, typed, limit)
    }

    fn symbols(&self, query: &SymbolQuery<'_>) -> Vec<SymbolHit> {
        let Ok(index) = self.symbols.read() else { return Vec::new() };
        let query = SymbolQuery { root: query.root.or(Some(self.root.as_path())), ..query.clone() };
        code_intelligence::search_symbols(&index, &query)
    }

    fn text(&self, query: &SearchQuery) -> Vec<Hit> {
        self.text.as_ref().map_or_else(Vec::new, |t| t.search(query))
    }

    fn files(&self, filter: &FileFilter) -> Vec<PathBuf> {
        let (files, _) = self.index();
        files.iter().filter(|f| filter.accepts(f)).map(|f| self.root.join(f)).collect()
    }

    fn class_diagram(&self, fqn: &str) -> Option<code_intelligence::ClassDiagram> {
        let index = self.symbols.read().ok()?;
        let module =
            |language: &str| self.intelligence.iter().find(|i| i.language().as_str() == language);
        let naming = |language: &str| module(language).map(|i| i.naming()).unwrap_or_default();
        let parameter_types = |language: &str, detail: &str| {
            module(language).map(|i| i.parameter_types(detail)).unwrap_or_default()
        };
        code_intelligence::class_diagram(&index, fqn, &naming, &parameter_types)
    }

    fn root(&self) -> PathBuf {
        self.root.clone()
    }

    fn file_list(&self) -> Arc<Vec<String>> {
        match &self.files {
            Some(files) => Arc::clone(files),
            None => Arc::new(self.index().0.to_vec()),
        }
    }

    fn symbol_index(&self) -> SymbolIndex {
        self.symbols.read().map(|i| i.clone()).unwrap_or_default()
    }

    fn language_of(&self, path: &Path) -> String {
        (self.language_of)(path)
    }

    fn generation(&self) -> u64 {
        self.generation
    }

    fn list_dir(&self, dir: &Path) -> std::io::Result<Vec<workspace::DirEntry>> {
        (self.list_dir)(dir)
    }

    fn file_icon(&self, name: &str) -> (&'static str, String) {
        (self.file_icon)(name)
    }

    fn source_rules(&self) -> Vec<code_intelligence::SourceRules> {
        self.intelligence.iter().filter_map(|i| i.source_rules()).collect()
    }

    fn usage_search(
        &self,
        language: &str,
        kind: code_intelligence::SymbolKind,
    ) -> Option<(&'static [&'static str], code_intelligence::SourceRules)> {
        let module = self.intelligence.iter().find(|i| i.language().as_str() == language)?;
        let extensions = module.usage_extensions(kind);
        // How its language's code is written (none: plain names).
        let rules = module.source_rules().unwrap_or(code_intelligence::SourceRules::plain(&[]));
        (!extensions.is_empty()).then_some((extensions, rules))
    }

    fn read_source(&self, relative: &Path) -> Option<String> {
        let path = self.root.join(relative);
        std::fs::metadata(&path).ok().filter(|m| m.len() <= workspace::MAX_SOURCE_FILE)?;
        std::fs::read_to_string(path).ok()
    }

    fn path_marks(&self) -> Option<Arc<workspace::marks::PathMarks>> {
        Some(self.marks.get()).filter(|marks| !marks.is_empty())
    }

    fn folder_color(&self, name: &str) -> Option<&'static str> {
        (self.folder_color)(name)
    }
}

#[cfg(test)]
#[path = "../tests/unit/queries.rs"]
mod tests;
