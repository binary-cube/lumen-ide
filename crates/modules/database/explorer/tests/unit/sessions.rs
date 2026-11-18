use database::Scope;

use super::*;
use crate::fake::{FakeDb, FakeDriver, Manual, counted_wake};
use crate::worker::Request;

const IDLE: Duration = Duration::from_secs(600);

fn project(manual: &Manual) -> Arc<Sessions> {
    Sessions::new(
        ProjectId::new(),
        Path::new("/projects/shop"),
        EventBus::new(),
        manual.runtime(),
        None,
    )
}

fn tables() -> Request {
    Request::Tables(Vec::new())
}

#[test]
fn a_source_has_one_worker_and_no_session_until_asked() {
    let manual = Manual::new();
    let sessions = project(&manual);
    let driver = FakeDriver::new(FakeDb::users(3), true);
    let source = DataSource::new("shop", "fake", Scope::Project);
    let one = sessions.open(&source, Arc::new(driver.clone()), Secret::default());
    let again = sessions.open(&source, Arc::new(driver.clone()), Secret::default());
    assert_eq!(sessions.len(), 1);
    assert_eq!(one.source().id, again.source().id);
    assert_eq!((sessions.connected(), driver.opened()), (0, 0));
    let (wake, _) = counted_wake();
    // Kept: a client dropped takes what it asked with it.
    let client = one.client(wake);
    client.send(tables());
    manual.run_all();
    assert_eq!((sessions.connected(), driver.opened()), (1, 1));
    assert!(sessions.get(source.id).is_some());
    sessions.close(source.id);
    // Its sessions close on the pool (never on the caller's thread).
    manual.run_all();
    assert_eq!((sessions.len(), driver.live()), (0, 0));
}

#[test]
fn a_file_source_opens_its_file_from_the_projects_root() {
    let manual = Manual::new();
    let sessions = project(&manual);
    let seen = Arc::new(Mutex::new(String::new()));
    #[derive(Clone)]
    struct Sees(Arc<Mutex<String>>, FakeDriver);
    impl Driver for Sees {
        fn kind(&self) -> database::Kind {
            database::Kind::Relational
        }
        fn language(&self) -> database::Language {
            database::Language::Sql
        }
        fn connect(
            &self,
            target: &database::Target,
            secret: &Secret,
        ) -> Result<Box<dyn database::Session>, database::DbError> {
            *self.0.lock().unwrap() = target.database.clone();
            self.1.connect(target, secret)
        }
    }
    let mut source = DataSource::new("data", "fake", Scope::Project);
    source.file = Some("db/app.sqlite".into());
    let driver = Sees(Arc::clone(&seen), FakeDriver::new(FakeDb::users(1), true));
    let worker = sessions.open(&source, Arc::new(driver), Secret::default());
    let (wake, _) = counted_wake();
    let client = worker.client(wake);
    client.send(tables());
    manual.run_all();
    let file = std::path::PathBuf::from(&*seen.lock().unwrap());
    assert_eq!(file, Path::new("/projects/shop").join("db/app.sqlite"));
}

#[test]
fn an_unused_session_is_closed_after_the_idle_time_and_opens_again_when_asked() {
    let manual = Manual::new();
    let sessions = project(&manual);
    let driver = FakeDriver::new(FakeDb::users(3), true);
    let worker = sessions.attach(driver.source(DataSourceId::new()));
    let (wake, _) = counted_wake();
    let client = worker.client(wake);
    client.send(tables());
    manual.run_all();
    assert_eq!(sessions.next_close(manual.now(), IDLE), Some(manual.now() + IDLE));
    manual.advance(Duration::from_secs(300));
    assert_eq!(sessions.sweep(manual.now(), IDLE), 0);
    // Used again: its time starts again.
    client.send(tables());
    manual.run_all();
    manual.advance(Duration::from_secs(400));
    assert_eq!(sessions.sweep(manual.now(), IDLE), 0);
    manual.advance(Duration::from_secs(200));
    assert_eq!(sessions.sweep(manual.now(), IDLE), 1);
    // Its sessions close on the pool (never on the caller's thread).
    manual.run_all();
    assert_eq!((driver.live(), sessions.connected(), sessions.len()), (0, 0, 1));
    assert_eq!(sessions.next_close(manual.now(), IDLE), None, "nothing open");
    client.send(tables());
    manual.run_all();
    assert_eq!((driver.opened(), driver.live()), (2, 1));
    assert!(client.poll().iter().all(|r| r.result.is_ok()));
}

