//! The diagram's reads through a source's worker (the fake driver, the pool run by hand
//! beside the reader's thread), and its tables opened where the explorer opens them.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use database::relations::RelationNotes;
use database::{ColumnInfo, DataSource, FkAction, ForeignKey, Scope, Secret, TableKind};
use kernel::{EventBus, ProjectId};

use super::*;
use crate::fake::{FakeDb, FakeDriver, FakeTable, Manual};

fn shop() -> FakeDb {
    let mut db = FakeDb::users(2);
    let mut id = ColumnInfo::new("id", "INTEGER");
    id.in_key = true;
    let orders = Structure {
        table: TableRef::new("orders"),
        columns: vec![id, ColumnInfo::new("user_id", "INTEGER")],
        foreign_keys: vec![ForeignKey {
            name: None,
            columns: vec!["user_id".into()],
            table: TableRef::new("users"),
            references: vec!["id".into()],
            on_update: FkAction::NoAction,
            on_delete: FkAction::Cascade,
        }],
        ..Structure::default()
    };
    db.tables.push(FakeTable { structure: orders, rows: Vec::new() });
    db
}

/// Reads `access` on a thread of its own while the pool's jobs are run here.
fn read(
    access: Arc<SourceSchema>,
    manual: &Manual,
    cancel: Cancel,
) -> Result<Vec<(TableInfo, Structure)>, DbError> {
    let done = Arc::new(AtomicBool::new(false));
    let (flag, seen) = (Arc::clone(&done), Arc::new(Mutex::new(Vec::new())));
    let told = Arc::clone(&seen);
    // The reader waits as the diagram's pool would (a thread of its own: the test's pool
    // is run by hand here).
    #[allow(clippy::disallowed_methods)]
    let reader = std::thread::spawn(move || {
        let out = access.read(None, &cancel, &move |d, t| told.lock().unwrap().push((d, t)));
        flag.store(true, Ordering::SeqCst);
        out
    });
    while !done.load(Ordering::SeqCst) {
        manual.run_all();
        std::thread::sleep(Duration::from_millis(2));
    }
    let out = reader.join().unwrap();
    if out.is_ok() {
        assert_eq!(seen.lock().unwrap().last(), Some(&(2, 2)), "its progress, to the end");
    }
    out
}

fn sessions(manual: &Manual) -> Arc<Sessions> {
    Sessions::new(
        ProjectId::new(),
        Path::new("/projects/shop"),
        EventBus::new(),
        manual.runtime(),
        None,
    )
}

#[test]
fn every_tables_structure_is_read_through_the_sources_worker() {
    let manual = Manual::new();
    let sessions = sessions(&manual);
    let driver = FakeDriver::new(shop(), true);
    let source = DataSource::new("shop", "fake", Scope::Project);
    let worker = sessions.open(&source, Arc::new(driver.clone()), Secret::default());
    let access = Arc::new(SourceSchema::new(&sessions, source.id, "shop".into(), worker, None));
    let tables = read(Arc::clone(&access), &manual, Cancel::default()).unwrap();
    let names: Vec<&str> = tables.iter().map(|(t, _)| t.table.name.as_str()).collect();
    assert_eq!(names, ["users", "orders"]);
    assert!(tables.iter().all(|(t, _)| t.kind == TableKind::Table));
    let graph = database::diagram::SchemaGraph::new(tables);
    assert_eq!(graph.label(&graph.links[0]), "orders.user_id → users.id");
    assert_eq!(driver.opened(), 1, "one session, as any reader's");
}

#[test]
fn a_read_cancelled_stops() {
    let manual = Manual::new();
    let sessions = sessions(&manual);
    let driver = FakeDriver::new(shop(), true);
    let source = DataSource::new("shop", "fake", Scope::Project);
    let worker = sessions.open(&source, Arc::new(driver), Secret::default());
    let access = Arc::new(SourceSchema::new(&sessions, source.id, "shop".into(), worker, None));
    let cancel = Cancel::default();
    cancel.cancel();
    assert_eq!(read(access, &manual, cancel), Err(DbError::Cancelled));
}

