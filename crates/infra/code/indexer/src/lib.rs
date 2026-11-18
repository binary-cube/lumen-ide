//! The project's symbol index, for completion and "go to definition", in any
//! language that contributes a [`LanguageIntelligence`].
//!
//! - **Persistent:** stored in `<project>/<project dir>/index/symbols.bin` (with its own
//!   `.gitignore`). It loads instantly at startup, then each file is checked:
//!   same size and date → symbols from the cache; otherwise the content hash; only
//!   files that actually changed are reparsed.
//! - **Synchronized:** whoever starts it tells it what changed ([`ProjectIndexer::changed`],
//!   already gathered: the indexing module's queue, from the project's events) or asks it
//!   to look again ([`ProjectIndexer::refresh`]); only what changed is reindexed, in the
//!   background. It watches nothing itself (the project has one watcher).
//! - **No chained reindexing:** the index holds each file's declarations, and names
//!   (`use`, `extends`, `new X`) are resolved only on demand. When a class changes, only
//!   its file is reparsed; the files using it find it through the index.

mod codec;
mod search;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, UNIX_EPOCH};

use code_intelligence::{LanguageIntelligence, SymbolIndex};
use text_search::{CodeSearch, Embedder, Hit, SearchIndex, SearchQuery};
use workspace::marks::{PathMarks, ProjectMarks};
use workspace::search::skipped_dir;

pub use codec::CachedFile;

/// `entry` into `index` at `path`: a library file packed (its symbols given for the tables,
/// then let go of here — its block is what both keep; plan 74), the others shared.
fn give(index: &mut SymbolIndex, path: &Path, entry: &mut CachedFile, library: bool) {
    if !library {
        entry.packed = None;
        index.set_file_shared(path, Arc::clone(&entry.symbols));
        return;
    }
    let packed = Arc::clone(entry.packed.get_or_insert_with(|| codec::pack(&entry.symbols).into()));
    // Already let go of (a library file kept from an earlier build): its block unpacked once,
    // for the tables.
    let unpacked;
    let whole = if entry.symbols.symbols.is_empty() {
        unpacked = codec::unpack(&packed, &mut codec::Pool::default()).unwrap_or_default();
        &unpacked
    } else {
        &*entry.symbols
    };
    // The same library in another project: its bytes, shared, kept instead of these.
    entry.packed = Some(index.set_file_packed(path, whole, packed, codec::Codec::shared()));
    entry.symbols = codec::no_symbols();
}

pub type Waker = Arc<dyn Fn() + Send + Sync>;
type Intelligences = Vec<Arc<dyn LanguageIntelligence>>;

/// The cache is saved once changes stop for this long (both files are written whole, the
/// search one with every chunk's vector: tens of MB, never every few seconds while typing)…
const SAVE_WHEN_QUIET: Duration = Duration::from_secs(30);
/// …or after this long of changes that never stop; and always when the project closes.
const SAVE_AT_MOST_EVERY: Duration = Duration::from_secs(5 * 60);

enum Message {
    Rest,
    Wake,
    LoadSearch,
    Changed(Vec<PathBuf>),
    Rebuild,
    /// The whole project looked at again (an incremental build: only files whose
    /// fingerprint changed are parsed again).
    Refresh,
    /// The indexer was dropped: save and end.
    Stop,
    /// The embeddings model became available (search becomes semantic too).
    Embedder(Option<Arc<dyn Embedder>>),
}

/// What a project's indexer did, counted as it goes (cheap atomics, read from any thread —
/// the Task Manager's): the project's files it read, the cache it wrote, the CPU time its
/// own thread used (every project has its own: its CPU is the project's), its builds (a
/// whole look at the project) and the current one's files.
#[derive(Debug, Default)]
pub struct Usage {
    pub read: AtomicU64,
    pub written: AtomicU64,
    /// Nanoseconds.
    pub cpu: AtomicU64,
    /// The CPU time of the threads that read files beside it in a build (nanoseconds).
    helpers: AtomicU64,
    /// Builds started, and finished.
    pub builds: AtomicU64,
    pub built: AtomicU64,
    /// The current build's files: all of them, and done.
    pub files: AtomicU64,
    pub files_done: AtomicU64,
}

impl Usage {
    /// Called on the indexer's thread: its CPU time so far.
    fn note_cpu(&self) {
        if let Some(time) = kernel::cpu::thread_time() {
            let nanos = u64::try_from(time.as_nanos()).unwrap_or(u64::MAX);
            let helpers = self.helpers.load(Ordering::Relaxed);
            self.cpu.store(nanos.saturating_add(helpers), Ordering::Relaxed);
        }
    }

    /// Called on a helper thread when it's done: its CPU time counted with the indexer's.
    fn note_helper_cpu(&self) {
        if let Some(time) = kernel::cpu::thread_time() {
            let nanos = u64::try_from(time.as_nanos()).unwrap_or(u64::MAX);
            self.helpers.fetch_add(nanos, Ordering::Relaxed);
        }
    }

