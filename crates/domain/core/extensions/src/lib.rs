//! Extensions bounded context: the `Module` contract, the registry and the lifecycle.
//!
//! A module doesn't receive references to the editor's services: it **contributes** objects
//! of known types (language descriptors, grammars, injection rules, themes...).
//! Contribution types are open: any crate can define a new one, and whoever
//! consumes it requests it by type. This way, a new kind of extension doesn't change this crate.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};

pub use kernel::ModuleId;

/// A module's description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleManifest {
    pub id: ModuleId,
    pub name: &'static str,
    pub description: &'static str,
    /// The modules it needs (e.g. `lang.php` → `lang.html`). Missing ones are reported.
    pub depends_on: &'static [&'static str],
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ModuleError {
    #[error("the module failed to activate: {0}")]
    Activation(String),
    #[error("the module panicked while activating")]
    Panicked,
    #[error("missing dependency `{0}`")]
    MissingDependency(String),
}

/// A feature. Activating it only contributes (cheap: descriptions, providers); the data it
/// works with (a dictionary, stubs) is read on first use — or ahead of it ([`preload`]),
/// and let go when unused ([`release_idle`]).
///
/// [`preload`]: Module::preload
/// [`release_idle`]: Module::release_idle
pub trait Module: Send + Sync {
    fn manifest(&self) -> &ModuleManifest;
    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError>;
    /// Reads now the data otherwise read on first use, so that first use is instant (on a
    /// background thread, at startup, for the modules chosen to load then).
    fn preload(&self) {}
    /// Lets go of its data unused for `idle` (read again when needed). `true` if something
    /// was released.
    fn release_idle(&self, _idle: std::time::Duration) -> bool {
        false
    }
    /// Some of its data is in memory now.
    fn loaded(&self) -> bool {
        false
    }
    /// The memory its data holds now (bytes; an estimate, kept as it's read), for the Task
    /// Manager. `None`: it holds none of its own.
    fn memory(&self) -> Option<u64> {
        None
    }
}

/// A contribution: the module that brought it and the object (of a type known to the consumer).
type Contribution = (ModuleId, Box<dyn Any + Send + Sync>);

/// The contributions of all active modules, grouped by type.
#[derive(Default)]
pub struct Contributions {
    items: HashMap<TypeId, Vec<Contribution>>,
}

impl fmt::Debug for Contributions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Contributions").field("types", &self.items.len()).finish()
    }
}

impl Contributions {
    /// Takes all contributions of type `T`, in module activation order.
    pub fn take<T: Any + Send + Sync>(&mut self) -> Vec<T> {
        self.items
            .remove(&TypeId::of::<T>())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(_, item)| item.downcast::<T>().ok().map(|b| *b))
            .collect()
    }

    /// Takes all contributions of type `T`, with the module each came from.
    pub fn take_by_module<T: Any + Send + Sync>(&mut self) -> Vec<(ModuleId, T)> {
        self.items
            .remove(&TypeId::of::<T>())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(id, item)| item.downcast::<T>().ok().map(|b| (id, *b)))
            .collect()
    }

    /// Lets go of the contributions of type `T` the modules `hidden` brought (a hidden
    /// module's buttons, menus, commands: what the app decides it doesn't show). Returns how
    /// many went.
    pub fn drop_from<T: Any + Send + Sync>(&mut self, hidden: impl Fn(&ModuleId) -> bool) -> usize {
        let Some(items) = self.items.get_mut(&TypeId::of::<T>()) else { return 0 };
        let before = items.len();
        items.retain(|(id, _)| !hidden(id));
        before - items.len()
    }

    pub fn count<T: Any + Send + Sync>(&self) -> usize {
        self.items.get(&TypeId::of::<T>()).map_or(0, Vec::len)
    }
}

/// The only gateway through which a module interacts with the editor.
#[derive(Debug)]
pub struct ModuleContext<'a> {
    module: ModuleId,
    contributions: &'a mut Contributions,
}

impl ModuleContext<'_> {
    pub fn module(&self) -> &ModuleId {
        &self.module
    }

    /// Adds a contribution (e.g. a `LanguageDescriptor`, a grammar, an injection rule).
    pub fn contribute<T: Any + Send + Sync>(&mut self, item: T) {
        self.contributions
            .items
            .entry(TypeId::of::<T>())
            .or_default()
            .push((self.module.clone(), Box::new(item)));
    }
}

/// Which modules are activated (from settings, the `[modules]` section).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleSelection {
    /// `true`: only the modules in `enabled`. `false` (default): all except `disabled`.
    pub explicit: bool,
    pub enabled: Vec<String>,
    pub disabled: Vec<String>,
    /// The modules that run but show nothing of their own (no buttons, menus, commands):
    /// what they serve others through requests stays. Which contributions that leaves out
    /// is the app's choice ([`Contributions::drop_from`]).
    pub hidden: Vec<String>,
}

impl ModuleSelection {
    pub fn allows(&self, id: &ModuleId) -> bool {
        let id = id.as_str();
        if self.explicit {
            self.enabled.iter().any(|e| e == id)
        } else {
            !self.disabled.iter().any(|d| d == id)
        }
    }

