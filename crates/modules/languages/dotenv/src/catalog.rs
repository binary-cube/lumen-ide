//! A project's `.env` files, read in the background and kept up to date from its events:
//! a folder's (the files of the `.env` language), in the order the project's framework
//! loads them. Code is served by its folder's files, or those of the nearest folder above it
//! that has some, up to the root (a monorepo's apps each have theirs). One catalog per open
//! project ([`of`]), gone with it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use code_intelligence::env::EnvFileOrder;
use kernel::ProjectId;
use workbench_model::LanguageOf;

use crate::order;
use crate::syntax::{Entry, EnvFile, Line};

/// A file larger than this isn't read (not a `.env` file anyone writes by hand).
const MAX_FILE: u64 = 1 << 20;

/// A `.env` file as last read.
#[derive(Debug, Clone)]
pub struct EnvDoc {
    pub path: PathBuf,
    /// Its name (`.env.local`).
    pub name: String,
    pub text: Arc<str>,
    pub file: EnvFile,
    /// It documents the keys (`.env.example`): never loaded.
    pub documents: bool,
}

impl EnvDoc {
    pub fn new(path: PathBuf, text: Arc<str>) -> Self {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let file = EnvFile::parse(&text);
        Self { documents: order::documents(&name), path, name, text, file }
    }

    /// The definition of `key` the loaders keep (its last): its line and its entry.
    pub fn defined(&self, key: &str) -> Option<(&Line, &Entry)> {
        self.file.definitions(&self.text, key).last()
    }

    /// The value its definition of `key` gives (as written, quotes left out).
    pub fn value(&self, key: &str) -> Option<&str> {
        self.defined(key).map(|(_, e)| &self.text[e.value.content.clone()])
    }
}

/// A folder's files: being read, or read (in the order they're loaded).
#[derive(Debug, Clone)]
enum Folder {
    Reading,
    Read(Arc<[EnvDoc]>),
}

/// An open project's `.env` files.
pub struct Catalog {
    root: PathBuf,
    language_of: LanguageOf,
    /// Called once a folder was read (from the reading thread): the app woken, what
    /// depends on the files (the checks) done again.
    read: Arc<dyn Fn() + Send + Sync>,
    /// The project's load order (its framework's markers looked for once, off the UI).
    order: OnceLock<EnvFileOrder>,
    folders: Mutex<HashMap<PathBuf, Folder>>,
    /// Bumped whenever a folder is read.
    generation: AtomicU64,
}

impl std::fmt::Debug for Catalog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Catalog").field("root", &self.root).finish_non_exhaustive()
    }
}

impl Catalog {
    /// The catalog of the project at `root`; its root's files are read at once.
    pub fn new(
        root: PathBuf,
        language_of: LanguageOf,
        read: Arc<dyn Fn() + Send + Sync>,
    ) -> Arc<Self> {
        let catalog = Arc::new(Self {
            root,
            language_of,
            read,
            order: OnceLock::new(),
            folders: Mutex::default(),
            generation: AtomicU64::new(0),
        });
        catalog.request(catalog.root.clone());
        catalog
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The order the project's `.env` files are loaded in (its framework's).
    pub fn order(&self) -> &EnvFileOrder {
        self.order.get_or_init(|| {
            *order::of_project(&code_intelligence::env::orders(), &|m| self.root.join(m).exists())
        })
    }

    /// Bumped whenever a folder's files are read.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Whether some folder is being read.
    pub fn busy(&self) -> bool {
        self.lock().values().any(|f| matches!(f, Folder::Reading))
    }

    /// A file of the `.env` language.
    pub fn is_env(&self, path: &Path) -> bool {
        (self.language_of)(path).as_deref() == Some(crate::ID)
    }