    pub(crate) fn read_bytes(&self, bytes: usize) {
        self.read.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    pub(crate) fn written_bytes(&self, bytes: usize) {
        self.written.fetch_add(bytes as u64, Ordering::Relaxed);
    }
}

/// A project's index, kept up to date on its own thread.
pub struct ProjectIndexer {
    marks: ProjectMarks,
    residency: Arc<SearchResidency>,
    usage: Arc<Usage>,
    index: Arc<RwLock<SymbolIndex>>,
    search: Arc<RwLock<SearchIndex>>,
    embedder: Arc<RwLock<Option<Arc<dyn Embedder>>>>,
    tx: Sender<Message>,
    busy: Arc<AtomicBool>,
    /// The files whose declarations an incremental reindex changed, not yet taken (see
    /// [`Self::take_updated`]).
    updated: Arc<std::sync::Mutex<Vec<PathBuf>>>,
    /// Set when the indexer is dropped (window closed, other project): a build in progress
    /// stops early instead of walking the whole project.
    stop: Arc<AtomicBool>,
    /// The project's root and the extensions indexed (what [`Self::wants`] answers from).
    root: PathBuf,
    extensions: Vec<&'static str>,
}

/// How long after its last question the text index leaves the memory (it stays on disk).
const SEARCH_UNUSED: Duration = Duration::from_secs(180);

/// Whether the text index is in memory: it's only asked when someone searches, and it's the
/// largest part of a project's index.
#[derive(Debug, Default)]
struct SearchResidency {
    /// Not in memory (kept up to date on disk: read and refreshed when it's asked).
    cold: AtomicBool,
    /// Its reading was asked for and hasn't ended.
    loading: AtomicBool,
    /// Read since [`ProjectIndexer::take_search_loaded`] last looked.
    loaded: AtomicBool,
    asked: std::sync::Mutex<Option<Instant>>,
}

impl SearchResidency {
    fn is_cold(&self) -> bool {
        self.cold.load(Ordering::Acquire)
    }

    fn set_cold(&self) {
        self.cold.store(true, Ordering::Release);
    }

    fn set_warm(&self) {
        self.cold.store(false, Ordering::Release);
        self.loading.store(false, Ordering::Release);
    }

    /// How long since it was last asked (never: as long as can be).
    fn unused_for(&self) -> Duration {
        self.asked
            .lock()
            .ok()
            .and_then(|asked| asked.map(|at| at.elapsed()))
            .unwrap_or(Duration::MAX)
    }
}

/// The [`CodeSearch`] implementation over the index kept up to date by the indexer.
struct SearchHandle {
    index: Arc<RwLock<SearchIndex>>,
    embedder: Arc<RwLock<Option<Arc<dyn Embedder>>>>,
    residency: Arc<SearchResidency>,
    tx: Sender<Message>,
}

impl SearchHandle {
    /// It's being used; not in memory: read in the background (never waited for: the caller
    /// may be the UI thread, and asks again when [`ProjectIndexer::take_search_loaded`] says).
    fn asked(&self) {
        if let Ok(mut asked) = self.residency.asked.lock() {
            *asked = Some(Instant::now());
        }
        if self.residency.is_cold() && !self.residency.loading.swap(true, Ordering::AcqRel) {
            let _ = self.tx.send(Message::LoadSearch);
        }
    }

    fn embedder(&self) -> Option<Arc<dyn Embedder>> {
        self.embedder.read().ok().and_then(|e| e.clone())
    }
}

impl CodeSearch for SearchHandle {
    fn search(&self, query: &SearchQuery) -> Vec<Hit> {
        self.asked();
        let vector = match query.strategy {
            text_search::Strategy::Lexical => None,
            _ => self.embedder().map(|e| e.embed(&query.text)),
        };
        self.index.read().map(|index| index.query(query, vector.as_deref())).unwrap_or_default()
    }

    fn related(&self, path: &str, line: u32, limit: usize) -> Vec<Hit> {
        self.asked();
        self.index.read().map(|index| index.related(path, line, limit)).unwrap_or_default()
    }

    fn is_semantic(&self) -> bool {
        self.embedder().is_some()
    }
}

impl std::fmt::Debug for ProjectIndexer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectIndexer").field("busy", &self.is_busy()).finish_non_exhaustive()
    }
}

impl Drop for ProjectIndexer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.tx.send(Message::Stop);
    }
}

impl ProjectIndexer {
    /// Starts indexing the project in `root` (from its cache: only what changed is parsed
    /// again). `waker` is called when the index has changed (the UI can redraw).
    pub fn start(
        root: PathBuf,
        intelligence: Intelligences,
        waker: Waker,
    ) -> std::io::Result<Self> {
        Self::start_marked(root, ProjectMarks::default(), intelligence, waker)
    }

