//! The "Database" window over a project's sessions and a fake driver: registered as a tool
//! window (its panel made only when shown), its sources following the project's events,
//! its nodes read when opened (in the background), tables opened by source and table,
//! sources added from the `+` menu (a file picked, a server's in the data sources' modal),
//! removed,
//! the keys moving through the rows.

use std::path::Path;
use std::sync::Arc;

use database::{Driver, TableRef};
use kernel::{EventBus, ProjectId};

use super::*;
use crate::fake::{FakeDb, FakeDriver, Manual};

/// A driver of a server (not file-based): it never connects in these tests.
#[derive(Clone)]
struct Server;

impl Driver for Server {
    fn kind(&self) -> database::Kind {
        database::Kind::Relational
    }

    fn language(&self) -> database::Language {
        database::Language::Sql
    }

    fn connect(
        &self,
        _target: &database::Target,
        _secret: &database::Secret,
    ) -> Result<Box<dyn database::Session>, database::DbError> {
        Err(database::DbError::Unsupported("never in the tests".into()))
    }
}

fn file_driver() -> Arc<dyn Driver> {
    Arc::new(FakeDriver::new(FakeDb::users(3), true))
}

fn server_driver() -> Arc<dyn Driver> {
    Arc::new(Server)
}

/// A project with its sessions registered, the fake drivers known (and registered, for the
/// `+` menu).
struct Project {
    manual: Manual,
    /// What the window and the modal told the user.
    told: Arc<Told>,
    id: ProjectId,
    sessions: Arc<Sessions>,
    driver: FakeDriver,
    _guard: kernel::Subscription,
}

fn project() -> Project {
    database::drivers::register(database::DriverFactory::new(
        "fake-window",
        "Fake File",
        database::Kind::Relational,
        None,
        file_driver,
    ));
    database::drivers::register(database::DriverFactory::new(
        "fake-server",
        "Fake Server",
        database::Kind::Relational,
        Some(1),
        server_driver,
    ));
    let manual = Manual::new();
    let id = ProjectId::new();
    let sessions =
        Sessions::new(id, Path::new("/projects/shop"), EventBus::new(), manual.runtime(), None);
    let driver = FakeDriver::new(FakeDb::users(3), true);
    let d = driver.clone();
    sessions.set_drivers(Arc::new(move |id| match id {
        "fake-window" => Some(Arc::new(d.clone()) as Arc<dyn Driver>),
        "fake-server" => Some(server_driver()),
        _ => None,
    }));
    let told = Arc::new(Told::default());
    sessions.set_notifier(told.clone());
    let guard = crate::sessions::register(Arc::clone(&sessions));
    Project { manual, told, id, sessions, driver, _guard: guard }
}

/// The notifications told, in order.
#[derive(Default)]
struct Told(std::sync::Mutex<Vec<workbench_model::Notification>>);

impl Told {
    fn kinds(&self) -> Vec<(workbench_model::NotificationKind, String)> {
        self.0.lock().unwrap().iter().map(|n| (n.kind, n.title.clone())).collect()
    }
}

impl workbench_model::Notifier for Told {
    fn show(&self, notification: workbench_model::Notification) {
        self.0.lock().unwrap().push(notification);
    }
    fn update(&self, notification: workbench_model::Notification) {
        self.show(notification);
    }
    fn dismiss(&self, _: workbench_model::NotificationId) {}
}

/// Polls and runs the background until nothing's left.
fn settle(window: &mut DatabaseWindow, manual: &Manual) {
    for _ in 0..10 {
        manual.run_all();
        window.poll();
    }
}

/// The rows as `depth` and label, the window's own words given back in English.
fn labels(window: &mut DatabaseWindow) -> Vec<String> {
    let w = words();
    let english = [(w.loading, "Loading…"), (w.tables, "tables"), (w.columns, "columns")];
    window
        .rows()
        .iter()
        .map(|r| {
            let label =
                english.iter().find(|(t, _)| *t == r.label).map_or(r.label.as_str(), |(_, e)| e);
            format!("{}{}", "  ".repeat(r.depth as usize), label)
        })
        .collect()
}

fn file_source(name: &str) -> DataSource {
    let mut source = DataSource::new(name, "fake-window", Scope::Project);
    source.file = Some(PathBuf::from("data/users.db"));
    source
}