#[test]
fn a_session_at_work_isnt_idle() {
    let manual = Manual::new();
    let sessions = project(&manual);
    let driver = FakeDriver::new(FakeDb::users(3), true);
    let worker = sessions.attach(driver.source(DataSourceId::new()));
    let (wake, _) = counted_wake();
    let client = worker.client(wake);
    client.send(tables());
    manual.run_all();
    client.send(tables());
    manual.advance(IDLE * 2);
    assert_eq!(sessions.sweep(manual.now(), IDLE), 0, "a request waits");
    assert_eq!(sessions.next_close(manual.now(), IDLE), Some(manual.now() + IDLE));
    manual.run_all();
    assert_eq!(driver.live(), 1);
    assert!(sessions.busy() == worker.busy());
}

#[test]
fn a_closed_project_closes_its_sessions_and_leaves_the_others_alone() {
    let manual = Manual::new();
    let (shop, blog) = (project(&manual), project(&manual));
    let (shop_id, blog_id) = (shop.project(), blog.project());
    let (shop_driver, blog_driver) =
        (FakeDriver::new(FakeDb::users(3), true), FakeDriver::new(FakeDb::users(3), true));
    // The same source in both: each project has its own session of it.
    let id = DataSourceId::new();
    let shop_guard = register(Arc::clone(&shop));
    let blog_guard = register(Arc::clone(&blog));
    let (wake, _) = counted_wake();
    let shop_client = shop.attach(shop_driver.source(id)).client(wake.clone());
    let blog_client = blog.attach(blog_driver.source(id)).client(wake);
    shop_client.send(tables());
    blog_client.send(tables());
    manual.run_all();
    let waiting = shop_client.send(tables());
    assert_eq!((shop_driver.live(), blog_driver.live()), (1, 1));
    assert!(sessions(shop_id).is_some() && sessions(blog_id).is_some());
    drop(shop);

    drop(shop_guard);
    assert!(sessions(shop_id).is_none());
    // Its sessions close on the pool (never on the caller's thread).
    manual.run_all();
    assert_eq!((shop_driver.live(), blog_driver.live()), (0, 1));
    assert!(
        shop_client
            .poll()
            .iter()
            .any(|r| r.ticket == waiting && r.result == Err(database::DbError::Cancelled))
    );
    assert_eq!(sessions(blog_id).map(|s| s.connected()), Some(1));
    drop(blog_guard);
    manual.run_all();
    assert_eq!(blog_driver.live(), 0);
}

#[test]
fn a_project_opening_gets_an_empty_table_of_sources() {
    let events = EventBus::new();
    let id = ProjectId::new();
    let root = Path::new("/projects/empty");
    let context = ProjectContext {
        marks: Default::default(),
        id,
        root,
        storage: root,
        events: &events,
        wake: Arc::new(|| {}),
        settings: None,
        language_of: Arc::new(|_| None),
        quality_tools: Vec::new(),
        text_formatters: Vec::new(),
        notifier: std::sync::Arc::new(workbench_model::NoNotifier),
    };
    let guards = SessionsService.attach(&context);
    let sessions = sessions(id).expect("registered");
    assert!(sessions.is_empty());
    assert!(!SessionsService.busy(id));
    drop(guards);
    assert!(super::sessions(id).is_none());
}

#[test]
fn a_read_only_file_source_is_opened_read_only_from_the_projects_folder() {
    let manual = Manual::new();
    let sessions = project(&manual);
    let driver = FakeDriver::new(FakeDb::users(1), true);
    let mut source = DataSource::new("shop", "fake", Scope::Project);
    source.file = Some("data/shop.db".into());
    source.read_only = true;
    let worker = sessions.open(&source, Arc::new(driver.clone()), Secret::default());
    let (wake, _) = counted_wake();
    let client = worker.client(wake);
    client.send(tables());
    manual.run_all();
    let targets = driver.db.lock().unwrap().targets.clone();
    assert_eq!(targets.len(), 1);
    assert!(targets[0].read_only(), "the driver is told");
    assert_eq!(
        Path::new(&targets[0].database),
        Path::new("/projects/shop").join("data/shop.db").as_path()
    );
}

#[test]
fn a_source_whose_driver_isnt_registered_isnt_opened() {
    let manual = Manual::new();
    let sessions = project(&manual);
    let source = DataSource::new("shop", "no-such-driver", Scope::Project);
    let opened = sessions.open_source(&source, Secret::default());
    assert!(matches!(opened, Err(database::DbError::Unsupported(_))));
    assert!(sessions.is_empty());
}

/// A store in memory, counting its writes.
#[derive(Default)]
struct MemoryLayouts {
    saved: Mutex<Vec<(database::layouts::LayoutKey, database::layouts::GridLayout)>>,
    writes: std::sync::atomic::AtomicUsize,
}

