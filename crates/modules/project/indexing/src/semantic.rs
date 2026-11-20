//! Semantic search's model: one per process (shared by every window and project), loaded in
//! the background if installed or downloaded on demand, with progress, and let go when no
//! search used it for a while. Semantic search can be turned off in the settings
//! ([`crate::settings::SEMANTIC_SEARCH`]); when off, search stays lexical only and the model
//! is not loaded. The open indexes take it (or give it back) when they're polled
//! ([`workbench_model::ProjectIndex::sync_model`]); the settings' search gets it as an
//! [`EmbeddingSource`].

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

use text_search::Embedder;
use workbench_model::EmbeddingSource;

type Wake = Arc<dyn Fn() + Send + Sync>;

/// The search mode's state (shown in the project search's footer and in the settings).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SearchMode {
    #[default]
    Lexical,
    Semantic,
    /// The model is downloading: (received, total) in bytes.
    Downloading(u64, u64),
    /// The model is loading (from disk).
    Loading,
    /// On, the model not loaded: it loads when a search needs it (and is let go when
    /// searches stop for a while).
    Standby,
    /// The download / load failed (the reason).
    Failed(String),
}

/// The semantic model, as the project search sees it (a fake in its tests).
pub trait SemanticSearch: std::fmt::Debug + Send + Sync {
    fn mode(&self) -> SearchMode;
    /// Turned on (searches may use the model).
    fn enabled(&self) -> bool;
    /// Turns it on: downloads / loads the model.
    fn enable(&self);
    /// A search needs it: loaded if it isn't, kept while searches go on.
    fn touch(&self);
}

/// How long the model stays loaded with no search using it.
const IDLE: Duration = Duration::from_secs(10 * 60);

#[derive(Default)]
struct Shared {
    /// The model (one per process); `None` until loaded or after it was turned off.
    model: Mutex<Option<Arc<dyn Embedder>>>,
    /// Semantic search is on (the setting, handed over by the settings' block).
    enabled: AtomicBool,
    busy: AtomicBool,
    downloading: AtomicBool,
    received: AtomicU64,
    total: AtomicU64,
    error: Mutex<Option<String>>,
    /// When semantic search was last used (a search opened or typed into).
    last_use: Mutex<Option<Instant>>,
    /// The open indexes' windows: woken when the model's state changes (it's every
    /// window's), so each one's index takes it or gives it back.
    wakes: Mutex<Vec<Weak<dyn Fn() + Send + Sync>>>,
}

fn shared() -> &'static Shared {
    static SHARED: OnceLock<Shared> = OnceLock::new();
    SHARED.get_or_init(Shared::default)
}

/// Semantic search is on (from the settings; with no choice, when the model is installed).
pub fn enabled() -> bool {
    shared().enabled.load(Ordering::Acquire)
}

/// The setting's value, as saved (at startup and after each change).
pub(crate) fn configure(on: bool) {
    shared().enabled.store(on, Ordering::Release);
}

/// Turns semantic search on / off (the setting already saved); when on, the model is loaded
/// (or downloaded, if missing). `wake`: the window that asked (besides the open indexes').
pub(crate) fn set_enabled(on: bool, wake: Option<Wake>) {
    configure(on);
    if on {
        download(wake);
    } else {
        release();
        wake_all(wake.as_ref());
    }
}

/// An index opened: its window is woken when the model's state changes.
pub(crate) fn follow(wake: &Wake) {
    if let Ok(mut wakes) = shared().wakes.lock() {
        wakes.retain(|w| w.strong_count() > 0);
        wakes.push(Arc::downgrade(wake));
    }
}

/// Wakes every open index's window (and `extra`, the one that asked).
fn wake_all(extra: Option<&Wake>) {
    let wakes: Vec<Wake> = shared()
        .wakes
        .lock()
        .map(|w| w.iter().filter_map(Weak::upgrade).collect())
        .unwrap_or_default();
    for wake in wakes.iter().chain(extra) {
        wake();
    }
}

fn model_dir() -> Option<std::path::PathBuf> {
    storage::config_dir().map(|d| embed::model_dir(&d))
}

/// The model, if loaded and on.
pub fn model() -> Option<Arc<dyn Embedder>> {
    shared().model.lock().ok().and_then(|m| m.clone()).filter(|_| enabled())
}

/// Semantic search was turned off, or unused for a while: the model is released (its memory
/// returns to the system once the indexes no longer hold it either, see
/// [`crate::Index::sync_model`]).
fn release() {
    if let Ok(mut model) = shared().model.lock()
        && model.take().is_some()
    {
        tracing::info!("the semantic model released");
        kernel::memory::release_free();
    }
}

/// The model is on disk (can be loaded without downloading).
pub fn is_installed() -> bool {
    model_dir().is_some_and(|d| embed::is_installed(&d))
}