#[test]
fn it_is_a_registered_tool_window_made_only_when_shown() {
    let mut registry = extensions::ModuleRegistry::default();
    registry.register(Box::new(crate::DatabaseExplorerModule));
    let (mut contributions, _) = registry.activate(&Default::default());
    let tools = contributions.take::<Arc<dyn ui_components::ToolWindowProvider>>();
    assert_eq!(tools.len(), 1);
    let tool = &tools[0];
    assert_eq!(
        (tool.id(), tool.icon(), tool.dock()),
        (TOOL, "database", ui_components::Dock::Right)
    );
    assert_eq!(tool.content(), ui_components::ToolContent::Panel);
    assert_eq!(tool.toggle_command(), Some(TOGGLE.0));
    // Its button's menu has its id.
    let menus = contributions.take::<workbench_model::MenuContribution>();
    assert_eq!(menus.iter().map(|m| m.id).collect::<Vec<_>>(), [TOOL]);
    // Made: nothing read, nothing opened until it's given a project — asked of the window
    // itself (the process's sessions are other tests' too, opening theirs meanwhile).
    let mut panel = tool.create().expect("a panel");
    assert!(!panel.poll());
    let mut window = DatabaseWindow::new();
    assert!(!ToolPanel::poll(&mut window));
    assert!(window.project.is_none() && window.sessions.is_none(), "no project's sources held");
}

#[test]
fn sources_follow_the_projects_events_and_open_lazily() {
    let p = project();
    let mut window = DatabaseWindow::new();
    window.set_project(Some(p.id));
    settle(&mut window, &p.manual);
    assert!(window.rows().is_empty());
    // Added elsewhere (the catalog, as a modal would): the window hears it on the bus.
    let source = file_source("users db");
    p.sessions.catalog().add(source.clone()).unwrap();
    settle(&mut window, &p.manual);
    assert_eq!(labels(&mut window), ["users db"]);
    assert_eq!(p.driver.opened(), 0, "closed: nothing connected");
    window.expand(&NodeKey::Source(source.id));
    assert_eq!(labels(&mut window), ["users db", "  Loading…"]);
    settle(&mut window, &p.manual);
    assert_eq!(p.driver.opened(), 1, "read in the background once opened");
    assert_eq!(labels(&mut window), ["users db", "  tables"]);
    assert_eq!(window.rows()[0].count, Some(1));
    window.expand(&NodeKey::Group(source.id, None, Group::Tables));
    let users = NodeKey::Table(source.id, TableRef::new("users"));
    window.expand(&users);
    window.expand(&NodeKey::Part(source.id, TableRef::new("users"), Part::Columns));
    settle(&mut window, &p.manual);
    assert_eq!(
        labels(&mut window),
        ["users db", "  tables", "    users", "      columns", "        id", "        name"]
    );
    assert_eq!(window.rows()[2].detail, (words().rows)(3));
    assert_eq!(window.rows()[2].count, Some(2), "its columns");
    // Removed: gone from the tree, its session closed.
    window.select(NodeKey::Source(source.id));
    assert!(window.run("remove"));
    settle(&mut window, &p.manual);
    assert!(window.rows().is_empty());
    assert!(p.sessions.get(source.id).is_none());
    use workbench_model::NotificationKind::Success;
    assert!(
        p.told.kinds().iter().any(|(kind, title)| *kind == Success && title.contains("users db")),
        "the removal told: {:?}",
        p.told.kinds()
    );
}

#[test]
fn a_table_opens_by_its_source_and_name() {
    let p = project();
    let mut window = DatabaseWindow::new();
    window.set_project(Some(p.id));
    settle(&mut window, &p.manual);
    // A server's table (not a file): the source's own view, through the module's command.
    let mut server = DataSource::new("prod", "fake-server", Scope::Project);
    server.target.host = "db".into();
    p.sessions.catalog().add(server.clone()).unwrap();
    settle(&mut window, &p.manual);
    let table = TableRef::in_schema("public", "orders");
    window.open_table(&NodeKey::Table(server.id, table.clone()), false);
    let open = opened_source(window.take_action());
    assert_eq!((open.project, open.source, open.table), (p.id, server.id, Some(table)));
    // A file's table: the file's view, told which table.
    let file = file_source("users db");
    p.sessions.catalog().add(file.clone()).unwrap();
    settle(&mut window, &p.manual);
    window.open_table(&NodeKey::Table(file.id, TableRef::new("users")), true);
    assert!(matches!(window.action, Some(ToolPanelAction::View(ViewerAction::WithHost(_)))));
}

