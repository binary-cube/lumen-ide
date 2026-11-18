//! The modules' manager: after activation (cheap: descriptions and providers), it decides
//! when their data — dictionaries, PHP's stubs, compiled grammars — is in memory, by the
//! settings `[modules]`:
//!
//! - `on-demand` (default): read on first need (a YAML grammar when a `.yaml` opens, the
//!   PHP stubs on the first PHP completion), let go once unused for [`IDLE`] (a grammar once
//!   no open file uses it either); the modules
//!   checked "at startup" are read when the app opens and kept;
//! - `startup`: every active module's data is read in the background at startup and kept.
//!
//! The work runs on background threads (each module's data on its own, `preload-<id>`, its
//! CPU on its row in the Task Manager; `module-release`); the UI never waits for it.

use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

use extensions::{ModuleId, ModuleRegistry};
use kernel::LanguageId;
use syntax_ts::SyntaxConfig;

use crate::settings::{
    HIDDEN_BY_DEFAULT, MODULE_LOADING, MODULES_DISABLED, MODULES_HIDDEN, MODULES_PRELOAD,
    MODULES_SHOWN, ON_DEMAND, STARTUP,
};

/// How long a module's data may stay unused before it's let go (`on-demand`).
pub const IDLE: Duration = Duration::from_secs(10 * 60);
/// How often unused data is looked for.
const RELEASE_EVERY: Duration = Duration::from_secs(60);

/// The thread reading module `id`'s data ahead (`preload-<id>`), as the one-name list the
/// Task Manager matches on the module's row: made once per module for the whole process.
pub fn preload_threads(id: &str) -> &'static [&'static str] {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static NAMES: OnceLock<Mutex<HashMap<String, &'static [&'static str]>>> = OnceLock::new();
    let mut names = NAMES.get_or_init(Mutex::default).lock().unwrap_or_else(|e| e.into_inner());
    names.entry(id.to_owned()).or_insert_with(|| {
        let name: &'static str = Box::leak(format!("preload-{id}").into_boxed_str());
        Box::leak(Box::new([name]))
    })
}

/// The thread reading module `id`'s data ahead.
fn preload_thread(id: &str) -> &'static str {
    preload_threads(id)[0]
}

pub struct ModuleManager {
    registry: ModuleRegistry,
    /// The modules that started.
    active: Vec<ModuleId>,
    /// The languages each module brought (their grammars are its data too).
    languages: Vec<(ModuleId, LanguageId)>,
    syntax: Arc<SyntaxConfig>,
}

impl std::fmt::Debug for ModuleManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModuleManager").field("active", &self.active.len()).finish_non_exhaustive()
    }
}

impl ModuleManager {
    pub fn new(
        registry: ModuleRegistry,
        active: Vec<ModuleId>,
        languages: Vec<(ModuleId, LanguageId)>,
        syntax: Arc<SyntaxConfig>,
    ) -> Self {
        Self { registry, active, languages, syntax }
    }

