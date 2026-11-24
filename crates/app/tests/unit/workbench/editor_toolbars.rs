//! A module's bar above a file's editor, as a query console has it: laid out above the text
//! (the text below it, where it starts as in any tab, never under it), the tab's title and
//! breadcrumb its, ⌘↵ the console's in it and the grid's in a data view.

use super::*;

use crate::workbench::tests::console;

#[test]
fn the_bar_sits_above_the_text_never_over_it() {
    let mut c = console("layout");
    let bar = c.wb.toolbar_bounds().expect("a console's file has its bar");
    assert_eq!(bar.size.height, 40.0);
    let console_view = c.wb.tabs[c.wb.active].view.bounds();
    // The text starts under the bar (the usual gap between them), as wide as any tab's.
    assert_eq!(console_view.min_y(), bar.max_y() + EDITOR_GAP);
    // A project's SQL file has one too ("No data source — Attach…"), at the same place.
    c.wb.open_file(&c.dir.join("plain.sql")).unwrap();
    assert_eq!(c.wb.toolbar_bounds(), Some(bar), "a project's SQL file's bar");
    std::fs::write(c.dir.join("notes.txt"), "x\n").unwrap();
    c.wb.open_file(&c.dir.join("notes.txt")).unwrap();
    assert!(c.wb.toolbar_bounds().is_none(), "a text file has none");
    let plain = c.wb.tabs[c.wb.active].view.bounds();
    assert_eq!(plain.min_x(), console_view.min_x());
    assert_eq!(plain.size.width, console_view.size.width);
    assert_eq!(plain.max_y(), console_view.max_y());
    assert_eq!(bar.min_y() + EDITOR_GAP, plain.min_y(), "the bar takes the gap's place");
    // The find strip opens under the bar.
    let path = c.path.clone();
    c.wb.open_file(&path).unwrap();
    let find = c.wb.tabs[c.wb.active].find_area(c.wb.editor_area, 1.0);
    assert_eq!(find.min_y(), bar.max_y());
}

#[test]
fn its_tab_and_breadcrumb_are_the_console_s() {
    let mut c = console("title");
    let i = c.wb.active_tab().unwrap();
    assert_eq!(c.wb.tab_title(i), "console [shop]");
    let labels: Vec<String> = c.wb.crumbs().iter().map(|c| c.label.clone()).collect();
    assert_eq!(labels, [i18n::t("console.crumb_consoles"), "shop".into(), "console".into()]);
    assert!(c.wb.toolbar_may_close(i), "no transaction: it closes");
}

/// ⌘↵ in a console runs it; in a data view it's the grid's Submit, as before.
#[test]
fn command_enter_is_the_console_s_in_it_and_the_grid_s_in_a_data_view() {
    let c = console("keys");
    let places = c.wb.key_places();
    assert_eq!(places, [command_model::When::EditorLanguage("sql"), command_model::When::Editor]);
    let keymap = crate::keymaps::keymap(true).unwrap();
    let chord = command_model::KeyChord::parse("CmdOrCtrl+Enter").unwrap().for_platform(true);
    assert_eq!(keymap.lookup_in(&chord, &places), Some(database_console::RUN));
    assert_eq!(
        keymap.lookup_in(&chord, &[command_model::When::View("data")]),
        Some(command_model::CommandId("database.submit"))
    );
    assert_ne!(
        keymap.lookup_in(&chord, &[command_model::When::Editor]),
        Some(database_console::RUN)
    );
    // ⇧⌘F10 opens a console from anywhere.
    let jump = command_model::KeyChord::parse("CmdOrCtrl+Shift+F10").unwrap().for_platform(true);
    assert_eq!(keymap.lookup_in(&jump, &[]), Some(database_console::JUMP));
}