    /// [`Self::start`], what it indexes decided by the user's `marks` too.
    pub fn start_marked(
        root: PathBuf,
        marks: ProjectMarks,
        intelligence: Intelligences,
        waker: Waker,
    ) -> std::io::Result<Self> {
        let cache = root.join(kernel::brand::PROJECT_DIR).join("index");
        Self::start_full(root, cache, marks, intelligence, waker)
    }

    /// [`Self::start`], its cache kept in folder `cache` (not the project's own data folder:
    /// a program that mustn't leave anything in the project).
    pub fn start_caching_in(
        root: PathBuf,
        cache: PathBuf,
        intelligence: Intelligences,
        waker: Waker,
    ) -> std::io::Result<Self> {
        Self::start_full(root, cache, ProjectMarks::default(), intelligence, waker)
    }

    fn start_full(
        root: PathBuf,
        cache: PathBuf,
        marks: ProjectMarks,
        intelligence: Intelligences,
        waker: Waker,
    ) -> std::io::Result<Self> {
        let index = Arc::new(RwLock::new(SymbolIndex::default()));
        let busy = Arc::new(AtomicBool::new(true));
        let updated = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = channel();
        let usage = Arc::new(Usage::default());
        let search = search::SearchState::new(Arc::clone(&usage), cache.clone(), marks.clone());
        let residency = Arc::<SearchResidency>::default();
        let search_index = Arc::clone(&search.index);
        let worker = Worker {
            usage: Arc::clone(&usage),
            pool: std::cell::RefCell::default(),
            search,
            root,
            cache,
            intelligence,
            index: Arc::clone(&index),
            busy: Arc::clone(&busy),
            updated: Arc::clone(&updated),
            stop: Arc::clone(&stop),
            interrupted: false,
            resting: false,
            marks: marks.clone(),
            found: Vec::new(),
            residency: Arc::clone(&residency),
            waker,
            files: HashMap::new(),
            dirty: false,
            last_save: Instant::now(),
            last_change: Instant::now(),
        };
        let (own_root, own_extensions) = (worker.root.clone(), extensions_of(&worker.intelligence));
        std::thread::Builder::new().name("indexer".into()).spawn(move || {
            // What a write killed half way left (a crash, a forced quit): never read, removed.
            let left = ::files::remove_left_temporaries(&worker.cache, LEFT_TEMPORARIES);
            if left > 0 {
                tracing::info!(left, "left temporary index files removed");
            }
            worker.run(&rx);
        })?;
        Ok(Self {
            marks,
            residency,
            usage,
            index,
            search: search_index,
            embedder: Arc::default(),
            tx,
            busy,
            updated,
            stop,
            root: own_root,
            extensions: own_extensions,
        })
    }

    /// `path` is one it indexes (a known extension or searchable text, outside the folders
    /// skipped): changes elsewhere needn't be told.
    pub fn wants(&self, path: &Path) -> bool {
        relevant(&self.root, path, &self.extensions, &self.marks.get())
    }

    /// What it did (for the Task Manager).
    pub fn usage(&self) -> Arc<Usage> {
        Arc::clone(&self.usage)
    }

    /// Shows as busy at once (a rebuild asked for, about to start), until the next build ends.
    pub fn mark_busy(&self) {
        self.busy.store(true, Ordering::Relaxed);
    }

    /// Looks at the whole project again, parsing only what changed (fingerprints): after
    /// changes that may have been missed (the project's watcher just started) or that can't
    /// be told file by file (a folder gone).
    pub fn refresh(&self) {
        let _ = self.tx.send(Message::Refresh);
    }

    /// The shared index (read by completion, updated by the indexer).
    pub fn index(&self) -> Arc<RwLock<SymbolIndex>> {
        Arc::clone(&self.index)
    }

    /// The files whose declarations changed in an incremental reindex (a save, a change on
    /// disk, a rename) since the last call — a build's aren't listed (`builds` counts it).
    pub fn take_updated(&self) -> Vec<PathBuf> {
        self.updated.lock().map(|mut u| std::mem::take(&mut *u)).unwrap_or_default()
    }

