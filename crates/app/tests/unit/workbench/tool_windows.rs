use super::*;

/// One window at a time at the bottom, whichever it is: showing one hides the other, a
/// hidden module window keeps its views (and lets go of their data later).
#[test]
fn the_bottom_area_shows_one_window_at_a_time() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    wb.set_bottom(Some("terminal"));
    assert!(wb.bottom_shows("terminal"));
    // A module's view: its window takes the area.
    assert!(wb.show_in_panel("git", "Log", "Log", "view-canvas", &()));
    assert!(wb.bottom_shows("git") && !wb.bottom_shows("terminal"));
    assert_eq!(wb.focus, Focus::Panel);
    // Its button hides it, then shows it again (its view kept).
    assert!(wb.toggle_tool_window("git"));
    assert!(wb.tools.bottom.is_none());
    assert_eq!(wb.focus, Focus::Editor, "the keyboard left it");
    assert!(wb.tools.windows[0].hidden_since.is_some());
    assert!(wb.toggle_tool_window("git"));
    assert!(wb.bottom_shows("git") && wb.tools.windows[0].hidden_since.is_none());
    // The terminal back: the module window hides.
    wb.set_bottom(Some("terminal"));
    assert!(wb.bottom_shows("terminal") && !wb.bottom_shows("git"));
    assert!(wb.tools.windows[0].hidden_since.is_some());
    // A window with no views has nothing to show.
    assert!(!wb.toggle_tool_window("remote"));
}

/// Another project opened in the window: the previous one's bottom windows go with it (its
/// session kept the ones to make again).
#[test]
fn another_project_leaves_no_bottom_window_behind() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    let dir = std::env::temp_dir().join(format!("bottom-switch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("one")).unwrap();
    std::fs::create_dir_all(dir.join("two")).unwrap();
    wb.open_project(&dir.join("one")).unwrap();
    assert!(wb.show_in_panel("git", "Log", "Log", "view-canvas", &()));
    assert!(wb.bottom_shows("git"));
    wb.open_project(&dir.join("two")).unwrap();
    assert!(wb.tools.windows.is_empty() && wb.tools.bottom.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

/// The terminal is a tool window like any: its shells are views (made, but no shell started
/// before they're drawn), their tabs pinned / renamed / closed like any window's, saved and
/// made again at the next start, the shown window too.
#[test]
fn the_terminal_s_shells_are_tabs_saved_and_reopened_like_any_window_s() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let support = Arc::new(support);
    let mut wb = Workbench::new(Arc::clone(&support), Ports::local());
    wb.forget_projects();
    // A project of its own: a shell's number is per project (other tests open shells too).
    let dir = std::env::temp_dir().join(format!("shell-tabs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    wb.open_path(&dir).unwrap();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    // Its toggle (⌥F12) makes the first shell, then the "+" another.
    let toggle = wb.tool_toggled_by(terminal_panel::TOGGLE).unwrap();
    assert_eq!(toggle, "terminal");
    assert!(wb.toggle_tool(toggle));
    assert!(wb.bottom_shows("terminal"));
    assert!(wb.new_in_panel());
    let titles = |wb: &Workbench| -> Vec<(String, bool)> {
        wb.tools.windows[0].groups[0].views.iter().map(|v| (v.title.clone(), v.pinned)).collect()
    };
    assert_eq!(titles(&wb), [("Local".into(), false), ("Local (2)".into(), false)]);
    assert!(wb.view_takes_keyboard(), "a shell takes the whole keyboard");
    // Its tabs: renamed, pinned (first), the others closed (the pinned one kept).
    wb.tools.rename_view(1, "build");
    wb.tools.toggle_pin_view(1);
    assert_eq!(titles(&wb), [("build".into(), true), ("Local".into(), false)]);
    // Saved, then made again in another window (shown, as it was).
    let saved = wb.saved_tool_windows();
    assert_eq!(saved[0].views.len(), 2);
    let mut again = Workbench::new(Arc::clone(&support), Ports::local());
    again.forget_projects();
    again.restore_tool_windows(&saved, Some("terminal"));
    assert!(again.bottom_shows("terminal"));
    assert_eq!(titles(&again), [("build".into(), true), ("Local".into(), false)]);
    wb.tools.close_other_views(1);
    assert_eq!(titles(&wb), [("build".into(), true), ("Local".into(), false)], "pinned kept");
    wb.close_tool_view(1);
    wb.close_tool_view(0);
    assert!(wb.tools.bottom.is_none(), "no shells: the window hides");
}

/// In a shell, ⌘K (macOS) / Ctrl+Shift+K is Clear Terminal: the keys' place is the
/// terminal's view (it takes the whole keyboard, its own commands still reach it), before
/// what the same keys do elsewhere.
#[test]
fn clear_terminal_s_keys_are_the_shell_s() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    assert!(wb.toggle_tool("terminal"));
    assert!(wb.view_takes_keyboard());
    let places = wb.key_places();
    assert_eq!(places, [command_model::When::View(terminal_panel::VIEWER)]);
    for (mac, keys) in [(true, "CmdOrCtrl+K"), (false, "CmdOrCtrl+Shift+K")] {
        let keymap = crate::keymaps::keymap(mac).unwrap();
        let chord = command_model::KeyChord::parse(keys).unwrap().for_platform(mac);
        assert_eq!(keymap.lookup_in(&chord, &places), Some(terminal_panel::CLEAR), "{keys}");
    }
    // Its command reaches the shell's view (which clears its session: see the module's
    // tests; none started here, nothing painted).
    assert_eq!(workbench_model::WorkbenchHost::keyed_view(&wb), Some(terminal_panel::VIEWER));
}

/// A tab is itself, whatever it's called: two named alike are still pinned / kept apart,
/// and its module finds a renamed one by the name it opened it with.
#[test]
fn tabs_are_told_apart_by_their_identity_not_their_title() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    assert!(wb.show_in_panel("git", "Log", "Log", "view-canvas", &()));
    assert!(wb.show_in_panel("git", "History", "History", "view-canvas", &()));
    assert!(wb.show_in_panel("git", "Blame", "Blame", "view-canvas", &()));
    // Two renamed alike: closing the others keeps exactly the one asked for.
    wb.tools.rename_view(0, "same");
    wb.tools.rename_view(1, "same");
    let second = wb.tools.windows[0].groups[0].views[1].id;
    wb.tools.close_other_views(1);
    assert_eq!(wb.tools.windows[0].groups[0].views.len(), 1);
    assert_eq!(wb.tools.windows[0].groups[0].views[0].id, second);
    // Its module asks for it by its first name: the same tab, not a new one.
    assert!(wb.show_in_panel("git", "History", "History", "view-canvas", &()));
    assert_eq!(wb.tools.windows[0].groups[0].views.len(), 1);
    assert_eq!(wb.tools.windows[0].groups[0].views[0].title, "same");
}

/// A tab's × (drawn over its tab) closes it; a click elsewhere on the tab only shows it.
#[test]
fn a_tab_closes_from_its_cross() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    assert!(wb.show_in_panel("git", "Log", "Log", "view-canvas", &()));
    assert!(wb.show_in_panel("git", "History", "History", "view-canvas", &()));
    let mut text = TextSystem::new();
    wb.paint(&mut text, &mut Scene::default());
    let cross =
        wb.tools.hits.iter().find_map(|(r, h)| (*h == ToolHit::CloseView(0, 1)).then_some(*r));
    let cross = cross.expect("the active tab shows its ×");
    let middle =
        Point::new(cross.min_x() + cross.size.width / 2.0, cross.min_y() + cross.size.height / 2.0);
    wb.mouse_down(&mut text, middle, false, false, false, 1);
    assert_eq!(wb.tools.windows[0].groups[0].views.len(), 1);
    assert_eq!(wb.tools.windows[0].groups[0].views[0].title, "Log");
}

/// The project panel's button comes from the registry like any: on the left, first,
/// toggled by its command; without a project, it asks for one to open.
#[test]
fn the_project_panel_is_registered_like_any_window() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    let left = wb.docked(ui_workbench::Dock::Left);
    assert_eq!(left[0].id(), PROJECT_TOOL);
    assert_eq!(wb.tool_toggled_by(crate::commands::TOGGLE_PROJECT), Some(PROJECT_TOOL));
    assert!(!wb.tool_button_of(&left[0], false).active, "no project: not lit");
    assert!(wb.toggle_tool(PROJECT_TOOL));
    assert_eq!(wb.request.take(), Some(MouseResult::Command("view.toggle_project")));
    assert!(wb.toggle_project_panel());
    assert_eq!(wb.request.take(), Some(MouseResult::Command("project.open")));
}

