//! The language servers' settings, `[language-servers]`:
//!
//! - `engines`: what answers each language, `language=engine` (`typescript=built-in`,
//!   `tsx=server:my-server`, `javascript=both`); a language not listed has its default: its
//!   server, unless its description says otherwise on this OS (`default-engine`: PHP's
//!   phpactor is the built-in engine's on Windows);
//! - `idle_minutes`: a server without documents stops after this long (0: never);
//! - `max_memory`: the memory a Node or PHP server may take (MB: `--max-old-space-size`, PHP's
//!   `memory_limit`, tsserver's limit);
//! - `trace`: the protocol's traffic written to the servers' logs;
//! - `disabled`: servers turned off (by name).
//!
//! Declared here, handed to the running servers by [`apply`] at startup and after every
//! change (applied at once: a server no longer chosen is stopped, one chosen started for
//! the documents open).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use code_intelligence::servers::Engine;
use settings_model::{Setting, SettingsService};

/// The idle times offered (minutes; 0: never).
pub const IDLE_MINUTES_CHOICES: [i64; 6] = [2, 5, 10, 30, 60, 0];
/// The memory limits offered (MB).
pub const MAX_MEMORY_CHOICES: [i64; 5] = [1024, 2048, 3072, 4096, 8192];

pub const ENGINES: Setting<Vec<String>> = Setting::new("language-servers", "engines", Vec::new)
    .valid(|v| v.iter().all(|e| parse_engine(e).is_some()));

pub const IDLE_MINUTES: Setting<i64> = Setting::new("language-servers", "idle_minutes", || 2)
    .valid(|m| IDLE_MINUTES_CHOICES.contains(m));

pub const MAX_MEMORY: Setting<i64> = Setting::new("language-servers", "max_memory", || 3072)
    .valid(|m| MAX_MEMORY_CHOICES.contains(m));

pub const TRACE: Setting<bool> = Setting::new("language-servers", "trace", || false);

pub const DISABLED: Setting<Vec<String>> = Setting::new("language-servers", "disabled", Vec::new);

/// What answers a language: the engine, and the server chosen (`None`: the first that can
/// start).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Choice {
    pub engine: Engine,
    pub server: Option<String>,
}

/// An `engines` entry: its language and choice.
pub fn parse_engine(entry: &str) -> Option<(String, Choice)> {
    let (language, value) = entry.split_once('=')?;
    let (engine, server) = match value.split_once(':') {
        Some((engine, server)) if !server.is_empty() => (engine, Some(server.to_owned())),
        Some(_) => return None,
        None => (value, None),
    };
    let engine = Engine::from_id(engine)?;
    if engine == Engine::BuiltIn && server.is_some() || language.is_empty() {
        return None;
    }
    Some((language.to_owned(), Choice { engine, server }))
}

/// An `engines` entry for `language`.
pub fn engine_entry(language: &str, choice: &Choice) -> String {
    match &choice.server {
        Some(server) => format!("{language}={}:{server}", choice.engine.id()),
        None => format!("{language}={}", choice.engine.id()),
    }
}

/// The settings as the servers use them.
#[derive(Debug, Clone)]
pub struct Config {
    pub engines: Vec<(String, Choice)>,
    pub idle: Option<Duration>,
    pub max_memory: u64,
    pub disabled: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            engines: Vec::new(),
            idle: Some(Duration::from_secs(120)),
            max_memory: 3072,
            disabled: Vec::new(),
        }
    }
}

impl Config {
    /// What answers `language`: the user's choice, else its default.
    pub fn choice(&self, language: &str) -> Choice {
        self.engines
            .iter()
            .find(|(l, _)| l == language)
            .map(|(_, c)| c.clone())
            .unwrap_or_else(|| default_choice(language))
    }
}

/// What answers `language` until the user chooses: its first server whose default engine
/// on this OS (`default-engine` in its description) isn't the built-in one and that can be
/// offered (one the user installs: once installed) — PHP's phpactor, or on Windows, where
/// phpactor is the built-in engine's by default, intelephense once it's installed; none:
/// the built-in engine (no server at all: a server, as before).
pub fn default_choice(language: &str) -> Choice {
    let catalog = crate::catalog::catalog();
    let mut any = false;
    for (i, server) in catalog.for_language(language).enumerate() {
        any = true;
        let engine = server.default_engine();
        if engine == Engine::BuiltIn || !crate::install::available(server) {
            continue;
        }
        return Choice { engine, server: (i > 0).then(|| server.name.clone()) };
    }
    if any { Choice { engine: Engine::BuiltIn, server: None } } else { Choice::default() }
}