    /// Indexing (initial or full) is in progress.
    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Relaxed)
    }

    /// Rebuilds the whole index, ignoring the cache (the "Reindex Project" command).
    pub fn rebuild(&self) {
        self.busy.store(true, Ordering::Relaxed);
        let _ = self.tx.send(Message::Rebuild);
    }

    /// Project search: BM25 on terms plus, if the model is available, semantic
    /// similarity (combined via Reciprocal Rank Fusion).
    pub fn search(&self, query: &str, limit: usize) -> Vec<Hit> {
        self.search_api().search(&SearchQuery::new(query, limit))
    }

    /// Search also uses the semantic model.
    pub fn is_semantic(&self) -> bool {
        self.search_api().is_semantic()
    }

    /// The search API, to hand out (UI, commands, an AI agent): a cheap handle,
    /// valid while the indexer lives (afterwards, it returns the remaining index).
    pub fn search_api(&self) -> Arc<dyn CodeSearch> {
        Arc::new(SearchHandle {
            index: Arc::clone(&self.search),
            embedder: Arc::clone(&self.embedder),
            residency: Arc::clone(&self.residency),
            tx: self.tx.clone(),
        })
    }

    /// The text index was read since the last look: whoever searched, searches again.
    pub fn take_search_loaded(&self) -> bool {
        self.residency.loaded.swap(false, Ordering::AcqRel)
    }

    /// The embeddings model (loaded by the application): vectors are computed in the background.
    pub fn set_embedder(&self, embedder: Arc<dyn Embedder>) {
        if let Ok(mut current) = self.embedder.write() {
            *current = Some(Arc::clone(&embedder));
        }
        let _ = self.tx.send(Message::Embedder(Some(embedder)));
    }

    /// Drops the model (semantic search off): its memory is freed; vectors
    /// already computed remain, but search becomes lexical again.
    pub fn release_embedder(&self) {
        if let Ok(mut current) = self.embedder.write() {
            *current = None;
        }
        let _ = self.tx.send(Message::Embedder(None));
    }

    /// Reindexes the given files (a batch: gathered already, done at once).
    /// What it holds lets go of the memory, until [`Self::wake`] (the project isn't in use).
    pub fn rest(&self) {
        let _ = self.tx.send(Message::Rest);
    }

    /// In use again: read back from its cache, what changed meanwhile parsed.
    pub fn wake(&self) {
        let _ = self.tx.send(Message::Wake);
    }

    pub fn changed(&self, paths: Vec<PathBuf>) {
        let _ = self.tx.send(Message::Changed(paths));
    }
}

/// The extensions its languages declare things in.
fn extensions_of(intelligence: &Intelligences) -> Vec<&'static str> {
    intelligence.iter().flat_map(|i| i.extensions().iter().copied()).collect()
}

fn relevant(root: &Path, path: &Path, extensions: &[&str], marks: &PathMarks) -> bool {
    let Ok(relative) = path.strip_prefix(root) else { return false };
    let shown = relative.to_string_lossy().replace('\\', "/");
    if marks.is_excluded(&shown) {
        return false;
    }
    let skipped = !marks.is_normal(&shown)
        && relative.components().any(|c| skipped_dir(c.as_os_str().to_str().unwrap_or("")));
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
    let indexed = ext.as_deref().is_some_and(|e| extensions.contains(&e));
    !skipped && (indexed || search::searchable(&shown, marks))
}

/// Files read for a build at once, on that many threads at most (half the cores, at most
/// four: the rest of the app keeps its own).
const READ_BATCH: usize = 256;
const MAX_READERS: usize = 4;

/// What reading a file for the index needs: shared by the threads a build reads on.
struct Reader<'a> {
    intelligence: &'a Intelligences,
    marks: Arc<PathMarks>,
    found: &'a [String],
    usage: &'a Usage,
}

impl Reader<'_> {
    fn intelligence_for(&self, path: &Path) -> Option<&Arc<dyn LanguageIntelligence>> {
        intelligence_for(self.intelligence, path)
    }

    fn is_library(&self, relative: &str) -> bool {
        is_library(&self.marks, self.found, relative)
    }

    /// The file's entry: from `cached` when its size and date (or its content) are the
    /// same, else parsed (`true`); `None`: not indexed (gone, too large). Its texts aren't
    /// shared with the index's yet (the caller does it: the pool is one thread's).
    fn read(
        &self,
        path: &Path,
        relative: &str,
        cached: Option<&CachedFile>,
    ) -> Option<(CachedFile, bool)> {
        let meta = std::fs::metadata(path).ok()?;
        if !meta.is_file() || meta.len() > workspace::MAX_SOURCE_FILE {
            return None;
        }
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos() as u64);
        if let Some(c) = cached.filter(|c| c.len == meta.len() && c.modified == modified) {
            return Some((c.clone(), false));
        }
        let bytes = std::fs::read(path).ok()?;
        self.usage.read_bytes(bytes.len());
        let hash = codec::hash(&bytes);
        if let Some(c) = cached.filter(|c| c.hash == hash) {
            return Some((CachedFile { modified, len: meta.len(), ..c.clone() }, false));
        }
        let text = String::from_utf8_lossy(&bytes);
        let intelligence = self.intelligence_for(path)?;
        let mut symbols = intelligence.extract(&text);
        // A library's file: as the project sees it.
        if self.is_library(relative) {
            symbols.keep_public();
        }
        // Kept for the whole session: no spare capacity.
        symbols.symbols.shrink_to_fit();
        symbols.uses.shrink_to_fit();
        symbols.language = intelligence.language().to_string().into();
        Some((
            CachedFile {
                path: relative.into(),
                len: meta.len(),
                modified,
                hash,
                symbols: Arc::new(symbols),
                packed: None,
                block: None,
            },
            true,
        ))
    }
}