#[test]
fn the_add_menu_lists_the_drivers_the_recent_first() {
    let p = project();
    let mut window = DatabaseWindow::new();
    window.set_project(Some(p.id));
    settle(&mut window, &p.manual);
    let drivers = |window: &DatabaseWindow| -> Vec<String> {
        match &window.add_menu()[0] {
            ViewMenuEntry::Submenu { entries, .. } => entries
                .iter()
                .map(|e| match e {
                    ViewMenuEntry::Item { id, .. } => id.clone(),
                    _ => "-".into(),
                })
                .collect(),
            _ => Vec::new(),
        }
    };
    let before = drivers(&window);
    assert!(!before.contains(&"-".to_owned()), "no recent yet");
    assert!(before.contains(&"add:fake-server".to_owned()));
    // A file-based driver asks for its file; the file picked is added, relative to the
    // project.
    assert!(window.run("add:fake-window"));
    assert!(matches!(
        window.take_action(),
        Some(ToolPanelAction::View(ViewerAction::Request(AppRequest::PickFile(_))))
    ));
    assert!(window.command("picked-file:/projects/shop/data/users.db"));
    let added = p.sessions.catalog().all();
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].file.as_deref(), Some(Path::new("data/users.db")));
    assert_eq!(added[0].name, "users.db");
    // Picked again: only shown.
    window.run("add:fake-window");
    window.command("picked-file:/projects/shop/data/users.db");
    assert_eq!(p.sessions.catalog().all().len(), 1);
    assert_eq!(
        p.told.kinds(),
        [(
            workbench_model::NotificationKind::Info,
            i18n::tr!("datasources.already", name = "users.db")
        )],
        "said as a notification"
    );
    // A server's driver: a new source of it in the data sources' modal, kept once applied
    // there (not configured yet: never connected).
    window.run("add:fake-server");
    let Some(ToolPanelAction::View(ViewerAction::OpenView(open))) = window.take_action() else {
        panic!("the modal")
    };
    assert!(open.modal);
    let request = open.data.downcast_ref::<crate::sources::SourcesRequest>().unwrap().clone();
    assert_eq!(request.add.as_deref(), Some("fake-server"));
    assert_eq!(p.sessions.catalog().all().len(), 1, "nothing kept before Apply");
    let mut modal = crate::sources::SourcesView::open(request);
    assert!(modal.apply());
    settle(&mut window, &p.manual);
    let server =
        p.sessions.catalog().all().into_iter().find(|s| s.driver == "fake-server").unwrap();
    assert_eq!(server.name, "Fake Server");
    // Saving the sources is a notification (over the modal), never a band in the window.
    assert_eq!(
        p.told.kinds().last(),
        Some(&(workbench_model::NotificationKind::Success, i18n::t("datasources.applied"))),
        "the modal's word"
    );
    window.expand(&NodeKey::Source(server.id));
    let rows = window.rows().to_vec();
    let at = rows.iter().position(|r| r.key == NodeKey::Source(server.id)).unwrap();
    assert_eq!(rows[at + 1].kind, RowKind::Hint);
    // The recent ones come first, then all.
    let after = drivers(&window);
    assert_eq!(&after[..3], ["add:fake-server", "add:fake-window", "-"]);
}

#[test]
fn the_keys_move_through_the_rows_and_open_them() {
    let p = project();
    let mut window = DatabaseWindow::new();
    window.set_project(Some(p.id));
    settle(&mut window, &p.manual);
    let source = file_source("users db");
    p.sessions.catalog().add(source.clone()).unwrap();
    settle(&mut window, &p.manual);
    assert!(window.edit_key(ViewerKey::Down));
    assert_eq!(window.selected(), Some(&NodeKey::Source(source.id)));
    window.edit_key(ViewerKey::Right);
    settle(&mut window, &p.manual);
    window.edit_key(ViewerKey::Right);
    assert_eq!(window.selected(), Some(&NodeKey::Group(source.id, None, Group::Tables)));
    window.edit_key(ViewerKey::Left);
    assert_eq!(window.selected(), Some(&NodeKey::Source(source.id)), "to its parent");
    window.edit_key(ViewerKey::Left);
    assert_eq!(labels(&mut window), ["users db"], "closed");
    // Typing on the rows filters them; Esc gives the filter up.
    assert!(window.text_on_rows("zzz"));
    assert!(window.rows().is_empty());
    window.edit_key(ViewerKey::Escape);
    assert_eq!(labels(&mut window), ["users db"]);
    // ⌘C copies the selected node's name.
    assert!(window.command("copy"));
    assert_eq!(
        window.take_action(),
        Some(ToolPanelAction::View(ViewerAction::Request(AppRequest::CopyText("users db".into()))))
    );
}

