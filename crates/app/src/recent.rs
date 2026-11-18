//! Recently opened projects (see `workspace::recent`), kept in `recent.toml` in the
//! config directory. Folders that no longer exist are skipped in the menu — looked at in
//! the background (a network folder gone can take seconds to answer, on Windows above all):
//! the list shows what was last seen, and follows once the look is done.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use ui_workbench::{RecentList, RecentProject};
use workspace::RecentStore;
use workspace::recent::RecentProjects;

/// The store (one for the whole process; in tests, a temporary file per thread, never the
/// user's list).
fn store() -> &'static dyn RecentStore {
    if cfg!(test) {
        thread_local! {
            static TEST: &'static storage::TomlRecentStore = {
                let id = format!("{:?}", std::thread::current().id()).replace(|c: char| !c.is_ascii_alphanumeric(), "");
                let file = std::env::temp_dir()
                    .join(format!("recent-test-{}-{id}.toml", std::process::id()));
                let _ = std::fs::remove_file(&file);
                Box::leak(Box::new(storage::TomlRecentStore::new(Some(file))))
            };
        }
        return TEST.with(|s| *s);
    }
    static STORE: OnceLock<storage::TomlRecentStore> = OnceLock::new();
    STORE.get_or_init(|| {
        storage::TomlRecentStore::new(crate::config::config_dir().map(|d| d.join("recent.toml")))
    })
}

pub fn load() -> RecentProjects {
    store().load()
}

/// Bumped each time the list changes (the app icon's menu follows).
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Held while the list is written: one change at a time (each reads it again first), so a
/// change made meanwhile is never lost.
static WRITING: Mutex<()> = Mutex::new(());

/// Records the opening of a project, in the background (opening it never waits for the
/// disk); the list's readers follow once it's written (its generation, the windows woken).
pub fn add(path: &Path) {
    let path = path.to_path_buf();
    let record = move || {
        let _writing = WRITING.lock().unwrap_or_else(PoisonError::into_inner);
        if let Err(e) = workspace::record_recent(store(), &path) {
            tracing::warn!("cannot save the recent projects: {e}");
        }
        GENERATION.fetch_add(1, Ordering::Relaxed);
        if let Some(wake) = WAKE.lock().unwrap_or_else(PoisonError::into_inner).clone() {
            wake();
        }
    };
    // Tests (a store per thread) record it at once.
    if cfg!(test) {
        record();
    } else {
        kernel::work::run(RECORD_THREAD, record);
    }
}

/// The shared pool's name recording a project runs under.
pub(crate) const RECORD_THREAD: &str = "recent-projects";

/// Changes the list (its groups, where a project is, a project taken out) with `change`;
/// `true` if it changed.
pub fn update(change: impl FnOnce(&mut RecentProjects) -> bool) -> bool {
    let _writing = WRITING.lock().unwrap_or_else(PoisonError::into_inner);
    match workspace::recent::update(store(), change) {
        Ok(changed) => {
            if changed {
                GENERATION.fetch_add(1, Ordering::Relaxed);
            }
            changed
        }
        Err(e) => {
            tracing::warn!("cannot save the recent projects: {e}");
            false
        }
    }
}

/// The list's version (changes whenever a project is recorded).
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

/// Whether each project's folder exists, as last seen.
static PRESENT: Mutex<Option<HashMap<PathBuf, bool>>> = Mutex::new(None);
/// A look at the folders is going on (one at a time).
static LOOKING: AtomicBool = AtomicBool::new(false);
/// Wakes the windows when a look changed what's shown.
static WAKE: Mutex<Option<Arc<dyn Fn() + Send + Sync>>> = Mutex::new(None);

/// What wakes the windows when the folders' look changed the list.
pub fn set_waker(wake: Arc<dyn Fn() + Send + Sync>) {
    *WAKE.lock().unwrap_or_else(PoisonError::into_inner) = Some(wake);
}

/// What the modules said of each recent project (a remote one: its connection), and the
/// list's generation it was found for (`u64::MAX`: never): found in the background, each
/// project's files read there (a remote one's settings), never by the UI thread.
static KINDS: Mutex<Option<HashMap<PathBuf, Option<workbench_model::ProjectKind>>>> =
    Mutex::new(None);
static KINDS_FOR: AtomicU64 = AtomicU64::new(u64::MAX);
static DESCRIBING: AtomicBool = AtomicBool::new(false);