fn intelligence_for<'a>(
    intelligence: &'a Intelligences,
    path: &Path,
) -> Option<&'a Arc<dyn LanguageIntelligence>> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    intelligence.iter().find(|i| i.extensions().contains(&ext.as_str()))
}

/// Whether `relative` is a library's (marked so, or in a dependency folder).
fn is_library(marks: &PathMarks, found: &[String], relative: &str) -> bool {
    use workspace::marks::PathMark;
    match marks.of(relative) {
        Some(PathMark::Library) => true,
        Some(PathMark::Normal) => false,
        _ => {
            let path = Path::new(relative);
            kernel::in_dependencies_or_git(path)
                || found.iter().any(|folder| path.starts_with(folder))
        }
    }
}

struct Worker {
    /// Where its cache is kept.
    cache: PathBuf,
    usage: Arc<Usage>,
    /// The index's shared texts (see [`codec::Pool`]).
    pool: std::cell::RefCell<codec::Pool>,
    root: PathBuf,
    intelligence: Intelligences,
    index: Arc<RwLock<SymbolIndex>>,
    busy: Arc<AtomicBool>,
    /// See [`ProjectIndexer::take_updated`].
    updated: Arc<std::sync::Mutex<Vec<PathBuf>>>,
    waker: Waker,
    /// Indexed files (relative path → fingerprint and symbols), for the cache.
    files: HashMap<Arc<str>, CachedFile>,
    /// The search index (code chunks, terms, vectors).
    search: search::SearchState,
    /// The on-disk cache is behind.
    dirty: bool,
    last_save: Instant,
    /// When the index last changed (saved once it's quiet).
    last_change: Instant,
    /// See [`ProjectIndexer::stop`].
    stop: Arc<AtomicBool>,
    /// A build was cut short: the in-memory state is partial and must not replace the
    /// cache on disk.
    interrupted: bool,
    /// Let go of what it holds (see [`Worker::rest`]).
    resting: bool,
    residency: Arc<SearchResidency>,
    marks: ProjectMarks,
    /// The dependency folders the project's managers' files name (relative, with `/`: plan
    /// 74g), read at each build and when one of those files changes.
    found: Vec<String>,
}

impl Worker {
    fn run(mut self, rx: &Receiver<Message>) {
        self.build(true);
        loop {
            let timeout = if self.dirty {
                SAVE_WHEN_QUIET
                    .saturating_sub(self.last_change.elapsed())
                    .max(Duration::from_secs(1))
            } else {
                Duration::from_secs(3600)
            };
            match rx.recv_timeout(timeout.min(self.until_search_unused())) {
                Ok(Message::Rebuild) => {
                    self.resting = false;
                    self.build(false);
                }
                // Waking looks at everything anyway.
                Ok(Message::Refresh) if self.resting => {}
                Ok(Message::Refresh) => self.build(true),
                Ok(Message::Rest) => self.rest(),
                Ok(Message::LoadSearch) => self.load_search(),
                Ok(Message::Wake) => {
                    if std::mem::take(&mut self.resting) {
                        self.build(true);
                    }
                }
                Ok(Message::Stop) => {
                    self.save();
                    return;
                }
                Ok(Message::Embedder(None)) => self.search.release_embedder(),
                // Used once it wakes (nothing here to compute vectors for).
                Ok(Message::Embedder(Some(embedder))) if self.resting => {
                    self.search.keep_embedder(embedder);
                }
                Ok(Message::Embedder(Some(embedder))) => {
                    self.load_search();
                    self.busy.store(true, Ordering::Relaxed);
                    let started = Instant::now();
                    if self.search.set_embedder(&self.root, embedder, &self.stop) {
                        tracing::info!(elapsed = ?started.elapsed(), "semantic vectors computed");
                        self.dirty = true;
                        self.save();
                    }
                    self.busy.store(false, Ordering::Relaxed);
                    (self.waker)();
                }
                // Looked at again, all of it, when it wakes.
                Ok(Message::Changed(_)) if self.resting => {}
                Ok(Message::Changed(mut paths)) => {
                    paths.sort();
                    paths.dedup();
                    self.update(&paths);
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    self.save();
                    return;
                }
            }
            self.usage.note_cpu();
            let quiet = self.last_change.elapsed() >= SAVE_WHEN_QUIET;
            if self.dirty && (quiet || self.last_save.elapsed() >= SAVE_AT_MOST_EVERY) {
                self.save();
            }
            self.evict_search_if_unused();
        }
    }