/// A panel's "hide" runs the app's own command (its toggle), not only a module's: it
/// reaches the app as from the menu.
#[test]
fn a_panel_s_hide_button_runs_the_app_s_toggle() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    let mut text = TextSystem::new();
    let hide = ui_workbench::ToolPanelAction::View(ui_workbench::ViewerAction::RunCommand(
        "view.toggle_project".into(),
    ));
    wb.side_action(&mut text, hide);
    assert_eq!(wb.request.take(), Some(MouseResult::Command("view.toggle_project")));
}

/// A view / a panel counting how many times it was asked to clear its caches.
struct Purged(Arc<std::sync::atomic::AtomicUsize>);

impl Purged {
    fn count(&self) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

impl ui_workbench::Viewer for Purged {
    fn paint(&mut self, _: ui_workbench::ViewerPaint<'_>) {}
    fn purge(&mut self) {
        self.count();
    }
}

impl ui_workbench::ToolPanel for Purged {
    fn update(&mut self, _: &ui_workbench::PanelContext<'_>) {}
    fn paint(&mut self, _: ui_workbench::ViewerPaint<'_>) {}
    fn purge(&mut self) {
        self.count();
    }
}

/// "Clear and Rebuild Caches" reaches every tool window's views, shown or not, and the
/// side panels, open or hidden.
#[test]
fn clearing_the_caches_reaches_the_windows_views_and_the_side_panels() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    let purged = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let view = |key: &str| PanelView {
        id: ViewId::next(),
        key: key.to_owned(),
        title: key.to_owned(),
        extension: "view-test".to_owned(),
        pinned: false,
        provider: "test",
        viewer: Box::new(Purged(Arc::clone(&purged))),
    };
    let mut window = ToolWindow::new("test");
    window.groups[0].views = vec![view("a"), view("b")];
    window.hidden_since = Some(std::time::Instant::now());
    wb.tools.windows.push(window);
    let side = |dock| SidePanel {
        id: "test-side",
        dock,
        panel: Box::new(Purged(Arc::clone(&purged))),
        bounds: Rect::default(),
        content: Rect::default(),
        header: PanelHeader::default(),
        dragging: false,
        key: None,
        project: None,
        parked_since: None,
    };
    wb.tools.sides.push(side(ui_workbench::Dock::Right));
    wb.tools.parked.push(side(ui_workbench::Dock::Left));
    wb.purge();
    assert_eq!(purged.load(std::sync::atomic::Ordering::Relaxed), 4);
}

/// A side panel hidden for a while lets its data go, once (like a hidden window's views);
/// one hidden a moment ago, or shown, keeps it.
#[test]
fn a_side_panel_hidden_for_a_while_lets_its_data_go() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    let purged = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let long_ago = std::time::Instant::now().checked_sub(RELEASE_AFTER * 2);
    let side = |id, dock, parked_since| SidePanel {
        id,
        dock,
        panel: Box::new(Purged(Arc::clone(&purged))),
        bounds: Rect::default(),
        content: Rect::default(),
        header: PanelHeader::default(),
        dragging: false,
        key: None,
        project: None,
        parked_since,
    };
    wb.tools.sides.push(side("shown", ui_workbench::Dock::Right, None));
    wb.tools.parked.push(side("old", ui_workbench::Dock::Left, long_ago));
    let now = Some(std::time::Instant::now());
    wb.tools.parked.push(side("recent", ui_workbench::Dock::Bottom, now));
    wb.poll_tool_windows();
    wb.poll_tool_windows();
    assert_eq!(purged.load(std::sync::atomic::Ordering::Relaxed), 1, "only the old one, once");
}

/// The bottom area's separator lights under the mouse and goes dark once it moves on into
/// the window below (it used to stay lit: the window took the hover first). A module's
/// window, not the terminal: the shells' numbering is shared by the tests.
#[test]
fn the_bottom_separator_goes_dark_when_the_mouse_enters_the_window_below() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    assert!(wb.show_in_panel("git", "Log", "Log", "view-canvas", &()));
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    let line = wb.tools.terminal_splitter.bounds();
    let on =
        Point::new(line.min_x() + line.size.width / 2.0, line.min_y() + line.size.height / 2.0);
    wb.hover(on);
    assert!(wb.tools.terminal_splitter.is_hovered());
    wb.hover(Point::new(on.x, line.max_y() + 120.0));
    assert!(!wb.tools.terminal_splitter.is_hovered(), "in the window below: dark again");
}

/// The keys are a view's where it has them: the bottom window's view when it's focused
/// (its actions sent to it), the editor's once the editor has them again.
#[test]
fn a_focused_view_has_its_actions_keys() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    assert_eq!(wb.key_place(), Some(command_model::When::Editor));
    assert!(wb.show_in_panel("git", "Log", "Log", "view-canvas", &()));
    assert_eq!(wb.focus, Focus::Panel);
    assert_eq!(wb.key_place(), Some(command_model::When::View("canvas")));
    assert_eq!(workbench_model::WorkbenchHost::keyed_view(&wb), Some("canvas"));
    let zoom = crate::commands::contributed()
        .into_iter()
        .find(|(s, _)| s.id.0 == "canvas.zoom_in")
        .map(|(s, _)| s.id)
        .unwrap();
    assert_eq!(wb.run_command(zoom), Some(true), "sent to the bottom window's canvas");
    wb.focus = Focus::Editor;
    assert_eq!(wb.key_place(), Some(command_model::When::Editor));
}

