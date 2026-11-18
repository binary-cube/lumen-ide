//! The drivers there are: each module bringing one contributes its [`DriverFactory`] (and,
//! for a file-based engine, the [`DataFiles`] it opens), the app registers them here at
//! startup, and whoever opens a source finds its driver by id — the explorer never depends
//! on a driver's module. Nothing of a driver runs until a connection opens.
//!
//! Drivers found at run time (an external driver's manifest, read from disk) come through
//! a discovery ([`register_discovery`]): run once, the first time the drivers are asked
//! for — nothing is looked for before a driver is needed.

use std::borrow::Cow;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use crate::DriverFactory;

/// Whether a file's first bytes are those a driver opens.
pub type HeadMatch = Arc<dyn Fn(&[u8]) -> bool + Send + Sync>;

/// The files a driver opens by themselves (a SQLite database, a CSV file): their
/// extensions, and what their first bytes must be — told without reading the whole file.
#[derive(Clone)]
pub struct DataFiles {
    /// The driver's id ([`DriverFactory::id`]).
    pub driver: Cow<'static, str>,
    /// Lowercase, without the dot (`sqlite`, `db`).
    pub extensions: Vec<Cow<'static, str>>,
    /// How many of the first bytes [`Self::matches`] looks at (0: the extension is enough).
    pub head: usize,
    /// Whether a file starting with these bytes (up to [`Self::head`] of them) is one.
    pub matches: HeadMatch,
    /// The files are text (a CSV): their data is shown beside their editable text, not
    /// instead of it.
    pub text: bool,
}

impl std::fmt::Debug for DataFiles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataFiles")
            .field("driver", &self.driver)
            .field("extensions", &self.extensions)
            .field("head", &self.head)
            .field("text", &self.text)
            .finish_non_exhaustive()
    }
}

impl DataFiles {
    /// A driver's files: known by `extensions`, then by what `matches` says of their first
    /// `head` bytes.
    pub fn new(
        driver: &'static str,
        extensions: &'static [&'static str],
        head: usize,
        matches: fn(&[u8]) -> bool,
        text: bool,
    ) -> Self {
        Self {
            driver: Cow::Borrowed(driver),
            extensions: extensions.iter().map(|e| Cow::Borrowed(*e)).collect(),
            head,
            matches: Arc::new(matches),
            text,
        }
    }

    /// Files known by `extensions` (any case) and starting with `prefix` (anything, if
    /// empty): what an external driver's manifest says of its files.
    pub fn starting_with(
        driver: String,
        extensions: Vec<String>,
        prefix: Vec<u8>,
        text: bool,
    ) -> Self {
        Self {
            driver: Cow::Owned(driver),
            extensions: extensions.into_iter().map(|e| Cow::Owned(e.to_lowercase())).collect(),
            head: prefix.len(),
            matches: Arc::new(move |head: &[u8]| head.starts_with(&prefix)),
            text,
        }
    }

    /// No files (the view of a source that isn't a file).
    pub fn none() -> Self {
        Self {
            driver: Cow::Borrowed(""),
            extensions: Vec::new(),
            head: 0,
            matches: Arc::new(|_| false),
            text: false,
        }
    }

    /// `path` has one of the extensions (any case).
    pub fn has_extension(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| self.extensions.iter().any(|x| x.eq_ignore_ascii_case(e)))
    }

    /// `path`, starting with `head`, is one of these files.
    pub fn recognizes(&self, path: &Path, head: &[u8]) -> bool {
        self.has_extension(path) && (self.matches)(head)
    }
}

static FACTORIES: RwLock<Vec<DriverFactory>> = RwLock::new(Vec::new());
static FILES: RwLock<Vec<DataFiles>> = RwLock::new(Vec::new());

/// Something registering the drivers it finds (through [`register`] and
/// [`register_files`]), run once.
type Discovery = Box<dyn FnOnce() + Send>;

static DISCOVERIES: Mutex<Vec<Discovery>> = Mutex::new(Vec::new());
/// A discovery waits to run (read without taking the lock).
static PENDING: AtomicBool = AtomicBool::new(false);

/// `discover` runs once, the first time a driver or a driver's files are asked for (not
/// before: nothing is read until a driver is needed), registering what it finds through
/// [`register`] and [`register_files`]. Whoever asks meanwhile waits for it.
pub fn register_discovery(discover: impl FnOnce() + Send + 'static) {
    DISCOVERIES.lock().unwrap_or_else(PoisonError::into_inner).push(Box::new(discover));
    PENDING.store(true, Ordering::Release);
}

/// Runs the discoveries waiting; whoever asks at the same time waits for them.
fn discovered() {
    if !PENDING.load(Ordering::Acquire) {
        return;
    }
    let mut waiting = DISCOVERIES.lock().unwrap_or_else(PoisonError::into_inner);
    for discover in waiting.drain(..) {
        discover();
    }
    PENDING.store(false, Ordering::Release);
}

/// Registers a driver (at startup, from its module's contribution); one with the same id
/// is replaced.
pub fn register(factory: DriverFactory) {
    let mut all = FACTORIES.write().unwrap_or_else(PoisonError::into_inner);
    all.retain(|f| f.id != factory.id);
    all.push(factory);
}

/// Registers the files a driver opens; those of the same driver are replaced.
pub fn register_files(files: DataFiles) {
    let mut all = FILES.write().unwrap_or_else(PoisonError::into_inner);
    all.retain(|f| f.driver != files.driver);
    all.push(files);
}

/// Every driver, in the order registered (the `+` menu's list).
pub fn all() -> Vec<DriverFactory> {
    discovered();
    FACTORIES.read().unwrap_or_else(PoisonError::into_inner).clone()
}

/// Whether a driver of id `id` is registered now, without running the discoveries: for a
/// discovery itself (a driver found never takes the place of one of the app's).
pub fn is_registered(id: &str) -> bool {
    FACTORIES.read().unwrap_or_else(PoisonError::into_inner).iter().any(|f| f.id == id)
}

/// The driver `id` (a source's [`crate::DataSource::driver`]).
pub fn find(id: &str) -> Option<DriverFactory> {
    discovered();
    FACTORIES.read().unwrap_or_else(PoisonError::into_inner).iter().find(|f| f.id == id).cloned()
}

/// The files the drivers open.
pub fn files() -> Vec<DataFiles> {
    discovered();
    FILES.read().unwrap_or_else(PoisonError::into_inner).clone()
}

/// Which driver opens `path` by itself: one whose extension it has and whose first bytes
/// it starts with — `head(n)` reads at most `n` of them (through the caller's port), only
/// when an extension matched; `None` if it can't be read.
pub fn for_file(path: &Path, head: impl FnOnce(usize) -> Option<Vec<u8>>) -> Option<DataFiles> {
    let candidates: Vec<DataFiles> =
        files().into_iter().filter(|f| f.has_extension(path)).collect();
    let len = candidates.iter().map(|f| f.head).max()?;
    let bytes = if len == 0 { Vec::new() } else { head(len)? };
    candidates.into_iter().find(|f| (f.matches)(&bytes))
}

#[cfg(test)]
#[path = "../tests/unit/drivers.rs"]
mod tests;
