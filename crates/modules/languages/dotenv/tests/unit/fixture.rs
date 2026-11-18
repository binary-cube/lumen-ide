//! What the tests share: a temporary project folder (never the user's files) and waiting for
//! background work.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// A folder of the system's temp dir, gone with it.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(name: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "lang-dotenv-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    /// Writes `text` to `relative` (its folders made).
    pub fn write(&self, relative: &str, text: &str) -> PathBuf {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        path
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A file's language as the app would say: `.env` files by their names, the rest by their
/// extension.
pub fn language_of() -> workbench_model::LanguageOf {
    Arc::new(|path: &Path| {
        let name = path.file_name()?.to_string_lossy().into_owned();
        if name == ".env" || (name.starts_with(".env.") && !name.ends_with(".php")) {
            return Some(crate::ID.to_owned());
        }
        let extension = path.extension()?.to_string_lossy().into_owned();
        Some(match extension.as_str() {
            "yaml" | "yml" => "yaml".to_owned(),
            "js" => "javascript".to_owned(),
            other => other.to_owned(),
        })
    })
}

/// Waits (a few seconds at most) for `done`.
pub fn wait_until(mut done: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < until, "timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A project on disk as the queries port tells it: its files listed, their languages as
/// [`language_of`] says.
pub struct Project {
    pub dir: TempDir,
}

impl workbench_model::ProjectQueries for Project {
    fn paths(&self, _: &workspace::PathQuery<'_>) -> workspace::PathResults {
        workspace::PathResults::default()
    }
    fn symbols(&self, _: &code_intelligence::SymbolQuery<'_>) -> Vec<code_intelligence::SymbolHit> {
        Vec::new()
    }
    fn text(&self, _: &text_search::SearchQuery) -> Vec<text_search::Hit> {
        Vec::new()
    }
    fn files(&self, _: &workspace::FileFilter) -> Vec<PathBuf> {
        Vec::new()
    }
    fn class_diagram(&self, _: &str) -> Option<code_intelligence::ClassDiagram> {
        None
    }
    fn root(&self) -> PathBuf {
        self.dir.0.clone()
    }
    fn file_list(&self) -> Arc<Vec<String>> {
        fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, root, out);
                } else {
                    out.push(path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"));
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.dir.0, &self.dir.0, &mut out);
        out.sort();
        Arc::new(out)
    }
    fn symbol_index(&self) -> code_intelligence::SymbolIndex {
        code_intelligence::SymbolIndex::default()
    }
    fn language_of(&self, path: &Path) -> String {
        language_of()(path).unwrap_or_default()
    }
    fn generation(&self) -> u64 {
        1
    }
    fn list_dir(&self, _: &Path) -> std::io::Result<Vec<workspace::DirEntry>> {
        Ok(Vec::new())
    }
    fn file_icon(&self, _: &str) -> (&'static str, String) {
        ("file", String::new())
    }
    fn read_source(&self, relative: &Path) -> Option<String> {
        std::fs::read_to_string(self.dir.0.join(relative)).ok()
    }
}

/// The ways PHP and Symfony's configuration read variables, registered as the app does.
pub fn register_reads() {
    use code_intelligence::env::{EnvAccess, EnvReads, register_reads};
    const PHP: EnvReads =
        EnvReads::new(&["php"], &[EnvAccess::call("getenv"), EnvAccess::index("$_ENV")]);
    const YAML: EnvReads = EnvReads::new(&["yaml"], &[EnvAccess::placeholder("%env(", &[")%"])]);
    register_reads(PHP);
    register_reads(YAML);
}