/// What `services` say of each of `paths` (a remote project), as last found; found again in
/// the background when the list changed, the windows woken once it's there.
pub fn kinds(
    paths: &[PathBuf],
    services: &[Arc<dyn workbench_model::ProjectService>],
) -> Vec<Option<workbench_model::ProjectKind>> {
    let describe = |path: &Path| services.iter().find_map(|s| s.describe(path));
    // Tests describe at once: what they check is the list, not the waiting.
    if cfg!(test) {
        return paths.iter().map(|p| describe(p)).collect();
    }
    let shown: Vec<Option<workbench_model::ProjectKind>> = {
        let known = KINDS.lock().unwrap_or_else(PoisonError::into_inner);
        paths.iter().map(|p| known.as_ref().and_then(|k| k.get(p)).cloned().flatten()).collect()
    };
    let generation = generation();
    if KINDS_FOR.load(Ordering::Acquire) != generation && !DESCRIBING.swap(true, Ordering::AcqRel) {
        let (paths, services) = (paths.to_vec(), services.to_vec());
        kernel::work::run("recent-check", move || {
            let found: HashMap<PathBuf, Option<workbench_model::ProjectKind>> = paths
                .into_iter()
                .map(|p| {
                    let kind = services.iter().find_map(|s| s.describe(&p));
                    (p, kind)
                })
                .collect();
            *KINDS.lock().unwrap_or_else(PoisonError::into_inner) = Some(found);
            // The list shown again with them: its generation moved, unless it moved meanwhile
            // (then it's found again for that one).
            if GENERATION
                .compare_exchange(generation, generation + 1, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                KINDS_FOR.store(generation + 1, Ordering::Release);
            }
            DESCRIBING.store(false, Ordering::Release);
            if let Some(wake) = WAKE.lock().unwrap_or_else(PoisonError::into_inner).clone() {
                wake();
            }
        });
    }
    shown
}

/// The projects among `paths` whose folders exist, as last seen (one not looked at yet is
/// shown); a look at all of them starts in the background (the list follows if it changes).
fn present(paths: &[PathBuf]) -> Vec<bool> {
    // Tests look at once: what they check is the list, not the waiting.
    if cfg!(test) {
        return paths.iter().map(|p| p.is_dir()).collect();
    }
    let shown: Vec<bool> = {
        let known = PRESENT.lock().unwrap_or_else(PoisonError::into_inner);
        paths
            .iter()
            .map(|p| known.as_ref().and_then(|k| k.get(p)).copied().unwrap_or(true))
            .collect()
    };
    if !LOOKING.swap(true, Ordering::AcqRel) {
        let paths = paths.to_vec();
        kernel::work::run("recent-check", move || {
            let seen: Vec<(PathBuf, bool)> = paths
                .into_iter()
                .map(|p| {
                    let there = p.is_dir();
                    (p, there)
                })
                .collect();
            let changed = {
                let mut known = PRESENT.lock().unwrap_or_else(PoisonError::into_inner);
                let known = known.get_or_insert_with(HashMap::new);
                let mut changed = false;
                for (path, there) in seen {
                    changed |= known.insert(path, there).unwrap_or(true) != there;
                }
                changed
            };
            LOOKING.store(false, Ordering::Release);
            if changed {
                GENERATION.fetch_add(1, Ordering::Relaxed);
                if let Some(wake) = WAKE.lock().unwrap_or_else(PoisonError::into_inner).clone() {
                    wake();
                }
            }
        });
    }
    shown
}

/// The list for the menu: only existing folders, with the name and path relative to home.
pub fn projects() -> Vec<RecentProject> {
    let model = load();
    let present = present(&model.projects);
    model
        .projects
        .iter()
        .zip(present)
        .filter(|(_, there)| *there)
        .map(|(p, _)| describe(p))
        .collect()
}

/// File › Open Recent's projects: the recent group's (outside the user's groups), whose
/// folders exist, most recently opened first, at most `limit`, without `except` (the
/// window's own).
pub fn menu_projects(limit: usize, except: Option<&Path>) -> Vec<RecentProject> {
    let model = load();
    let present = present(&model.projects);
    model
        .projects
        .iter()
        .zip(present)
        .filter(|(p, there)| {
            *there && model.group_of(p) == workspace::recent::RECENT && Some(p.as_path()) != except
        })
        .take(limit)
        .map(|(p, _)| describe(p))
        .collect()
}

/// The list for the projects picker: its groups, and the projects whose folders exist.
pub fn list() -> RecentList {
    let model = load();
    let present = present(&model.projects);
    let projects = model
        .projects
        .iter()
        .zip(present)
        .filter(|(_, there)| *there)
        .map(|(p, _)| describe(p))
        .collect();
    RecentList { model, projects, sessions: Vec::new() }
}

pub fn describe(path: &Path) -> RecentProject {
    let name = path
        .file_name()
        .map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
    let display = kernel::paths::tildify(path, dirs::home_dir().as_deref());
    RecentProject { name, display, path: path.to_path_buf(), kind: None }
}