/// A console's profile opened in an editor tab of its own (`view-profile`): its breadcrumb
/// the console's and its own, its actions' keys there (F2, ⇧F2, H) — the same keys a
/// Services tab of it gets while clicked into.
#[test]
fn a_profile_s_tab_has_its_breadcrumb_and_its_keys() {
    let mut c = console("profile-tab");
    let id = workbench_model::WorkbenchHost::project_id(&c.wb).unwrap();
    let consoles = database_console::consoles::consoles(id).unwrap();
    let runner = consoles.runners().into_iter().next().unwrap();
    let plan = database::plan::from_query_plan(&[(2, 0, "SCAN orders".into())]);
    let profile = database::profile::Profile::from_plan(
        &plan,
        database::profile::ProfileDepth::Estimated,
        database::profile::Engine::default(),
    );
    let mut set = database_console::results::PlanSet::new(
        "Profile".into(),
        "SELECT * FROM orders".into(),
        0..20,
        plan,
    );
    set.report = Some(Arc::new(database::profile::derive(profile)));
    assert!(workbench_model::ViewHost::open_view(
        &mut c.wb,
        "profile [shop]",
        database_console::profile::VIEW,
        &database_console::profile::ProfileTabData { runner, set },
    ));
    let labels: Vec<String> = c.wb.crumbs().iter().map(|c| c.label.clone()).collect();
    assert_eq!(labels.len(), 4, "{labels:?}");
    assert_eq!(labels[0], i18n::t("console.crumb_consoles"));
    assert_eq!(labels[3], i18n::t("profile.view.crumb"));
    let places = c.wb.key_places();
    assert_eq!(places, [command_model::When::View(database_console::profile::VIEW)]);
    let keymap = crate::keymaps::keymap(true).unwrap();
    for (keys, command) in [
        ("F2", database_console::profile::NEXT_PROBLEM),
        ("Shift+F2", database_console::profile::PREV_PROBLEM),
        ("h", database_console::profile::HOTTEST),
    ] {
        let chord = command_model::KeyChord::parse(keys).unwrap().for_platform(true);
        assert_eq!(keymap.lookup_in(&chord, &places), Some(command), "{keys}");
    }
    // Elsewhere F2 isn't the profile's.
    let chord = command_model::KeyChord::parse("F2").unwrap().for_platform(true);
    assert_ne!(
        keymap.lookup_in(&chord, &[command_model::When::Editor]),
        Some(database_console::profile::NEXT_PROBLEM)
    );
}

/// Where row `path` is drawn in the project panel, if it shows (the panel asked what's
/// under each place).
fn project_row(wb: &mut Workbench, path: &Path) -> Option<Point> {
    wb.paint(&mut TextSystem::new(), &mut render_api::Scene::default());
    let side = wb.tools.sides.iter_mut().find(|s| s.id == PROJECT_TOOL)?;
    let b = side.content;
    let mut y = b.min_y() + 2.0;
    while y < b.max_y() {
        let p = Point::new(b.min_x() + 60.0, y);
        if side.panel.context_at(p).is_some_and(|c| c.path() == Some(path)) {
            return Some(p);
        }
        y += 4.0;
    }
    None
}

/// Row `path` shows in the project panel within a few seconds (the panel polled meanwhile:
/// a folder may be read in the background under load), after `reveal`.
fn revealed(wb: &mut Workbench, path: &Path, reveal: impl Fn(&mut Workbench)) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        reveal(wb);
        if project_row(wb, path).is_some() {
            return true;
        }
        wb.poll_side_panel();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    false
}

/// The console behind the active tab (its spec, as the module keeps it).
fn active_spec(c: &crate::workbench::tests::Console) -> database::console::ConsoleSpec {
    let id = workbench_model::WorkbenchHost::project_id(&c.wb).unwrap();
    let consoles = database_console::consoles::consoles(id).unwrap();
    let path = c.wb.tabs[c.wb.active].doc.path().unwrap().to_path_buf();
    consoles.specs().into_iter().find(|s| s.path(&c.dir) == path).expect("a console's file")
}

