//! Files shown by the modules' viewers: who's asked first (the binary files' notice last),
//! a data file opened as data, a CSV's data shown first with its text kept as a text
//! file's (saved, reloaded).

use super::*;

fn workbench() -> Workbench {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    wb
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("viewers-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    crate::workbench::canonical(&dir)
}

#[test]
fn the_binary_files_notice_is_asked_last() {
    let wb = workbench();
    let ids: Vec<&str> = wb.support.viewers.iter().map(|v| v.id()).collect();
    assert_eq!(ids.last(), Some(&"binary"), "{ids:?}");
    assert!(ids.contains(&"data"));
}

#[test]
fn a_sqlite_file_opens_as_data_and_other_bytes_as_binary() {
    let dir = scratch("sqlite");
    let db = dir.join("shop.db");
    let mut bytes = b"SQLite format 3\0".to_vec();
    bytes.extend([0u8; 84]);
    std::fs::write(&db, &bytes).unwrap();
    let blob = dir.join("other.db");
    std::fs::write(&blob, [0u8, 1, 2, 3, 0, 9]).unwrap();
    let mut wb = workbench();
    wb.open_file(&db).unwrap();
    let v = wb.tabs[wb.active].viewer.as_ref().unwrap();
    assert_eq!((v.provider, v.kind), ("data", ui_workbench::ViewerKind::Replace));
    wb.open_file(&blob).unwrap();
    let v = wb.tabs[wb.active].viewer.as_ref().unwrap();
    assert_eq!(v.provider, "binary", "not a database: the notice");
    let _ = std::fs::remove_dir_all(dir);
}

/// A `.drawio` file opens in our diagram editor while it's shown, and in draw.io's tab while
/// it's hidden (the default: docs/plan/82) — the file's own tab either way.
#[test]
fn a_diagram_opens_in_our_editor_or_in_draw_io_while_ours_is_hidden() {
    let dir = scratch("drawio");
    let file = dir.join("flow.drawio");
    std::fs::write(&file, "<mxfile><diagram id=\"a\" name=\"Page-1\"></diagram></mxfile>").unwrap();
    for (hidden, opener) in [(false, "drawio"), (true, "diagram-web")] {
        let selection = extensions::ModuleSelection {
            hidden: if hidden { vec!["tool.diagram-editor".into()] } else { Vec::new() },
            ..Default::default()
        };
        let (support, _) = crate::modules::load(&selection);
        let mut wb = Workbench::new(Arc::new(support), Ports::local());
        wb.forget_projects();
        wb.resize(PhysicalSize::new(1200, 800), 1.0);
        wb.open_file(&file).unwrap();
        let tab = &wb.tabs[wb.active];
        let v = tab.viewer.as_ref().unwrap();
        assert_eq!((v.provider, v.kind), (opener, ui_workbench::ViewerKind::Replace));
        assert_eq!(tab.doc.path(), Some(file.as_path()), "the file's own tab");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_csv_shows_its_data_first_and_its_text_stays_a_text_files() {
    let dir = scratch("csv");
    let csv = dir.join("people.csv");
    std::fs::write(&csv, "id,name\n1,Ana\n").unwrap();
    let mut wb = workbench();
    wb.open_file(&csv).unwrap();
    let tab = &wb.tabs[wb.active];
    let v = tab.viewer.as_ref().unwrap();
    assert_eq!((v.provider, v.kind), ("data", ui_workbench::ViewerKind::Preview));
    assert_eq!(v.mode, ViewMode::Preview, "the data first");
    assert!(tab.holds_text());
    assert_eq!(tab.doc.buffer().to_string(), "id,name\n1,Ana\n", "the text is there, as it was");
    // Edited (from the editor, a toggle away), it's saved as any text file.
    wb.set_file_text(&csv, "id,name\n1,Ana\n2,Dan\n");
    wb.last_edit = Some(std::time::Instant::now() - crate::autosave::IDLE);
    assert!(wb.autosave(false));
    assert_eq!(std::fs::read_to_string(&csv).unwrap(), "id,name\n1,Ana\n2,Dan\n");
    // Written by another program (a commit of its data): the clean text follows.
    std::fs::write(&csv, "id,name\n1,Ana\n2,Dan\n3,Ion\n").unwrap();
    assert!(wb.reload_written(std::slice::from_ref(&csv)));
    assert_eq!(wb.tabs[wb.active].doc.buffer().to_string(), "id,name\n1,Ana\n2,Dan\n3,Ion\n");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_views_own_menu_rows_come_back_to_it() {
    let entries = vec![
        ui_workbench::ViewMenuEntry::item("grid:copy:tsv", "Copy"),
        ui_workbench::ViewMenuEntry::Separator,
        ui_workbench::ViewMenuEntry::Submenu {
            label: "As".into(),
            icon: None,
            entries: vec![ui_workbench::ViewMenuEntry::item("grid:copy:json", "JSON")],
        },
    ];
    let menu = view_menu(entries);
    let ids: Vec<String> = menu
        .iter()
        .flat_map(|e| match e {
            ui_workbench::MenuEntry::Item { id, .. } => vec![id.clone()],
            ui_workbench::MenuEntry::Submenu { entries, .. } => entries
                .iter()
                .filter_map(|e| match e {
                    ui_workbench::MenuEntry::Item { id, .. } => Some(id.clone()),
                    _ => None,
                })
                .collect(),
            ui_workbench::MenuEntry::Separator => Vec::new(),
        })
        .collect();
    assert_eq!(ids, ["view-menu:grid:copy:tsv", "view-menu:grid:copy:json"]);
}

/// The middle of a rectangle.
fn middle(r: Rect) -> Point {
    Point::new(r.min_x() + r.size.width / 2.0, r.min_y() + r.size.height / 2.0)
}

#[test]
fn in_split_the_half_clicked_has_the_keys() {
    let dir = scratch("split-keys");
    let csv = dir.join("people.csv");
    std::fs::write(&csv, "id,name\n1,Ana\n").unwrap();
    let mut text = TextSystem::new();
    let mut wb = workbench();
    wb.open_file(&csv).unwrap();
    wb.set_viewer_mode_split();
    let (viewer, editor) = {
        let tab = &wb.tabs[wb.active];
        (tab.viewer.as_ref().unwrap().bounds, tab.view.bounds())
    };
    // The text's half has them first: typing edits it, its commands are the editor's.
    let before = wb.tabs[wb.active].doc.version();
    wb.mouse_down(&mut text, middle(editor), false, false, false, 1);
    assert!(wb.editor_focused());
    wb.type_text(&mut text, "x");
    assert_ne!(wb.tabs[wb.active].doc.version(), before, "typed in the text");
    assert_eq!(wb.key_place(), Some(command_model::When::Editor));
    // A click in the grid's half: its keys (the view's commands, nothing typed in the text).
    wb.mouse_down(&mut text, middle(viewer), false, false, false, 1);
    assert!(!wb.editor_focused());
    assert_eq!(wb.key_place(), Some(command_model::When::View("data")));
    let version = wb.tabs[wb.active].doc.version();
    wb.type_text(&mut text, "y");
    wb.edit_key(&mut text, EditKey::Enter { shift: false });
    wb.edit_key(&mut text, EditKey::Backspace { word: false });
    assert_eq!(wb.tabs[wb.active].doc.version(), version, "the text untouched");
    assert!(!wb.select_all(&mut text) || wb.tabs[wb.active].doc.selections().primary().is_empty());
    // Back in the text: its keys again.
    wb.mouse_down(&mut text, middle(editor), false, false, false, 1);
    assert!(wb.editor_focused());
    wb.type_text(&mut text, "z");
    assert_ne!(wb.tabs[wb.active].doc.version(), version);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_preview_without_keys_leaves_them_to_the_text() {
    let dir = scratch("split-markdown");
    let md = dir.join("notes.md");
    std::fs::write(&md, "# Notes\n").unwrap();
    let mut text = TextSystem::new();
    let mut wb = workbench();
    wb.open_file(&md).unwrap();
    wb.set_viewer_mode_split();
    let Some(viewer) = wb.tabs[wb.active].viewer.as_ref().map(|v| v.bounds) else {
        // No preview for Markdown in this build: nothing to check.
        return;
    };
    wb.mouse_down(&mut text, middle(viewer), false, false, false, 1);
    let version = wb.tabs[wb.active].doc.version();
    wb.type_text(&mut text, "x");
    assert_ne!(wb.tabs[wb.active].doc.version(), version, "typed in the text, as before");
    assert_eq!(wb.key_place(), Some(command_model::When::Editor));
    let _ = std::fs::remove_dir_all(dir);
}

/// A project in a scratch folder (`a.txt`), opened.
fn with_project(name: &str) -> (Workbench, PathBuf) {
    let dir = scratch(name);
    std::fs::write(dir.join("a.txt"), "a").unwrap();
    let mut wb = workbench();
    wb.open_project(&dir).unwrap();
    paint(&mut wb);
    (wb, dir)
}

fn paint(wb: &mut Workbench) {
    wb.paint(&mut TextSystem::new(), &mut render_api::Scene::default());
}

fn is_task_manager(tab: &Tab) -> bool {
    tab.doc.path().and_then(Path::extension).is_some_and(|e| e == task_manager::EXTENSION)
}

/// How many Task Manager tabs the window has, in every group.
fn task_managers(wb: &Workbench) -> usize {
    all_tabs(&wb.tabs, &wb.editors.groups).filter(|t| is_task_manager(t)).count()
}

fn active_is_task_manager(wb: &Workbench) -> bool {
    wb.tabs.get(wb.active).is_some_and(is_task_manager)
}

fn source(wb: &Workbench) -> Arc<dyn workbench_model::ResourceSource> {
    Arc::clone(&wb.task_data) as Arc<dyn workbench_model::ResourceSource>
}

fn hub(wb: &Workbench) -> Option<task_manager::HubInfo> {
    task_manager::Hub::info(&source(wb))
}

/// Who holds the window's picture besides the window (a shown view's hub).
fn held(wb: &Workbench) -> usize {
    Arc::strong_count(&wb.task_data) - 1
}

fn wait(mut done: impl FnMut() -> bool) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while std::time::Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    false
}

/// The command opens one tab per window: again, it brings that one to the front — in
/// whichever group it is. It's never kept in the session.
#[test]
fn the_task_manager_opens_once_and_comes_to_the_front_in_any_group() {
    let (mut wb, dir) = with_project("tasks-once");
    assert_eq!(wb.run_command(task_manager::OPEN), Some(true));
    assert_eq!(wb.run_command(task_manager::OPEN), Some(true));
    assert_eq!(task_managers(&wb), 1, "one tab");
    assert!(active_is_task_manager(&wb));
    // Moved to a group of its own, the file's group current again.
    wb.open_file(&dir.join("a.txt")).unwrap();
    let at = wb.tabs.iter().position(is_task_manager).unwrap();
    assert!(wb.split_tab(at, Split::Right, true));
    let its_group = wb.editors.current;
    let other = 1 - its_group;
    assert!(wb.focus_group(other));
    assert!(!active_is_task_manager(&wb));
    assert_eq!(wb.run_command(task_manager::OPEN), Some(true));
    assert_eq!(wb.editors.current, its_group, "its group made current");
    assert!(active_is_task_manager(&wb));
    assert_eq!(task_managers(&wb), 1, "still one");
    // Not remembered: the session has only the file.
    let session = wb.current_session().unwrap();
    let saved: Vec<PathBuf> =
        session.groups.iter().flat_map(|g| g.tabs.iter().map(|t| t.path.clone())).collect();
    assert_eq!(saved, [PathBuf::from("a.txt")]);
    let _ = std::fs::remove_dir_all(dir);
}

/// A click, the command it asks for run (as the app's loop does).
fn click(wb: &mut Workbench, text: &mut TextSystem, p: Point) {
    if let MouseResult::Command(id) = wb.mouse_down(text, p, false, false, false, 1) {
        wb.run_command_named(id);
    }
}

/// The status bar's CPU / memory / GPU and the right strip's button open it.
#[test]
fn the_status_bar_pins_its_resources_and_the_button_opens_the_task_manager() {
    let (mut wb, dir) = with_project("tasks-open");
    wb.set_resources(
        crate::resources::Sample { cpu: 3.0, ram_bytes: 1 << 28, ..Default::default() },
        Some(render_api::GpuMemory { shared: 0, window: 1024 }),
    );
    paint(&mut wb);
    let mut text = TextSystem::new();
    let find = |wb: &Workbench, hit: &dyn Fn(&Workbench, Point) -> bool| {
        let (w, h) = (wb.size.width as f32, wb.size.height as f32);
        (0..(h as i32 / 2))
            .flat_map(|y| {
                (0..(w as i32 / 2)).map(move |x| Point::new(x as f32 * 2.0, y as f32 * 2.0))
            })
            .find(|&p| hit(wb, p))
    };
    // The resources' meter: its click pins them (the pills in the corner), another unpins.
    let cpu = find(&wb, &|wb, p| wb.status.hit(p) == Some(crate::workbench::status::RESOURCES))
        .expect("the status bar's resources");
    let was = wb.resources_pinned();
    click(&mut wb, &mut text, cpu);
    paint(&mut wb);
    assert_eq!(wb.resources_pinned(), !was);
    assert_eq!(wb.resource_pills.is_open(), !was);
    click(&mut wb, &mut text, cpu);
    paint(&mut wb);
    assert_eq!(wb.resources_pinned(), was);
    assert_eq!(wb.resource_pills.is_open(), was);
    assert_eq!(task_managers(&wb), 0, "no Task Manager from the meter");
    let button = find(&wb, &|wb, p| wb.tools.right_strip.hit(p) == Some(task_manager::TOOL))
        .expect("its button on the right");
    // At the right strip's bottom (the window's bottom right corner), below the others
    // (plan 55: its provider asks for the strip's end).
    let strip = wb.tools.right_strip.bounds();
    let (rect, _) = wb.tools.right_strip.hit_rect(button).unwrap();
    assert!(rect.max_y() > strip.max_y() - 60.0, "{rect:?} in {strip:?}");
    let others =
        find(&wb, &|wb, p| wb.tools.right_strip.hit(p).is_some_and(|id| id != task_manager::TOOL))
            .expect("the strip's top group");
    assert!(others.y < rect.min_y());
    click(&mut wb, &mut text, button);
    assert_eq!(task_managers(&wb), 1);
    assert!(active_is_task_manager(&wb));
    let _ = std::fs::remove_dir_all(dir);
}

/// Split to the right: two views, one collector (one hub, one thread) for both.
#[test]
fn a_split_task_manager_has_two_views_and_one_collector() {
    let (mut wb, dir) = with_project("tasks-split");
    wb.run_command(task_manager::OPEN);
    paint(&mut wb);
    let at = wb.active;
    assert!(wb.split_tab(at, Split::Right, false));
    paint(&mut wb);
    assert_eq!(task_managers(&wb), 2, "a mirror in the new group");
    let info = hub(&wb).expect("its hub");
    assert_eq!((info.views, info.shown, info.thread), (2, 2, true), "{info:?}");
    // Both get the window's tables.
    assert!(wait(|| hub(&wb).is_some_and(|h| h.looks >= 1)));
    let _ = std::fs::remove_dir_all(dir);
}

/// Its actions from the palette (remappable, its keys only in its tab) reach the view that
/// has the keys — never its mirror in the other group; what one copies reaches the app.
#[test]
fn the_task_managers_actions_reach_the_active_view_only() {
    let (mut wb, dir) = with_project("tasks-actions");
    wb.run_command(task_manager::OPEN);
    paint(&mut wb);
    let at = wb.active;
    assert!(wb.split_tab(at, Split::Right, false));
    paint(&mut wb);
    let paused = |wb: &Workbench| -> Vec<bool> {
        all_tabs(&wb.tabs, &wb.editors.groups)
            .filter(|t| is_task_manager(t))
            .map(|t| t.viewer.as_ref().unwrap().viewer.status().is_some())
            .collect()
    };
    assert_eq!(paused(&wb), [false, false]);
    assert_eq!(wb.key_place(), Some(command_model::When::View("task-manager")));
    assert_eq!(wb.run_command(command_model::CommandId("tasks.pause")), Some(true));
    let now = paused(&wb);
    assert_eq!(now.iter().filter(|p| **p).count(), 1, "one view paused: {now:?}");
    let active = wb.tabs[wb.active].viewer.as_ref().unwrap().viewer.status().is_some();
    assert!(active, "the one with the keys");
    assert_eq!(wb.run_command(command_model::CommandId("tasks.pause")), Some(true));
    assert_eq!(paused(&wb), [false, false], "resumed");
    // Copy the row: the text reaches the app (its clipboard).
    let copied = wait(|| {
        // Its first look (the window's parts), then ↓ to the first part.
        wb.poll_viewers();
        paint(&mut wb);
        wb.edit_key(&mut TextSystem::new(), EditKey::Move { motion: Motion::Down, extend: false });
        wb.run_command(command_model::CommandId("tasks.copy_row")) == Some(true)
    });
    assert!(copied);
    assert!(matches!(
        wb.take_request(),
        Some(MouseResult::ViewerRequest(ui_workbench::AppRequest::CopyText(row))) if row.contains('\t')
    ));
    let _ = std::fs::remove_dir_all(dir);
}

/// Behind another tab, its collector sleeps and the window isn't watched; in front again,
/// both come back; closed, the thread ends and the picture is let go.
#[test]
fn a_hidden_task_manager_keeps_nothing_running() {
    let (mut wb, dir) = with_project("tasks-hidden");
    assert_eq!(held(&wb), 0, "closed: nobody");
    wb.run_command(task_manager::OPEN);
    paint(&mut wb);
    assert!(held(&wb) > 0, "shown: its source is held");
    assert!(wb.task_data.watched());
    // Another tab in front of it.
    wb.open_file(&dir.join("a.txt")).unwrap();
    paint(&mut wb);
    assert_eq!(hub(&wb).map(|h| (h.shown, h.thread)), Some((0, true)), "asleep");
    // Nobody holds it: not watched for it (another test's view of every project, running
    // meanwhile, may still make every window watched for a few seconds).
    assert!(wait(|| held(&wb) == 0), "hidden: let go");
    // In front again.
    wb.run_command(task_manager::OPEN);
    paint(&mut wb);
    assert_eq!(hub(&wb).map(|h| h.shown), Some(1));
    assert!(held(&wb) > 0);
    // Closed: the thread ends, nothing holds the picture.
    let at = wb.active;
    wb.close_tab(at);
    paint(&mut wb);
    assert!(wait(|| hub(&wb).is_none_or(|h| !h.thread)), "the thread ended");
    assert!(wait(|| held(&wb) == 0));
    let _ = std::fs::remove_dir_all(dir);
}

/// The window hidden: its Task Manager pauses; shown again, it goes on.
#[test]
fn a_hidden_window_pauses_its_task_manager() {
    let (mut wb, dir) = with_project("tasks-occluded");
    wb.run_command(task_manager::OPEN);
    paint(&mut wb);
    assert_eq!(hub(&wb).map(|h| h.shown), Some(1));
    wb.set_occluded(true);
    assert_eq!(hub(&wb).map(|h| (h.shown, h.holds)), Some((0, false)));
    wb.set_occluded(false);
    assert_eq!(hub(&wb).map(|h| (h.shown, h.holds)), Some((1, true)));
    let _ = std::fs::remove_dir_all(dir);
}

/// Two windows: each its own collector, looking at its own project.
#[test]
fn each_window_has_its_own_task_manager() {
    let (mut first, a) = with_project("tasks-first");
    let (mut second, b) = with_project("tasks-second");
    first.run_command(task_manager::OPEN);
    second.run_command(task_manager::OPEN);
    paint(&mut first);
    paint(&mut second);
    let (one, two) = (hub(&first).unwrap(), hub(&second).unwrap());
    assert_eq!((one.views, two.views), (1, 1), "a hub each");
    use workbench_model::ResourceSource;
    first.refresh_task_data();
    second.refresh_task_data();
    let own = first.task_data.snapshot();
    assert!(own.parts.iter().all(|p| p.project.is_none() || p.project == Some(first.data.id)));
    assert!(own.parts.iter().all(|p| p.project != Some(second.data.id)));
    drop((first, second));
    let _ = std::fs::remove_dir_all(a);
    let _ = std::fs::remove_dir_all(b);
}

/// The parts are listed in their kinds: the index under Indexing, the terminal under
/// Terminals, git and the formatters under External programs — those with nothing running
/// (no session, no program) not listed at all.
#[test]
fn the_parts_come_in_their_groups() {
    use workbench_model::ResourceSource;
    use workbench_model::resources::{APP, INDEXING, MODULES, PROGRAMS, TERMINALS};
    let (wb, dir) = with_project("tasks-groups");
    // The app's own (filled in only while watched) and the modules' reporters'.
    let mut parts = wb.app_parts();
    parts.extend(wb.task_data.snapshot().parts);
    // (Each module also has its own row, among the modules, named as the module.)
    let group = |owner: &str, name: &str| {
        let part = |p: &&workbench_model::ResourceUse| p.owner == owner && p.name == name;
        let found: Vec<_> = parts.iter().filter(part).map(|p| p.group).collect();
        found.iter().copied().find(|g| *g != MODULES).or(found.first().copied())
    };
    assert_eq!(group("app", &i18n::t("tasks.index")), Some(INDEXING));
    assert_eq!(group("app", &i18n::t("tasks.ui")), Some(APP));
    // Not the module's own row (named as the module): its part, if it has one now.
    let part_group = |owner: &str, name: &str| group(owner, name).filter(|g| *g != MODULES);
    assert!(matches!(part_group("terminal", &i18n::t("tasks.terminal")), None | Some(TERMINALS)));
    // Git's: listed once it follows a project (it loads when first needed).
    assert!(matches!(part_group("vcs.git", &i18n::t("tasks.git_programs")), None | Some(PROGRAMS)));
    assert!(matches!(part_group("vcs.git", &i18n::t("tasks.git")), None | Some(PROGRAMS)));
    assert!(matches!(part_group("quality", &i18n::t("tasks.quality")), None | Some(PROGRAMS)));
    let _ = std::fs::remove_dir_all(dir);
}

/// A view in the text's place (the Task Manager) has no text caret for the system: its
/// input source's popup (macOS: "US") never shows over the view.
#[test]
fn a_view_in_the_text_s_place_tells_no_caret() {
    let (mut wb, dir) = with_project("tasks-caret");
    wb.run_command(task_manager::OPEN);
    paint(&mut wb);
    assert!(is_task_manager(&wb.tabs[wb.active]));
    assert_eq!(wb.caret_rect(&mut TextSystem::new()), None);
    let _ = std::fs::remove_dir_all(dir);
}

/// File `name`'s local history opened (as its command does), from the current group.
fn open_history(wb: &mut Workbench, dir: &Path, name: &str) {
    let data = local_history::HistoryData {
        store: Arc::new(history_fs::DiskHistory::new(dir.join(".history"))),
        path: name.to_owned(),
        file: dir.join(name),
    };
    let title = format!("{name} history");
    assert!(workbench_model::ViewHost::open_view(wb, &title, local_history::EXTENSION, &data));
}

fn histories_in(tabs: &[Tab]) -> usize {
    tabs.iter().filter(|t| is_view_of(t, local_history::EXTENSION)).count()
}

/// A version clicked in a history compares above it (its group split once); another file's
/// history, asked for from the editor above, joins the first one's tabs below — no new
/// split for each — and its versions compare in the same group above.
#[test]
fn another_file_s_history_opens_beside_the_first() {
    let (mut wb, dir) = with_project("history-beside");
    std::fs::write(dir.join("b.txt"), "b").unwrap();
    wb.open_file(&dir.join("a.txt")).unwrap();
    open_history(&mut wb, &dir, "a.txt");
    let version = |name: &str| {
        diff::CompareInput::files(
            (dir.join(format!("{name}.old")), "old".into()),
            (dir.join(name), "new".into()),
        )
    };
    wb.compare_beside(version("a.txt"), "a.txt · old");
    assert_eq!(wb.editors.groups.len(), 2);
    let below = wb.editors.current;
    // Back in the editor above (where the comparison is), another file's history.
    assert!(wb.focus_group(1 - below));
    wb.open_file(&dir.join("b.txt")).unwrap();
    open_history(&mut wb, &dir, "b.txt");
    assert_eq!(wb.editors.current, below, "beside the first one");
    assert_eq!(histories_in(&wb.tabs), 2);
    wb.compare_beside(version("b.txt"), "b.txt · old");
    assert_eq!(wb.editors.groups.len(), 2, "no new split");
    assert_eq!(wb.editors.current, below);
    // Asked again for the first file (from above): its tab comes to the front below.
    assert!(wb.focus_group(1 - below));
    open_history(&mut wb, &dir, "a.txt");
    assert_eq!(wb.editors.current, below);
    assert_eq!(histories_in(&wb.tabs), 2, "not opened twice");
    let _ = std::fs::remove_dir_all(dir);
}

/// "Show Local History" opens the file's history in the bottom window, a tab for each file
/// (asked again: the same tab); a version clicked there compares in the editor above,
/// which isn't split for it.
#[test]
fn local_histories_are_tabs_of_the_bottom_window() {
    let (mut wb, dir) = with_project("history-window");
    std::fs::write(dir.join("b.txt"), "b").unwrap();
    for name in ["a.txt", "b.txt", "a.txt"] {
        wb.open_file(&dir.join(name)).unwrap();
        assert_eq!(wb.run_command(local_history::SHOW_HISTORY), Some(true), "{name}");
    }
    assert!(wb.bottom_shows(local_history::TOOL));
    let window = wb.tools.window().expect("its window");
    let titles: Vec<&str> = window.views().map(|v| v.title.as_str()).collect();
    assert_eq!(titles.len(), 2, "a tab a file: {titles:?}");
    assert_eq!(wb.editors.groups.len(), 1, "nothing split in the editor");
    let input = diff::CompareInput::files(
        (dir.join("a.txt.old"), "old".into()),
        (dir.join("a.txt"), "a".into()),
    );
    let mut text = TextSystem::new();
    wb.panel_action(
        &mut text,
        ui_workbench::ViewerAction::CompareBeside(Box::new(input), "a · old".into()),
    );
    assert_eq!(wb.editors.groups.len(), 1, "compared in the editor, not a new zone");
    assert!(
        wb.tabs[wb.active]
            .doc
            .path()
            .is_some_and(|p| p.extension().is_some_and(|e| e == diff::COMPARE_VIEW))
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// A program another module asks the terminal to run (an agent's session) opens as an
/// editor tab with the icon asked for; it takes the keyboard whole there (keys, text,
/// selection: as the bottom terminal), is never reopened as a shell at the next start, and
/// asked again comes to the front instead of starting twice.
#[test]
fn a_program_runs_in_a_terminal_tab_of_the_editor() {
    use contracts::terminal::{RunInTerminal, TerminalPlace};
    let (mut wb, dir) = with_project("agent-tab");
    assert!(workbench_model::presence::request::<RunInTerminal>(), "the terminal answers it");
    let run = || RunInTerminal {
        launch: agents::Launch::new("/bin/cat", &[], None),
        key: "claude-1/../x".into(),
        title: "fix src/Auth/Login.php".into(),
        icon: Some("terminal"),
        place: TerminalPlace::Editor,
    };
    let host: &mut dyn workbench_model::WorkbenchHost = &mut wb;
    assert_eq!(host.ask(run()), Some(true));
    let terminal = |t: &Tab| {
        t.doc.path().and_then(Path::extension).is_some_and(|e| e == terminal_panel::EXTENSION)
    };
    assert!(wb.tabs.get(wb.active).is_some_and(terminal), "an editor tab");
    let viewer = &wb.tabs[wb.active].viewer.as_ref().unwrap().viewer;
    assert_eq!(viewer.tab_icon(), Some("terminal"));
    assert!(!viewer.restorable(), "its module takes it up again, not the terminal");
    assert!(wb.view_takes_keyboard(), "the keyboard is its, whole");
    let tab = &wb.tabs[wb.active];
    assert_eq!(tab.title(), "fix src/Auth/Login.php", "shown by its title");
    let name = tab.doc.path().and_then(Path::file_stem).unwrap().to_string_lossy().into_owned();
    assert_eq!(name, "claude-1----x", "known by its key, never a path");
    assert_eq!(wb.keyed_view_kind(), Some(terminal_panel::VIEWER), "its own keys (⌘K)");
    let host: &mut dyn workbench_model::WorkbenchHost = &mut wb;
    assert_eq!(host.ask(run()), Some(true));
    assert_eq!(wb.tabs.iter().filter(|t| terminal(t)).count(), 1, "not started twice");
    let _ = std::fs::remove_dir_all(dir);
}

/// The MCP server turned on in the settings: an agent's call over the loopback (with the
/// token) is queued, run by the window at its next poll on its own thread, and answered —
/// the open files said; without the token, refused.
#[test]
fn an_agent_s_call_is_run_by_the_window() {
    let (mut wb, dir) = with_project("mcp-call");
    // The server is the process's: no other test's window applies its settings (which have
    // it off) meanwhile.
    let _alone = crate::modules::hold_process();
    // The window worked in (the calls without a file are its).
    wb.set_decorations_window(true, true, false);
    // The token in memory (never the user's keychain).
    mcp_server::secret::use_secrets(std::sync::Arc::new(MemorySecret::default()));
    wb.open_file(&dir.join("a.txt")).unwrap();
    wb.settings_choose("mcp-server", "enabled", 1);
    // Started on a worker: listening soon; its port saved at the window's next poll.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while mcp_server_url().is_none() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let url = mcp_server_url().expect("listening");
    let token = mcp_server::listening().map(|(_, t)| t).unwrap();
    assert!(
        crate::config::settings().get(&mcp_server::settings::TOKEN).is_empty(),
        "not in the file"
    );
    // (Its port saved in the settings: the module's own test — here the settings are each
    // test thread's while the server is the process's, so any test's window may save it.)
    let call = |tool: &str| {
        format!(
            r#"{{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{{"name":"{tool}","arguments":{{}}}}}}"#
        )
    };
    let post = move |auth: Option<String>, body: String| {
        let addr = url.trim_start_matches("http://").trim_end_matches("/mcp").to_owned();
        let auth = auth.map(|t| format!("Authorization: Bearer {t}\r\n")).unwrap_or_default();
        let raw = format!(
            "POST /mcp HTTP/1.1\r\nHost: {addr}\r\n{auth}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let mut stream = std::net::TcpStream::connect(&addr).unwrap();
        std::io::Write::write_all(&mut stream, raw.as_bytes()).unwrap();
        let mut answer = String::new();
        std::io::Read::read_to_string(&mut stream, &mut answer).unwrap();
        answer
    };
    let refused = post(None, call("get_open_files"));
    assert!(refused.starts_with("HTTP/1.1 401"), "{refused}");
    let mut ask = |body: String| {
        let (post, token) = (post.clone(), token.clone());
        let asking = std::thread::spawn(move || post(Some(token), body));
        // The window's polls (the app's loop) until the call is answered.
        let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !asking.is_finished() && std::time::Instant::now() < end {
            wb.poll_viewers();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        asking.join().unwrap()
    };
    let answer = ask(call("get_open_files"));
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    assert!(answer.contains("a.txt"), "{answer}");
    // A module's tool (Git's), found through the registry; read on a worker.
    let answer = ask(call("git_status"));
    assert!(
        answer.contains("isn't in a git repository") && answer.contains("\"isError\":true"),
        "{answer}"
    );
    wb.settings_choose("mcp-server", "enabled", 0);
    assert!(mcp_server_url().is_none(), "stopped");
    let _ = std::fs::remove_dir_all(dir);
}

/// Where the MCP server listens, while it does.
/// A secret store in memory (never the keychain).
#[derive(Default)]
struct MemorySecret(std::sync::Mutex<Option<String>>);

impl kernel::SecretStore for MemorySecret {
    fn get(&self, _: &str) -> Option<kernel::Secret> {
        self.0.lock().unwrap().as_deref().map(kernel::Secret::new)
    }
    fn set(&self, _: &str, secret: &str) -> Result<(), String> {
        *self.0.lock().unwrap() = Some(secret.to_owned());
        Ok(())
    }
    fn remove(&self, _: &str) -> Result<(), String> {
        *self.0.lock().unwrap() = None;
        Ok(())
    }
}

fn mcp_server_url() -> Option<String> {
    mcp_server::listening().map(|(url, _)| url)
}

/// A session's terminal tab renamed by the user (Rename Tab…, the file's dialog): its tab's
/// name changes, and its session is asked to be renamed too (whoever follows it).
#[test]
fn a_session_s_tab_is_renamed_from_its_dialog() {
    use contracts::terminal::{RunInTerminal, TerminalPlace};
    let (mut wb, dir) = with_project("tab-rename");
    let host: &mut dyn workbench_model::WorkbenchHost = &mut wb;
    let run = RunInTerminal {
        launch: agents::Launch::new("/bin/cat", &[], None),
        key: "fake-session-1".into(),
        title: "Claude Code".into(),
        icon: None,
        place: TerminalPlace::Editor,
    };
    assert_eq!(host.ask(run), Some(true));
    let i = wb.active;
    assert!(wb.tab_renamable(i));
    wb.ask_view_rename(i);
    // Typed in the dialog's field (with what a paste may bring).
    wb.dialogs.dialog.input.as_mut().unwrap().set_text("  test-new-tab\n");
    assert!(wb.answer_dialog(Some("rename")));
    assert_eq!(wb.tabs[wb.active].title(), "test-new-tab");
    // Its session asked to be renamed too (as the view's own action).
    let asked = wb.tabs[wb.active].viewer.as_mut().unwrap().viewer.take_action();
    assert!(format!("{asked:?}").contains("RenameSession"), "{asked:?}");
    let _ = std::fs::remove_dir_all(dir);
}

/// A large Markdown file opened (its preview laid out on a worker): once shown, nothing
/// keeps loading — the app would draw it 20 times a second for nothing.
#[test]
fn a_shown_markdown_file_stops_loading() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 2.0);
    wb.set_waker(Arc::new(|| {}));
    let dir = std::env::temp_dir().join(format!("md-loading-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("README.md");
    let source: String = (0..1500)
        .map(|i| {
            format!(
                "## Section {i}\n\nSome **bold** text and `code` with a [link](https://example.com) in paragraph {i}.\n\n- item one\n- item two\n\n"
            )
        })
        .collect();
    std::fs::write(&file, source).unwrap();
    wb.open_file(&file).unwrap();
    let mut text = TextSystem::new();
    let started = std::time::Instant::now();
    let mut why = String::new();
    while started.elapsed() < std::time::Duration::from_secs(15) {
        wb.paint(&mut text, &mut Scene::default());
        wb.poll_viewers();
        if !wb.loading() {
            break;
        }
        why = format!(
            "tools {} toolbars {} sides {} viewer {:?}",
            wb.tool_window_loading(),
            wb.toolbars_loading(),
            wb.tools.sides.iter().any(|s| s.panel.is_loading()),
            wb.tabs.get(wb.active).and_then(|t| t.viewer.as_ref()).map(|v| v.viewer.is_loading()),
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let loading = wb.loading();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!loading, "still loading: {why}");
}

/// A view with an app page: what it was told, what it has to send.
struct AppView {
    page: u128,
    told: Arc<std::sync::Mutex<Vec<ui_workbench::WebEvent>>>,
    sends: Vec<(u128, String)>,
}

impl ui_workbench::Viewer for AppView {
    fn paint(&mut self, _: ui_workbench::ViewerPaint<'_>) {}
    fn web_page(&self) -> Option<ui_workbench::WebPage> {
        Some(ui_workbench::WebPage {
            id: self.page,
            source: ui_workbench::WebSource::App {
                root: Arc::from(Path::new("/apps/one")),
                entry: "index.html".into(),
            },
            area: Rect::new(0.0, 0.0, 10.0, 10.0),
        })
    }
    fn web_event(&mut self, page: u128, event: ui_workbench::WebEvent) {
        assert_eq!(page, self.page);
        self.told.lock().unwrap().push(event);
    }
    fn take_web_sends(&mut self) -> Vec<(u128, String)> {
        std::mem::take(&mut self.sends)
    }
}

#[test]
fn an_app_pages_messages_reach_the_view_that_has_it() {
    let mut wb = workbench();
    let (one, two): (Arc<std::sync::Mutex<_>>, Arc<std::sync::Mutex<_>>) =
        (Arc::default(), Arc::default());
    for (page, told) in [(1, &one), (2, &two)] {
        let view =
            AppView { page, told: Arc::clone(told), sends: vec![(page, format!("to {page}"))] };
        let mut doc = Document::new("");
        doc.mark_saved(PathBuf::from(format!("/virtual/app-{page}")));
        let mut tab = Tab::new(doc, wb.code_font.clone(), None, LanguageId::PLAIN_TEXT);
        tab.viewer = Some(TabViewer::new(
            Box::new(view),
            "test",
            ui_workbench::ViewerKind::Replace,
            ViewMode::Preview,
        ));
        wb.push_tab(tab);
    }
    let message = ui_workbench::WebEvent::Message("hello".into());
    assert!(wb.web_event(2, message.clone()));
    assert!(wb.web_event(1, ui_workbench::WebEvent::OpenLink("https://example.com/".into())));
    assert!(!wb.web_event(9, message.clone()), "no view has it: let go");
    assert_eq!(*two.lock().unwrap(), [message]);
    assert_eq!(
        *one.lock().unwrap(),
        [ui_workbench::WebEvent::OpenLink("https://example.com/".into())]
    );
    let mut sends = wb.take_web_sends();
    sends.sort();
    assert_eq!(sends, [(1, "to 1".to_owned()), (2, "to 2".to_owned())]);
    assert!(wb.take_web_sends().is_empty(), "taken once");
}

/// A window that stays (pinned) over a page's lower half: the page ends above it, never
/// hidden; one over its upper half, or beside it, changes nothing.
#[test]
fn a_page_ends_above_a_pinned_window() {
    let page = kernel::Rect::new(0.0, 100.0, 1000.0, 800.0);
    let low = kernel::Rect::new(700.0, 700.0, 280.0, 180.0);
    let shorter = above(page, low, 10.0);
    assert_eq!((shorter.min_y(), shorter.max_y()), (100.0, 690.0));
    assert_eq!(shorter.size.width, page.size.width);
    let high = kernel::Rect::new(700.0, 150.0, 280.0, 180.0);
    assert_eq!(above(page, high, 10.0), page);
    let beside = kernel::Rect::new(1100.0, 700.0, 100.0, 100.0);
    assert_eq!(above(page, beside, 10.0), page);
}

/// A page's picture is drawn only at its page's size: never stretched or squeezed.
#[test]
fn a_page_picture_is_drawn_only_at_its_size() {
    let picture = render_api::ImageData::new(100, 50, vec![0; 100 * 50 * 4]);
    assert!(fits(&picture, kernel::Rect::new(10.0, 10.0, 100.0, 50.0)));
    assert!(fits(&picture, kernel::Rect::new(10.0, 10.0, 101.0, 49.0)), "rounding");
    assert!(!fits(&picture, kernel::Rect::new(10.0, 10.0, 160.0, 50.0)));
    assert!(!fits(&picture, kernel::Rect::new(10.0, 10.0, 100.0, 30.0)));
}