/// A panel docked at the bottom (Services) shares the area with the windows of views: one
/// at a time, its button lit while it shows, the terminal taking its place (it's set aside
/// as it is, made only once), its header the bottom windows' with the app's hide, its keys
/// (⌘8 / Alt+8) its toggle's.
#[test]
fn a_bottom_panel_shares_the_bottom_area_with_the_windows() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    assert_eq!(wb.tool_toggled_by(services_panel::TOGGLE), Some(services_panel::TOOL));
    assert_eq!(
        crate::keymaps::shortcuts(services_panel::TOGGLE).first().map(String::as_str),
        Some("CmdOrCtrl+8")
    );
    let bottom = wb.docked(ui_workbench::Dock::Bottom);
    let ids: Vec<&str> = bottom.iter().map(|t| t.id()).collect();
    let (terminal, services) = (
        ids.iter().position(|i| *i == "terminal").unwrap(),
        ids.iter().position(|i| *i == services_panel::TOOL).unwrap(),
    );
    assert!(services < terminal, "above the terminal: {ids:?}");
    // Shown: at the bottom, in the bottom area.
    assert!(wb.toggle_tool(services_panel::TOOL));
    assert!(wb.bottom_shows(services_panel::TOOL) && !wb.tool_window_shown());
    let panel = wb.tools.bottom_panel().expect("its panel, made");
    assert_eq!(panel.bounds, wb.tools.bottom_bounds);
    assert!(panel.bounds.size.height > 0.0 && panel.content.min_y() > panel.bounds.min_y());
    let services_button = bottom.iter().find(|t| t.id() == services_panel::TOOL).unwrap();
    assert!(wb.tool_button_of(services_button, true).active);
    // Its header: the window's buttons, then the app's hide, at the right end.
    let mut text = TextSystem::new();
    wb.paint(&mut text, &mut Scene::default());
    let hide = wb.side_header_button(services_panel::TOOL, side_panel::BOTTOM_HIDE).unwrap();
    assert!(hide.max_x() > wb.tools.bottom_bounds.max_x() - 40.0);
    assert_eq!(
        wb.side_button_at(Point::new(hide.min_x() + 2.0, hide.min_y() + 2.0)).map(|(_, t)| t),
        Some(i18n::t("tooltip.hide"))
    );
    // The terminal in its place: the panel set aside, the area the terminal's.
    wb.set_bottom(Some("terminal"));
    assert!(wb.tools.bottom_panel().is_none());
    assert!(wb.tools.parked.iter().any(|s| s.id == services_panel::TOOL));
    assert!(wb.bottom_shows("terminal"));
    // Back: the same panel (not made again), the terminal's place given up.
    assert!(wb.toggle_tool(services_panel::TOOL));
    assert!(
        wb.bottom_shows(services_panel::TOOL)
            && wb.tools.parked.iter().all(|s| s.id != services_panel::TOOL)
    );
    // The hide button: hidden, nothing at the bottom.
    wb.mouse_down(
        &mut text,
        Point::new(hide.min_x() + 2.0, hide.min_y() + 2.0),
        false,
        false,
        false,
        1,
    );
    assert!(wb.tools.bottom.is_none() && wb.tools.bottom_panel().is_none());
    // Its toggle again, then its strip button hides it.
    assert!(wb.toggle_tool(services_panel::TOOL));
    assert!(wb.toggle_tool_window(services_panel::TOOL));
    assert!(wb.tools.bottom.is_none() && wb.tools.bottom_panel().is_none());
}