/// A console's file renamed from its tab (Rename File…, ⇧F6): the tab follows — its path,
/// its title and breadcrumb the new name's —, the console stays the same one (its id, its
/// schema, its Tx mode, its runner with what it ran) and "Select in Project View" finds it.
#[test]
fn a_renamed_console_follows_its_file() {
    let mut c = console("rename");
    let i = c.wb.active_tab().unwrap();
    let before = active_spec(&c);
    let id = workbench_model::WorkbenchHost::project_id(&c.wb).unwrap();
    let consoles = database_console::consoles::consoles(id).unwrap();
    consoles.update(before.id, |s| {
        s.schema = Some("main".into());
        s.tx = database::console::TxMode::Manual;
    });
    assert!(consoles.runner_of(before.id).is_some(), "its bar made its runner");
    // The console's folder made after the tree was listed: as the watcher says it.
    let own = c.dir.join(kernel::brand::PROJECT_DIR);
    c.wb.data.events.publish(&workspace::events::EntriesChanged { paths: vec![own] });
    // As the user had it: the console's folder open in the tree.
    let path = c.path.clone();
    assert!(revealed(&mut c.wb, &path, |wb| assert!(wb.reveal_in_project(i))));
    c.wb.rename_tab(i, "my-events-select.sql").unwrap();
    c.wb.poll_entry_changes();
    c.wb.poll_toolbars();
    c.wb.poll_side_panel();
    let new = c.path.with_file_name("my-events-select.sql");
    assert!(new.is_file() && !c.path.exists());
    assert_eq!(c.wb.tab_path(i).as_deref(), Some(new.as_path()));
    assert_eq!(c.wb.tab_title(i), "my-events-select [shop]");
    let labels: Vec<String> = c.wb.crumbs().iter().map(|c| c.label.clone()).collect();
    assert_eq!(
        labels,
        [i18n::t("console.crumb_consoles"), "shop".into(), "my-events-select".into()]
    );
    let after = active_spec(&c);
    assert_eq!(after.id, before.id, "the same console");
    assert_eq!(after.name, "my-events-select");
    assert_eq!(after.schema.as_deref(), Some("main"));
    assert_eq!(after.tx, database::console::TxMode::Manual);
    assert_eq!(consoles.specs().len(), 1, "no other console made for the new name");
    assert_eq!(consoles.runners().len(), 1, "the same runner");
    let now = consoles.runner_of(before.id).unwrap();
    assert_eq!(now.name(), "my-events-select");
    assert_eq!(now.file().as_deref(), Some(new.as_path()));
    // Select in Project View.
    let found = revealed(&mut c.wb, &new, |wb| assert!(wb.reveal_in_project(i)));
    assert!(found, "the renamed file is found in the tree");
    // The panel's "Select Opened File" (its crosshair) knows the new path too.
    let side = c.wb.tools.sides.iter_mut().find(|s| s.id == PROJECT_TOOL).unwrap();
    side.panel.header_action("collapse");
    assert!(project_row(&mut c.wb, &new).is_none(), "collapsed");
    let located = revealed(&mut c.wb, &new, |wb| {
        let side = wb.tools.sides.iter_mut().find(|s| s.id == PROJECT_TOOL).unwrap();
        side.panel.header_action("locate");
    });
    assert!(located, "located at its new path");
}

/// Renamed by another program (the disk's watcher heard both paths, which the consoles'
/// folder lets through): the tab follows as for the app's own rename, the same console;
/// moved out of the consoles' folder into the project, it's a SQL file of the project still
/// attached to its source (39n: the same console, its file's own title, the file's bar); a
/// project's `.sql` moved into a console's folder is a console again.
#[test]
fn a_console_follows_its_file_moved_on_disk() {
    let mut c = console("disk-move");
    let own: Vec<&str> =
        c.wb.support.project_services.iter().flat_map(|s| s.own_dirs()).copied().collect();
    assert!(own.contains(&database::console::DIR), "the watcher hears the consoles' files");
    let i = c.wb.active_tab().unwrap();
    let before = active_spec(&c);
    let renamed = c.path.with_file_name("by-hand.sql");
    std::fs::rename(&c.path, &renamed).unwrap();
    let paths = vec![c.path.clone(), renamed.clone()];
    c.wb.data.events.publish(&workspace::events::EntriesChanged { paths });
    assert!(c.wb.poll_entry_changes());
    assert_eq!(c.wb.tab_path(i).as_deref(), Some(renamed.as_path()));
    assert_eq!(c.wb.tab_title(i), "by-hand [shop]");
    assert_eq!(active_spec(&c).id, before.id);
    // Out of the consoles' folder into the project: still attached to its source.
    let out = c.dir.join("by-hand.sql");
    std::fs::rename(&renamed, &out).unwrap();
    let paths = vec![renamed.clone(), out.clone()];
    c.wb.data.events.publish(&workspace::events::EntriesChanged { paths });
    assert!(c.wb.poll_entry_changes());
    assert_eq!(c.wb.tab_path(i).as_deref(), Some(out.as_path()));
    assert_eq!(c.wb.tab_title(i), "by-hand.sql");
    assert!(c.wb.tabs[i].toolbar.is_some(), "the file's bar, on its source");
    let id = workbench_model::WorkbenchHost::project_id(&c.wb).unwrap();
    let consoles = database_console::consoles::consoles(id).unwrap();
    let kept = consoles.spec(before.id).expect("the same one");
    assert!(kept.is_attached() && kept.source == before.source);
    assert_eq!(consoles.attached_of(&out).map(|s| s.id), Some(before.id));
    // Moved back in (the tree's move): a console again, with its bar — the same one.
    c.wb.move_file(&out, &renamed).unwrap();
    assert!(c.wb.tabs[i].toolbar.is_some(), "a console's bar again");
    assert_eq!(c.wb.tab_title(i), "by-hand [shop]");
    assert_eq!(active_spec(&c).id, before.id);
    assert!(consoles.attached_files().is_empty());
}