/// The servers offered for `language`, in order: those described, but one the user installs
/// only once it's installed.
pub fn offered(language: &str) -> Vec<String> {
    crate::catalog::catalog()
        .for_language(language)
        .filter(|s| crate::install::available(s))
        .map(|s| s.name.clone())
        .collect()
}

static CONFIG: RwLock<Option<Config>> = RwLock::new(None);
static TRACING: AtomicBool = AtomicBool::new(false);
/// Bumped at every change of the settings (the bars over the files look again).
static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// What changed since: a number bumped at every change of the settings.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

/// The settings now.
pub fn config() -> Config {
    CONFIG.read().ok().and_then(|c| c.clone()).unwrap_or_default()
}

/// Whether the servers' traffic is logged (shared with their drivers).
pub fn tracing_flag() -> Arc<AtomicBool> {
    static FLAG: std::sync::OnceLock<Arc<AtomicBool>> = std::sync::OnceLock::new();
    Arc::clone(FLAG.get_or_init(|| Arc::new(AtomicBool::new(TRACING.load(Ordering::Relaxed)))))
}

/// The saved settings handed to the servers (the running ones follow at once).
pub fn apply(settings: &SettingsService) {
    let minutes = settings.get(&IDLE_MINUTES);
    let config = Config {
        engines: settings.get(&ENGINES).iter().filter_map(|e| parse_engine(e)).collect(),
        idle: (minutes > 0).then(|| Duration::from_secs(minutes as u64 * 60)),
        max_memory: settings.get(&MAX_MEMORY) as u64,
        disabled: settings.get(&DISABLED),
    };
    // Pinned (a test's choice): the saved settings applied again (another window's
    // modules loaded, the file read again) don't undo it. Looked at and written in one turn:
    // a pin coming between the two is never overwritten.
    {
        let _turn = turn();
        if PINNED.load(Ordering::Acquire) {
            return;
        }
        set(config);
    }
    tracing_flag().store(settings.get(&TRACE), Ordering::Relaxed);
}

/// The settings given by [`pin`] stay whatever the saved ones say.
static PINNED: AtomicBool = AtomicBool::new(false);

/// One at a time: the saved settings applied, a pin made or let go (each looks at
/// [`PINNED`] and writes the settings as one step).
fn turn() -> std::sync::MutexGuard<'static, ()> {
    static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());
    TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// `config` used until the guard is dropped, whatever the saved settings say (tests: the
/// app's other tests load the modules side by side, each applying the saved settings
/// again); then the settings before it again.
pub fn pin(config: Config) -> Pinned {
    let _turn = turn();
    let before = self::config();
    PINNED.store(true, Ordering::Release);
    set(config);
    Pinned { before: Some(before) }
}

/// See [`pin`].
#[derive(Debug)]
#[must_use = "the settings are pinned only while it's held"]
pub struct Pinned {
    before: Option<Config>,
}

impl Drop for Pinned {
    fn drop(&mut self) {
        let _turn = turn();
        PINNED.store(false, Ordering::Release);
        if let Some(before) = self.before.take() {
            set(before);
        }
    }
}

/// The settings as given (tests).
pub fn set(config: Config) {
    let changed = CONFIG.read().ok().is_none_or(|c| {
        c.as_ref().map(|c| (&c.engines, &c.disabled)) != Some((&config.engines, &config.disabled))
    });
    if let Ok(mut c) = CONFIG.write() {
        *c = Some(config);
    }
    GENERATION.fetch_add(1, Ordering::Relaxed);
    if changed {
        crate::projects::refresh_all();
    }
}

/// `language`'s choice saved (the others kept).
pub fn save_choice(settings: &SettingsService, language: &str, choice: &Choice) {
    let mut entries: Vec<String> = settings
        .get(&ENGINES)
        .into_iter()
        .filter(|e| parse_engine(e).is_none_or(|(l, _)| l != language))
        .collect();
    if *choice != default_choice(language) {
        entries.push(engine_entry(language, choice));
    }
    if let Err(e) = settings.set(&ENGINES, entries) {
        tracing::warn!("can't save a language's engine: {e}");
    }
    apply(settings);
}

#[cfg(test)]
#[path = "../tests/unit/settings.rs"]
mod tests;
