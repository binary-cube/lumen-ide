//! The data view on a real CSV file (the driver registered as the app does), its source
//! opened on the shared pool: the table listed, its rows read, the file's source shared
//! and closed with the last view, a change on disk read again — never our own commit.

use std::time::{Duration, Instant};

use database::TableRef;
use ui_components::data_grid::{GridFilter, GridQuery, GridSource, RowState};

use super::*;

fn csv_files() -> DataFiles {
    DataFiles::new("csv", db_csv::EXTENSIONS, db_csv::HEAD, db_csv::is_text, true)
}

fn temp_csv(name: &str, text: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("data-view-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("people.csv");
    std::fs::write(&path, text).unwrap();
    path
}

/// Polls `view` until `done` or 5 s.
fn until(view: &mut DataView, mut done: impl FnMut(&mut DataView) -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        view.poll();
        if done(view) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

fn opened(path: &Path) -> DataView {
    database::drivers::register(db_csv::factory());
    let mut view = DataView::new(path.to_path_buf(), csv_files());
    view.start();
    assert!(until(&mut view, |v| !v.tables_loading), "the table listed");
    view
}

#[test]
fn a_csv_file_is_one_table_whose_rows_are_read_in_the_background() {
    let path = temp_csv("rows", "id,name\n1,Ana\n2,Dan\n3,Ion\n");
    let mut view = opened(&path);
    assert!(view.is_text() && view.opens_alone() && view.reads_file());
    assert_eq!(view.tables.len(), 1);
    assert_eq!(view.table().map(|t| t.table.name.as_str()), Some("people"));
    let grid = view.grid.as_mut().unwrap();
    assert!(until_grid(grid));
    grid.source_mut().request(&GridQuery::default(), 0..10);
    assert!(until(&mut view, |v| {
        v.grid.as_ref().is_some_and(|g| g.source().row_state(2) == RowState::Ready)
    }));
    let source = view.grid.as_ref().unwrap().source();
    assert_eq!(source.cell(1, 1), CellRefOwned::text("Dan"));
    assert!(view.status().unwrap().starts_with("people"));
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// The grid's columns came.
fn until_grid(grid: &mut DataGrid<TableSource>) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        grid.poll();
        if !grid.source().columns().is_empty() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

/// A cell's expected text.
struct CellRefOwned;

impl CellRefOwned {
    fn text(t: &'static str) -> ui_components::data_grid::CellRef<'static> {
        ui_components::data_grid::CellRef::Text(t)
    }
}

#[test]
fn views_of_a_file_share_its_source_closed_with_the_last() {
    let path = temp_csv("share", "a\n1\n");
    let mut first = opened(&path);
    let sessions = Arc::clone(&first.open.as_ref().unwrap().sessions);
    assert_eq!(sessions.file_users(&path), 1);
    // A second view of a file in the same project's sessions shares the source.
    let (worker, lease) = sessions.open_file(&path, "csv", &[]).unwrap();
    assert_eq!(worker.source().id, first.open.as_ref().unwrap().worker.source().id);
    assert_eq!(sessions.file_users(&path), 2);
    drop(lease);
    first.open = None;
    assert_eq!(sessions.file_users(&path), 0);
    assert!(sessions.is_empty(), "the source closed with the last view");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_change_on_disk_is_read_again_but_not_our_own_commit() {
    let path = temp_csv("disk", "a\n1\n");
    let mut view = opened(&path);
    let bus = view.open.as_ref().unwrap().sessions.events().clone();
    let written = workspace::events::FilesWritten { paths: vec![path.clone()] };
    // Our commit left the file so: nothing to read.
    view.own = stamp(&path);
    bus.publish(&written);
    assert!(!view.file_changed());
    // Another program wrote it.
    std::fs::write(&path, "a\n1\n2\n").unwrap();
    bus.publish(&written);
    assert!(view.file_changed());
    assert!(view.tables_loading, "listed again");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// Its file renamed (the tab follows and tells it): the view is the new file's — its
/// title, its rows read from there, the disk's changes of the new file heard.
#[test]
fn a_renamed_file_s_view_follows_it() {
    let path = temp_csv("renamed", "id,name\n1,Ana\n");
    let mut view = opened(&path);
    let bus = view.open.as_ref().unwrap().sessions.events().clone();
    let to = path.with_file_name("clients.csv");
    std::fs::rename(&path, &to).unwrap();
    // As the app does: said on the bus (the sessions follow), then the view told.
    bus.publish(&workspace::events::FileRenamed { from: path.clone(), to: to.clone() });
    view.moved(&to);
    assert_eq!(view.path(), to.as_path());
    assert_eq!(view.title(), "clients.csv");
    assert!(until(&mut view, |v| !v.tables_loading), "listed again from the new file");
    assert!(view.error.is_none(), "{:?}", view.error);
    let grid = view.grid.as_mut().unwrap();
    assert!(until_grid(grid));
    grid.source_mut().request(&GridQuery::default(), 0..10);
    assert!(until(&mut view, |v| {
        v.grid.as_ref().is_some_and(|g| g.source().row_state(0) == RowState::Ready)
    }));
    assert_eq!(view.grid.as_ref().unwrap().source().cell(0, 1), CellRefOwned::text("Ana"));
    // Another program writes the new file: read again; the old path is nobody's.
    std::fs::write(&to, "id,name\n1,Ana\n2,Dan\n").unwrap();
    bus.publish(&workspace::events::FilesWritten { paths: vec![path] });
    assert!(!view.file_changed());
    bus.publish(&workspace::events::FilesWritten { paths: vec![to.clone()] });
    assert!(view.file_changed());
    let _ = std::fs::remove_dir_all(to.parent().unwrap());
}

#[test]
fn a_journal_beside_a_database_is_its_change_too() {
    let file = Path::new("/data/shop.db");
    assert!(touches(Path::new("/data/shop.db"), file));
    assert!(touches(Path::new("/data/shop.db-wal"), file));
    assert!(touches(Path::new("/data/shop.db-journal"), file));
    assert!(!touches(Path::new("/data/shop.db2"), file));
    assert!(!touches(Path::new("/other/shop.db-wal"), file));
}

#[test]
fn counts_in_the_list_are_short() {
    assert_eq!(super::paint::short_count(950), "950");
    assert_eq!(super::paint::short_count(12_000), "12k");
    assert_eq!(super::paint::short_count(12_345), "12.3k");
    assert_eq!(super::paint::short_count(3_400_000), "3.4M");
}

#[test]
fn a_missing_file_says_why() {
    database::drivers::register(db_csv::factory());
    let mut view = DataView::new(PathBuf::from("/nonexistent/x.csv"), csv_files());
    view.start();
    assert!(until(&mut view, |v| v.error.is_some()));
}

#[test]
fn a_cell_edited_is_previewed_then_written_to_the_file() {
    let path = temp_csv("edit", "id,name\n1,Ana\n2,Dan\n");
    let mut view = opened(&path);
    let grid = view.grid.as_mut().unwrap();
    assert!(until_grid(grid));
    grid.source_mut().request(&GridQuery::default(), 0..10);
    assert!(until(&mut view, |v| {
        v.grid.as_ref().is_some_and(|g| g.source().row_state(1) == RowState::Ready)
    }));
    // Unsaved text: the data can't be edited.
    view.set_unsaved(true);
    view.poll();
    assert!(!view.grid.as_ref().unwrap().can_edit());
    view.set_unsaved(false);
    view.poll();
    let grid = view.grid.as_mut().unwrap();
    assert!(grid.can_edit());
    grid.source_mut().set(1, 1, ui_components::data_grid::CellValue::Text("Dana".into())).unwrap();
    // Submit: the preview is asked for (a line per change: a CSV writes no SQL).
    assert!(view.command("submit"));
    let Some(ViewerAction::OpenView(open)) = view.take_action() else { panic!("the modal") };
    let request = open.data.downcast_ref::<crate::preview::PreviewRequest>().unwrap();
    assert!(!request.sql && request.lines.len() == 1, "{:?}", request.lines);
    *request.answer.lock().unwrap() = Some(true);
    assert!(until(&mut view, |v| !v.told.is_empty()));
    let told = view.take_notifications();
    assert_eq!(told.len(), 1);
    assert_eq!(told[0].kind, workbench_model::NotificationKind::Success, "{told:?}");
    assert!(view.take_notifications().is_empty(), "taken once");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "id,name\n1,Ana\n2,Dana\n");
    assert_eq!(view.grid.as_ref().unwrap().source().pending(), 0);
    // The disk reports our own write: not read again.
    let bus = view.open.as_ref().unwrap().sessions.events().clone();
    bus.publish(&workspace::events::FilesWritten { paths: vec![path.clone()] });
    assert!(!view.file_changed());
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_column_added_from_the_structure_shows_in_the_grid() {
    let path = temp_csv("schema", "id,name\n1,Ana\n2,Dan\n");
    let mut view = opened(&path);
    assert!(until(&mut view, |v| v
        .grid
        .as_ref()
        .is_some_and(|g| g.source().structure().is_some())));
    // A CSV file: its columns added, renamed, moved and dropped; no table nor index.
    let can = view.schema_changes();
    assert!(can.columns && can.renames && can.reorder && !can.tables && !can.indexes);
    assert!(view.list_menu(None).is_none(), "a file is one table");
    let menu = view.row_menu(structure::Part::Columns, 1).unwrap();
    let ids: Vec<String> = menu
        .iter()
        .filter_map(|e| match e {
            ViewMenuEntry::Item { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        ids,
        ["schema:alter-column:name", "schema:move-column:name", "schema:drop-column:name"]
    );
    assert!(view.command("schema:add-column"));
    let Some(ViewerAction::OpenView(open)) = view.take_action() else { panic!("the modal") };
    assert!(open.modal);
    let request = open.data.downcast_ref::<crate::schema::SchemaRequest>().unwrap();
    assert_eq!(request.kind, crate::schema::Kind::AddColumn);
    // The modal, as the app opens it, run.
    let mut modal = crate::schema::SchemaView::open(request.clone(), None);
    modal.form_mut().unwrap().columns[0].name.set_text("city");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !modal.submit() {
        assert!(Instant::now() < deadline, "planned in time");
        modal.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
    while modal.take_action().is_none() {
        assert!(Instant::now() < deadline, "ran in time");
        modal.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
    // The view: told, the grid made again with the new column.
    assert!(until(&mut view, |v| {
        v.grid.as_ref().and_then(|g| g.source().structure()).is_some_and(|s| s.columns.len() == 3)
    }));
    assert!(
        view.take_notifications()
            .iter()
            .any(|n| n.kind == workbench_model::NotificationKind::Success),
        "told, as a notification"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "id,name,city\n1,Ana,\n2,Dan,\n");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_configured_source_opens_by_its_id_and_table() {
    use crate::fake::{FakeDb, FakeDriver, Manual};
    let manual = Manual::new();
    let project = ProjectId::new();
    let sessions = Sessions::new(project, Path::new("/p"), EventBus::new(), manual.runtime(), None);
    let mut db = FakeDb::users(3);
    let mut orders = db.tables[0].clone();
    orders.structure.table = database::TableRef::new("orders");
    db.tables.push(orders);
    let driver = FakeDriver::new(db, true);
    let d = driver.clone();
    sessions.set_drivers(Arc::new(move |id| {
        (id == "fake").then(|| Arc::new(d.clone()) as Arc<dyn database::Driver>)
    }));
    let _guard = crate::sessions::register(Arc::clone(&sessions));
    sessions.catalog().ensure_loaded();
    manual.run_all();
    let source = database::DataSource::new("shop db", "fake", database::Scope::Project);
    sessions.catalog().add(source.clone()).unwrap();
    let mut view = DataView::for_source();
    view.set_data(&OpenSource {
        project,
        source: source.id,
        name: "shop db".into(),
        table: Some(database::TableRef::new("users")),
        structure: true,
        column: None,
        filter: None,
    });
    view.start();
    for _ in 0..20 {
        manual.run_all();
        view.poll();
    }
    assert_eq!(view.source_id(), Some(source.id));
    assert_eq!(view.title(), "shop db");
    assert_eq!(view.tables.len(), 2);
    assert_eq!(view.table().map(|t| t.table.name.as_str()), Some("users"), "the one asked");
    assert_eq!(view.tab, Tab::Structure);
    assert_eq!(sessions.len(), 1, "through the source's own worker");
    assert_eq!(driver.opened(), 2, "its session and its counting one (the grid's total)");
    // Asked again for another table (the tab exists): shown there.
    view.set_data(&OpenSource {
        project,
        source: source.id,
        name: "shop db".into(),
        table: Some(database::TableRef::new("orders")),
        structure: false,
        column: None,
        filter: None,
    });
    assert_eq!(view.table().map(|t| t.table.name.as_str()), Some("orders"));
    assert_eq!(view.tab, Tab::Data);
}

fn structure_of(table: &str, column: Option<&str>) -> FileTable {
    FileTable {
        table: database::TableRef::new(table),
        structure: true,
        column: column.map(str::to_owned),
        filter: None,
    }
}

#[test]
fn a_column_asked_for_is_pointed_at_in_the_structure() {
    let path = temp_csv("column", "a,b\n1,2\n");
    let mut view = opened(&path);
    view.set_data(&structure_of("people", Some("b")));
    assert!(until(&mut view, |v| v.tab == Tab::Structure));
    assert_eq!(view.pointed_column(), Some("b"));
    // Another table shown: nothing pointed at.
    view.show_table(database::TableRef::new("other"));
    assert_eq!(view.pointed_column(), None);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_file_view_shows_the_table_the_window_asked_for() {
    let path = temp_csv("wanted", "a\n1\n");
    let mut view = opened(&path);
    view.set_data(&structure_of("people", None));
    assert!(until(&mut view, |v| v.tab == Tab::Structure));
    assert_eq!(view.table().map(|t| t.table.name.as_str()), Some("people"));
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// A SQLite file: users, orders pointing to them by a key, reviews only naming them.
fn temp_store(name: &str) -> PathBuf {
    database::drivers::register(db_sqlite::factory());
    let dir = std::env::temp_dir().join(format!("data-view-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("store.sqlite");
    let factory = database::drivers::find("sqlite").unwrap();
    let mut target =
        database::Target { database: path.to_string_lossy().into_owned(), ..Default::default() };
    target.options.insert("create".into(), "true".into());
    let mut session = (factory.create)().connect(&target, &database::Secret::new("")).unwrap();
    for sql in [
        "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)",
        "CREATE TABLE orders (id INTEGER PRIMARY KEY, user_id INTEGER REFERENCES users(id))",
        "CREATE TABLE reviews (id INTEGER PRIMARY KEY, user_id INTEGER)",
        "INSERT INTO users VALUES (1, 'Ana'), (2, 'Dan')",
        "INSERT INTO orders VALUES (10, 2)",
        "INSERT INTO reviews VALUES (20, 1)",
    ] {
        session
            .execute(sql, &mut database::Collect::default(), &database::Cancel::default())
            .unwrap();
    }
    path
}

fn sqlite_files() -> DataFiles {
    DataFiles::new("sqlite", &["sqlite"], db_sqlite::HEADER.len(), db_sqlite::is_database, false)
}

/// Table `name` shown, its first row read.
fn show(view: &mut DataView, name: &str) {
    assert!(view.command(&format!("table:{name}")));
    let grid = view.grid.as_mut().unwrap();
    assert!(until_grid(grid));
    grid.source_mut().request(&GridQuery::default(), 0..10);
    assert!(until(view, |v| {
        v.grid.as_ref().is_some_and(|g| g.source().row_state(0) == RowState::Ready)
    }));
}

#[test]
fn a_keys_cell_goes_to_the_row_it_points_to_by_a_real_or_a_virtual_key() {
    let path = temp_store("goto");
    let mut view = DataView::new(path.clone(), sqlite_files());
    view.start();
    assert!(until(&mut view, |v| !v.tables_loading), "the tables listed");
    show(&mut view, "orders");
    // Not a key's column: nothing.
    assert!(view.command("grid:select:0,0"));
    assert_eq!(view.reference(), None);
    assert!(view.command("grid:select:0,1"));
    assert_eq!(view.reference(), Some((TableRef::new("users"), "id".into(), "2".into())));
    assert!(view.command("goto-ref"));
    assert_eq!(view.table().map(|t| t.table.name.as_str()), Some("users"));
    assert!(until(&mut view, |v| v.grid.as_ref().is_some_and(|g| g.query().filter.is_some())));
    let filter = view.grid.as_ref().unwrap().query().filter.clone();
    assert!(
        matches!(&filter, Some(GridFilter::Compare { column, .. }) if column == "id"),
        "{filter:?}"
    );
    // A column only naming the table: a key once accepted (kept with the file's source).
    show(&mut view, "reviews");
    assert!(view.command("grid:select:0,1"));
    assert_eq!(view.reference(), None);
    let open = view.open.as_ref().unwrap();
    let mut source = database::DataSource::new("store", "sqlite", database::Scope::Project);
    source.id = open.worker.source().id;
    source.file = Some(path.clone());
    source.relations.accept(database::relations::VirtualKey {
        table: TableRef::new("reviews"),
        columns: vec!["user_id".into()],
        target: TableRef::new("users"),
        references: vec!["id".into()],
    });
    open.sessions.catalog().ensure_loaded();
    assert!(until(&mut view, |v| {
        v.open.as_ref().unwrap().sessions.catalog().list(database::Scope::Project)
            != crate::catalog::Listed::Loading
    }));
    let open = view.open.as_ref().unwrap();
    open.sessions.catalog().add(source).unwrap();
    assert_eq!(view.reference(), Some((TableRef::new("users"), "id".into(), "1".into())));
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn the_tab_carries_its_sources_color_and_follows_it() {
    let path = temp_store("accent");
    let mut view = DataView::new(path.clone(), sqlite_files());
    view.start();
    assert!(until(&mut view, |v| !v.tables_loading), "the tables listed");
    let palette = ui_components::Palette::dark();
    assert_eq!(view.accent(&palette), None, "a file shown by itself: no color");
    let open = view.open.as_ref().unwrap();
    open.sessions.catalog().ensure_loaded();
    assert!(until(&mut view, |v| {
        v.open.as_ref().unwrap().sessions.catalog().list(database::Scope::Project)
            != crate::catalog::Listed::Loading
    }));
    let catalog = view.open.as_ref().unwrap().sessions.catalog();
    let mut prod = database::SourceFolder::new("prod", database::Scope::Project);
    prod.color = Some(database::SourceColor::Red);
    catalog.put_folder(prod.clone()).unwrap();
    let mut source = database::DataSource::new("store", "sqlite", database::Scope::Project);
    source.file = Some(path.clone());
    source.folder = Some(prod.id);
    catalog.add(source).unwrap();
    let red = ui_components::tag_colors::named("red", &palette);
    assert_eq!(view.accent(&palette), red, "its folder's");
    prod.color = Some(database::SourceColor::Green);
    catalog.put_folder(prod).unwrap();
    assert_eq!(view.accent(&palette), ui_components::tag_colors::named("green", &palette));
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// `view` painted `width` wide at scale 1, its scene given back.
fn paint_at(view: &mut DataView, width: f32) -> render_api::Scene {
    let mut text = text_layout::TextSystem::new();
    let mut scene = render_api::Scene::default();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    view.paint(ViewerPaint {
        bounds: kernel::Rect::new(0.0, 0.0, width, 700.0),
        scale: 1.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
    scene
}

fn mid(r: kernel::Rect) -> kernel::Point {
    kernel::Point::new(r.min_x() + r.size.width / 2.0, r.min_y() + r.size.height / 2.0)
}

fn keys(id: &str) -> Option<String> {
    (id == "database.refresh").then(|| "F5".to_owned())
}

#[test]
fn a_narrow_view_keeps_its_header_and_toolbar_apart_and_says_what_each_button_does() {
    ui_components::shortcuts::set_reader(keys);
    let path = temp_store("narrow");
    let mut view = DataView::new(path, sqlite_files());
    view.start();
    assert!(until(&mut view, |v| !v.tables_loading));
    show(&mut view, "reviews");
    // Wide: the whole toolbar, nothing in "…".
    paint_at(&mut view, 1400.0);
    assert!(view.grid.as_ref().unwrap().collapsed_tools().is_empty());
    // Narrow (the Database window open beside it): the rest in "…", every button named.
    paint_at(&mut view, 460.0);
    let grid = view.grid.as_ref().unwrap();
    assert!(!grid.collapsed_tools().is_empty());
    let more = grid.tool_area(Tool::More).expect("the … shown");
    let tip = view.tooltip_at(mid(more)).expect("its tooltip");
    assert!(!tip.label.is_empty());
    // The header: Data / Structure and the list's tables have theirs.
    let tabs: Vec<_> = view.hits.iter().filter(|(_, h)| matches!(h, Hit::Tab(_))).collect();
    assert_eq!(tabs.len(), 2);
    for (r, _) in &tabs {
        assert!(view.tooltip_at(mid(*r)).is_some_and(|t| !t.label.is_empty()));
    }
    let (row, _) = view.hits.iter().find(|(_, h)| *h == Hit::Table(0)).unwrap();
    let label = view.tooltip_at(mid(*row)).unwrap().label;
    assert!(label.contains(&view.tables[0].table.name) && label.contains(" · "), "{label}");
    // A long name is cut before Data / Structure (its whole name in its tooltip).
    let left_of_tabs = tabs.iter().map(|(r, _)| r.min_x()).fold(f32::MAX, f32::min);
    for (r, _) in &view.tips {
        if r.max_y() <= 52.0 {
            assert!(r.max_x() <= left_of_tabs, "{r:?} reaches {left_of_tabs}");
        }
    }
    // Refresh from "…" shows its keys.
    view.grid.as_mut().unwrap().open_more();
    paint_at(&mut view, 460.0);
    assert!(view.grid.as_ref().unwrap().more_open());
    view.grid.as_mut().unwrap().edit_key(ViewerKey::Escape, KeyReach::default());
    paint_at(&mut view, 460.0);
    if let Some(r) = view.grid.as_ref().unwrap().tool_area(Tool::Refresh) {
        assert_eq!(view.tooltip_at(mid(r)).unwrap().shortcut.as_deref(), Some("F5"));
    }
    // The structure, narrower still: the sections' + buttons only their icon, named (the
    // widths are the Romanian labels': longer than English's).
    assert!(view.command("structure"));
    i18n::set_locale("ro");
    paint_at(&mut view, 330.0);
    let compact: Vec<_> =
        view.hits.iter().filter(|(r, h)| matches!(h, Hit::Add(_)) && r.size.width < 40.0).collect();
    assert!(!compact.is_empty(), "{:?}", view.hits);
    for (r, _) in compact {
        let tip = view.tooltip_at(mid(*r));
        assert!(tip.is_some_and(|t| !t.label.is_empty()), "{r:?}");
    }
    i18n::set_locale("en");
}

#[test]
fn the_list_s_lines_meet_the_data_s_across_the_splitter_and_its_handle_is_over_both() {
    let path = temp_store("lines");
    let mut view = DataView::new(path, sqlite_files());
    view.start();
    assert!(until(&mut view, |v| !v.tables_loading));
    show(&mut view, "reviews");
    let scene = paint_at(&mut view, 1200.0);
    let list = view.list_area;
    // The thin full-width lines of the list and of the panes right of it.
    let lines = |left: bool| -> Vec<f32> {
        let mut ys: Vec<f32> = scene
            .layers()
            .iter()
            .flat_map(|l| &l.quads)
            .filter(|q| q.bounds.size.height == 1.0 && q.bounds.size.width > 100.0)
            .filter(|q| {
                if left {
                    q.bounds.min_x() == list.min_x() && q.bounds.max_x() == list.max_x()
                } else {
                    q.bounds.min_x() == list.max_x()
                }
            })
            .map(|q| q.bounds.min_y())
            .collect();
        ys.sort_by(f32::total_cmp);
        ys
    };
    let (left, right) = (lines(true), lines(false));
    assert_eq!(left.len(), 1, "the list's header line: {left:?}");
    assert!(right.len() >= 2, "the header's and the toolbar's: {right:?}");
    assert_eq!(left[0], right[0], "one line across the splitter");
    // The selected first row centered in the band of the grid's toolbar.
    let row = view.hits.iter().find(|(_, h)| *h == Hit::Table(0)).unwrap().0;
    let band = (right[0] + 1.0, right[1]);
    assert_eq!(row.min_y() - band.0, band.1 - row.max_y(), "{row:?} in {band:?}");
    assert!(view.splitter.is_on_top(&scene));
}

#[test]
fn a_file_view_shows_the_rows_a_console_asked_for_filtered() {
    // A folder of its own (the CSV test's is "rows").
    let path = temp_store("rows-wanted");
    let mut view = DataView::new(path.clone(), sqlite_files());
    view.set_data(&rows_of(TableRef::new("users"), Some("id = 2".into())));
    view.start();
    assert!(until(&mut view, |v| !v.tables_loading), "the tables listed");
    assert!(until(&mut view, |v| v.grid.as_ref().is_some_and(|g| g.query().filter.is_some())));
    assert_eq!(view.table().map(|t| t.table.name.as_str()), Some("users"));
    assert_eq!(view.tab, Tab::Data);
    let filter = view.grid.as_ref().unwrap().query().filter.clone();
    assert!(
        matches!(&filter, Some(GridFilter::Compare { column, .. }) if column == "id"),
        "{filter:?}"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// A source of 1000 tables whose counts take as long as the test wants (held until it lets
/// them go), its jobs on threads of their own: the first page arrives while the grid's
/// total and the counts asked by the mouse are still held; the list shows the estimates,
/// marked `~`, then the counts once they're let go. Closing the view lets go what's asked.
#[test]
fn the_first_page_never_waits_for_the_counts() {
    use crate::fake::{FakeDb, FakeDriver};
    use std::sync::Condvar;
    // Each job on a thread of its own: the counts block on the gate as long as the test
    // wants, and a bounded pool would hold the page read behind them.
    #[allow(clippy::disallowed_methods)]
    let runtime = Runtime::new(|job| drop(std::thread::spawn(job)), Instant::now);
    let project = ProjectId::new();
    let sessions = Sessions::new(project, Path::new("/p"), EventBus::new(), runtime, None);
    let mut db = FakeDb::users(40);
    let first = db.tables[0].clone();
    db.tables = (0..1000)
        .map(|i| {
            let mut t = first.clone();
            t.structure.table = TableRef::new(format!("t{i:04}"));
            t
        })
        .collect();
    // Every count waits for the gate.
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let held = Arc::clone(&gate);
    db.on_count = Some(Arc::new(move || {
        let (open, cv) = &*held;
        let mut open = open.lock().unwrap();
        while !*open {
            open = cv.wait(open).unwrap();
        }
    }));
    let driver = FakeDriver::new(db, true);
    let d = driver.clone();
    sessions.set_drivers(Arc::new(move |id| {
        (id == "fake").then(|| Arc::new(d.clone()) as Arc<dyn database::Driver>)
    }));
    let _guard = crate::sessions::register(Arc::clone(&sessions));
    sessions.catalog().ensure_loaded();
    let source = database::DataSource::new("big db", "fake", database::Scope::Project);
    // The catalog is read in the background first.
    let deadline = Instant::now() + Duration::from_secs(5);
    while sessions.catalog().add(source.clone()).is_err() {
        assert!(Instant::now() < deadline, "the catalog read");
        std::thread::sleep(Duration::from_millis(5));
    }
    let mut view = DataView::for_source();
    view.set_data(&OpenSource {
        project,
        source: source.id,
        name: "big db".into(),
        table: None,
        structure: false,
        column: None,
        filter: None,
    });
    view.start();
    assert!(until(&mut view, |v| !v.tables_loading), "the tables listed");
    assert_eq!(view.tables.len(), 1000);
    assert_eq!(view.counts.get(0).map(|c| c.label.as_str()), Some("~40"), "its estimate");
    // The mouse over a few rows: their counts asked (held, like the grid's total).
    paint_at(&mut view, 1200.0);
    for i in 0..5 {
        let (row, _) = *view.hits.iter().find(|(_, h)| *h == Hit::Table(i)).unwrap();
        view.hover(mid(row));
    }
    view.grid.as_mut().unwrap().source_mut().request(&GridQuery::default(), 0..20);
    assert!(
        until(&mut view, |v| {
            v.grid.as_mut().unwrap().source_mut().request(&GridQuery::default(), 0..20);
            v.grid.as_ref().is_some_and(|g| g.source().row_state(0) == RowState::Ready)
        }),
        "the first page came while every count is held"
    );
    let counting = driver.db.lock().unwrap().counts;
    assert!(counting >= 1, "a count is held while the rows came: {counting}");
    assert!(!*gate.0.lock().unwrap());
    assert!(view.counts.get(1).is_some_and(|c| !c.exact), "still the estimate");
    // Let go: the counts come, the shown table's from its grid.
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
    assert!(until(&mut view, |v| (0..5).all(|i| v.counts.get(i).is_some_and(|c| c.exact))));
    assert_eq!(view.counts.get(1).map(|c| c.label.as_str()), Some("40"));
    assert!(view.counts.get(10).is_some_and(|c| !c.exact), "not asked: its estimate");
    drop(view);
}

/// A small SQLite file: every table counted exactly (cheap), on a session of its own, the
/// labels without `~`; a CSV file's one table takes its grid's total.
#[test]
fn a_small_files_tables_are_all_counted_exactly() {
    let path = temp_store("counts");
    let mut view = DataView::new(path.clone(), sqlite_files());
    view.start();
    assert!(until(&mut view, |v| !v.tables_loading), "the tables listed");
    assert!(until(&mut view, |v| (0..3).all(|i| v.counts.get(i).is_some_and(|c| c.exact))));
    let labels: Vec<&str> = (0..3).map(|i| view.counts.get(i).unwrap().label.as_str()).collect();
    // orders, reviews, users (by name).
    assert_eq!(labels, ["1", "1", "2"]);
    assert!(view.open.as_ref().unwrap().worker.counting_connected());
    let _ = std::fs::remove_dir_all(path.parent().unwrap());

    let csv = temp_csv("count", "id,name\n1,Ana\n2,Dan\n3,Ion\n");
    let mut view = opened(&csv);
    let grid = view.grid.as_mut().unwrap();
    assert!(until_grid(grid));
    grid.source_mut().request(&GridQuery::default(), 0..10);
    assert!(until(&mut view, |v| v.counts.get(0).is_some_and(|c| c.exact)));
    assert_eq!(view.counts.get(0).unwrap().label, "3");
    assert!(!view.open.as_ref().unwrap().worker.counting_connected(), "one session");
    let _ = std::fs::remove_dir_all(csv.parent().unwrap());
}

/// The tables list's width is the project's: dragged, it's kept in its sizes; the next data
/// view of the project (or of its next session) is as wide.
#[test]
fn the_list_s_width_is_kept_in_the_project_s_sizes() {
    let path = temp_store("list-width");
    let sizes = workbench_model::LayoutSizes::new();
    let mut view = DataView::new(path.clone(), sqlite_files());
    view.set_data(&sizes);
    view.start();
    assert!(until(&mut view, |v| !v.tables_loading));
    show(&mut view, "reviews");
    paint_at(&mut view, 1200.0);
    let handle = view.splitter.handle();
    let at = Point::new(handle.min_x() + handle.size.width / 2.0, handle.min_y() + 10.0);
    view.pointer_down(at, 1, PressMods::default());
    assert!(view.drag(Point::new(at.x + 80.0, at.y)));
    view.release();
    let kept = sizes.get(LIST_SIZE).expect("kept in the project");
    assert_eq!(kept, view.list_width);
    assert!(kept > 300.0, "{kept}");
    let mut next = DataView::new(path, sqlite_files());
    assert_eq!(next.list_width, 240.0);
    next.set_data(&sizes);
    assert_eq!(next.list_width, kept);
}

/// The file's size is seen when its tables come (and after our own writes), not when a
/// table is clicked: a click reads nothing from the disk.
#[test]
fn a_file_s_size_is_known_before_a_table_is_clicked() {
    let path = temp_csv("size", "id,name\n1,Ana\n");
    let mut view = opened(&path);
    let len = std::fs::metadata(&path).unwrap().len();
    assert_eq!(view.file_bytes, Some(len), "seen when the table came");
    // Grown by another program, not yet seen: a click uses what was seen.
    std::fs::write(&path, "id,name\n1,Ana\n2,Dan\n3,Ion\n").unwrap();
    view.select(0);
    assert_eq!(view.file_bytes, Some(len));
    let _ = std::fs::remove_file(&path);
}
