//! The data sources configured: the project's list and every project's (global), as a
//! [`DataSourceStore`] keeps them. Nothing is read until the "Database" window first asks
//! ([`Catalog::ensure_loaded`]: in the background); every change is made in memory at once,
//! said on the project's bus ([`DataSourcesChanged`]) and written in the background, one
//! write at a time per list, the newest list last. A list whose file can't be read is
//! [`Listed::Unreadable`]: shown with why, never written over (nothing in it is lost).
//! A list holds its folders too ([`SourceList`]): made, renamed, colored, moved and taken
//! away here the same way (taking one away keeps its sources: they move up).
//!
//! The global list is every project's: a change made in one project reaches the others'
//! catalogs ([`Catalog::on_global`], set by [`crate::Sessions`]), each saying it on its own
//! bus — another project's own sources never reach this one.
//!
//! A server's other databases (PostgreSQL's: a session opens one) are sources of their own
//! while the project is open: [`Catalog::database_id`] gives database `name` of a source its
//! identity ([`DataSourceId::of_database`]: the same after a restart) and
//! [`Catalog::get`] that identity the source as it opens that database — never saved, its
//! password, color, production mark and relations its source's.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use database::events::DataSourcesChanged;
use database::folders::{self, FolderId, SourceFolder, SourceList};
use database::{DataSource, DataSourceId, DataSourceStore, Scope};
use kernel::EventBus;

use crate::identity::FileIdentities;
use crate::runtime::Runtime;

/// A list as it is now.
#[derive(Debug, Clone, PartialEq)]
pub enum Listed {
    /// Not read yet (nobody asked), or being read.
    Loading,
    Ready(SourceList),
    /// Its file isn't a list of sources: why (it's left as it is).
    Unreadable(String),
}

impl Listed {
    pub fn sources(&self) -> &[DataSource] {
        match self {
            Self::Ready(list) => &list.sources,
            _ => &[],
        }
    }

    pub fn folders(&self) -> &[SourceFolder] {
        match self {
            Self::Ready(list) => &list.folders,
            _ => &[],
        }
    }
}

/// One list and its writing.
#[derive(Debug)]
struct Slot {
    list: Listed,
    /// Its reading was asked (it's read once).
    asked: bool,
    /// Bumped by each change; a write job is on its way while `writing`.
    version: u64,
    writing: bool,
    /// Why the last write failed (cleared by the next that works).
    failed: Option<String>,
}

impl Default for Slot {
    fn default() -> Self {
        Self { list: Listed::Loading, asked: false, version: 0, writing: false, failed: None }
    }
}

#[derive(Debug, Default)]
struct State {
    project: Slot,
    global: Slot,
}

impl State {
    fn slot(&mut self, scope: Scope) -> &mut Slot {
        match scope {
            Scope::Project => &mut self.project,
            Scope::Global => &mut self.global,
        }
    }
}

/// Called with the global list after this project changed it (the other projects take it).
pub type OnGlobal = Arc<dyn Fn(&SourceList) + Send + Sync>;

/// A project's view of the configured sources (see the module's doc).
pub struct Catalog {
    store: Mutex<Arc<dyn DataSourceStore>>,
    runtime: Runtime,
    events: EventBus,
    state: Arc<Mutex<State>>,
    /// Bumped whenever a list changes (what's shown from it is worked out again).
    generation: Arc<AtomicU64>,
    on_global: Mutex<Option<OnGlobal>>,
    /// Which file a source's path names (its project's: a link in it never makes two).
    identities: Arc<FileIdentities>,
    /// A server's databases seen: their identity, and which source's database they are.
    databases: Mutex<std::collections::HashMap<DataSourceId, (DataSourceId, String)>>,
}

impl std::fmt::Debug for Catalog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Catalog").finish_non_exhaustive()
    }
}

impl Catalog {
    pub fn new(store: Arc<dyn DataSourceStore>, runtime: Runtime, events: EventBus) -> Self {
        Self {
            store: Mutex::new(store),
            runtime,
            events,
            state: Arc::default(),
            generation: Arc::default(),
            on_global: Mutex::new(None),
            identities: Arc::default(),
            databases: Mutex::default(),
        }
    }

