//! The configured sources: read once when asked (in the background), changed in memory at
//! once and said on the bus, written in the background (the newest last), a list that
//! can't be read never written over, the global list reaching the other projects.

use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::fake::Manual;

/// A store in memory, counting its writes (and failing them when told).
struct Memory {
    project: Mutex<Result<SourceList, String>>,
    global: Mutex<Result<SourceList, String>>,
    writes: AtomicUsize,
    fail: Mutex<Option<String>>,
}

impl Memory {
    fn with(project: Result<Vec<DataSource>, String>) -> Arc<Self> {
        Arc::new(Self {
            project: Mutex::new(project.map(SourceList::of)),
            global: Mutex::new(Ok(SourceList::default())),
            writes: AtomicUsize::new(0),
            fail: Mutex::new(None),
        })
    }

    fn slot(&self, scope: Scope) -> &Mutex<Result<SourceList, String>> {
        match scope {
            Scope::Project => &self.project,
            Scope::Global => &self.global,
        }
    }
}

impl DataSourceStore for Memory {
    fn load(&self, scope: Scope) -> Result<SourceList, String> {
        self.slot(scope).lock().unwrap().clone()
    }

    fn save(&self, scope: Scope, list: &SourceList) -> Result<(), String> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        if let Some(why) = self.fail.lock().unwrap().clone() {
            return Err(why);
        }
        *self.slot(scope).lock().unwrap() = Ok(list.clone());
        Ok(())
    }
}

fn said(events: &EventBus) -> (Arc<Mutex<Vec<Scope>>>, kernel::Subscription) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s = Arc::clone(&seen);
    let sub = events.subscribe(move |e: &DataSourcesChanged| s.lock().unwrap().push(e.scope));
    (seen, sub)
}

#[test]
fn nothing_is_read_until_asked_then_both_lists_once() {
    let manual = Manual::new();
    let shop = DataSource::new("shop", "sqlite", Scope::Project);
    let store = Memory::with(Ok(vec![shop.clone()]));
    let events = EventBus::new();
    let (seen, _sub) = said(&events);
    let catalog = Catalog::new(store, manual.runtime(), events);
    assert_eq!(manual.jobs(), 0, "nothing read yet");
    assert_eq!(catalog.list(Scope::Project), Listed::Loading);
    catalog.ensure_loaded();
    catalog.ensure_loaded();
    assert_eq!(manual.run_all(), 2, "each list read once");
    assert_eq!(catalog.list(Scope::Project), Listed::Ready(SourceList::of(vec![shop.clone()])));
    assert_eq!(catalog.list(Scope::Global), Listed::Ready(SourceList::default()));
    assert_eq!(*seen.lock().unwrap(), [Scope::Project, Scope::Global]);
    assert_eq!(catalog.get(shop.id), Some(shop));
}

#[test]
fn a_change_is_said_at_once_and_written_in_the_background_newest_last() {
    let manual = Manual::new();
    let store = Memory::with(Ok(Vec::new()));
    let events = EventBus::new();
    let (seen, _sub) = said(&events);
    let catalog =
        Catalog::new(Arc::clone(&store) as Arc<dyn DataSourceStore>, manual.runtime(), events);
    catalog.ensure_loaded();
    manual.run_all();
    seen.lock().unwrap().clear();
    let (a, b) = (
        DataSource::new("a", "sqlite", Scope::Project),
        DataSource::new("b", "csv", Scope::Project),
    );
    catalog.add(a.clone()).unwrap();
    catalog.add(b.clone()).unwrap();
    assert_eq!(catalog.all(), [a.clone(), b.clone()], "in memory at once");
    assert_eq!(*seen.lock().unwrap(), [Scope::Project, Scope::Project]);
    assert_eq!(manual.jobs(), 1, "one write job for both");
    assert_eq!(catalog.remove(a.id), Ok(Some(a.clone())));
    manual.run_all();
    assert_eq!(
        store.load(Scope::Project).map(|l| l.sources),
        Ok(vec![b]),
        "the newest list written"
    );
    assert!(store.writes.load(Ordering::SeqCst) <= 2);
    assert_eq!(catalog.remove(a.id), Ok(None), "gone already: nothing");
    // A write failing is told (and kept in memory): the next that works clears it.
    *store.fail.lock().unwrap() = Some("disk full".into());
    catalog.add(a.clone()).unwrap();
    manual.run_all();
    assert_eq!(catalog.write_error(Scope::Project).as_deref(), Some("disk full"));
    assert_eq!(catalog.all().len(), 2);
    *store.fail.lock().unwrap() = None;
    catalog.remove(a.id).unwrap();
    manual.run_all();
    assert_eq!(catalog.write_error(Scope::Project), None);
}