/// A console's file deleted by another program: its saved tab closes (as for a deletion in
/// the app), the console leaves the list.
#[test]
fn a_console_deleted_on_disk_closes_its_tab() {
    let mut c = console("disk-delete");
    let before = active_spec(&c);
    std::fs::remove_file(&c.path).unwrap();
    let paths = vec![c.path.clone()];
    c.wb.data.events.publish(&workspace::events::EntriesChanged { paths });
    assert!(c.wb.poll_entry_changes());
    assert!(c.wb.tabs.iter().all(|t| t.doc.path() != Some(c.path.as_path())), "closed");
    let id = workbench_model::WorkbenchHost::project_id(&c.wb).unwrap();
    assert!(database_console::consoles::consoles(id).unwrap().spec(before.id).is_none());
}

/// A SQLite file's data view renamed from the tree (said on the bus): its tab and its view
/// are the new file's.
#[test]
fn a_data_view_follows_its_file_renamed() {
    let mut c = console("data-rename");
    let db = c.dir.join("shop.db");
    // A SQLite file as its first bytes tell (nothing read from it here).
    let mut head = b"SQLite format 3\0".to_vec();
    head.resize(4096, 0);
    std::fs::write(&db, head).unwrap();
    c.wb.open_file(&db).unwrap();
    let i = c.wb.active_tab().unwrap();
    assert!(c.wb.tabs[i].viewer.is_some(), "a data view");
    let to = c.dir.join("orders.db");
    std::fs::rename(&db, &to).unwrap();
    c.wb.data.events.publish(&workspace::events::FileRenamed { from: db, to: to.clone() });
    assert!(c.wb.poll_entry_changes());
    assert_eq!(c.wb.tab_path(i).as_deref(), Some(to.as_path()));
    assert_eq!(c.wb.tab_title(i), "orders.db");
    // The source configured on it follows (its consoles with it).
    let id = workbench_model::WorkbenchHost::project_id(&c.wb).unwrap();
    let sessions = database_explorer::sessions(id).unwrap();
    let shop = sessions.catalog().all().into_iter().find(|s| s.name == "shop").unwrap();
    assert_eq!(shop.file_in(&c.dir), Some(to));
}