impl database::layouts::LayoutStore for MemoryLayouts {
    fn load(&self) -> Vec<(database::layouts::LayoutKey, database::layouts::GridLayout)> {
        self.saved.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
    fn save(
        &self,
        layouts: &[(database::layouts::LayoutKey, database::layouts::GridLayout)],
    ) -> std::io::Result<()> {
        self.writes.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        *self.saved.lock().unwrap_or_else(PoisonError::into_inner) = layouts.to_vec();
        Ok(())
    }
}

#[test]
fn layouts_are_written_once_changes_stop_and_read_by_the_next_opening() {
    use database::layouts::GridLayout;
    let manual = Manual::new();
    let store = Arc::new(MemoryLayouts::default());
    let sessions = project(&manual);
    sessions.set_layout_store(store.clone(), Duration::ZERO);
    let file = Path::new("/projects/shop/data/shop.sqlite");
    let table = database::TableRef::new("orders");
    let wide = |w: f32| GridLayout { widths: vec![("id".into(), w)], ..GridLayout::default() };
    sessions.keep_layout(file, &table, wide(80.0));
    sessions.keep_layout(file, &table, wide(90.0));
    // The same again: nothing to write.
    sessions.keep_layout(file, &table, wide(90.0));
    assert_eq!(manual.run_all(), 1, "one write job for the changes in a row");
    let writes = || store.writes.load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(writes(), 1);
    assert_eq!(sessions.layout(file, &table), Some(wide(90.0)));
    // Named relative to the project.
    let saved = store.load();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].0.source, "data/shop.sqlite");
    // The project opened again: its layouts come back.
    let again = project(&manual);
    again.set_layout_store(store.clone(), Duration::ZERO);
    assert_eq!(again.layout(file, &table), Some(wide(90.0)));
    // A file outside the project: kept while it's open, never written.
    let outside = Path::new("/elsewhere/a.csv");
    again.keep_layout(outside, &table, wide(70.0));
    assert_eq!(manual.run_all(), 0);
    assert_eq!(again.layout(outside, &table), Some(wide(70.0)));
    assert_eq!(writes(), 1);
}

#[test]
fn a_file_configured_as_a_source_is_opened_as_that_source() {
    let manual = Manual::new();
    let sessions = project(&manual);
    let driver = FakeDriver::new(FakeDb::users(1), true);
    let d = driver;
    sessions.set_drivers(Arc::new(move |id| {
        (id == "fake").then(|| Arc::new(d.clone()) as Arc<dyn Driver>)
    }));
    sessions.catalog().ensure_loaded();
    manual.run_all();
    let mut source = DataSource::new("shop", "fake", Scope::Project);
    source.file = Some(PathBuf::from("data/shop.db"));
    source.read_only = true;
    sessions.catalog().add(source.clone()).unwrap();
    let path = Path::new("/projects/shop/data/shop.db");
    let (worker, lease) = sessions.open_file(path, "fake", &[]).unwrap();
    assert_eq!(worker.source().id, source.id, "its id: the tree and the view share it");
    assert!(worker.source().read_only);
    assert_eq!(sessions.open_files(), [(path.to_path_buf(), source.id, "fake".to_owned())]);
    // The window's worker for it is the same.
    assert_eq!(sessions.source_worker(source.id).unwrap().source().id, source.id);
    assert_eq!(sessions.len(), 1);
    drop(lease);
    assert!(sessions.open_files().is_empty());
}