    /// The identity of database `name` of source `source` (kept: [`Self::get`] finds it).
    pub fn database_id(&self, source: DataSourceId, name: &str) -> DataSourceId {
        let id = source.of_database(name);
        self.databases
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, (source, name.to_owned()));
        id
    }

    /// Which source's database `id` is, and its name (`None`: a source of its own).
    pub fn database_of(&self, id: DataSourceId) -> Option<(DataSourceId, String)> {
        self.databases.lock().unwrap_or_else(PoisonError::into_inner).get(&id).cloned()
    }

    /// The source configured that `id` is, or whose database it is.
    pub fn base_id(&self, id: DataSourceId) -> DataSourceId {
        self.database_of(id).map_or(id, |(source, _)| source)
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Where its lists are kept from now on (read again when next asked).
    pub fn set_store(&self, store: Arc<dyn DataSourceStore>) {
        *self.store.lock().unwrap_or_else(PoisonError::into_inner) = store;
        *self.lock() = State::default();
        self.generation.fetch_add(1, Ordering::Relaxed);
    }

    fn store(&self) -> Arc<dyn DataSourceStore> {
        Arc::clone(&self.store.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// Changes with every change of a list (a cache of what's shown from them is kept
    /// until it does).
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    /// Who hears this project's changes of the global list.
    pub fn on_global(&self, hook: OnGlobal) {
        *self.on_global.lock().unwrap_or_else(PoisonError::into_inner) = Some(hook);
    }

    /// Both lists read in the background if they weren't yet (each once); said on the bus
    /// when they are.
    pub fn ensure_loaded(&self) {
        for scope in [Scope::Project, Scope::Global] {
            {
                let mut state = self.lock();
                let slot = state.slot(scope);
                if slot.asked {
                    continue;
                }
                slot.asked = true;
            }
            let (store, shared, events) =
                (self.store(), Arc::clone(&self.state), self.events.clone());
            let generation = Arc::clone(&self.generation);
            self.runtime.spawn(move || {
                let listed = match store.load(scope) {
                    Ok(list) => Listed::Ready(list),
                    Err(why) => {
                        tracing::warn!(?scope, error = %why, "the data sources can't be read");
                        Listed::Unreadable(why)
                    }
                };
                {
                    let mut state = shared.lock().unwrap_or_else(PoisonError::into_inner);
                    let slot = state.slot(scope);
                    // Another project's change came first (the global list): it's newer.
                    if slot.list == Listed::Loading {
                        slot.list = listed;
                    }
                }
                generation.fetch_add(1, Ordering::Relaxed);
                events.publish(&DataSourcesChanged { scope });
            });
        }
    }

    /// `scope`'s list as it is now.
    pub fn list(&self, scope: Scope) -> Listed {
        self.lock().slot(scope).list.clone()
    }

    /// Why `scope`'s last write failed, if it did.
    pub fn write_error(&self, scope: Scope) -> Option<String> {
        self.lock().slot(scope).failed.clone()
    }

    /// Every source known: the project's, then the global ones.
    pub fn all(&self) -> Vec<DataSource> {
        let state = self.lock();
        state.project.list.sources().iter().chain(state.global.list.sources()).cloned().collect()
    }

    /// Source `id`; a database of a source's server ([`Self::database_id`]): that source as
    /// it opens it — its identity, its name with the database's, the database its target's.
    pub fn get(&self, id: DataSourceId) -> Option<DataSource> {
        if let Some(found) = self.all().into_iter().find(|s| s.id == id) {
            return Some(found);
        }
        let (base, name) = self.database_of(id)?;
        let mut source = self.all().into_iter().find(|s| s.id == base)?;
        source.id = id;
        source.name = database_name(&source.name, &name);
        source.target.database = name;
        Some(source)
    }

    /// Every folder known: the project's, then the global ones.
    pub fn folders(&self) -> Vec<SourceFolder> {
        let state = self.lock();
        state.project.list.folders().iter().chain(state.global.list.folders()).cloned().collect()
    }

    /// Which file a path names, as the project's sources are matched by.
    pub fn identities(&self) -> &Arc<FileIdentities> {
        &self.identities
    }

    /// The source kept for file `file` (absolute; a file-based source's own, under
    /// `root` when relative): the same file by any way to it (a link in its path, `/var`
    /// and `/private/var`; on Windows another case or a `\\?\` prefix).
    pub fn source_of_file(
        &self,
        root: &std::path::Path,
        file: &std::path::Path,
    ) -> Option<DataSource> {
        self.source_of_file_with(root, file, |_| true)
    }

    /// As [`Self::source_of_file`], among the sources `keep` accepts (a driver's).
    pub fn source_of_file_with(
        &self,
        root: &std::path::Path,
        file: &std::path::Path,
        keep: impl Fn(&DataSource) -> bool,
    ) -> Option<DataSource> {
        let all: Vec<DataSource> = self.all().into_iter().filter(|s| keep(s)).collect();
        if let Some(found) = all.iter().find(|s| s.file_in(root).as_deref() == Some(file)) {
            return Some(found.clone());
        }
        let key = self.identities.key(file);
        all.into_iter().find(|s| s.file_in(root).is_some_and(|f| self.identities.key(&f) == key))
    }

    /// The color source `id` is shown in (its own, else its folder's; a database's, its
    /// source's).
    pub fn color_of(&self, id: DataSourceId) -> Option<database::SourceColor> {
        let id = self.base_id(id);
        let state = self.lock();
        [&state.project.list, &state.global.list].into_iter().find_map(|list| {
            let source = list.sources().iter().find(|s| s.id == id)?;
            folders::effective_color(source, list.folders())
        })
    }

    /// Source `id` is production (marked so, or in a folder that is; a database, when its
    /// source is).
    pub fn production_of(&self, id: DataSourceId) -> bool {
        let id = self.base_id(id);
        let state = self.lock();
        [&state.project.list, &state.global.list].into_iter().any(|list| {
            list.sources()
                .iter()
                .find(|s| s.id == id)
                .is_some_and(|source| folders::effective_production(source, list.folders()))
        })
    }

    /// `folder` added to its scope's list (or changed in place: renamed, colored, moved —
    /// never into itself nor a folder inside it).
    pub fn put_folder(&self, folder: SourceFolder) -> Result<(), String> {
        self.change(folder.scope, move |list| {
            if let Some(parent) = folder.parent
                && list.folders.iter().any(|f| f.id == folder.id)
                && !folders::can_move_folder(&list.folders, folder.id, Some(parent))
            {
                return false;
            }
            match list.folders.iter_mut().find(|f| f.id == folder.id) {
                Some(slot) if *slot == folder => false,
                Some(slot) => {
                    *slot = folder;
                    true
                }
                None => {
                    list.folders.push(folder);
                    true
                }
            }
        })
    }

    /// Folder `id` of `scope` taken away; what it held (folders, sources) moves up to its
    /// parent — no source goes with it.
    pub fn remove_folder(&self, scope: Scope, id: FolderId) -> Result<(), String> {
        self.change(scope, |list| folders::delete_folder(&mut list.folders, &mut list.sources, id))
    }

    /// `scope`'s folders as `folders` (the modal's Apply).
    pub fn set_folders(&self, scope: Scope, folders: Vec<SourceFolder>) -> Result<(), String> {
        self.change(scope, move |list| {
            if list.folders == folders {
                return false;
            }
            list.folders = folders;
            folders::repair(list);
            true
        })
    }

    /// `source` added to its scope's list (last): said, then written. An error (and nothing
    /// changed) while that list is being read or can't be.
    pub fn add(&self, source: DataSource) -> Result<(), String> {
        let scope = source.scope;
        self.change(scope, move |list| {
            list.sources.retain(|s| s.id != source.id);
            list.sources.push(source);
            true
        })
    }

    /// Source `id` changed to `source` (in place; another scope moves it there — the 37h
    /// modal's).
    pub fn update(&self, source: DataSource) -> Result<(), String> {
        // A server's database: only what it decided (its relations) goes to its source.
        if let Some((base, _)) = self.database_of(source.id) {
            let Some(mut kept) = self.get(base) else { return Ok(()) };
            kept.relations = source.relations;
            return self.update(kept);
        }
        let id = source.id;
        let Some(old) = self.get(id) else { return self.add(source) };
        if old.scope != source.scope {
            self.remove(id)?;
            return self.add(source);
        }
        self.change(source.scope, move |list| match list.sources.iter_mut().find(|s| s.id == id) {
            Some(slot) => {
                *slot = source;
                true
            }
            None => false,
        })
    }

    /// Source `id` removed from its list; it, if it was there.
    pub fn remove(&self, id: DataSourceId) -> Result<Option<DataSource>, String> {
        let Some(source) = self.get(id) else { return Ok(None) };
        self.change(source.scope, |list| {
            let before = list.sources.len();
            list.sources.retain(|s| s.id != id);
            list.sources.len() != before
        })?;
        Ok(Some(source))
    }

    /// `edit` made to `scope`'s list (if it changes it: `true`), said and written.
    fn change(
        &self,
        scope: Scope,
        edit: impl FnOnce(&mut SourceList) -> bool,
    ) -> Result<(), String> {
        let global = {
            let mut state = self.lock();
            let slot = state.slot(scope);
            let list = match &mut slot.list {
                Listed::Ready(list) => list,
                Listed::Loading => return Err(i18n::t("datasources.still_loading")),
                Listed::Unreadable(why) => {
                    return Err(i18n::tr!("datasources.unreadable", why = why.as_str()));
                }
            };
            if !edit(list) {
                return Ok(());
            }
            let global = (scope == Scope::Global).then(|| list.clone());
            slot.version += 1;
            if !std::mem::replace(&mut slot.writing, true) {
                let (store, shared) = (self.store(), Arc::clone(&self.state));
                let events = self.events.clone();
                self.runtime.spawn(move || write(&*store, &shared, &events, scope));
            }
            global
        };
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.events.publish(&DataSourcesChanged { scope });
        if let Some(list) = global {
            let hook = self.on_global.lock().unwrap_or_else(PoisonError::into_inner).clone();
            if let Some(hook) = hook {
                hook(&list);
            }
        }
        Ok(())
    }

    /// The global list as another project left it (it wrote it): taken, said on this
    /// project's bus. Nothing is written from here.
    pub fn take_global(&self, list: &SourceList) {
        {
            let mut state = self.lock();
            let slot = state.slot(Scope::Global);
            if matches!(slot.list, Listed::Unreadable(_)) {
                return;
            }
            slot.list = Listed::Ready(list.clone());
            slot.asked = true;
        }
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.events.publish(&DataSourcesChanged { scope: Scope::Global });
    }
}

/// The name a source's database `database` is shown by (its tabs' title).
pub fn database_name(source: &str, database: &str) -> String {
    format!("{source} · {database}")
}

/// A source's color as last worked out, kept until the catalog changes (a tab asks at each
/// paint: nothing is looked up then).
#[derive(Debug, Default)]
pub struct ColorCache(Mutex<Option<(u64, Option<database::SourceColor>)>>);

impl ColorCache {
    /// The color `find` works out in `catalog` (asked again only once the catalog changed).
    pub fn get(
        &self,
        catalog: &Catalog,
        find: impl FnOnce(&Catalog) -> Option<database::SourceColor>,
    ) -> Option<database::SourceColor> {
        let generation = catalog.generation();
        let mut kept = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        match *kept {
            Some((g, color)) if g == generation => color,
            _ => {
                let color = find(catalog);
                *kept = Some((generation, color));
                color
            }
        }
    }
}

/// Writes `scope`'s list until what's written is the newest (a write job: one at a time);
/// a write failing (or working again after one failed) is said on the bus.
fn write(store: &dyn DataSourceStore, shared: &Mutex<State>, events: &EventBus, scope: Scope) {
    loop {
        let (version, list) = {
            let mut state = shared.lock().unwrap_or_else(PoisonError::into_inner);
            let slot = state.slot(scope);
            let list = match &slot.list {
                Listed::Ready(list) => list.clone(),
                _ => SourceList::default(),
            };
            (slot.version, list)
        };
        let outcome = store.save(scope, &list);
        if let Err(why) = &outcome {
            tracing::warn!(?scope, error = %why, "the data sources weren't written");
        }
        let (said, done) = {
            let mut state = shared.lock().unwrap_or_else(PoisonError::into_inner);
            let slot = state.slot(scope);
            let failed = outcome.err();
            let said = slot.failed != failed;
            slot.failed = failed;
            let done = slot.version == version;
            if done {
                slot.writing = false;
            }
            (said, done)
        };
        if said {
            events.publish(&DataSourcesChanged { scope });
        }
        if done {
            return;
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/catalog.rs"]
mod tests;