/// ⌥⌘E in a console opens its history (a module's modal over the editor); ⇧⌘F10 the
/// consoles' popover, its filter having the keys; a console renamed from it (the
/// module's rename) is the same console, its tab following the file.
#[test]
fn the_history_and_the_consoles_popover_are_the_module_s_views() {
    let mut c = console("history");
    let keymap = crate::keymaps::keymap(true).unwrap();
    let chord = command_model::KeyChord::parse("CmdOrCtrl+Alt+E").unwrap().for_platform(true);
    assert_eq!(keymap.lookup_in(&chord, &c.wb.key_places()), Some(database_console::HISTORY));
    c.wb.run_command(database_console::HISTORY);
    assert!(c.wb.has_modal_view(), "the history window");
    assert!(c.wb.modal_view_has_input(), "its search has the keys");
    c.wb.modal_view = None;
    // The popover of the source's consoles (it has one: listed, not made).
    let id = workbench_model::WorkbenchHost::project_id(&c.wb).unwrap();
    let consoles = database_console::consoles::consoles(id).unwrap();
    let spec = consoles.spec_of_path(&c.path).unwrap();
    c.wb.run_command(database_console::JUMP);
    // Its consoles listed in the background (the disk never on the UI thread), then shown.
    until(&mut c, |wb| {
        wb.poll_jobs();
        wb.has_modal_view()
    });
    assert!(c.wb.has_modal_view(), "the consoles' popover");
    assert!(c.wb.modal_view_has_input());
    c.wb.modal_view = None;
    // Renamed as the popover's pencil does: the same console, the tab following its file.
    let renamed = consoles.rename(spec.id, "daily").unwrap();
    assert_eq!(renamed.id, spec.id);
    c.wb.poll_entry_changes();
    let i = c.wb.active_tab().unwrap();
    assert_eq!(c.wb.tabs[i].doc.path(), Some(renamed.path(&c.dir).as_path()));
    assert_eq!(c.wb.tab_title(i), "daily [shop]");
}

/// A console completes from its source's schema (its module's `DocumentAssist`, asked
/// before the language): the schema read in the background (said meanwhile), the list made
/// again when it comes; the columns after `alias.`, a table made by a statement the console
/// ran (DDL: the schema read again); a plain `.sql` file keeps the language's keywords.
#[test]
fn a_console_completes_from_its_source_s_schema() {
    let mut c = console("completion");
    // An empty file is an empty SQLite database.
    std::fs::write(c.dir.join("shop.db"), b"").unwrap();
    let mut text = TextSystem::new();
    let id = workbench_model::WorkbenchHost::project_id(&c.wb).unwrap();
    let consoles = database_console::consoles::consoles(id).unwrap();
    // A table made from the console (its DDL makes the schema stale).
    let ddl = "CREATE TABLE events (id INTEGER PRIMARY KEY, kind TEXT, at TEXT);";
    let len = c.wb.tabs[c.wb.active].doc.buffer().len();
    assert!(workbench_model::EditorHost::edit_active(&mut c.wb, 0..len, ddl));
    workbench_model::EditorHost::select_active(&mut c.wb, 5..5);
    assert_eq!(c.wb.run_command(database_console::RUN), Some(true));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let spec = active_spec(&c);
    while consoles.runner_of(spec.id).is_some_and(|r| r.busy() || r.snapshot().last.is_none())
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let sql = "select * from events e where e.";
    let len = c.wb.tabs[c.wb.active].doc.buffer().len();
    assert!(workbench_model::EditorHost::edit_active(&mut c.wb, 0..len, sql));
    workbench_model::EditorHost::select_active(&mut c.wb, sql.len()..sql.len());
    assert!(c.wb.update_completion(&mut text));
    let labels = |wb: &Workbench| -> Vec<String> {
        wb.completion.items().iter().map(|i| i.label.clone()).collect()
    };
    // Read in the background; meanwhile said (or, read quickly, already there).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while labels(&c.wb) != ["id", "kind", "at"] && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
        if c.wb.poll_assists() {
            c.wb.refresh_assisted_completion(&mut text);
        }
    }
    assert_eq!(labels(&c.wb), ["id", "kind", "at"]);
    assert_eq!(c.wb.completion.items()[0].kind, code_intelligence::CompletionKind::Column);
    // Accepted: inserted after the dot.
    assert!(c.wb.accept_completion(&mut text));
    assert_eq!(c.wb.tabs[c.wb.active].text().as_ref(), "select * from events e where e.id");
    // A table, its source beside it.
    let sql = "SELECT * FROM ev";
    let len = c.wb.tabs[c.wb.active].doc.buffer().len();
    workbench_model::EditorHost::edit_active(&mut c.wb, 0..len, sql);
    workbench_model::EditorHost::select_active(&mut c.wb, sql.len()..sql.len());
    assert!(c.wb.update_completion(&mut text));
    let first = &c.wb.completion.items()[0];
    assert_eq!((first.label.as_str(), first.detail.as_deref()), ("events", Some("main · shop")));
    // A plain `.sql` file of the project: the language's own words.
    c.wb.open_file(&c.dir.join("plain.sql")).unwrap();
    let sql = "SEL";
    let len = c.wb.tabs[c.wb.active].doc.buffer().len();
    workbench_model::EditorHost::edit_active(&mut c.wb, 0..len, sql);
    workbench_model::EditorHost::select_active(&mut c.wb, sql.len()..sql.len());
    c.wb.update_completion(&mut text);
    assert!(c.wb.completion_assist.is_none(), "no module's list");
}