#[test]
fn an_external_driver_is_marked_so_in_the_add_menu() {
    let p = project();
    let mut external = database::DriverFactory::new(
        "fake-external",
        "Fake External",
        database::Kind::Relational,
        Some(2),
        server_driver,
    );
    external.external = true;
    database::drivers::register(external);
    let mut window = DatabaseWindow::new();
    window.set_project(Some(p.id));
    settle(&mut window, &p.manual);
    let ViewMenuEntry::Submenu { entries, .. } = &window.add_menu()[0] else { panic!() };
    let marks = |id: &str| {
        entries.iter().find_map(|e| match e {
            ViewMenuEntry::Item { id: i, shortcut, .. } if i == id => Some(shortcut.clone()),
            _ => None,
        })
    };
    assert_eq!(marks("add:fake-external"), Some(Some(i18n::t("datasources.external"))));
    assert_eq!(marks("add:fake-server"), Some(None), "the app's own: no mark");
}

#[test]
fn the_relations_diagram_opens_from_a_source_a_schema_or_a_table() {
    let p = project();
    let mut window = DatabaseWindow::new();
    window.set_project(Some(p.id));
    settle(&mut window, &p.manual);
    let file = file_source("users db");
    p.sessions.catalog().add(file.clone()).unwrap();
    settle(&mut window, &p.manual);
    let wanted = |window: &mut DatabaseWindow, key: NodeKey| {
        window.select(key);
        assert!(window.run("diagram"));
        let Some(ToolPanelAction::View(ViewerAction::OpenView(view))) = window.take_action() else {
            panic!("the diagram's tab");
        };
        let title = view.title.clone();
        let request =
            view.data.downcast_ref::<database::diagram::DiagramRequest>().unwrap().clone();
        assert_eq!(request.access.source(), file.id);
        assert!(request.events.is_some(), "it follows the project's bus");
        (title, request.schema, request.focus)
    };
    // The whole source; a schema; around a table.
    let (title, schema, focus) = wanted(&mut window, NodeKey::Source(file.id));
    assert!(title.contains("users db"));
    assert_eq!((schema, focus), (None, None));
    let (_, schema, _) = wanted(&mut window, NodeKey::Schema(file.id, "main".into()));
    assert_eq!(schema.as_deref(), Some("main"));
    let users = TableRef::new("users");
    let (title, _, focus) = wanted(&mut window, NodeKey::Table(file.id, users.clone()));
    assert!(title.contains("users"));
    assert_eq!(focus, Some(users));
    // Its menu offers it on the source and on a table.
    window.expand(&NodeKey::Source(file.id));
    settle(&mut window, &p.manual);
    window.expand(&NodeKey::Group(file.id, None, Group::Tables));
    settle(&mut window, &p.manual);
    window.rows();
    for key in [NodeKey::Source(file.id), NodeKey::Table(file.id, TableRef::new("users"))] {
        let entries = window.menu_of(Some(&key));
        let has =
            entries.iter().any(|e| matches!(e, ViewMenuEntry::Item { id, .. } if id == "diagram"));
        assert!(has, "{key:?}");
    }
}