/// A project in a folder of its own, its `data/shop.db` there, and `linked` a link to
/// `data`.
#[cfg(unix)]
fn linked_project(manual: &Manual, name: &str) -> (Arc<Sessions>, PathBuf) {
    let dir = std::env::temp_dir().join(format!("sessions-link-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("data")).unwrap();
    std::fs::write(dir.join("data/shop.db"), b"").unwrap();
    std::os::unix::fs::symlink(dir.join("data"), dir.join("linked")).unwrap();
    let sessions = Sessions::new(ProjectId::new(), &dir, EventBus::new(), manual.runtime(), None);
    let driver = FakeDriver::new(FakeDb::users(1), true);
    sessions.set_drivers(Arc::new(move |id| {
        (id == "fake").then(|| Arc::new(driver.clone()) as Arc<dyn Driver>)
    }));
    sessions.catalog().ensure_loaded();
    manual.run_all();
    (sessions, dir)
}

#[cfg(unix)]
#[test]
fn a_file_opened_through_a_link_is_its_configured_source() {
    let manual = Manual::new();
    let (sessions, dir) = linked_project(&manual, "configured");
    let mut source = DataSource::new("shop", "fake", Scope::Project);
    source.file = Some(PathBuf::from("data/shop.db"));
    sessions.catalog().add(source.clone()).unwrap();
    let linked = dir.join("linked/shop.db");
    let found = sessions.catalog().source_of_file(sessions.root(), &linked);
    assert_eq!(found.map(|s| s.id), Some(source.id), "found by the link's path");
    let (worker, lease) = sessions.open_file(&linked, "fake", &[]).unwrap();
    assert_eq!(worker.source().id, source.id, "the configured source, not a new one");
    // Opened again by its own path: the same file, one entry, two users.
    let (_, second) = sessions.open_file(&dir.join("data/shop.db"), "fake", &[]).unwrap();
    assert_eq!(sessions.open_files(), [(linked.clone(), source.id, "fake".to_owned())]);
    assert_eq!(sessions.file_users(&dir.join("data/./shop.db")), 2);
    assert_eq!(sessions.source_worker(source.id).unwrap().source().id, source.id);
    assert_eq!(sessions.len(), 1);
    drop(lease);
    assert_eq!(sessions.file_users(&linked), 1);
    drop(second);
    assert!(sessions.open_files().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A driver telling which file each session opened.
#[derive(Clone)]
struct Opens(Arc<Mutex<Vec<String>>>, FakeDriver);

impl Driver for Opens {
    fn kind(&self) -> database::Kind {
        database::Kind::Relational
    }
    fn language(&self) -> database::Language {
        database::Language::Sql
    }
    fn connect(
        &self,
        target: &database::Target,
        secret: &Secret,
    ) -> Result<Box<dyn database::Session>, database::DbError> {
        self.0.lock().unwrap().push(target.database.clone());
        self.1.connect(target, secret)
    }
}

/// A file renamed or moved (said on the bus): its session opens again at the new path, the
/// file shown by itself is known by it (its lease still lets it go), its grids' layouts are
/// kept under it; a configured source's file follows (its name too, when it was the file's).
#[test]
fn a_renamed_file_is_followed() {
    use database::layouts::GridLayout;
    let manual = Manual::new();
    let sessions = project(&manual);
    sessions.set_layout_store(Arc::new(MemoryLayouts::default()), Duration::ZERO);
    let opened: Arc<Mutex<Vec<String>>> = Arc::default();
    let driver = Opens(Arc::clone(&opened), FakeDriver::new(FakeDb::users(1), true));
    sessions.set_drivers(Arc::new(move |id| {
        (id == "fake").then(|| Arc::new(driver.clone()) as Arc<dyn Driver>)
    }));
    sessions.catalog().ensure_loaded();
    manual.run_all();
    let root = Path::new("/projects/shop");
    let (from, to) = (root.join("data/a.sqlite"), root.join("data/b.sqlite"));
    let table = database::TableRef::new("orders");
    let wide = GridLayout { widths: vec![("id".into(), 90.0)], ..GridLayout::default() };
    sessions.keep_layout(&from, &table, wide.clone());
    let (worker, lease) = sessions.open_file(&from, "fake", &[]).unwrap();
    let (wake, _) = counted_wake();
    let client = worker.client(wake);
    client.send(tables());
    manual.run_all();
    sessions.file_moved(&from, &to);
    assert_eq!(sessions.open_files(), [(to.clone(), worker.source().id, "fake".to_owned())]);
    assert_eq!(sessions.file_users(&to), 1);
    assert_eq!(sessions.layout(&to, &table), Some(wide));
    client.send(tables());
    manual.run_all();
    let paths: Vec<String> = opened.lock().unwrap().clone();
    assert_eq!(paths, [from.to_string_lossy().into_owned(), to.to_string_lossy().into_owned()]);
    drop(lease);
    assert!(sessions.open_files().is_empty(), "its lease let it go");
    // Configured: its file (relative, as kept) and the name that was the file's.
    let mut named = DataSource::new("b.sqlite", "fake", Scope::Project);
    named.file = Some(PathBuf::from("data/b.sqlite"));
    let mut own = DataSource::new("Orders", "fake", Scope::Project);
    own.file = Some(PathBuf::from("data/b.sqlite"));
    sessions.catalog().add(named.clone()).unwrap();
    sessions.catalog().add(own.clone()).unwrap();
    sessions.file_moved(&root.join("data"), &root.join("kept"));
    let named = sessions.catalog().get(named.id).unwrap();
    let own = sessions.catalog().get(own.id).unwrap();
    assert_eq!(
        (named.name.as_str(), named.file),
        ("b.sqlite", Some(PathBuf::from("kept/b.sqlite")))
    );
    assert_eq!((own.name.as_str(), own.file), ("Orders", Some(PathBuf::from("kept/b.sqlite"))));
    sessions.file_moved(&root.join("kept/b.sqlite"), &root.join("kept/c.sqlite"));
    assert_eq!(sessions.catalog().get(named.id).unwrap().name, "c.sqlite");
    assert_eq!(sessions.catalog().get(own.id).unwrap().name, "Orders");
}