    /// The files serving code in `file`'s folder: the nearest folder's with some, from it up
    /// to the root. `None` while a folder on the way is still being read (asked now: the
    /// catalog calls back once it's read).
    pub fn files_for(self: &Arc<Self>, file: &Path) -> Option<Arc<[EnvDoc]>> {
        let mut waiting = false;
        for folder in self.chain(file) {
            let known = self.lock().get(&folder).cloned();
            match known {
                Some(Folder::Read(docs)) if !docs.is_empty() => return (!waiting).then_some(docs),
                Some(Folder::Read(_)) => {}
                Some(Folder::Reading) => waiting = true,
                None => {
                    self.request(folder);
                    waiting = true;
                }
            }
        }
        (!waiting).then(|| Arc::from(Vec::new()))
    }

    /// Every folder's files read so far.
    pub fn all(&self) -> Vec<Arc<[EnvDoc]>> {
        self.lock()
            .values()
            .filter_map(|f| match f {
                Folder::Read(docs) if !docs.is_empty() => Some(Arc::clone(docs)),
                _ => None,
            })
            .collect()
    }

    /// `path` changed on disk (saved, written, made, removed, renamed): its folder is read
    /// again if `path` is a `.env` file there, or a folder read before.
    pub fn changed(self: &Arc<Self>, path: &Path) {
        let known = |p: &Path| self.lock().contains_key(p);
        let folder = if known(path) {
            Some(path.to_path_buf())
        } else if self.is_env(path) {
            path.parent().filter(|p| known(p)).map(Path::to_path_buf)
        } else {
            None
        };
        if let Some(folder) = folder {
            self.request(folder);
        }
    }

    /// Every folder read again (changes may have been missed).
    pub fn rescan(self: &Arc<Self>) {
        let folders: Vec<PathBuf> = self.lock().keys().cloned().collect();
        for folder in folders {
            self.request(folder);
        }
    }

    /// The folders from `file`'s up to the root (its own alone, outside the project).
    fn chain(&self, file: &Path) -> Vec<PathBuf> {
        let Some(folder) = file.parent() else { return Vec::new() };
        if !folder.starts_with(&self.root) {
            return vec![folder.to_path_buf()];
        }
        folder
            .ancestors()
            .take_while(|f| f.starts_with(&self.root))
            .map(Path::to_path_buf)
            .collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<PathBuf, Folder>> {
        self.folders.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Reads `folder`'s files in the background.
    fn request(self: &Arc<Self>, folder: PathBuf) {
        self.lock().insert(folder.clone(), Folder::Reading);
        let catalog = Arc::clone(self);
        kernel::work::run(crate::THREAD, move || {
            let docs = catalog.read_folder(&folder);
            catalog.lock().insert(folder, Folder::Read(docs));
            catalog.generation.fetch_add(1, Ordering::AcqRel);
            (catalog.read)();
        });
    }

    /// `folder`'s `.env` files, in the order they're loaded.
    fn read_folder(&self, folder: &Path) -> Arc<[EnvDoc]> {
        let mut docs: Vec<EnvDoc> = std::fs::read_dir(folder)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .filter(|e| e.metadata().is_ok_and(|m| m.len() <= MAX_FILE))
            .map(|e| e.path())
            .filter(|p| self.is_env(p))
            .filter_map(|p| Some(EnvDoc::new(p.clone(), std::fs::read_to_string(&p).ok()?.into())))
            .collect();
        order::sort(&mut docs, self.order(), |d| &d.name);
        docs.into()
    }
}

/// The open projects' catalogs.
static CATALOGS: workbench_model::PerProject<Catalog> = workbench_model::PerProject::new();

/// Project `id`'s catalog, while it's open.
pub fn of(id: ProjectId) -> Option<Arc<Catalog>> {
    CATALOGS.get(id)
}

/// Keeps `catalog` as project `id`'s until the subscription returned is dropped (the
/// project closes).
pub fn keep(id: ProjectId, catalog: Arc<Catalog>) -> kernel::Subscription {
    CATALOGS.insert(id, &catalog);
    kernel::Subscription::on_end(move || {
        CATALOGS.remove(id);
        drop(catalog);
    })
}

#[cfg(test)]
#[path = "../tests/unit/catalog.rs"]
mod tests;