#[cfg(unix)]
#[test]
fn a_source_opened_through_a_link_is_listed_once() {
    let dir = std::env::temp_dir().join(format!("window-link-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("data")).unwrap();
    std::fs::write(dir.join("data/users.db"), b"").unwrap();
    std::os::unix::fs::symlink(dir.join("data"), dir.join("linked")).unwrap();
    let manual = Manual::new();
    let id = ProjectId::new();
    let sessions = Sessions::new(id, &dir, EventBus::new(), manual.runtime(), None);
    sessions.set_drivers(Arc::new(|id| (id == "fake-window").then(file_driver)));
    let _guard = crate::sessions::register(Arc::clone(&sessions));
    let mut window = DatabaseWindow::new();
    window.set_project(Some(id));
    settle(&mut window, &manual);
    // Kept by its absolute path, opened through the link (as `/var` and `/private/var`).
    let mut source = file_source("users db");
    source.file = Some(dir.join("data/users.db"));
    sessions.catalog().add(source).unwrap();
    let (_, _lease) = sessions.open_file(&dir.join("linked/users.db"), "fake-window", &[]).unwrap();
    settle(&mut window, &manual);
    assert_eq!(labels(&mut window), ["users db"], "one row: the configured source");
    // Added again by the link's path: already there.
    window.add_file(&dir.join("linked/users.db"), Some("fake-window"));
    assert_eq!(sessions.catalog().all().len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The window drawn once (its rows' places known).
fn paint_window(window: &mut DatabaseWindow) {
    let mut text = text_layout::TextSystem::new();
    let mut scene = render_api::Scene::default();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    window.paint(ViewerPaint {
        bounds: Rect::new(0.0, 0.0, 420.0, 800.0),
        scale: 1.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: false,
        waker: None,
    });
}

#[test]
fn every_row_icon_says_what_it_stands_for() {
    let p = project();
    let mut window = DatabaseWindow::new();
    window.set_project(Some(p.id));
    settle(&mut window, &p.manual);
    let source = file_source("users db");
    p.sessions.catalog().add(source.clone()).unwrap();
    settle(&mut window, &p.manual);
    // Opened down to the columns: the source, its tables, the table, its columns.
    window.edit_key(ViewerKey::Down);
    for _ in 0..4 {
        window.edit_key(ViewerKey::Right);
        settle(&mut window, &p.manual);
        window.edit_key(ViewerKey::Down);
    }
    window.edit_key(ViewerKey::Right);
    settle(&mut window, &p.manual);
    // A foreign key on `name` (to teams.id).
    let users = TableRef::new("users");
    let mut structure = window.tree.structure(source.id, &users).cloned().unwrap();
    structure.foreign_keys.push(database::ForeignKey {
        name: None,
        columns: vec!["name".into()],
        table: TableRef::new("teams"),
        references: vec!["id".into()],
        on_update: database::FkAction::default(),
        on_delete: database::FkAction::default(),
    });
    window.tree.set_structure(source.id, users, Ok(structure));
    window.stale = true;
    window.rows();
    paint_window(&mut window);
    let rows = window.rows().to_vec();
    let tip_of = |window: &DatabaseWindow, i: usize| {
        let r = window.icon_rect(i);
        let at = Point::new(r.min_x() + r.size.width / 2.0, r.min_y() + r.size.height / 2.0);
        window.tooltip_at(at).map(|t| t.label)
    };
    let mut seen = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        let tip = tip_of(&window, i);
        match row.kind {
            RowKind::Source | RowKind::Table(_) | RowKind::Column { .. } => {
                let tip = tip.unwrap_or_else(|| panic!("no tooltip on {}", row.label));
                assert!(!tip.is_empty());
                seen.push((row.label.clone(), tip));
            }
            _ => {}
        }
    }
    let tip = |label: &str| seen.iter().find(|(l, _)| l == label).map(|(_, t)| t.clone());
    assert_eq!(tip("id"), Some(i18n::t("datasources.tip_primary")));
    assert_eq!(
        tip("name"),
        Some(i18n::tr!("datasources.tip_foreign_to", target = "teams.id")),
        "{seen:?}"
    );
    assert_eq!(tip("users"), Some(i18n::t("datasources.tip_table")));
    assert!(tip("users db").is_some_and(|t| t.contains("Fake File")));
    // Beside the icon (its name): no tooltip.
    let r = window.icon_rect(0);
    assert_eq!(window.tooltip_at(Point::new(r.max_x() + 40.0, r.min_y() + 4.0)), None);
    // The header's buttons (drawn by the app, their tooltips the window's).
    let header = window.header().unwrap();
    let ids: Vec<&str> = header.iter().map(|b| b.id).collect();
    assert_eq!(ids, ["add", "console", "refresh", "collapse"], "Jump to Query Console between");
    assert!(header.iter().all(|b| !b.tooltip.is_empty() && !b.tooltip.starts_with("datasources.")));
}

/// Refresh asked by the user says the source's schema is stale (what else knows it, a
/// console's completion, reads it again): the source's row's, or every source's.
#[test]
fn refresh_says_the_schemas_are_stale() {
    let p = project();
    let mut window = DatabaseWindow::new();
    window.set_project(Some(p.id));
    settle(&mut window, &p.manual);
    let (one, two) = (file_source("one"), file_source("two"));
    p.sessions.catalog().add(one.clone()).unwrap();
    p.sessions.catalog().add(two.clone()).unwrap();
    settle(&mut window, &p.manual);
    let heard: Arc<std::sync::Mutex<Vec<database::DataSourceId>>> = Arc::default();
    let into = Arc::clone(&heard);
    let _s = p.sessions.events().subscribe(move |e: &database::events::SchemaInvalidated| {
        into.lock().unwrap().push(e.source);
    });
    window.select(NodeKey::Source(one.id));
    assert!(window.run("refresh"));
    assert_eq!(*heard.lock().unwrap(), [one.id]);
    heard.lock().unwrap().clear();
    let _ = ui_components::ToolPanel::header_action(&mut window, "refresh");
    let mut all = heard.lock().unwrap().clone();
    all.sort();
    let mut both = vec![one.id, two.id];
    both.sort();
    assert_eq!(all, both);
}

/// A server with several databases (as PostgreSQL's): each session opens one, says which,
/// and lists the others; only `shop` and `analytics` have tables (`postgres` none: the
/// user's report, a source naming `postgres` while its data lives in the others).
#[derive(Clone, Default)]
struct Cluster {
    /// The databases sessions were opened on, in order.
    opened: Arc<std::sync::Mutex<Vec<String>>>,
}

struct ClusterSession {
    database: String,
}

impl Driver for Cluster {
    fn kind(&self) -> database::Kind {
        database::Kind::Relational
    }

    fn language(&self) -> database::Language {
        database::Language::Sql
    }

    fn capabilities(&self) -> database::Capabilities {
        database::Capabilities {
            schemas: true,
            console: Some(database::ConsoleCaps { databases: true, ..Default::default() }),
            ..Default::default()
        }
    }

    fn dialect(&self) -> Option<Arc<dyn database::Dialect>> {
        Some(Arc::new(database::GenericSql))
    }

    fn connect(
        &self,
        target: &database::Target,
        _secret: &database::Secret,
    ) -> Result<Box<dyn database::Session>, database::DbError> {
        self.opened.lock().unwrap().push(target.database.clone());
        Ok(Box::new(ClusterSession { database: target.database.clone() }))
    }
}

impl database::Session for ClusterSession {
    fn server(&self) -> String {
        "Cluster 14".into()
    }

    fn children(&mut self, _path: &[String]) -> Result<Vec<database::Node>, database::DbError> {
        Ok(Vec::new())
    }

    fn execute(
        &mut self,
        _query: &str,
        _sink: &mut dyn database::RowSink,
        _cancel: &database::Cancel,
    ) -> Result<database::Outcome, database::DbError> {
        Ok(database::Outcome::default())
    }

    fn tables(&mut self, _path: &[String]) -> Result<Vec<database::TableInfo>, database::DbError> {
        let table = |schema: &str, name: &str, kind| database::TableInfo {
            table: TableRef::in_schema(schema, name),
            kind,
            rows: None,
        };
        use database::TableKind::{Table, View};
        Ok(match self.database.as_str() {
            "shop" => vec![
                table("public", "customers", Table),
                table("sales", "orders", Table),
                table("sales", "order_totals", View),
            ],
            "analytics" => vec![table("public", "visits", Table)],
            _ => Vec::new(),
        })
    }

    fn databases(&mut self) -> Result<Vec<String>, database::DbError> {
        Ok(vec!["shop".into(), "analytics".into(), "postgres".into()])
    }

    fn current(&mut self) -> Result<database::Place, database::DbError> {
        Ok(database::Place { database: Some(self.database.clone()), schema: Some("public".into()) })
    }
}

/// A project with the cluster's driver, its window, and a source of it naming `postgres`.
fn cluster_project() -> (Project, Cluster, DataSource, DatabaseWindow) {
    let p = project();
    let mut window = DatabaseWindow::new();
    window.set_project(Some(p.id));
    settle(&mut window, &p.manual);
    let cluster = Cluster::default();
    let c = cluster.clone();
    p.sessions.set_drivers(Arc::new(move |id| match id {
        "fake-cluster" => Some(Arc::new(c.clone()) as Arc<dyn Driver>),
        _ => None,
    }));
    let mut source = DataSource::new("pg", "fake-cluster", Scope::Project);
    source.target.host = "db".into();
    source.target.database = "postgres".into();
    source.secrets = database::SecretPolicy::Never;
    p.sessions.catalog().add(source.clone()).unwrap();
    settle(&mut window, &p.manual);
    (p, cluster, source, window)
}

/// The user's report: a source naming `postgres` (no tables there) showed "No tables". Its
/// server's databases are listed under it — every one it may open, its own marked — each
/// read through a session of its own once opened, the others' tables there.
#[test]
fn a_servers_databases_are_listed_each_read_by_a_session_of_its_own() {
    let (p, cluster, source, mut window) = cluster_project();
    window.expand(&NodeKey::Source(source.id));
    settle(&mut window, &p.manual);
    assert_eq!(labels(&mut window), ["pg", "  analytics", "  postgres", "  shop"]);
    assert_eq!(window.rows()[0].count, Some(3), "its databases");
    assert_eq!(window.rows()[2].detail, words().default_database, "its own marked");
    assert_eq!(window.rows()[2].kind, RowKind::Database);
    assert_eq!(*cluster.opened.lock().unwrap(), ["postgres"], "nothing else connected yet");
    // Its own database by its own identity (its consoles, grids, completion as before), the
    // others by theirs: the same after a restart.
    let shop = source.id.of_database("shop");
    let keys: Vec<NodeKey> = window.rows()[1..].iter().map(|r| r.key.clone()).collect();
    assert_eq!(
        keys,
        [
            NodeKey::Database(source.id.of_database("analytics")),
            NodeKey::Database(source.id),
            NodeKey::Database(shop),
        ]
    );
    // Its own: no tables (said under it, not under the source).
    window.expand(&NodeKey::Database(source.id));
    // Another: its own session, on that database, in the background.
    window.expand(&NodeKey::Database(shop));
    settle(&mut window, &p.manual);
    let mut opened = cluster.opened.lock().unwrap().clone();
    opened.sort();
    assert_eq!(opened, ["postgres", "shop"]);
    window.expand(&NodeKey::Schema(shop, "sales".into()));
    window.expand(&NodeKey::Group(shop, Some("sales".into()), Group::Tables));
    settle(&mut window, &p.manual);
    let no_tables = i18n::t("database.no_tables");
    assert_eq!(
        labels(&mut window),
        [
            "pg",
            "  analytics",
            "  postgres",
            &format!("    {no_tables}"),
            "  shop",
            "    public",
            "    sales",
            "      tables",
            "        orders",
            &format!("      {}", words().views),
        ]
    );
    let shop_row = window.rows().iter().find(|r| r.key == NodeKey::Database(shop)).unwrap();
    assert_eq!(shop_row.count, Some(3));
    // A table of it opens in its own view (its own name, never mixed with the source's).
    let orders = TableRef::in_schema("sales", "orders");
    window.open_table(&NodeKey::Table(shop, orders.clone()), false);
    let open = opened_source(window.take_action());
    assert_eq!((open.source, open.name.as_str()), (shop, "pg · shop"));
    let resolved = p.sessions.catalog().get(shop).expect("found by its identity");
    assert_eq!((resolved.target.database.as_str(), resolved.name.as_str()), ("shop", "pg · shop"));
    // Its console: the source's, on that database.
    window.console(Some(&NodeKey::Table(shop, orders)), true);
    let asked = database::console::take_jump().expect("a console asked");
    assert_eq!((asked.source, asked.database.as_deref()), (source.id, Some("shop")));
    // The filter finds what's read in any database.
    assert!(window.text_on_rows("order"));
    let found = labels(&mut window);
    assert!(found.contains(&"        orders".to_owned()), "{found:?}");
    assert!(!found.iter().any(|l| l.trim() == "analytics"), "{found:?}");
    window.edit_key(ViewerKey::Escape);
    // Refreshed: its databases read again, the open one's tables too.
    window.select(NodeKey::Source(source.id));
    assert!(window.run("refresh"));
    settle(&mut window, &p.manual);
    assert!(labels(&mut window).contains(&"        orders".to_owned()));
}

/// The databases a source shows (its Schemas tab's choice): only those; none left, said.
#[test]
fn a_source_shows_only_the_databases_chosen() {
    let (p, _cluster, mut source, mut window) = cluster_project();
    source.databases = vec!["shop".into()];
    p.sessions.catalog().update(source.clone()).unwrap();
    settle(&mut window, &p.manual);
    window.expand(&NodeKey::Source(source.id));
    settle(&mut window, &p.manual);
    assert_eq!(labels(&mut window), ["pg", "  shop"]);
    assert_eq!(window.rows()[0].count, Some(1));
    source.databases = vec!["gone".into()];
    p.sessions.catalog().update(source.clone()).unwrap();
    settle(&mut window, &p.manual);
    window.expand(&NodeKey::Source(source.id));
    settle(&mut window, &p.manual);
    assert_eq!(labels(&mut window), ["pg".to_owned(), format!("  {}", words().no_databases)]);
}

/// A database's session closes with its source's (its settings changed, removed).
#[test]
fn a_databases_session_goes_with_its_sources() {
    let (p, _cluster, source, mut window) = cluster_project();
    window.expand(&NodeKey::Source(source.id));
    settle(&mut window, &p.manual);
    let shop = source.id.of_database("shop");
    window.expand(&NodeKey::Database(shop));
    settle(&mut window, &p.manual);
    assert!(p.sessions.get(shop).is_some_and(|w| w.connected()));
    p.sessions.close(source.id);
    assert!(p.sessions.get(shop).is_none() && p.sessions.get(source.id).is_none());
    // Opened again when asked: still on its database.
    let worker = p.sessions.source_worker(shop).unwrap();
    assert_eq!(worker.source().id, shop);
}

/// The source's view an action opens.
fn opened_source(action: Option<ToolPanelAction>) -> crate::view::OpenSource {
    let Some(ToolPanelAction::View(ViewerAction::OpenView(view))) = action else {
        panic!("a source's view");
    };
    view.data.downcast_ref::<crate::view::OpenSource>().unwrap().clone()
}

/// Each row's icon: a source's its engine's (in its tone), the objects' their own — a
/// schema, a view, a foreign key and a unique index told apart from a table, a column and
/// an index.
#[test]
fn each_row_kind_has_its_own_icon() {
    use crate::tree::{Group, Part};
    let p = ui_components::Palette::dark();
    let row = |kind: RowKind| Row {
        key: NodeKey::Folder(database::FolderId::new()),
        depth: 0,
        kind,
        label: String::new(),
        detail: String::new(),
        count: None,
        expandable: false,
        expanded: false,
        color: None,
        marks: crate::tree::RowMarks::default(),
    };
    let icon = |kind: RowKind| {
        super::paint::icon_of(&row(kind), ("engine-x", database::IconTone::Blue), &p).0
    };
    assert_eq!(
        super::paint::icon_of(&row(RowKind::Source), ("engine-x", database::IconTone::Blue), &p),
        ("engine-x", p.terminal_ansi[4])
    );
    assert_eq!(icon(RowKind::Schema), "schema");
    assert_eq!(icon(RowKind::Table(database::TableKind::Table)), "table");
    assert_eq!(icon(RowKind::Table(database::TableKind::View)), "view");
    assert_eq!(icon(RowKind::Group(Group::Tables)), "folder");
    assert_eq!(icon(RowKind::Part(Part::Columns)), "folder");
    assert_eq!(icon(RowKind::Column { key: true, foreign: false }), "key");
    assert_eq!(icon(RowKind::Column { key: false, foreign: true }), "key-foreign");
    assert_eq!(icon(RowKind::Column { key: false, foreign: false }), "column");
    assert_eq!(icon(RowKind::Index { unique: true }), "index-unique");
    assert_eq!(icon(RowKind::Index { unique: false }), "index");
    assert_eq!(icon(RowKind::Key), "key-foreign");
    // The session's mark: none while closed, green open, red after it failed.
    assert_eq!(super::paint::link_color(crate::tree::Link::Closed, &p), None);
    assert_eq!(super::paint::link_color(crate::tree::Link::Open, &p), Some(p.vcs_added));
    assert_eq!(super::paint::link_color(crate::tree::Link::Failed, &p), Some(p.danger));
}

/// The filter is the app's search field: a press gives it the keys, typing filters, ⌘A
/// selects it all, ⌘X / ⌘Z edit it (the rows following), its × empties it and the rows
/// come back, a press on a row gives the keys back.
#[test]
fn the_filter_is_the_shared_search_field() {
    let p = project();
    let mut window = DatabaseWindow::new();
    window.set_project(Some(p.id));
    settle(&mut window, &p.manual);
    p.sessions.catalog().add(file_source("users db")).unwrap();
    settle(&mut window, &p.manual);
    paint_window(&mut window);
    let field = window.field;
    let on = Point::new(field.min_x() + 40.0, field.min_y() + field.size.height / 2.0);
    window.press(on, 1);
    window.release();
    assert!(window.wants_text(), "the press gave it the keys");
    assert!(window.text_input("zzz"));
    assert!(window.rows().is_empty());
    assert!(window.command("select_all"));
    assert_eq!(window.selected_text().as_deref(), Some("zzz"), "⌘A");
    assert!(window.command("cut"));
    assert_eq!(labels(&mut window), ["users db"], "⌘X: the rows again");
    assert!(window.command("undo"));
    assert!(window.rows().is_empty(), "⌘Z: filtered again");
    paint_window(&mut window);
    let x = Point::new(field.max_x() - 17.0, on.y);
    assert_eq!(window.cursor_at(x), Some(ViewerCursor::Pointer), "a hand on ×");
    window.press(x, 1);
    window.release();
    assert_eq!(window.filter.text(), "");
    assert_eq!(labels(&mut window), ["users db"], "× filtered again");
    let row = Point::new(field.min_x() + 40.0, window.list.min_y() + 14.0);
    window.press(row, 1);
    assert!(!window.wants_text(), "a row's press gave the keys back");
}