    fn cache_file(&self) -> PathBuf {
        self.cache.join("symbols.bin")
    }

    /// How long the wait for messages may last, for the text index to leave on time.
    fn until_search_unused(&self) -> Duration {
        if self.residency.is_cold() {
            return Duration::from_secs(3600);
        }
        SEARCH_UNUSED.saturating_sub(self.residency.unused_for()).max(Duration::from_secs(1))
    }

    /// The text index leaves the memory when nobody has asked it for a while (saved first).
    fn evict_search_if_unused(&mut self) {
        let waits = self.residency.is_cold() || self.busy.load(Ordering::Relaxed);
        if waits || self.residency.unused_for() < SEARCH_UNUSED {
            return;
        }
        if self.dirty {
            self.save();
        }
        self.search.clear();
        self.residency.set_cold();
        kernel::memory::release_free();
    }

    /// The text index read back, what changed since parsed; whoever asked is told.
    fn load_search(&mut self) {
        if !self.residency.is_cold() {
            return;
        }
        let all = walk(&self.root, &self.marks.get());
        self.dirty |= self.search.build(&self.root, &all, true, &self.stop);
        self.residency.set_warm();
        self.residency.loaded.store(true, Ordering::Release);
        (self.waker)();
    }

    /// What it holds goes from memory, kept in its cache: read back when it [wakes].
    ///
    /// [wakes]: ProjectIndexer::wake
    fn rest(&mut self) {
        if self.resting {
            return;
        }
        self.save();
        // Never written again from what's empty now.
        self.dirty = false;
        self.resting = true;
        self.files = HashMap::new();
        self.search.clear();
        self.residency.set_cold();
        self.pool = std::cell::RefCell::default();
        if let Ok(mut shared) = self.index.write() {
            shared.replace_symbols(SymbolIndex::default());
        }
        kernel::memory::release_free();
    }

    /// The combined version of the extractors (changing any of them invalidates the cache).
    fn extractors(&self) -> u32 {
        self.intelligence.iter().fold(17u32, |h, i| h.wrapping_mul(31).wrapping_add(i.version()))
    }

    /// The file at `relative` is a library's: marked so (marked "normal": never), in a
    /// folder a module names (`vendor`, `node_modules`) or one the project's managers' files
    /// name (Composer's `vendor-dir`). Seen as the project sees it (its private members left
    /// out), kept packed (plan 74).
    fn is_library(&self, relative: &str) -> bool {
        is_library(&self.marks.get(), &self.found, relative)
    }