    fn languages_of<'a>(&'a self, id: &'a str) -> impl Iterator<Item = &'a LanguageId> + 'a {
        self.languages.iter().filter(move |(m, _)| m.as_str() == id).map(|(_, l)| l)
    }

    /// Module `id` has data in memory now (its own, or its language's compiled grammar).
    /// Module `id`'s data let go now (the Task Manager's "Release its data").
    pub fn release(&self, id: &str) -> bool {
        self.registry.release(id)
    }

    pub fn loaded(&self, id: &str) -> bool {
        self.registry.loaded(id)
            || self.languages_of(id).any(|l| self.syntax.grammars().is_compiled(l))
    }

    /// The memory module `id`'s data holds now, its compiled grammars included (`None`: none
    /// of its own).
    pub fn memory(&self, id: &str) -> Option<u64> {
        let grammars: u64 = self.languages_of(id).map(|l| self.syntax.grammars().memory(l)).sum();
        match self.registry.memory(id) {
            Some(data) => Some(data + grammars),
            None => (grammars > 0).then_some(grammars),
        }
    }

    /// The modules whose data is read at startup: every active one (`startup`), else the
    /// ones chosen.
    fn at_startup(&self) -> Vec<String> {
        let settings = crate::config::settings();
        let all = settings.get(&MODULE_LOADING) == STARTUP;
        let chosen = settings.get(&MODULES_PRELOAD);
        self.active
            .iter()
            .map(|id| id.as_str().to_owned())
            .filter(|id| all || chosen.contains(id))
            .collect()
    }

    /// Reads the data of `ids` now, one module after another, each on its own thread
    /// ([`preload_thread`]; blocking: call it on a background thread).
    fn preload(&self, ids: &[String]) {
        std::thread::scope(|scope| {
            for id in ids {
                let read = std::thread::Builder::new()
                    .name(preload_thread(id).into())
                    .spawn_scoped(scope, || self.preload_one(id));
                if let Ok(read) = read {
                    let _ = read.join();
                }
            }
        });
    }

    /// Module `id`'s data and its grammars read.
    fn preload_one(&self, id: &str) {
        for (id, took) in self.registry.preload(|m| m.as_str() == id) {
            for language in self.languages_of(id.as_str()) {
                self.syntax.grammars().warm_up(language);
            }
            match took {
                Some(took) => tracing::debug!(module = %id, ?took, "preloaded"),
                None => tracing::warn!(module = %id, "preloading panicked"),
            }
        }
    }

    fn preload_in_background(self: &Arc<Self>, ids: Vec<String>) {
        if ids.is_empty() {
            return;
        }
        let manager = Arc::clone(self);
        let _ = std::thread::Builder::new()
            .name("module-preload".into())
            .spawn(move || manager.preload(&ids));
    }

    /// At startup: the data chosen read ahead, and — `on-demand` — unused data let go.
    pub fn start(self: &Arc<Self>) {
        self.preload_in_background(self.at_startup());
        let manager = Arc::clone(self);
        // In `startup` nothing is let go: the thread sleeps until the loading mode may have
        // changed (the `[modules]` section), instead of looking every minute for nothing.
        let changed = Arc::new((Mutex::new(false), Condvar::new()));
        let signal = Arc::clone(&changed);
        let followed = crate::config::settings().subscribe(move |e| {
            if e.section == MODULE_LOADING.section {
                *signal.0.lock().unwrap_or_else(PoisonError::into_inner) = true;
                signal.1.notify_all();
            }
        });
        let _ = std::thread::Builder::new().name("module-release".into()).spawn(move || {
            let _followed = followed;
            loop {
                std::thread::sleep(RELEASE_EVERY);
                let settings = crate::config::settings();
                if settings.get(&MODULE_LOADING) == STARTUP {
                    let (flag, wake) = &*changed;
                    let mut flag = flag.lock().unwrap_or_else(PoisonError::into_inner);
                    *flag = false;
                    // The mode read again first: a change just before the flag was cleared
                    // isn't missed.
                    if settings.get(&MODULE_LOADING) == STARTUP {
                        let _woken =
                            wake.wait_while(flag, |f| !*f).unwrap_or_else(PoisonError::into_inner);
                    }
                    continue;
                }
                let kept = settings.get(&MODULES_PRELOAD);
                let released =
                    manager.registry.release_idle(IDLE, |id| kept.iter().any(|k| k == id.as_str()));
                if released > 0 {
                    tracing::debug!(modules = released, "unused module data let go");
                }
                // The grammars no open file uses: their compiled queries.
                let kept_language = |language: &LanguageId| {
                    manager
                        .languages
                        .iter()
                        .any(|(m, l)| l == language && kept.iter().any(|k| k == m.as_str()))
                };
                let grammars = manager.syntax.grammars().release_idle(IDLE, kept_language);
                if grammars > 0 {
                    tracing::debug!(grammars, "unused compiled grammars let go");
                }
            }
        });
    }

    /// "Clear and Rebuild Caches": every module's data in memory and every compiled grammar
    /// no open file holds, let go now (read again when next used).
    pub fn purge(&self) -> (usize, usize) {
        let modules = self.registry.release_idle(std::time::Duration::ZERO, |_| false);
        let grammars = self.syntax.grammars().release_idle(std::time::Duration::ZERO, |_| false);
        tracing::debug!(modules, grammars, "module data let go");
        (modules, grammars)
    }

    /// Turns module `id` on / off — from the next start (contributions are read once).
    pub fn toggle_enabled(&self, id: &str) {
        let settings = crate::config::settings();
        let mut disabled = settings.get(&MODULES_DISABLED);
        match disabled.iter().position(|d| d == id) {
            Some(i) => {
                disabled.remove(i);
            }
            None => disabled.push(id.to_owned()),
        }
        if let Err(e) = settings.set(&MODULES_DISABLED, disabled) {
            tracing::warn!("can't save the modules: {e}");
        }
    }

    /// Module `id` hidden / shown — from the next start (contributions are read once). One
    /// hidden by default and shown again is remembered as shown (`[modules] shown`), so the
    /// default never hides it again.
    pub fn toggle_hidden(&self, id: &str) {
        set_hidden(
            crate::config::settings(),
            id,
            !crate::settings::module_hidden(crate::config::settings(), id),
        );
    }

    /// Module `id`'s data read at startup, or not; chosen now, it's read now.
    pub fn toggle_preload(self: &Arc<Self>, id: &str) {
        let settings = crate::config::settings();
        let mut chosen = settings.get(&MODULES_PRELOAD);
        let added = match chosen.iter().position(|c| c == id) {
            Some(i) => {
                chosen.remove(i);
                false
            }
            None => {
                chosen.push(id.to_owned());
                true
            }
        };
        if let Err(e) = settings.set(&MODULES_PRELOAD, chosen) {
            tracing::warn!("can't save the modules: {e}");
        }
        if added && self.active.iter().any(|a| a.as_str() == id) {
            self.preload_in_background(vec![id.to_owned()]);
        }
    }

    /// Every module's data at startup (and kept), or on demand; chosen now, it's read now.
    pub fn set_at_startup(self: &Arc<Self>, startup: bool) {
        let mode = if startup { STARTUP } else { ON_DEMAND };
        if let Err(e) = crate::config::settings().set(&MODULE_LOADING, mode.to_owned()) {
            tracing::warn!("can't save the modules: {e}");
        }
        if startup {
            self.preload_in_background(self.at_startup());
        }
    }
}

/// Module `id` hidden (`true`) or shown from the next start, in `settings`.
pub(crate) fn set_hidden(settings: &settings_model::SettingsService, id: &str, hidden: bool) {
    let mut chosen = settings.get(&MODULES_HIDDEN);
    let mut shown = settings.get(&MODULES_SHOWN);
    chosen.retain(|h| h != id);
    shown.retain(|s| s != id);
    if hidden {
        chosen.push(id.to_owned());
    } else if HIDDEN_BY_DEFAULT.contains(&id) {
        shown.push(id.to_owned());
    }
    for (setting, value) in [(&MODULES_HIDDEN, chosen), (&MODULES_SHOWN, shown)] {
        if settings.get(setting) != value
            && let Err(e) = settings.set(setting, value)
        {
            tracing::warn!("can't save the modules: {e}");
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/module_manager.rs"]
mod tests;