/// Runs `ddl` from the active console and waits for it (its schema made stale).
fn run_ddl(c: &mut crate::workbench::tests::Console, ddl: &str) {
    let id = workbench_model::WorkbenchHost::project_id(&c.wb).unwrap();
    let consoles = database_console::consoles::consoles(id).unwrap();
    let len = c.wb.tabs[c.wb.active].doc.buffer().len();
    assert!(workbench_model::EditorHost::edit_active(&mut c.wb, 0..len, ddl));
    workbench_model::EditorHost::select_active(&mut c.wb, 5..5);
    assert_eq!(c.wb.run_command(database_console::RUN), Some(true));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let spec = active_spec(c);
    while consoles.runner_of(spec.id).is_some_and(|r| r.busy() || r.snapshot().last.is_none())
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// Waits until `ready` holds (the background's work: the schema read, a text checked).
fn until(c: &mut crate::workbench::tests::Console, mut ready: impl FnMut(&mut Workbench) -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !ready(&mut c.wb) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// A console resolves its names through its source's schema (its module's `DocumentAssist`):
/// the hover card of a table lists its columns; Go to Declaration on an alias goes to where
/// it's given, on a table opens its data (the file's view); the text is checked against the
/// schema (an unknown table underlined as an error, through the project's diagnostics),
/// cleared once fixed.
#[test]
fn a_console_resolves_names_shows_cards_and_checks_its_text() {
    let mut c = console("resolve");
    std::fs::write(c.dir.join("shop.db"), b"").unwrap();
    let mut text = TextSystem::new();
    run_ddl(&mut c, "CREATE TABLE events (id INTEGER PRIMARY KEY, kind TEXT NOT NULL);");
    let sql = "SELECT e.kind FROM events e;\nSELECT * FROM nope;";
    let len = c.wb.tabs[c.wb.active].doc.buffer().len();
    assert!(workbench_model::EditorHost::edit_active(&mut c.wb, 0..len, sql));
    // The card of a table (the schema read in the background the first time).
    let on_table = sql.find("events").unwrap() + 2;
    until(&mut c, |wb| wb.assist_hover(sql, on_table).is_some_and(|h| !h.params.is_empty()));
    let card = c.wb.assist_hover(sql, on_table).expect("a table's card");
    let columns: Vec<&str> = card.params.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(columns, ["id", "kind"]);
    assert_eq!(card.params[1].doc.as_deref(), Some("NOT NULL"));
    // An alias used: to where it's given.
    let alias = sql.find("e.kind").unwrap();
    workbench_model::EditorHost::select_active(&mut c.wb, alias..alias);
    assert!(c.wb.goto_definition(&mut text));
    let head = c.wb.tabs[c.wb.active].doc.selections().primary().head;
    assert_eq!(head, sql.find("events e").unwrap() + "events ".len());
    // The text checked: the unknown table told, an error at its place.
    let relative = c.path.strip_prefix(&c.dir).unwrap().to_path_buf();
    let told = |wb: &mut Workbench| {
        wb.sync_toolbar_editor();
        wb.poll_diagnostics();
        wb.data
            .diagnostics
            .of(&relative)
            .filter(|d| d.message.contains("nope"))
            .map(|d| (d.line, d.column, d.severity))
            .collect::<Vec<_>>()
    };
    until(&mut c, |wb| !told(wb).is_empty());
    assert_eq!(told(&mut c.wb), [(1, Some(14), workbench_model::Severity::Error)]);
    // Fixed: cleared.
    let fixed = "SELECT e.kind FROM events e;";
    let len = c.wb.tabs[c.wb.active].doc.buffer().len();
    assert!(workbench_model::EditorHost::edit_active(&mut c.wb, 0..len, fixed));
    until(&mut c, |wb| told(wb).is_empty());
    assert!(told(&mut c.wb).is_empty());
    // A table: its data, in the source file's view.
    let on_table = fixed.find("events").unwrap() + 1;
    workbench_model::EditorHost::select_active(&mut c.wb, on_table..on_table);
    assert!(c.wb.goto_definition(&mut text));
    let active = c.wb.tabs[c.wb.active].doc.path().map(Path::to_path_buf);
    assert_eq!(active, Some(c.dir.join("shop.db")));
}

/// "Reformat Code" (⌥⌘L, Code quality's command) lays a console out with the SQL language's
/// own formatter (no tool chosen; nothing saved) by its source's dialect — SQLite's, where
/// `[a  b]` is a name kept as written; "Reformat Statement" only the one at the cursor.
#[test]
fn a_console_is_reformatted_by_its_source_s_dialect() {
    use workbench_model::{EditorHost, WorkbenchHost};
    let mut c = console("reformat");
    let keymap = crate::keymaps::keymap(true).unwrap();
    let chord = command_model::KeyChord::parse("CmdOrCtrl+Alt+L").unwrap().for_platform(true);
    assert_eq!(keymap.lookup_in(&chord, &c.wb.key_places()), Some(workbench_model::REFORMAT_FILE));
    let text = |wb: &Workbench| wb.active_file().unwrap().text.to_string();
    let put = |wb: &mut Workbench, new: &str| {
        let len = wb.active_file().unwrap().text.len();
        assert!(wb.edit_active(0..len, new));
    };
    put(&mut c.wb, "select [a  b], x from t where y = 1;\nselect  2");
    c.wb.run_command(workbench_model::REFORMAT_FILE);
    assert_eq!(text(&c.wb), "SELECT [a  b], x\nFROM t\nWHERE y = 1;\nSELECT 2");
    // The statement at the cursor alone.
    put(&mut c.wb, "select 1;\nselect  a from t;\nselect  3;");
    let at = text(&c.wb).find("from").unwrap();
    assert!(c.wb.select_active(at..at));
    c.wb.run_command(lang_sql::format::FORMAT_STATEMENT);
    assert_eq!(text(&c.wb), "select 1;\nSELECT a\nFROM t;\nselect  3;");
    // A plain `.sql` file is laid out too (by its name's dialect).
    c.wb.open_file(&c.dir.join("plain.sql")).unwrap();
    put(&mut c.wb, "select a,b from t");
    c.wb.run_command(workbench_model::REFORMAT_FILE);
    assert_eq!(text(&c.wb), "SELECT a, b\nFROM t");
}

/// A project's own SQL file (39n): its bar says "No data source" until it's attached from
/// it, then it's the console's bar on the source — the file's own title, its text checked —
/// and the file runs, completes from the schema and is laid out by the source's dialect as a
/// console; detached, it's attached to nothing again (its language's own words).
#[test]
fn a_project_s_sql_file_is_attached_from_its_bar_and_runs() {
    use workbench_model::{EditorHost, WorkbenchHost};
    let mut c = console("attached");
    std::fs::write(c.dir.join("shop.db"), b"").unwrap();
    let mut text = TextSystem::new();
    let id = c.wb.project_id().unwrap();
    let consoles = database_console::consoles::consoles(id).unwrap();
    let source = active_spec(&c).source;
    let path = c.dir.join("plain.sql");
    c.wb.open_file(&path).unwrap();
    let i = c.wb.active_tab().unwrap();
    assert!(c.wb.tabs[i].toolbar.is_some(), "No data source — Attach…");
    assert_eq!(c.wb.tab_title(i), "plain.sql");
    assert!(!c.wb.tabs[i].toolbar.as_ref().unwrap().bar.follows_editor(), "nothing to check");
    // Attached from the bar's menu.
    assert!(c.wb.run_bar_menu(&mut text, &format!("attach:{source}")));
    let spec = consoles.attached_of(&path).expect("attached");
    assert_eq!(spec.source, source);
    assert_eq!(c.wb.tab_title(i), "plain.sql", "the file's own title");
    assert!(c.wb.tabs[i].toolbar.as_ref().unwrap().bar.follows_editor(), "checked as typed");
    let labels: Vec<String> = c.wb.crumbs().iter().map(|c| c.label.clone()).collect();
    assert!(!labels.contains(&i18n::t("console.crumb_consoles")), "{labels:?}");
    // Runs as a console (⌘↵), on a connection of its own.
    let ddl = "CREATE TABLE events (id INTEGER PRIMARY KEY, kind TEXT);";
    let len = c.wb.tabs[c.wb.active].doc.buffer().len();
    assert!(c.wb.edit_active(0..len, ddl));
    c.wb.select_active(5..5);
    assert_eq!(c.wb.run_command(database_console::RUN), Some(true));
    until(&mut c, |_| {
        consoles.runner_of(spec.id).is_some_and(|r| !r.busy() && r.snapshot().last.is_some())
    });
    let last = consoles.runner_of(spec.id).unwrap().snapshot().last.unwrap();
    assert!(last.ok, "{last:?}");
    // Completes from the schema.
    let sql = "select * from events e where e.";
    let len = c.wb.tabs[c.wb.active].doc.buffer().len();
    c.wb.edit_active(0..len, sql);
    c.wb.select_active(sql.len()..sql.len());
    until(&mut c, |wb| {
        wb.update_completion(&mut text);
        wb.completion.items().iter().any(|i| i.label == "kind")
    });
    assert!(c.wb.completion.items().iter().any(|i| i.label == "kind"));
    // Laid out by its source's dialect (SQLite's `[a  b]` a name).
    let len = c.wb.tabs[c.wb.active].doc.buffer().len();
    c.wb.edit_active(0..len, "select [a  b] from t");
    c.wb.run_command(workbench_model::REFORMAT_FILE);
    assert_eq!(c.wb.active_file().unwrap().text.as_ref(), "SELECT [a  b]\nFROM t");
    // Detached: attached to nothing again.
    assert_eq!(c.wb.run_command(database_console::DETACH), Some(true));
    assert!(consoles.attached_of(&path).is_none());
    c.wb.poll_toolbars();
    assert!(!c.wb.tabs[i].toolbar.as_ref().unwrap().bar.follows_editor());
    c.wb.update_completion(&mut text);
    assert!(c.wb.completion_assist.is_none(), "no module's list");
}

/// A function's parameter help in a console (48c): its signature from the console's source
/// (SQLite's `substr`), the argument typed lit, following the cursor; a plain `.sql` file
/// has the known functions' too.
#[test]
fn a_console_s_function_calls_show_their_parameters() {
    let mut c = console("signature");
    std::fs::write(c.dir.join("shop.db"), b"").unwrap();
    let mut text = TextSystem::new();
    // A comment with a quote before it (SQL's `--`: no string opened).
    let sql = "-- the customers' initials\nSELECT substr";
    let len = c.wb.tabs[c.wb.active].doc.buffer().len();
    assert!(workbench_model::EditorHost::edit_active(&mut c.wb, 0..len, sql));
    workbench_model::EditorHost::select_active(&mut c.wb, sql.len()..sql.len());
    c.wb.type_text(&mut text, "(");
    let card = c.wb.param_hint.card().cloned().expect("its parameters");
    assert_eq!(card.code, "substr(text, start, length) → text");
    assert_eq!(card.active, Some(0));
    c.wb.type_text(&mut text, "name,");
    assert_eq!(c.wb.param_hint.card().and_then(|c| c.active), Some(1));
    c.wb.type_text(&mut text, " 2)");
    assert!(c.wb.param_hint.card().is_none(), "out of the call: hidden");
    // A plain SQL file: the same functions, by the language.
    c.wb.open_file(&c.dir.join("plain.sql")).unwrap();
    let len = c.wb.tabs[c.wb.active].doc.buffer().len();
    assert!(workbench_model::EditorHost::edit_active(&mut c.wb, 0..len, "SELECT coalesce"));
    workbench_model::EditorHost::select_active(&mut c.wb, 15..15);
    c.wb.type_text(&mut text, "(");
    assert_eq!(c.wb.param_hint.card().map(|c| c.code.as_str()), Some("coalesce(value, …)"));
}