    fn reader(&self) -> Reader<'_> {
        Reader {
            intelligence: &self.intelligence,
            marks: self.marks.get(),
            found: &self.found,
            usage: &self.usage,
        }
    }

    /// The dependency folders the project's managers' files name (each module's finders:
    /// `composer.json`'s `vendor-dir`…), read now and told to everyone (`kernel::paths`).
    /// `true` if they changed.
    fn find_dependencies(&mut self) -> bool {
        let mut found: Vec<String> = kernel::paths::finders()
            .into_iter()
            .filter_map(|finder| {
                let text = std::fs::read_to_string(self.root.join(finder.manifest)).ok()?;
                Some((finder.folders)(&text))
            })
            .flatten()
            .collect();
        found.sort();
        found.dedup();
        if found == self.found {
            return false;
        }
        kernel::paths::set_found(&self.root, found.clone());
        self.found = found;
        true
    }

    fn intelligence_for(&self, path: &Path) -> Option<&Arc<dyn LanguageIntelligence>> {
        intelligence_for(&self.intelligence, path)
    }

    /// Full build: from the cache (if `use_cache`) plus the changed files.
    fn build(&mut self, use_cache: bool) {
        self.busy.store(true, Ordering::Relaxed);
        let started = Instant::now();
        self.find_dependencies();
        // Already built (a refresh): what's in memory is the newest, taken (not copied) and
        // given back if the build is interrupted.
        let in_memory = use_cache && !self.files.is_empty();
        let cached: HashMap<Arc<str>, CachedFile> = if in_memory {
            std::mem::take(&mut self.files)
        } else if use_cache {
            map_file(&self.cache_file())
                .and_then(|bytes| {
                    codec::decode(self.extractors(), &bytes, &mut self.pool.borrow_mut())
                })
                .map(|files| files.into_iter().map(|f| (Arc::clone(&f.path), f)).collect())
                .unwrap_or_default()
        } else {
            HashMap::new()
        };
        let mut files = HashMap::new();
        let (mut reused, mut parsed) = (0usize, 0usize);
        self.usage.builds.fetch_add(1, Ordering::Relaxed);
        self.usage.files_done.store(0, Ordering::Relaxed);
        let all = walk(&self.root, &self.marks.get());
        self.usage.files.store(all.len() as u64, Ordering::Relaxed);
        let readers =
            std::thread::available_parallelism().map_or(1, |n| n.get() / 2).clamp(1, MAX_READERS);
        let mut done = 0usize;
        for batch in all.chunks(READ_BATCH) {
            if self.stop.load(Ordering::Relaxed) {
                self.interrupted = true;
                if in_memory {
                    self.files = cached;
                }
                self.busy.store(false, Ordering::Relaxed);
                self.usage.built.fetch_add(1, Ordering::Relaxed);
                return;
            }
            let wanted: Vec<(&PathBuf, String)> = batch
                .iter()
                .filter(|p| self.intelligence_for(p).is_some())
                .map(|p| (p, relative(&self.root, p)))
                .collect();
            // Read and parsed on a few threads (the parser is each thread's), in order.
            let reader = self.reader();
            let read = |part: &[(&PathBuf, String)]| -> Vec<Option<(CachedFile, bool)>> {
                part.iter()
                    .map(|(path, relative)| {
                        reader.read(path, relative, cached.get(relative.as_str()))
                    })
                    .collect()
            };
            let entries: Vec<Option<(CachedFile, bool)>> = if readers > 1 && wanted.len() > 1 {
                let size = wanted.len().div_ceil(readers);
                std::thread::scope(|scope| {
                    let parts: Vec<_> = wanted
                        .chunks(size)
                        .skip(1)
                        .map(|part| {
                            let read = &read;
                            let usage = &*self.usage;
                            std::thread::Builder::new().name("index-read".into()).spawn_scoped(
                                scope,
                                move || {
                                    let out = read(part);
                                    usage.note_helper_cpu();
                                    out
                                },
                            )
                        })
                        .collect();
                    let mut all = read(&wanted[..size.min(wanted.len())]);
                    for part in parts {
                        // A thread that couldn't start or ended badly: its part is missing
                        // and the batch is read here (below).
                        if let Ok(Ok(out)) = part.map(|h| h.join()) {
                            all.extend(out);
                        }
                    }
                    all
                })
            } else {
                read(&wanted)
            };
            // A part lost (a thread that failed): its files read here, in order.
            let entries = if entries.len() == wanted.len() { entries } else { read(&wanted) };
            for mut entry in entries.into_iter().flatten() {
                if entry.1 {
                    parsed += 1;
                    if let Some(symbols) = Arc::get_mut(&mut entry.0.symbols) {
                        self.pool.borrow_mut().share(symbols);
                    }
                } else {
                    reused += 1;
                }
                files.insert(Arc::clone(&entry.0.path), entry.0);
            }
            done += batch.len();
            self.usage.files_done.store(done as u64, Ordering::Relaxed);
            self.usage.note_cpu();
        }
        let mut index = SymbolIndex::default();
        for (relative, file) in &mut files {
            let library = self.is_library(relative);
            give(&mut index, &self.root.join(&**relative), file, library);
        }
        index.shrink_to_fit();
        if let Ok(mut shared) = self.index.write() {
            shared.replace_symbols(index);
        }
        tracing::info!(reused, parsed, elapsed = ?started.elapsed(), "symbol index");
        // The symbols first (navigation, completion), then the search index (slower: the
        // vectors), published to the app as soon as they're ready.
        (self.waker)();
        // Not in memory: left there (read, and brought up to date, when it's asked).
        let search_changed = !(use_cache && self.residency.is_cold())
            && self.search.build(&self.root, &all, use_cache, &self.stop);
        if !use_cache {
            self.residency.set_warm();
        }
        // Changes not saved yet (updates since the last save) stay to be saved.
        self.dirty |= parsed > 0 || files.len() != cached.len() || search_changed;
        self.files = files;
        if self.dirty {
            self.save();
        }
        self.busy.store(false, Ordering::Relaxed);
        self.usage.note_cpu();
        self.usage.built.fetch_add(1, Ordering::Relaxed);
        // The build's temporary buffers (the files read, the old cache) are freed: their
        // pages go back to the system instead of staying in the process's footprint.
        kernel::memory::release_free();
        (self.waker)();
    }

    /// A file's symbols: from the cache if the fingerprint is the same, otherwise extracted.
    /// `(entry, extracted now)`; `None` if the file can't be read or is too large.
    fn index_file(
        &self,
        path: &Path,
        relative: &str,
        cached: Option<&CachedFile>,
    ) -> Option<(CachedFile, bool)> {
        let (mut entry, fresh) = self.reader().read(path, relative, cached)?;
        if fresh && let Some(symbols) = Arc::get_mut(&mut entry.symbols) {
            self.pool.borrow_mut().share(symbols);
        }
        Some((entry, fresh))
    }

    /// Reindexes only the given files (created, modified or deleted).
    fn update(&mut self, paths: &[PathBuf]) {
        // A manager's file changed (`composer.json`): its dependency folders read again; if
        // they moved, every file is told apart again (a build).
        let manifests = kernel::paths::finders();
        let manifest_changed =
            paths.iter().any(|p| manifests.iter().any(|f| relative(&self.root, p) == f.manifest));
        if manifest_changed && self.find_dependencies() {
            self.build(true);
            return;
        }
        // Not in memory: it looks at every file when it's read.
        let mut changed =
            usize::from(!self.residency.is_cold() && self.search.update(&self.root, paths));
        let mut declarations = Vec::new();
        for path in paths {
            let relative = relative(&self.root, path);
            let entry = self.index_file(path, &relative, self.files.get(relative.as_str()));
            let Ok(mut index) = self.index.write() else { return };
            match entry {
                Some((mut entry, fresh)) => {
                    if fresh {
                        give(&mut index, path, &mut entry, self.is_library(&relative));
                        changed += 1;
                        declarations.push(path.clone());
                    }
                    self.files.insert(Arc::clone(&entry.path), entry);
                }
                None => {
                    if self.files.remove(relative.as_str()).is_some() {
                        index.remove_file(path);
                        changed += 1;
                        declarations.push(path.clone());
                    }
                }
            }
        }
        if !declarations.is_empty()
            && let Ok(mut updated) = self.updated.lock()
        {
            // Each told once (a checkout's thousands: not a search of the list each).
            let mut told: std::collections::HashSet<PathBuf> = updated.iter().cloned().collect();
            for path in declarations {
                if told.insert(path.clone()) {
                    updated.push(path);
                }
            }
        }
        if changed > 0 {
            self.dirty = true;
            self.last_change = Instant::now();
            tracing::debug!(changed, "incremental reindex");
            (self.waker)();
        }
    }

    fn save(&mut self) {
        // Resting holds nothing: what's on disk is what it reads back.
        if self.interrupted || self.resting {
            return;
        }
        let path = self.cache_file();
        let Some(dir) = path.parent() else { return };
        let mut files: Vec<CachedFile> = self.files.values().cloned().collect();
        files.sort_by(|a, b| a.path.cmp(&b.path));
        let mut blocks = Vec::new();
        let result = std::fs::create_dir_all(dir)
            .and_then(|()| {
                // The `<project dir>` directory doesn't end up in git.
                let ignore = self.root.join(kernel::brand::PROJECT_DIR).join(".gitignore");
                if !self.cache.starts_with(&self.root) || ignore.exists() {
                    Ok(())
                } else {
                    std::fs::write(ignore, "*\n")
                }
            })
            .and_then(|()| {
                let (encoded, written) = codec::encode_with_blocks(self.extractors(), &files);
                blocks = written;
                self.usage.written_bytes(encoded.len());
                ::files::write_atomic(&path, &encoded)
            });
        match result {
            Ok(()) => {
                // Each file's block where the file just written has it (read alone later).
                for (file, block) in files.iter().zip(blocks) {
                    if let Some(kept) = self.files.get_mut(&file.path) {
                        kept.block = Some(block);
                    }
                }
                if self.residency.is_cold() {
                    // On disk already; nothing in memory to write.
                } else if let Err(e) = self.search.save() {
                    tracing::warn!("can't save the search index: {e}");
                }
                self.dirty = false;
                self.last_save = Instant::now();
            }
            Err(e) => tracing::warn!("can't save the index in {}: {e}", path.display()),
        }
    }
}