#[test]
fn a_table_opens_in_the_files_view_or_the_sources() {
    let manual = Manual::new();
    let sessions = sessions(&manual);
    let driver = FakeDriver::new(shop(), true);
    let source = DataSource::new("shop", "fake", Scope::Project);
    let worker = sessions.open(&source, Arc::new(driver), Secret::default());
    let orders = TableRef::new("orders");
    let file = PathBuf::from("/projects/shop/shop.db");
    let access = SourceSchema::new(
        &sessions,
        source.id,
        "shop".into(),
        worker.clone(),
        Some((file.clone(), None)),
    );
    assert_eq!(
        access.open(&orders),
        Some(Opening::File(file, crate::view::rows_of(orders.clone(), None)))
    );
    let access = SourceSchema::new(&sessions, source.id, "shop".into(), worker, None);
    let Some(Opening::View(view)) = access.open(&orders) else { panic!("a view") };
    let open = view.data.downcast_ref::<crate::view::OpenSource>().unwrap();
    assert_eq!((open.source, open.table.clone()), (source.id, Some(orders)));
}

/// A file's diagram follows the file renamed or moved (said on the project's bus): its
/// tables open in the new file's view, its name the new file's.
#[test]
fn a_file_s_diagram_follows_its_rename() {
    let manual = Manual::new();
    let sessions = sessions(&manual);
    let driver = FakeDriver::new(shop(), true);
    let source = DataSource::new("shop.db", "fake", Scope::Project);
    let worker = sessions.open(&source, Arc::new(driver), Secret::default());
    let file = PathBuf::from("/projects/shop/shop.db");
    let access = SourceSchema::new(
        &sessions,
        source.id,
        "shop.db".into(),
        worker,
        Some((file.clone(), None)),
    );
    let to = PathBuf::from("/projects/shop/data/orders.db");
    sessions.events().publish(&workspace::events::FileRenamed { from: file, to: to.clone() });
    assert_eq!(access.name(), "orders.db");
    let orders = TableRef::new("orders");
    assert_eq!(
        access.open(&orders),
        Some(Opening::File(to, crate::view::rows_of(orders.clone(), None)))
    );
}

/// The catalog read (an empty store): decisions can be kept.
fn loaded(sessions: &Arc<Sessions>, manual: &Manual) {
    sessions.catalog().ensure_loaded();
    while sessions.catalog().list(Scope::Project) == crate::catalog::Listed::Loading {
        manual.run_all();
    }
}

fn accepted() -> RelationNotes {
    let mut notes = RelationNotes::default();
    notes.accept(database::relations::VirtualKey {
        table: TableRef::new("reviews"),
        columns: vec!["user_id".into()],
        target: TableRef::new("users"),
        references: vec!["id".into()],
    });
    notes
}

#[test]
fn decisions_are_kept_with_a_configured_source() {
    let manual = Manual::new();
    let sessions = sessions(&manual);
    loaded(&sessions, &manual);
    let source = DataSource::new("shop", "fake", Scope::Project);
    sessions.catalog().add(source.clone()).unwrap();
    let worker = sessions.open(&source, Arc::new(FakeDriver::new(shop(), true)), Secret::default());
    let access = SourceSchema::new(&sessions, source.id, "shop".into(), worker, None);
    assert_eq!(access.notes(), RelationNotes::default());
    access.set_notes(accepted());
    assert_eq!(sessions.catalog().get(source.id).unwrap().relations, accepted());
    assert_eq!(access.notes(), accepted());
}

#[test]
fn a_file_shown_by_itself_becomes_a_source_once_a_relation_is_decided() {
    let manual = Manual::new();
    let sessions = sessions(&manual);
    loaded(&sessions, &manual);
    let driver = FakeDriver::new(shop(), true);
    sessions.set_drivers(Arc::new(move |id| {
        (id == "fake-file").then(|| Arc::new(driver.clone()) as Arc<dyn database::Driver>)
    }));
    let file = PathBuf::from("/projects/shop/data/shop.db");
    let (worker, lease) = sessions.open_file(&file, "fake-file", &[]).unwrap();
    let id = worker.source().id;
    let access = SourceSchema::new(
        &sessions,
        id,
        "shop.db".into(),
        worker,
        Some((file.clone(), Some(lease))),
    );
    // Only which relations are shown: kept in memory, nothing added.
    let shown = RelationNotes {
        show: database::relations::RelationKinds::Real,
        ..RelationNotes::default()
    };
    access.set_notes(shown.clone());
    assert!(sessions.catalog().all().is_empty());
    assert_eq!(access.notes(), shown);
    // A key accepted: the file is the project's source now (its session's id, its file
    // relative to the project), the decision with it.
    access.set_notes(accepted());
    let kept = sessions.catalog().get(id).expect("kept as a source");
    assert_eq!(kept.file.as_deref(), Some(Path::new("data/shop.db")));
    assert_eq!((kept.driver.as_str(), kept.relations.clone()), ("fake-file", accepted()));
    assert_eq!(configured(&sessions, DataSourceId::new(), Some(&file)).map(|s| s.id), Some(id));
}