#[test]
fn a_list_that_cant_be_read_is_never_written_over() {
    let manual = Manual::new();
    let store = Memory::with(Err("expected `=`".into()));
    let catalog = Catalog::new(
        Arc::clone(&store) as Arc<dyn DataSourceStore>,
        manual.runtime(),
        EventBus::new(),
    );
    let early = catalog.add(DataSource::new("x", "sqlite", Scope::Project));
    assert!(early.is_err(), "still loading: refused, not lost");
    catalog.ensure_loaded();
    manual.run_all();
    assert_eq!(catalog.list(Scope::Project), Listed::Unreadable("expected `=`".into()));
    assert!(catalog.add(DataSource::new("x", "sqlite", Scope::Project)).is_err());
    manual.run_all();
    assert_eq!(store.writes.load(Ordering::SeqCst), 0);
    // The global list still works.
    assert!(catalog.add(DataSource::new("g", "sqlite", Scope::Global)).is_ok());
}

#[test]
fn the_global_list_reaches_the_other_projects() {
    let manual = Manual::new();
    let store: Arc<dyn DataSourceStore> = Memory::with(Ok(Vec::new()));
    let (one_bus, other_bus) = (EventBus::new(), EventBus::new());
    let (heard, _sub) = said(&other_bus);
    let one = Arc::new(Catalog::new(Arc::clone(&store), manual.runtime(), one_bus));
    let other = Arc::new(Catalog::new(store, manual.runtime(), other_bus));
    let to = Arc::clone(&other);
    one.on_global(Arc::new(move |list| to.take_global(list)));
    one.ensure_loaded();
    manual.run_all();
    let global = DataSource::new("prod", "postgres", Scope::Global);
    one.add(global.clone()).unwrap();
    assert_eq!(other.list(Scope::Global), Listed::Ready(SourceList::of(vec![global])));
    assert_eq!(*heard.lock().unwrap(), [Scope::Global]);
    // The project's own never goes there.
    one.add(DataSource::new("mine", "sqlite", Scope::Project)).unwrap();
    assert_eq!(other.list(Scope::Project), Listed::Loading);
}

#[test]
fn folders_are_kept_with_their_list_and_taking_one_away_keeps_its_sources() {
    let manual = Manual::new();
    let store = Memory::with(Ok(Vec::new()));
    let events = EventBus::new();
    let (seen, _sub) = said(&events);
    let catalog =
        Catalog::new(Arc::clone(&store) as Arc<dyn DataSourceStore>, manual.runtime(), events);
    catalog.ensure_loaded();
    manual.run_all();
    seen.lock().unwrap().clear();
    let before = catalog.generation();
    let mut prod = SourceFolder::new("prod", Scope::Project);
    prod.color = Some(database::SourceColor::Red);
    let mut eu = SourceFolder::new("eu", Scope::Project);
    eu.parent = Some(prod.id);
    catalog.put_folder(prod.clone()).unwrap();
    catalog.put_folder(eu.clone()).unwrap();
    let mut shop = DataSource::new("shop", "postgres", Scope::Project);
    shop.folder = Some(eu.id);
    catalog.add(shop.clone()).unwrap();
    assert_eq!(*seen.lock().unwrap(), [Scope::Project; 3], "each change said");
    assert!(catalog.generation() > before, "what's shown from it is worked out again");
    assert_eq!(catalog.color_of(shop.id), Some(database::SourceColor::Red), "the folder's");
    // Recolored, renamed in place; never moved into itself.
    let mut renamed = prod.clone();
    renamed.name = "production".into();
    renamed.color = Some(database::SourceColor::Orange);
    catalog.put_folder(renamed).unwrap();
    let mut looped = prod.clone();
    looped.parent = Some(eu.id);
    catalog.put_folder(looped).unwrap();
    assert_eq!(catalog.folders().len(), 2);
    assert_eq!(catalog.folders()[0].parent, None, "no loop");
    assert_eq!(catalog.color_of(shop.id), Some(database::SourceColor::Orange));
    manual.run_all();
    let written = store.load(Scope::Project).unwrap();
    assert_eq!(written.folders.len(), 2, "written with the sources");
    assert_eq!(written.folders[0].name, "production");
    // Taken away: what it held moves up, the source stays.
    catalog.remove_folder(Scope::Project, eu.id).unwrap();
    assert_eq!(catalog.get(shop.id).unwrap().folder, Some(prod.id));
    catalog.remove_folder(Scope::Project, prod.id).unwrap();
    assert!(catalog.folders().is_empty());
    assert_eq!(catalog.get(shop.id).unwrap().folder, None);
    assert_eq!(catalog.color_of(shop.id), None);
    // The modal's Apply: the folders set whole.
    catalog.set_folders(Scope::Project, vec![prod.clone()]).unwrap();
    assert_eq!(catalog.folders(), [prod]);
    manual.run_all();
    assert_eq!(store.load(Scope::Project).unwrap().sources, [catalog.get(shop.id).unwrap()]);
}