/// How old a temporary file in the index's folder is before it's taken as left by a write
/// that never finished (a younger one may be another instance's, writing now).
const LEFT_TEMPORARIES: Duration = Duration::from_secs(3600);

pub(crate) fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

/// All files under `root`, excluding ignored directories.
fn walk(root: &Path, marks: &PathMarks) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Ok(kind) = entry.file_type() else { continue };
            let shown = relative(root, &entry.path());
            if marks.is_excluded(&shown) {
                continue;
            }
            if kind.is_dir() {
                if !skipped_dir(name.to_str().unwrap_or("")) || marks.is_normal(&shown) {
                    stack.push(entry.path());
                }
            } else if kind.is_file() {
                out.push(entry.path());
            }
        }
    }
    out
}

/// A cache file mapped instead of read: its pages are the file's (clean, dropped by the
/// system when it needs them), not copies on the heap that would stay in the footprint
/// after decoding.
pub(crate) fn map_file(path: &Path) -> Option<memmap2::Mmap> {
    let file = std::fs::File::open(path).ok()?;
    // SAFETY: the cache files are this indexer's own, written only by its worker thread,
    // and every mapping is dropped before that thread writes them again; another process
    // changing one meanwhile would at worst make decoding fail (it validates everything).
    #[allow(unsafe_code)]
    let map = unsafe { memmap2::Mmap::map(&file) };
    map.ok()
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