/// Loads the model in the background, if on; `wake` is called (with the open indexes'
/// windows) when it is ready.
pub fn load(wake: Option<Wake>) {
    if !enabled() || model().is_some() {
        return;
    }
    // On in settings, but the model is missing: download it (the user chose it). Whether
    // it's on disk is looked at on the worker (not at each key of a search).
    spawn(true, wake);
}

/// The model's download (the Task Manager's network column, on the module's row).
pub(crate) static TRAFFIC: workbench_model::resources::Traffic =
    workbench_model::resources::Traffic::new();

/// The thread the model is downloaded and loaded on (its CPU on the module's row).
pub(crate) const THREAD: &str = "embed";

/// Downloads the model (if missing) and loads it, in the background.
fn download(wake: Option<Wake>) {
    if model().is_some() {
        return;
    }
    spawn(true, wake);
}

fn spawn(download: bool, wake: Option<Wake>) {
    let s = shared();
    if s.busy.swap(true, Ordering::AcqRel) {
        return;
    }
    *s.error.lock().unwrap_or_else(|e| e.into_inner()) = None;
    let Some(dir) = model_dir() else {
        s.busy.store(false, Ordering::Release);
        return;
    };
    kernel::work::run(THREAD, move || {
        let s = shared();
        let mut outcome = Ok(());
        if download && !embed::is_installed(&dir) {
            s.downloading.store(true, Ordering::Release);
            let last = AtomicU64::new(0);
            // What came since the last call (counted per file: `got` starts again at 0).
            let counted = AtomicU64::new(0);
            outcome = embed::download(&dir, &|got, total| {
                let before = counted.swap(got, Ordering::Relaxed);
                TRAFFIC.received(if got >= before { got - before } else { got });
                s.received.store(got, Ordering::Relaxed);
                s.total.store(total, Ordering::Relaxed);
                // Redraw every ~1 MB, not on every chunk read.
                if got >> 20 != last.swap(got >> 20, Ordering::Relaxed) {
                    wake_all(wake.as_ref());
                }
            });
            s.downloading.store(false, Ordering::Release);
        }
        let outcome = outcome.and_then(|()| embed::Model2Vec::load(&dir));
        match outcome {
            Ok(model) => {
                // Meanwhile, semantic search may have been turned off: the model is not kept.
                if enabled()
                    && let Ok(mut slot) = s.model.lock()
                {
                    *slot = Some(Arc::new(model));
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "the semantic model isn't available");
                *s.error.lock().unwrap_or_else(|e| e.into_inner()) = Some(e.to_string());
            }
        }
        s.busy.store(false, Ordering::Release);
        wake_all(wake.as_ref());
    });
}

/// A search needs the model (it was opened, or typed into): loaded if it isn't, and kept
/// while searches go on.
pub fn touch() {
    let s = shared();
    if let Ok(mut last) = s.last_use.lock() {
        *last = Some(Instant::now());
    }
    // A load that failed (no network for the download) isn't tried again at each key: the
    // user asks for it again (turning it on: `enable`).
    let failed = s.error.lock().is_ok_and(|e| e.is_some());
    if !failed && !s.busy.load(Ordering::Acquire) {
        load(None);
    }
}

/// Lets the model go when no search used it for a while (its memory returns; it loads
/// again on the next search). `true` if it was let go.
pub(crate) fn release_if_idle() -> bool {
    let s = shared();
    let unused = s.last_use.lock().ok().and_then(|l| *l).is_none_or(|t| t.elapsed() > IDLE);
    let loaded = s.model.lock().is_ok_and(|m| m.is_some());
    if unused && loaded && !s.busy.load(Ordering::Acquire) {
        release();
        return true;
    }
    false
}

/// The state, for the UI.
pub fn mode() -> SearchMode {
    let s = shared();
    if !enabled() {
        SearchMode::Lexical
    } else if model().is_some() {
        SearchMode::Semantic
    } else if s.downloading.load(Ordering::Acquire) {
        SearchMode::Downloading(s.received.load(Ordering::Relaxed), s.total.load(Ordering::Relaxed))
    } else if let Some(error) = s.error.lock().ok().and_then(|e| e.clone()) {
        SearchMode::Failed(error)
    } else if s.busy.load(Ordering::Acquire) {
        SearchMode::Loading
    } else {
        SearchMode::Standby
    }
}

/// The model, as the project search sees it: turned on there, the choice is saved in the
/// user's settings.
#[derive(Debug)]
pub(crate) struct Port(pub Option<&'static settings_model::SettingsService>);

impl SemanticSearch for Port {
    fn mode(&self) -> SearchMode {
        mode()
    }

    fn enabled(&self) -> bool {
        enabled()
    }

    fn enable(&self) {
        if let Some(settings) = self.0 {
            crate::settings::save_semantic(settings, true);
        }
        set_enabled(true, None);
    }

    fn touch(&self) {
        touch();
    }
}

/// The model, for searching by meaning elsewhere (the settings' search).
#[derive(Debug)]
pub(crate) struct Source;

impl EmbeddingSource for Source {
    fn embedder(&self) -> Option<Arc<dyn Embedder>> {
        model()
    }
}