/// The bottom panel shown and its splitter's place are the project's session's: shown again
/// at the next start, its tree as wide as it was; another project starts without them.
#[test]
fn the_bottom_panel_and_its_place_come_back_with_the_session() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let support = Arc::new(support);
    let mut wb = Workbench::new(Arc::clone(&support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    assert!(wb.toggle_tool(services_panel::TOOL));
    let side = wb.tools.sides.iter_mut().find(|s| s.id == services_panel::TOOL).unwrap();
    side.panel.set_data(&ui_workbench::PanelMemento("tree=420".into()));
    let mementos = wb.tools.panel_mementos();
    assert!(mementos.contains(&(services_panel::TOOL.to_owned(), "tree=420".to_owned())));
    // Another window, as a session restores it: the mementos given, then the bottom shown.
    let mut again = Workbench::new(Arc::clone(&support), Ports::local());
    again.forget_projects();
    again.resize(PhysicalSize::new(1200, 800), 1.0);
    again.tools.mementos = mementos.into_iter().collect();
    again.restore_tool_windows(&[], Some(services_panel::TOOL));
    assert!(again.bottom_shows(services_panel::TOOL));
    let panel = again.tools.bottom_panel().unwrap();
    assert_eq!(panel.panel.memento().as_deref(), Some("tree=420"));
    // A project opened: nothing of the previous one's bottom left.
    let dir = std::env::temp_dir().join(format!("bottom-panel-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    again.open_project(&dir).unwrap();
    assert!(again.tools.bottom.is_none() && again.tools.bottom_panel().is_none());
    assert!(again.tools.mementos.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// The separators between the editor's card and the panels beside it are painted after
/// both sides: their handles, reaching into each, never half covered by a panel.
#[test]
fn the_window_s_separators_are_drawn_over_the_panels_on_both_sides() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1400, 900), 1.0);
    assert!(wb.show_side_panel("structure"));
    assert!(wb.toggle_tool(services_panel::TOOL));
    let mut text = TextSystem::new();
    let mut scene = Scene::default();
    wb.paint(&mut text, &mut scene);
    // Painted twice: the layout settled by the first frame.
    let mut scene = Scene::default();
    wb.paint(&mut text, &mut scene);
    assert!(wb.tools.side_on(ui_workbench::Dock::Right).is_some());
    assert!(wb.tools.side_splitter.is_on_top(&scene), "the right one");
    assert!(wb.tools.terminal_splitter.is_on_top(&scene), "the bottom one");
    if wb.tools.side_on(ui_workbench::Dock::Left).is_some() {
        assert!(wb.tools.splitter.is_on_top(&scene), "the left one");
    }
    // A module's window at the bottom instead: its separator over it too.
    assert!(wb.show_in_panel("git", "Log", "Log", "view-canvas", &()));
    assert!(wb.tool_window_shown());
    let mut scene = Scene::default();
    wb.paint(&mut text, &mut scene);
    assert!(wb.tools.terminal_splitter.is_on_top(&scene), "over the bottom window");
}

/// A workbench with `titles` open as views of Git's bottom window, its header painted.
fn git_tabs(titles: &[&str]) -> (Workbench, TextSystem) {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    for title in titles {
        assert!(wb.show_in_panel("git", title, title, "view-canvas", &()));
    }
    let mut text = TextSystem::new();
    wb.paint(&mut text, &mut Scene::default());
    (wb, text)
}

/// The middle of what the header shows for `hit`.
fn hit_center(wb: &Workbench, hit: ToolHit) -> Point {
    let r = wb.tools.hits.iter().find_map(|(r, h)| (*h == hit).then_some(*r)).unwrap();
    Point::new(r.min_x() + r.size.width / 2.0, r.min_y() + r.size.height / 2.0)
}

fn tab_titles(wb: &Workbench) -> Vec<String> {
    wb.tools.window().unwrap().group().views.iter().map(|v| v.title.clone()).collect()
}

/// A tab dragged past the others' middles goes there: it follows the pointer, the others
/// make room while it moves, it stays the active one and itself (its identity).
#[test]
fn a_dragged_tab_moves_among_the_others() {
    let (mut wb, mut text) = git_tabs(&["Log", "History", "Blame"]);
    let ids: Vec<ViewId> = wb.tools.window().unwrap().group().views.iter().map(|v| v.id).collect();
    let from = hit_center(&wb, ToolHit::View(0, 0));
    let past = hit_center(&wb, ToolHit::View(0, 2));
    assert_eq!(wb.mouse_down(&mut text, from, false, false, false, 1), MouseResult::StartDrag);
    assert!(wb.mouse_drag(&mut text, Point::new(past.x + 30.0, from.y + 3.0)));
    assert_eq!(tab_titles(&wb), ["History", "Blame", "Log"], "the others made room");
    // Painted while dragged: its place is kept empty, last.
    wb.paint(&mut text, &mut Scene::default());
    assert!(wb.mouse_up(&mut text));
    assert!(wb.tools.tab_drag.is_none());
    let w = wb.tools.window().unwrap();
    assert_eq!(w.group().active, 2, "the dragged tab stays the active one");
    assert_eq!(w.group().views[2].id, ids[0], "the same view, moved");
    // Back to the middle.
    wb.paint(&mut text, &mut Scene::default());
    let from = hit_center(&wb, ToolHit::View(0, 2));
    let middle = hit_center(&wb, ToolHit::View(0, 1));
    wb.mouse_down(&mut text, from, false, false, false, 1);
    wb.mouse_drag(&mut text, Point::new(middle.x, from.y));
    wb.mouse_up(&mut text);
    assert_eq!(tab_titles(&wb), ["History", "Log", "Blame"]);
}

/// A click on a tab (or a press that barely moves) only shows it: no reorder.
#[test]
fn a_click_on_a_tab_does_not_move_it() {
    let (mut wb, mut text) = git_tabs(&["Log", "History", "Blame"]);
    let at = hit_center(&wb, ToolHit::View(0, 0));
    wb.mouse_down(&mut text, at, false, false, false, 1);
    assert!(!wb.mouse_drag(&mut text, Point::new(at.x + 3.0, at.y + 2.0)), "under the threshold");
    assert!(!wb.mouse_up(&mut text));
    assert_eq!(tab_titles(&wb), ["Log", "History", "Blame"]);
    assert_eq!(wb.tools.window().unwrap().group().active, 0, "the click showed it");
}

/// Esc while dragging puts the tab back where it was; the rest of the drag does nothing.
#[test]
fn esc_gives_a_tab_drag_up() {
    let (mut wb, mut text) = git_tabs(&["Log", "History", "Blame"]);
    let from = hit_center(&wb, ToolHit::View(0, 0));
    let end = hit_center(&wb, ToolHit::View(0, 2));
    assert!(!wb.cancel_tool_tab_drag(), "nothing dragged");
    wb.mouse_down(&mut text, from, false, false, false, 1);
    wb.mouse_drag(&mut text, Point::new(end.x + 40.0, from.y));
    assert_eq!(tab_titles(&wb), ["History", "Blame", "Log"]);
    assert!(wb.cancel_tool_tab_drag());
    assert_eq!(tab_titles(&wb), ["Log", "History", "Blame"]);
    assert_eq!(wb.tools.window().unwrap().group().active, 0);
    assert!(!wb.mouse_drag(&mut text, Point::new(end.x + 60.0, from.y)));
    wb.mouse_up(&mut text);
    assert_eq!(tab_titles(&wb), ["Log", "History", "Blame"]);
    assert!(wb.tools.tab_drag.is_none());
}

/// The pinned tabs stay first: neither kind is dragged among the other.
#[test]
fn pinned_tabs_stay_first_when_dragged() {
    let (mut wb, mut text) = git_tabs(&["Log", "History", "Blame"]);
    wb.tools.toggle_pin_view(0);
    wb.paint(&mut text, &mut Scene::default());
    let first = hit_center(&wb, ToolHit::View(0, 0));
    let last = hit_center(&wb, ToolHit::View(0, 2));
    // The unpinned last one, dragged to the very start: after the pinned one.
    wb.mouse_down(&mut text, last, false, false, false, 1);
    // (Over the window's name: still its strip.)
    wb.mouse_drag(&mut text, Point::new(wb.tools.bottom_bounds.min_x() + 4.0, last.y));
    wb.mouse_up(&mut text);
    assert_eq!(tab_titles(&wb), ["Log", "Blame", "History"]);
    // The pinned one, dragged to the end: stays first.
    wb.paint(&mut text, &mut Scene::default());
    wb.mouse_down(&mut text, first, false, false, false, 1);
    wb.mouse_drag(&mut text, Point::new(last.x + 200.0, first.y));
    wb.mouse_up(&mut text);
    assert_eq!(tab_titles(&wb), ["Log", "Blame", "History"]);
}

/// The place a dragged tab takes: before the first one whose middle it hasn't passed.
#[test]
fn a_dragged_tab_s_place_is_where_its_middle_is() {
    let widths = [50.0, 30.0, 70.0];
    let all = 0..=2;
    // The first one (50 wide), moved right from 0 (gap 4).
    assert_eq!(tab_drop_index(&widths, 0, 0.0, 0.0, 4.0, all.clone()), 0);
    assert_eq!(tab_drop_index(&widths, 0, 16.0, 0.0, 4.0, all.clone()), 0);
    assert_eq!(tab_drop_index(&widths, 0, 18.0, 0.0, 4.0, all.clone()), 1);
    assert_eq!(tab_drop_index(&widths, 0, 120.0, 0.0, 4.0, all), 2);
    assert_eq!(tab_drop_index(&widths, 0, 120.0, 0.0, 4.0, 0..=1), 1, "kept in its range");
}

/// The terminal's "+" stays after the tabs whatever is dragged (a press on it drags
/// nothing), and the order the tabs were left in is the one saved and made again.
#[test]
fn the_plus_stays_last_and_the_order_is_saved() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let support = Arc::new(support);
    let mut wb = Workbench::new(Arc::clone(&support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    // In a project of its own: its shells are numbered apart from other tests'.
    let dir = std::env::temp_dir().join(format!("tab-order-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    wb.open_project(&dir).unwrap();
    assert!(wb.toggle_tool("terminal"));
    assert!(wb.new_in_panel());
    assert!(wb.new_in_panel());
    wb.tools.rename_view(0, "one");
    wb.tools.rename_view(1, "two");
    wb.tools.rename_view(2, "three");
    let mut text = TextSystem::new();
    // Only the header: no shell started.
    wb.paint_tool_header(&mut text, &mut Scene::default());
    let plus = hit_center(&wb, ToolHit::New(0));
    let first = hit_center(&wb, ToolHit::View(0, 0));
    // Dragged over the "+" and beyond: last among the tabs, drawn before the "+".
    wb.mouse_down(&mut text, first, false, false, false, 1);
    wb.mouse_drag(&mut text, Point::new(plus.x + 300.0, first.y));
    let drag = wb.tools.tab_drag.as_ref().unwrap();
    let at =
        wb.tools.window().unwrap().group().views.iter().position(|v| v.id == drag.view).unwrap();
    assert_eq!(at, 2);
    let lifted = drag.floating_left(at) + drag.widths[at];
    wb.paint_tool_header(&mut text, &mut Scene::default());
    let plus_left =
        wb.tools.hits.iter().find_map(|(r, h)| (*h == ToolHit::New(0)).then_some(r.min_x()));
    assert!(lifted <= plus_left.unwrap(), "the dragged tab never covers the +");
    wb.mouse_up(&mut text);
    assert_eq!(tab_titles(&wb), ["two", "three", "one"]);
    wb.paint_tool_header(&mut text, &mut Scene::default());
    let plus =
        wb.tools.hits.iter().find_map(|(r, h)| (*h == ToolHit::New(0)).then_some(*r)).unwrap();
    for (r, h) in &wb.tools.hits {
        if matches!(h, ToolHit::View(..)) {
            assert!(r.max_x() <= plus.min_x(), "the + is after every tab");
        }
    }
    // A press on the "+" is no tab's drag.
    let count = wb.tools.window().unwrap().group().views.len();
    wb.mouse_down(&mut text, hit_center(&wb, ToolHit::New(0)), false, false, false, 1);
    assert!(wb.tools.tab_drag.is_none());
    wb.mouse_up(&mut text);
    assert_eq!(wb.tools.window().unwrap().group().views.len(), count + 1, "it made another shell");
    wb.close_tool_view(count);
    // Saved, made again in another window: the same order.
    let saved = wb.saved_tool_windows();
    let mut again = Workbench::new(Arc::clone(&support), Ports::local());
    again.forget_projects();
    again.restore_tool_windows(&saved, Some("terminal"));
    assert_eq!(tab_titles(&again), ["two", "three", "one"]);
    assert_eq!(again.tools.window().unwrap().group().active, 2, "the dragged one was active");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The groups' tabs, group after group.
fn group_titles(wb: &Workbench) -> Vec<Vec<String>> {
    let w = wb.tools.window().unwrap();
    w.groups.iter().map(|g| g.views.iter().map(|v| v.title.clone()).collect()).collect()
}

/// A tab's menu splits its window like the editor's: Split and Move takes it into a new
/// group beside (or below) its own, which becomes current; each group has its own tabs,
/// header and shown view; a click in a group's view makes it current.
#[test]
fn a_tab_moves_into_a_split_of_its_window() {
    use crate::commands::ToolTabAction as A;
    let (mut wb, mut text) = git_tabs(&["Log", "History", "Blame"]);
    assert!(!wb.tool_tab_action(A::SplitRight, 1), "Git makes no views: nothing to split off");
    assert!(wb.tool_tab_action(A::SplitMoveRight, 1));
    assert_eq!(group_titles(&wb), [vec!["Log", "Blame"], vec!["History"]]);
    let w = wb.tools.window().unwrap();
    assert_eq!(w.current, 1, "the new group is current");
    assert!(w.tree.to_spec().starts_with("R("));
    let (left, right) = (w.groups[0].bounds, w.groups[1].bounds);
    assert!(left.max_x() < right.min_x() && left.min_y() == right.min_y(), "side by side");
    assert_eq!(wb.focus, Focus::Panel);
    wb.paint(&mut text, &mut Scene::default());
    // Each group's tabs in its own header; one hide, on the right.
    assert!(wb.tools.hits.iter().any(|(_, h)| *h == ToolHit::View(1, 0)));
    let hides: Vec<Rect> =
        wb.tools.hits.iter().filter(|(_, h)| *h == ToolHit::Hide).map(|(r, _)| *r).collect();
    assert_eq!(hides.len(), 1);
    assert!(hides[0].min_x() > right.min_x());
    // The only tab can't move out of its group; below the first one, a group under it.
    assert!(!wb.tool_tab_action(A::SplitMoveDown, 0));
    let left_view = Point::new(left.min_x() + 40.0, left.max_y() - 20.0);
    wb.mouse_down(&mut text, left_view, false, false, false, 1);
    wb.mouse_up(&mut text);
    assert_eq!(wb.tools.window().unwrap().current, 0, "a click in a view: its group");
    assert!(wb.tool_tab_action(A::SplitMoveDown, 0));
    assert_eq!(group_titles(&wb), [vec!["Blame"], vec!["Log"], vec!["History"]]);
    let w = wb.tools.window().unwrap();
    assert!(w.groups[1].bounds.min_y() > w.groups[0].bounds.max_y(), "below the first");
}

/// The closes act on the tab's group; a group left empty goes (its space to its
/// neighbour), the window hides when none is left.
#[test]
fn an_emptied_group_goes() {
    use crate::commands::ToolTabAction as A;
    let (mut wb, mut text) = git_tabs(&["Log", "History", "Blame"]);
    assert!(wb.tool_tab_action(A::SplitMoveRight, 2));
    wb.focus_tool_group(0);
    assert!(wb.tool_tab_action(A::CloseRight, 0));
    assert_eq!(group_titles(&wb), [vec!["Log"], vec!["Blame"]]);
    assert!(wb.tool_tab_action(A::CloseAll, 0));
    assert_eq!(group_titles(&wb), [vec!["Blame"]]);
    let w = wb.tools.window().unwrap();
    assert_eq!((w.current, w.tree.to_spec().as_str()), (0, "L0"));
    assert_eq!(w.groups[0].bounds, wb.tools.bottom_bounds, "the whole card again");
    wb.paint(&mut text, &mut Scene::default());
    let cross = hit_center(&wb, ToolHit::CloseView(0, 0));
    wb.mouse_down(&mut text, cross, false, false, false, 1);
    assert!(wb.tools.bottom.is_none(), "no views: the window hides");
}

/// The separators between groups are the editor's: dragged by their handle, a double click
/// shares the space equally.
#[test]
fn the_groups_separator_drags_and_equalizes() {
    use crate::commands::ToolTabAction as A;
    let (mut wb, mut text) = git_tabs(&["Log", "History"]);
    assert!(wb.tool_tab_action(A::SplitMoveRight, 1));
    wb.paint(&mut text, &mut Scene::default());
    let width = |wb: &Workbench| wb.tools.window().unwrap().groups[0].bounds.size.width;
    let even = width(&wb);
    let line = wb.tools.window().unwrap().split.dividers[0].rect;
    let at =
        Point::new(line.min_x() + line.size.width / 2.0, line.min_y() + line.size.height / 2.0);
    assert_eq!(wb.mouse_down(&mut text, at, false, false, false, 1), MouseResult::StartDrag);
    assert!(wb.mouse_drag(&mut text, Point::new(at.x - 150.0, at.y)));
    assert!(wb.mouse_up(&mut text));
    assert!((width(&wb) - (even - 150.0)).abs() < 2.0, "{} vs {even}", width(&wb));
    let line = wb.tools.window().unwrap().split.dividers[0].rect;
    let at =
        Point::new(line.min_x() + line.size.width / 2.0, line.min_y() + line.size.height / 2.0);
    wb.mouse_down(&mut text, at, false, false, false, 2);
    wb.mouse_up(&mut text);
    assert!((width(&wb) - even).abs() < 1.0, "equal again");
}

/// The terminal split: another shell in a new group (Split Right), the "+" of a group
/// makes a shell there; the layout — the tree, its shares, which tab in which group, each
/// one's shown tab and the current group — is saved and made again.
#[test]
fn the_terminal_splits_and_its_layout_is_saved() {
    use crate::commands::ToolTabAction as A;
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let support = Arc::new(support);
    let mut wb = Workbench::new(Arc::clone(&support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    let dir = std::env::temp_dir().join(format!("tool-splits-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    wb.open_project(&dir).unwrap();
    assert!(wb.toggle_tool("terminal"));
    assert!(wb.new_in_panel());
    wb.tools.rename_view(0, "one");
    wb.tools.rename_view(1, "two");
    // Split Right on a shell: a new shell, in a new group on the right.
    assert!(wb.tool_tab_action(A::SplitRight, 0));
    let shells = group_titles(&wb);
    assert_eq!(shells.len(), 2);
    assert_eq!(shells[0], ["one", "two"]);
    assert_eq!(shells[1].len(), 1);
    wb.tools.rename_view(0, "three");
    // The first group's "+": a shell there.
    let mut text = TextSystem::new();
    wb.paint_tool_header(&mut text, &mut Scene::default());
    wb.mouse_down(&mut text, hit_center(&wb, ToolHit::New(0)), false, false, false, 1);
    wb.mouse_up(&mut text);
    let shells = group_titles(&wb);
    assert_eq!(shells[0].len(), 3);
    assert_eq!(shells[1], ["three"]);
    wb.tools.rename_view(2, "four");
    wb.tools.window_mut().unwrap().groups[0].active = 1;
    // Its separator moved, then the second group current.
    let divider = wb.tools.window().unwrap().split.dividers[0].clone();
    let at = divider.rect.min_x() - 200.0;
    wb.tools.window_mut().unwrap().tree.drag(&divider, Point::new(at, at));
    wb.focus_tool_group(1);
    let spec = wb.tools.window().unwrap().tree.to_spec();
    let saved = wb.saved_tool_windows();
    let mut again = Workbench::new(Arc::clone(&support), Ports::local());
    again.forget_projects();
    again.resize(PhysicalSize::new(1200, 800), 1.0);
    again.restore_tool_windows(&saved, Some("terminal"));
    assert_eq!(group_titles(&again), [vec!["one", "two", "four"], vec!["three"]]);
    let w = again.tools.window().unwrap();
    assert_eq!(w.tree.to_spec(), spec);
    assert_eq!((w.current, w.groups[0].active, w.groups[1].active), (1, 1, 0));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Git's window split in two: [Log, History] | [Blame], painted.
fn two_groups() -> (Workbench, TextSystem) {
    use crate::commands::ToolTabAction as A;
    let (mut wb, mut text) = git_tabs(&["Log", "History", "Blame"]);
    assert!(wb.tool_tab_action(A::SplitMoveRight, 2));
    wb.paint(&mut text, &mut Scene::default());
    (wb, text)
}

/// Group `g`'s view area.
fn view_of(wb: &Workbench, g: usize) -> Rect {
    wb.tool_group_view_bounds(wb.tools.window().unwrap().groups[g].bounds)
}

/// Presses tab `i` of group `g`, drags it to `to` (painting on the way, as the window
/// does) and drops it there.
fn drag_tab(wb: &mut Workbench, text: &mut TextSystem, (g, i): (usize, usize), to: Point) {
    let from = hit_center(wb, ToolHit::View(g, i));
    wb.mouse_down(text, from, false, false, false, 1);
    wb.mouse_drag(text, Point::new(from.x + 10.0, from.y));
    wb.mouse_drag(text, to);
    wb.paint(text, &mut Scene::default());
    wb.mouse_up(text);
    wb.paint(text, &mut Scene::default());
}

/// A tab dragged to another group's tabs goes there, at the place pointed (the insertion
/// line shown while dragged); to the middle of another group's view, at its end. Its
/// group, left empty, goes; where it lands is current.
#[test]
fn a_tab_dragged_into_another_group_moves_there() {
    let (mut wb, mut text) = two_groups();
    // Log, before Blame.
    let blame = hit_center(&wb, ToolHit::View(1, 0));
    let from = hit_center(&wb, ToolHit::View(0, 0));
    wb.mouse_down(&mut text, from, false, false, false, 1);
    wb.mouse_drag(&mut text, Point::new(blame.x - 20.0, blame.y));
    let drag = wb.tools.tab_drag.as_ref().unwrap();
    assert!(!drag.in_strip);
    assert!(matches!(drag.target, Some(ToolDrop::Strip { group: 1, index: 0, .. })));
    assert_eq!(group_titles(&wb)[0], ["Log", "History"], "back in its place meanwhile");
    wb.paint(&mut text, &mut Scene::default());
    wb.mouse_up(&mut text);
    assert_eq!(group_titles(&wb), [vec!["History"], vec!["Log", "Blame"]]);
    let w = wb.tools.window().unwrap();
    assert_eq!((w.current, w.groups[1].active), (1, 0));
    // History, to the middle of the other group's view: at its end; its group goes.
    wb.paint(&mut text, &mut Scene::default());
    let middle = view_of(&wb, 1);
    let middle = Point::new(
        middle.min_x() + middle.size.width / 2.0,
        middle.min_y() + middle.size.height / 2.0,
    );
    drag_tab(&mut wb, &mut text, (0, 0), middle);
    assert_eq!(group_titles(&wb), [vec!["Log", "Blame", "History"]]);
    assert_eq!(wb.tools.window().unwrap().group().active, 2);
}

/// A tab dragged to a side of a view (its zone shown) makes a new group there: right and
/// bottom after it, left and top before it — its own group's too, when it has others.
#[test]
fn a_tab_dragged_to_a_view_s_side_splits_there() {
    let (mut wb, mut text) = two_groups();
    // Blame, to the left side of the first group's view: a group before it.
    let left = view_of(&wb, 0);
    let at = Point::new(left.min_x() + 10.0, left.min_y() + left.size.height / 2.0);
    let from = hit_center(&wb, ToolHit::View(1, 0));
    wb.mouse_down(&mut text, from, false, false, false, 1);
    wb.mouse_drag(&mut text, Point::new(from.x + 10.0, from.y));
    wb.mouse_drag(&mut text, at);
    let drag = wb.tools.tab_drag.as_ref().unwrap();
    let Some(ToolDrop::Edge { group: 0, side: DropSide::Left, zone }) = drag.target else {
        panic!("{:?}", drag.target);
    };
    assert_eq!(zone.size.width, left.size.width / 2.0, "the half it would take");
    wb.paint(&mut text, &mut Scene::default());
    wb.mouse_up(&mut text);
    assert_eq!(group_titles(&wb), [vec!["Blame"], vec!["Log", "History"]]);
    let w = wb.tools.window().unwrap();
    assert_eq!(w.current, 0);
    assert!(w.groups[0].bounds.max_x() < w.groups[1].bounds.min_x());
    // History, to the bottom of its own group's view: a group under it.
    wb.paint(&mut text, &mut Scene::default());
    let own = view_of(&wb, 1);
    let bottom = Point::new(own.min_x() + own.size.width / 2.0, own.max_y() - 8.0);
    drag_tab(&mut wb, &mut text, (1, 1), bottom);
    assert_eq!(group_titles(&wb), [vec!["Blame"], vec!["Log"], vec!["History"]]);
    let w = wb.tools.window().unwrap();
    assert!(w.groups[2].bounds.min_y() > w.groups[1].bounds.max_y(), "below Log");
    assert_eq!(w.tree.to_spec().matches('D').count(), 1);
}

/// Nowhere to go: outside the window, its own view's middle, a side of its own group when
/// it's alone there, or Esc: the tab stays where it was.
#[test]
fn a_tab_dropped_nowhere_stays() {
    let (mut wb, mut text) = two_groups();
    let before = group_titles(&wb);
    let outside =
        Point::new(wb.tools.bottom_bounds.min_x() + 50.0, wb.tools.bottom_bounds.min_y() - 100.0);
    drag_tab(&mut wb, &mut text, (0, 0), outside);
    assert_eq!(group_titles(&wb), before);
    let own = view_of(&wb, 0);
    let middle =
        Point::new(own.min_x() + own.size.width / 2.0, own.min_y() + own.size.height / 2.0);
    drag_tab(&mut wb, &mut text, (0, 1), middle);
    assert_eq!(group_titles(&wb), before);
    let alone = view_of(&wb, 1);
    drag_tab(&mut wb, &mut text, (1, 0), Point::new(alone.max_x() - 5.0, alone.min_y() + 40.0));
    assert_eq!(group_titles(&wb), before);
    // Esc over another group: given up.
    let from = hit_center(&wb, ToolHit::View(0, 0));
    let other = view_of(&wb, 1);
    wb.mouse_down(&mut text, from, false, false, false, 1);
    wb.mouse_drag(&mut text, Point::new(from.x + 10.0, from.y));
    wb.mouse_drag(
        &mut text,
        Point::new(other.min_x() + other.size.width / 2.0, other.min_y() + 60.0),
    );
    assert!(wb.cancel_tool_tab_drag());
    wb.mouse_up(&mut text);
    assert_eq!(group_titles(&wb), before);
}

/// With the bottom window's keyboard, Next / Previous Group move it between its groups and
/// Close Group closes the current one; with the editor's, they're the editor's groups.
#[test]
fn the_group_keys_act_where_the_keyboard_is() {
    let (mut wb, _) = two_groups();
    assert_eq!(wb.focus, Focus::Panel);
    assert_eq!(wb.tools.window().unwrap().current, 1);
    assert!(wb.cycle_group(true));
    assert_eq!(wb.tools.window().unwrap().current, 0);
    assert!(wb.cycle_group(false));
    assert_eq!(wb.tools.window().unwrap().current, 1);
    assert!(wb.close_group());
    assert_eq!(group_titles(&wb), [vec!["Log", "History"]]);
    assert!(!wb.cycle_group(true), "one group left");
    wb.focus = Focus::Editor;
    assert!(!wb.close_group(), "not the editor's");
    assert!(!wb.cycle_group(true), "the editor isn't split");
}

/// A view whose tab can't be closed (Git's Log and Changes).
struct Fixed;

impl ui_workbench::Viewer for Fixed {
    fn paint(&mut self, _: ui_workbench::ViewerPaint<'_>) {}
    fn closable(&self) -> bool {
        false
    }
}

/// A fixed tab has no ×, and no close takes it (its own, the others', its view's, Close
/// Group: it moves to the neighbour group); it still moves into a split like any other.
#[test]
fn a_fixed_tab_is_never_closed() {
    use crate::commands::ToolTabAction as A;
    let (mut wb, mut text) = git_tabs(&["Log", "Changes", "History", "Blame"]);
    for view in &mut wb.tools.window_mut().unwrap().groups[0].views[..2] {
        view.viewer = Box::new(Fixed);
    }
    wb.focus_tool_group(0);
    wb.tools.window_mut().unwrap().group_mut().active = 1;
    wb.paint(&mut text, &mut Scene::default());
    assert!(!wb.tools.hits.iter().any(|(_, h)| *h == ToolHit::CloseView(0, 1)), "no ×");
    assert!(wb.tool_view_at(1).is_some_and(|(_, tab)| !tab.closable));
    assert!(wb.tool_tab_action(A::Close, 1));
    wb.close_tool_view(0);
    assert_eq!(tab_titles(&wb), ["Log", "Changes", "History", "Blame"]);
    assert!(wb.tool_tab_action(A::CloseOthers, 3));
    assert_eq!(tab_titles(&wb), ["Log", "Changes", "Blame"]);
    assert!(wb.tool_tab_action(A::CloseAll, 0));
    assert_eq!(tab_titles(&wb), ["Log", "Changes"]);
    // Split off, then its group closed: back beside the other one.
    assert!(wb.tool_tab_action(A::SplitMoveRight, 1));
    assert_eq!(group_titles(&wb), [vec!["Log"], vec!["Changes"]]);
    wb.focus = Focus::Panel;
    assert!(wb.close_group());
    assert_eq!(group_titles(&wb), [vec!["Log", "Changes"]]);
    // Alone, Close Group leaves them; the window stays.
    assert!(wb.close_group());
    assert_eq!(tab_titles(&wb), ["Log", "Changes"]);
    assert!(wb.bottom_shows("git"));
}

/// A view saved to be made again (as Git's Log and histories).
struct Kept;

impl ui_workbench::Viewer for Kept {
    fn paint(&mut self, _: ui_workbench::ViewerPaint<'_>) {}
    fn restorable(&self) -> bool {
        true
    }
    fn memento(&self) -> Option<String> {
        Some("src/a.php".into())
    }
}

/// A restorable view is saved with what it says it needs and the name its module finds it
/// by (renamed: the title differs); made again, it's found by that name in its group.
#[test]
fn a_saved_view_keeps_its_name_and_memento() {
    let (mut wb, _) = git_tabs(&["Log", "History"]);
    let window = wb.tools.window_mut().unwrap();
    window.groups[0].views[1].viewer = Box::new(Kept);
    window.groups[0].views[1].title = "Mine".into();
    let saved = wb.saved_tool_windows();
    let git = saved.iter().find(|w| w.id == "git").unwrap();
    assert_eq!(git.views.len(), 1, "only the restorable one");
    let view = &git.views[0];
    assert_eq!((view.title.as_str(), view.key.as_deref()), ("Mine", Some("History")));
    assert_eq!(view.memento.as_deref(), Some("src/a.php"));
    wb.tools.windows.clear();
    wb.restore_tool_windows(&saved, None);
    let w = wb.tools.windows.iter().find(|w| w.id == "git").unwrap();
    let v = &w.groups[0].views[0];
    assert_eq!((v.key.as_str(), v.title.as_str()), ("History", "Mine"));
}

/// Git's Log and Changes, saved in Romanian (an old session: by their tab names only) or
/// renamed, are found again by their own names whatever the language now: one each.
#[test]
fn the_git_tabs_are_found_by_their_own_names_in_any_language() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    let view = |title: &str, extension: &str, key: Option<&str>| workbench_model::SavedView {
        title: title.into(),
        extension: extension.into(),
        pinned: false,
        group: 0,
        key: key.map(str::to_owned),
        memento: None,
    };
    let saved = vec![workbench_model::SavedToolWindow {
        id: "git".into(),
        views: vec![
            view("Mine", "view-git-log", Some("Jurnal")),
            view("Modificări", "view-git-changes", None),
        ],
        ..Default::default()
    }];
    wb.restore_tool_windows(&saved, None);
    let w = wb.tools.windows.iter().find(|w| w.id == "git").unwrap();
    let keys: Vec<&str> = w.views().map(|v| v.key.as_str()).collect();
    assert_eq!(keys, ["git.log", "git.changes"]);
    assert_eq!(w.views().next().unwrap().title, "Mine", "its own name kept");
    use workbench_model::ViewHost;
    assert!(wb.in_panel("git", "git.log") && wb.in_panel("git", "git.changes"));
    // Shown again by its name: the one there, no other made.
    assert!(wb.show_in_panel("git", "git.changes", "Changes", "view-git-changes", &()));
    assert_eq!(wb.tools.windows.iter().find(|w| w.id == "git").unwrap().views().count(), 2);
}

/// A bottom view's text field (as the Git log's filters): a field and what it was sent.
#[derive(Default)]
struct Field {
    input: ui_kit::TextInput,
    sent: Arc<std::sync::Mutex<Vec<String>>>,
}

impl ui_workbench::Viewer for Field {
    fn paint(&mut self, _: ui_workbench::ViewerPaint<'_>) {}
    fn wants_text(&self) -> bool {
        true
    }
    fn text_input(&mut self, text: &str) -> bool {
        self.input.insert(text);
        true
    }
    fn selected_text(&self) -> Option<String> {
        Some(self.input.selected_text().to_owned()).filter(|s| !s.is_empty())
    }
    fn edit_key_with(
        &mut self,
        key: ui_workbench::ViewerKey,
        reach: ui_workbench::KeyReach,
    ) -> bool {
        self.sent
            .lock()
            .unwrap()
            .push(format!("{key:?} word={} extend={}", reach.word, reach.extend));
        true
    }
    fn command(&mut self, id: &str) -> bool {
        self.sent.lock().unwrap().push(id.to_owned());
        match id {
            "select_all" => self.input.select_all(),
            "cut" => self.input.insert(""),
            _ => {}
        }
        true
    }
}

/// The edit commands go to a bottom view's text field when it has the keyboard: ⌘A selects
/// in it, Copy takes its selection (nothing selected: nothing, never the editor's text),
/// Cut and Paste edit it, ⌘Z is its; ⌥← reaches it by word, ⇧ selecting.
#[test]
fn the_edit_commands_reach_a_bottom_view_s_field() {
    let (mut wb, mut text) = git_tabs(&["Log"]);
    let sent = Arc::new(std::sync::Mutex::new(Vec::new()));
    let field = Field { sent: Arc::clone(&sent), ..Field::default() };
    wb.tools.window_mut().unwrap().groups[0].views[0].viewer = Box::new(field);
    wb.focus = Focus::Panel;
    assert!(wb.paste(&mut text, "chat bot\nsecond line"));
    assert_eq!(wb.copy(), None, "nothing selected: nothing copied");
    assert!(wb.select_all(&mut text));
    assert_eq!(wb.copy().as_deref(), Some("chat bot"));
    assert_eq!(wb.selected_text().as_deref(), Some("chat bot"), "what the search is filled with");
    assert_eq!(wb.cut(&mut text).as_deref(), Some("chat bot"));
    assert!(wb.paste(&mut text, "x"));
    assert_eq!(wb.viewer_undo(false), Some(true));
    let word_left = EditKey::Move { motion: document::Motion::WordLeft, extend: true };
    wb.edit_key(&mut text, word_left);
    let sent = sent.lock().unwrap().clone();
    assert_eq!(sent, ["select_all", "cut", "undo", "Left word=true extend=true"]);
}

/// The bottom window's buttons: half the height, all of it (nothing above, no separator),
/// pressed again back to its own size — which they never change; dragging or hiding it ends
/// them too.
#[test]
fn the_bottom_window_takes_half_or_all_the_height_and_comes_back() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    wb.set_bottom(Some("terminal"));
    wb.layout();
    let own = wb.tools.bottom_bounds;
    let room =
        wb.tools.terminal_splitter.bounds().max_y() + own.size.height - wb.tools.sidebar.min_y();
    wb.toggle_bottom_size(BottomSize::Half);
    let half = wb.tools.bottom_bounds.size.height;
    assert!((half - (room - wb.tools.terminal_splitter.bounds().size.height) / 2.0).abs() <= 1.0);
    wb.toggle_bottom_size(BottomSize::Full);
    assert_eq!(wb.tools.sidebar.size.height, 0.0, "nothing above it");
    assert_eq!(wb.tools.terminal_splitter.bounds().size.width, 0.0, "no separator");
    assert!(wb.tools.bottom_bounds.size.height > half);
    wb.toggle_bottom_size(BottomSize::Full);
    assert_eq!(wb.tools.bottom_bounds, own, "back to its own size");
    // Hidden at half: shown again at its own size.
    wb.toggle_bottom_size(BottomSize::Half);
    wb.set_bottom(None);
    wb.set_bottom(Some("terminal"));
    wb.layout();
    assert_eq!(wb.tools.bottom_bounds, own);
}

/// From the strip's bottom corner up: Git, Remote, the local history, the terminal, then
/// the services.
#[test]
fn the_bottom_group_goes_from_git_up_to_the_services() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let wb = Workbench::new(Arc::new(support), Ports::local());
    let order = |id: &str| wb.support.tools.iter().find(|t| t.id() == id).map(|t| t.order());
    let up = ["git", "remote", "local-history", "terminal", "services"];
    for pair in up.windows(2) {
        assert!(order(pair[0]).is_some() && order(pair[1]).is_some(), "{pair:?}");
        assert!(order(pair[0]) > order(pair[1]), "{} under {}", pair[0], pair[1]);
    }
}

/// A view with a button saying what it does, in its corner.
struct Tipped;

impl ui_workbench::Viewer for Tipped {
    fn paint(&mut self, _: ui_workbench::ViewerPaint<'_>) {}
    fn tooltip_at(&self, p: Point) -> Option<ui_workbench::Tip> {
        Some(ui_workbench::Tip::new(Rect::new(p.x, p.y, 1.0, 1.0), "Expand all".to_owned()))
    }
}

/// A bottom window's view tells its buttons' tooltips, as a tab's view and a side panel do
/// (the Git log's "expand all" had none).
#[test]
fn a_bottom_window_s_view_tells_its_tooltips() {
    let (mut wb, mut text) = git_tabs(&["Log"]);
    wb.tools.window_mut().unwrap().groups[0].views[0].viewer = Box::new(Tipped);
    wb.paint(&mut text, &mut Scene::default());
    let area = view_of(&wb, 0);
    let inside = Point::new(area.min_x() + 20.0, area.min_y() + 20.0);
    assert_eq!(wb.tooltip_at(inside).map(|t| t.label), Some("Expand all".to_owned()));
}