#[test]
fn a_color_is_worked_out_once_until_the_catalog_changes() {
    let manual = Manual::new();
    let catalog = Catalog::new(Memory::with(Ok(Vec::new())), manual.runtime(), EventBus::new());
    catalog.ensure_loaded();
    manual.run_all();
    let cache = ColorCache::default();
    let asked = AtomicUsize::new(0);
    let find = |_: &Catalog| {
        asked.fetch_add(1, Ordering::SeqCst);
        Some(database::SourceColor::Green)
    };
    assert_eq!(cache.get(&catalog, find), Some(database::SourceColor::Green));
    assert_eq!(cache.get(&catalog, find), Some(database::SourceColor::Green));
    assert_eq!(asked.load(Ordering::SeqCst), 1, "kept");
    catalog.add(DataSource::new("x", "sqlite", Scope::Project)).unwrap();
    cache.get(&catalog, find);
    assert_eq!(asked.load(Ordering::SeqCst), 2, "the catalog changed");
}

/// A server's database is a source of its own while the project is open: found by its
/// identity (the same for the same pair), as its source opening it; its color and
/// production mark its source's; what it decides (its relations) kept by its source.
#[test]
fn a_servers_database_is_its_source_on_that_database() {
    let manual = Manual::new();
    let mut pg = DataSource::new("pg", "postgres", Scope::Project);
    pg.target.database = "postgres".into();
    pg.production = true;
    pg.color = Some(database::SourceColor::Red);
    let store = Memory::with(Ok(vec![pg.clone()]));
    let catalog = Catalog::new(store, manual.runtime(), EventBus::new());
    catalog.ensure_loaded();
    manual.run_all();
    let shop = pg.id.of_database("shop");
    assert_eq!(catalog.get(shop), None, "not one until it's seen");
    assert_eq!(catalog.database_id(pg.id, "shop"), shop);
    let found = catalog.get(shop).unwrap();
    assert_eq!(
        (found.id, found.name.as_str(), found.target.database.as_str()),
        (shop, "pg · shop", "shop")
    );
    assert_eq!(catalog.database_of(shop), Some((pg.id, "shop".to_owned())));
    assert_eq!((catalog.base_id(shop), catalog.base_id(pg.id)), (pg.id, pg.id));
    assert!(catalog.production_of(shop));
    assert_eq!(catalog.color_of(shop), Some(database::SourceColor::Red));
    // Never listed, never saved as a source.
    assert_eq!(catalog.all().len(), 1);
    let mut decided = found;
    decided.relations.show = database::relations::RelationKinds::Real;
    catalog.update(decided).unwrap();
    manual.run_all();
    assert_eq!(catalog.all().len(), 1);
    let kept = catalog.get(pg.id).unwrap().relations.show;
    assert_eq!(kept, database::relations::RelationKinds::Real);
    assert_eq!(catalog.get(pg.id).unwrap().name, "pg");
}
