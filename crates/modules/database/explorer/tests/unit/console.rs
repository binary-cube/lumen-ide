//! The consoles' port over a project's sources: a SQLite source as a console sees it (its
//! dialect, its flavor, its color), a CSV one without SQL; a console's own connection made
//! as the explorer makes its (the file found from the project, read-only kept); the window's
//! selection; a removed source gone.

use super::*;
use crate::fake::Manual;
use database::{Cancel, Collect, Driver, Scope, Secret, SecretPolicy, Target, Value};
use kernel::{EventBus, ProjectId};

struct Setup {
    root: std::path::PathBuf,
    sessions: Arc<Sessions>,
    access: ExplorerConsoles,
    manual: Manual,
}

impl Drop for Setup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn setup(name: &str) -> Setup {
    let root = std::env::temp_dir().join(format!("explorer-console-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let mut target = Target {
        database: root.join("shop.db").to_string_lossy().into_owned(),
        ..Target::default()
    };
    target.options.insert("create".into(), "true".into());
    let mut session = db_sqlite::SqliteDriver.connect(&target, &Secret::default()).unwrap();
    session
        .execute("CREATE TABLE t (a INTEGER)", &mut Collect::default(), &Cancel::default())
        .unwrap();
    std::fs::write(root.join("people.csv"), "id,name\n1,ana\n").unwrap();
    let manual = Manual::new();
    let sessions = Sessions::new(ProjectId::new(), &root, EventBus::new(), manual.runtime(), None);
    sessions.set_drivers(Arc::new(|id| -> Option<Arc<dyn Driver>> {
        match id {
            "sqlite" => Some(Arc::new(db_sqlite::SqliteDriver)),
            "csv" => Some(Arc::new(db_csv::CsvDriver)),
            _ => None,
        }
    }));
    sessions.catalog().ensure_loaded();
    manual.run_all();
    let access = ExplorerConsoles::new(&sessions);
    Setup { root, sessions, access, manual }
}

fn file_source(name: &str, driver: &str, file: &str) -> DataSource {
    let mut source = DataSource::new(name, driver, Scope::Project);
    source.file = Some(file.into());
    source.secrets = SecretPolicy::Never;
    source
}

#[test]
fn a_source_is_seen_with_its_dialect_and_color() {
    let s = setup("brief");
    let mut shop = file_source("shop", "sqlite", "shop.db");
    shop.color = Some(database::SourceColor::Red);
    let people = file_source("people", "csv", "people.csv");
    s.sessions.catalog().add(shop.clone()).unwrap();
    s.sessions.catalog().add(people.clone()).unwrap();
    let brief = s.access.source(shop.id).unwrap();
    assert!(brief.dialect && brief.file_based && !brief.read_only);
    assert_eq!(brief.flavor, database::sql::Flavor::Sqlite);
    assert_eq!(brief.color, Some(database::SourceColor::Red));
    assert_eq!(brief.icon, "database");
    assert!(brief.capabilities.console.is_some());
    let csv = s.access.source(people.id).unwrap();
    assert!(!csv.dialect, "a CSV file speaks no SQL");
    assert!(s.access.connector(people.id, &Overrides::default()).is_err());
    assert_eq!(s.access.sources().len(), 2);
    s.sessions.catalog().remove(shop.id).unwrap();
    assert!(s.access.source(shop.id).is_none());
    let _ = &s.manual;
}

/// The console's connection is its own (not the explorer's worker), on the source's file.
#[test]
fn a_console_connects_on_its_own() {
    let s = setup("connect");
    let mut shop = file_source("shop", "sqlite", "shop.db");
    shop.read_only = true;
    s.sessions.catalog().add(shop.clone()).unwrap();
    let connect = s.access.connector(shop.id, &Overrides::default()).unwrap();
    let mut session = connect().unwrap();
    let mut rows = Collect::default();
    session.execute("SELECT count(*) FROM t", &mut rows, &Cancel::default()).unwrap();
    assert_eq!(rows.rows, [[Value::Int(0)]]);
    // Read-only, by the driver too.
    let wrote =
        session.execute("INSERT INTO t VALUES (1)", &mut Collect::default(), &Cancel::default());
    assert!(wrote.is_err());
    assert!(s.sessions.is_empty(), "no worker of the explorer's made");
}

#[test]
fn the_window_s_selection_is_the_palette_s_source() {
    let s = setup("selected");
    assert!(s.access.selected().is_none());
    let id = DataSourceId::new();
    s.sessions.set_selected(Some(crate::sessions::Selected {
        source: id,
        schema: Some("main".into()),
        table: Some(TableRef::new("t")),
    }));
    let asked = s.access.selected().unwrap();
    assert_eq!((asked.source, asked.table), (id, Some(TableRef::new("t"))));
    assert_eq!(asked.project, s.sessions.project());
}

/// A source is production when marked, or when a folder it's in is (as its color).
#[test]
fn production_is_seen_from_the_source_or_its_folder() {
    let s = setup("production");
    let shop = file_source("shop", "sqlite", "shop.db");
    s.sessions.catalog().add(shop.clone()).unwrap();
    assert!(!s.access.source(shop.id).unwrap().production);
    assert!(!s.access.production(shop.id));
    let mut prod = database::SourceFolder::new("prod", Scope::Project);
    prod.production = true;
    s.sessions.catalog().put_folder(prod.clone()).unwrap();
    let mut inside = shop.clone();
    inside.folder = Some(prod.id);
    s.sessions.catalog().add(inside).unwrap();
    assert!(s.access.source(shop.id).unwrap().production, "from its folder");
    assert!(s.access.production(shop.id));
    let mut own = file_source("own", "sqlite", "shop.db");
    own.production = true;
    s.sessions.catalog().add(own.clone()).unwrap();
    assert!(s.access.production(own.id));
}

/// A console's transaction heard on the bus: its source marked, and a write the file's lock
/// refused explained by it.
#[test]
fn a_console_s_transaction_is_followed_and_explains_a_lock() {
    let s = setup("tx");
    let source = DataSourceId::new();
    let console = database::console::ConsoleId::new();
    assert!(!s.sessions.console_transaction(source));
    let locked = DbError::InUse("busy".into());
    assert_eq!(s.sessions.refusal(Some(source), &locked), i18n::t("database.file_in_use"));
    s.sessions.events().publish(&database::events::TransactionChanged {
        console,
        source,
        open: true,
    });
    assert!(s.sessions.console_transaction(source));
    assert!(!s.sessions.console_transaction(DataSourceId::new()), "only its source");
    assert_eq!(s.sessions.refusal(Some(source), &locked), i18n::t("database.console_tx_in_use"));
    // Another failure says what it is.
    let other = DbError::Query("no such table".into());
    assert_eq!(s.sessions.refusal(Some(source), &other), crate::grid_source::message(&other));
    s.sessions.events().publish(&database::events::TransactionChanged {
        console,
        source,
        open: false,
    });
    assert!(!s.sessions.console_transaction(source));
}