    /// Module `id` runs hidden (when it runs at all).
    pub fn hides(&self, id: &ModuleId) -> bool {
        self.hidden.iter().any(|h| h == id.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleState {
    Active,
    Disabled,
    Failed(ModuleError),
}

/// The activation result, for logging and the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleReport {
    pub id: ModuleId,
    pub name: &'static str,
    pub description: &'static str,
    /// The modules it needs.
    pub depends_on: &'static [&'static str],
    pub state: ModuleState,
    /// It runs hidden: its own buttons, menus and commands aren't shown
    /// ([`ModuleSelection::hidden`]); `false` for a module that doesn't run.
    pub hidden: bool,
    /// How long its activation took.
    pub took: std::time::Duration,
}

#[derive(Default)]
pub struct ModuleRegistry {
    modules: Vec<Box<dyn Module>>,
    /// What tells the time, to measure how long a module took (the domain doesn't read the
    /// clock itself); `None`: nothing is measured (every duration is zero).
    clock: Option<fn() -> std::time::Instant>,
}

impl fmt::Debug for ModuleRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.modules.iter().map(|m| m.manifest().id.as_str())).finish()
    }
}

impl ModuleRegistry {
    pub fn register(&mut self, module: Box<dyn Module>) {
        self.modules.push(module);
    }

    /// Measures how long each module takes to activate and preload with `clock`.
    pub fn set_clock(&mut self, clock: fn() -> std::time::Instant) {
        self.clock = Some(clock);
    }

    /// A stopwatch started now: how long since, when read (zero without a clock).
    fn stopwatch(&self) -> impl Fn() -> std::time::Duration + use<> {
        let clock = self.clock;
        let started = clock.map(|now| now());
        move || match (clock, started) {
            (Some(now), Some(started)) => now().saturating_duration_since(started),
            _ => std::time::Duration::ZERO,
        }
    }

    /// Preloads the data of the modules `chosen` (call it on a background thread). Returns
    /// how long each took (`None`: it panicked).
    pub fn preload(
        &self,
        chosen: impl Fn(&ModuleId) -> bool,
    ) -> Vec<(ModuleId, Option<std::time::Duration>)> {
        self.modules
            .iter()
            .filter(|m| chosen(&m.manifest().id))
            .map(|module| {
                let took = self.stopwatch();
                let ok = catch_unwind(AssertUnwindSafe(|| module.preload())).is_ok();
                (module.manifest().id.clone(), ok.then(took))
            })
            .collect()
    }

    /// Lets the modules' data unused for `idle` go, except the modules `kept`. Returns how
    /// many modules released something.
    pub fn release_idle(
        &self,
        idle: std::time::Duration,
        kept: impl Fn(&ModuleId) -> bool,
    ) -> usize {
        self.modules
            .iter()
            .filter(|m| !kept(&m.manifest().id))
            .filter(|m| m.release_idle(idle))
            .count()
    }

    /// Lets module `id`'s data go now, whatever its age (read again when needed): `true` if
    /// something was released.
    pub fn release(&self, id: &str) -> bool {
        self.modules
            .iter()
            .filter(|m| m.manifest().id.as_str() == id)
            .any(|m| m.release_idle(std::time::Duration::ZERO))
    }

    /// Whether module `id` has data in memory now.
    pub fn loaded(&self, id: &str) -> bool {
        self.modules.iter().any(|m| m.manifest().id.as_str() == id && m.loaded())
    }

    /// The memory module `id`'s data holds now (see [`Module::memory`]).
    pub fn memory(&self, id: &str) -> Option<u64> {
        self.modules.iter().find(|m| m.manifest().id.as_str() == id).and_then(|m| m.memory())
    }

    /// Activates the selected modules, in registration order. A module that fails
    /// or panics is marked `Failed`; its partial contributions are discarded.
    pub fn activate(&self, selection: &ModuleSelection) -> (Contributions, Vec<ModuleReport>) {
        let mut contributions = Contributions::default();
        let mut reports = Vec::with_capacity(self.modules.len());
        let selected = |id: &str| {
            self.modules
                .iter()
                .any(|m| m.manifest().id.as_str() == id && selection.allows(&m.manifest().id))
        };
        for module in &self.modules {
            let manifest = module.manifest();
            let took = self.stopwatch();
            // A dependency that already tried to start and didn't (it failed, or its own
            // dependency is missing) is missing too.
            let failed = |id: &str| {
                reports
                    .iter()
                    .any(|r: &ModuleReport| r.id.as_str() == id && r.state != ModuleState::Active)
            };
            let state = if !selection.allows(&manifest.id) {
                ModuleState::Disabled
            } else if let Some(dep) = manifest.depends_on.iter().find(|d| !selected(d) || failed(d))
            {
                ModuleState::Failed(ModuleError::MissingDependency((*dep).to_owned()))
            } else {
                let mut staged = Contributions::default();
                let mut ctx =
                    ModuleContext { module: manifest.id.clone(), contributions: &mut staged };
                match catch_unwind(AssertUnwindSafe(|| module.activate(&mut ctx))) {
                    Ok(Ok(())) => {
                        for (ty, items) in staged.items {
                            contributions.items.entry(ty).or_default().extend(items);
                        }
                        ModuleState::Active
                    }
                    Ok(Err(e)) => ModuleState::Failed(e),
                    Err(_) => ModuleState::Failed(ModuleError::Panicked),
                }
            };
            reports.push(ModuleReport {
                id: manifest.id.clone(),
                name: manifest.name,
                description: manifest.description,
                depends_on: manifest.depends_on,
                hidden: state == ModuleState::Active && selection.hides(&manifest.id),
                state,
                took: took(),
            });
        }
        (contributions, reports)
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
