use super::*;

/// A project in a scratch folder with a file open, laid out, the bar with the menu button.
fn opened(name: &str) -> (Workbench, PathBuf, TextSystem) {
    let dir = std::env::temp_dir().join(format!("menu-drawer-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.txt"), "abc\n".repeat(50)).unwrap();
    let dir = crate::workbench::canonical(&dir);
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    wb.open_project(&dir).unwrap();
    wb.open_file(&dir.join("a.txt")).unwrap();
    wb.set_menu_button(true);
    let mut text = TextSystem::new();
    paint(&mut wb, &mut text);
    (wb, dir, text)
}

fn paint(wb: &mut Workbench, text: &mut TextSystem) {
    if let Some(drawer) = &mut wb.menu_drawer {
        drawer.settle();
    }
    wb.paint(text, &mut render_api::Scene::default());
}

fn center(r: Rect) -> Point {
    Point::new(r.min_x() + r.size.width / 2.0, r.min_y() + r.size.height / 2.0)
}

fn click(wb: &mut Workbench, text: &mut TextSystem, p: Point) -> MouseResult {
    let result = wb.mouse_down(text, p, false, false, false, 1);
    wb.mouse_up(text);
    result
}

#[test]
fn nothing_is_made_until_it_opens_and_it_is_gone_after_it_closes() {
    let (mut wb, dir, mut text) = opened("lifetime");
    assert!(wb.menu_drawer.is_none(), "nothing before it first opens");
    let button = wb.chrome.top_bar.menu_button().expect("the bar has the menu button");
    // The button opens it: the native menus, a modal over the window.
    assert_eq!(click(&mut wb, &mut text, center(button)), MouseResult::Redraw);
    assert!(wb.menu_drawer_open());
    assert_eq!(wb.active_overlay(), Some(Overlay::MenuDrawer));
    assert!(wb.modal_open(), "nothing behind it gets commands");
    let titles: Vec<String> =
        wb.menu_drawer.as_ref().unwrap().nav().menus().iter().map(|m| m.title.clone()).collect();
    let native: Vec<String> = crate::commands::menus().into_iter().map(|m| m.title).collect();
    assert_eq!(titles, native);
    paint(&mut wb, &mut text);
    // The button again closes it; once it has slid out, it's dropped.
    click(&mut wb, &mut text, center(button));
    assert!(!wb.menu_drawer_open());
    paint(&mut wb, &mut text);
    assert!(wb.menu_drawer.is_none(), "gone once closed");
    assert!(!wb.animating(), "no frames asked for after");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn without_the_button_it_does_not_open_nor_over_another_window() {
    let (mut wb, dir, mut text) = opened("refused");
    wb.set_menu_button(false);
    paint(&mut wb, &mut text);
    assert_eq!(wb.chrome.top_bar.menu_button(), None);
    assert!(!wb.toggle_menu_drawer(), "no button: no drawer (macOS, the setting off)");
    wb.set_menu_button(true);
    wb.open_about();
    assert!(!wb.toggle_menu_drawer(), "not over a modal");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_keyboard_runs_a_command_the_way_the_native_menu_does() {
    let (mut wb, dir, mut text) = opened("keys");
    assert!(wb.toggle_menu_drawer());
    paint(&mut wb, &mut text);
    let before = wb.tabs[wb.active].doc.buffer().to_string();
    // Typing filters (the editor gets nothing); ↵ runs the first result.
    let label = crate::commands::label(crate::commands::SAVE_AS);
    wb.type_text(&mut text, label.trim_end_matches('…'));
    paint(&mut wb, &mut text);
    assert!(wb.menu_drawer.as_ref().unwrap().has_query());
    assert_eq!(wb.tabs[wb.active].doc.buffer().to_string(), before);
    wb.edit_key(&mut text, EditKey::Enter { shift: false });
    assert!(!wb.menu_drawer_open(), "running a command closes it");
    assert_eq!(
        wb.take_request(),
        Some(MouseResult::MenuCommand(crate::commands::SAVE_AS.0.to_owned()))
    );
    // Esc: first clears the filter, then closes.
    assert!(wb.toggle_menu_drawer());
    wb.type_text(&mut text, "x");
    wb.edit_key(&mut text, EditKey::Escape);
    assert!(wb.menu_drawer_open() && !wb.menu_drawer.as_ref().unwrap().has_query());
    wb.edit_key(&mut text, EditKey::Escape);
    assert!(!wb.menu_drawer_open());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_click_outside_closes_it_and_an_item_clicked_runs() {
    let (mut wb, dir, mut text) = opened("clicks");
    wb.toggle_menu_drawer();
    paint(&mut wb, &mut text);
    // The dimmed window: closes it (it's a menu), nothing behind it clicked.
    let active = wb.active;
    assert_eq!(click(&mut wb, &mut text, Point::new(1100.0, 500.0)), MouseResult::Redraw);
    assert!(!wb.menu_drawer_open());
    assert_eq!(wb.active, active);
    // A category clicked shows its submenu; its item clicked runs.
    wb.toggle_menu_drawer();
    paint(&mut wb, &mut text);
    let drawer = wb.menu_drawer.as_ref().unwrap();
    let file = drawer.nav().menus().iter().position(|m| m.title == i18n::t("menu.file")).unwrap();
    wb.menu_drawer.as_mut().unwrap().key(ui_workbench::NavKey::Down);
    for _ in 0..file {
        wb.menu_drawer.as_mut().unwrap().key(ui_workbench::NavKey::Down);
    }
    paint(&mut wb, &mut text);
    assert_eq!(wb.menu_drawer.as_ref().unwrap().nav().open_menu(), Some(file));
    let row = wb.menu_drawer.as_ref().unwrap().item_rect(0).expect("File's first row drawn");
    let at = center(row);
    let result = click(&mut wb, &mut text, at);
    assert_eq!(result, MouseResult::MenuCommand(crate::commands::NEW_FILE.0.to_owned()));
    assert!(!wb.menu_drawer_open());
    let _ = std::fs::remove_dir_all(&dir);
}

/// View › Tool Windows: the third panel, made from the registered tool windows; a window
/// without a toggle of its own (Git) is shown by its row — greyed out outside a repository.
#[test]
fn the_tool_windows_open_as_a_third_panel_and_run_like_the_menu() {
    use ui_workbench::{NavFocus, NavKey};
    let (mut wb, dir, mut text) = opened("third");
    assert!(wb.toggle_menu_drawer());
    paint(&mut wb, &mut text);
    let drawer = wb.menu_drawer.as_mut().unwrap();
    let view = drawer.nav().menus().iter().position(|m| m.title == i18n::t("menu.view")).unwrap();
    for _ in 0..=view {
        drawer.key(NavKey::Down);
    }
    // Into View, its first row (Tool Windows ›), into it.
    drawer.key(NavKey::Right);
    drawer.key(NavKey::Right);
    assert_eq!(drawer.nav().focus(), NavFocus::Sub);
    let (title, rows) = drawer.nav().sub_entries().expect("its submenu");
    assert_eq!(title, i18n::t("menu.tool_windows"));
    let ids: Vec<String> = rows
        .iter()
        .filter_map(|e| match e {
            ui_workbench::MenuEntry::Item { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect();
    assert!(ids.contains(&"view.toggle_project".to_owned()), "{ids:?}");
    assert!(ids.contains(&"tool-window:git".to_owned()), "{ids:?}");
    paint(&mut wb, &mut text);
    let drawer = wb.menu_drawer.as_ref().unwrap();
    let git = rows_index(drawer, "tool-window:git");
    let row = drawer.sub_item_rect(git).expect("drawn");
    // Not a repository: Git's row is there, greyed out, and runs nothing.
    assert_eq!(click(&mut wb, &mut text, center(row)), MouseResult::Redraw);
    assert!(wb.menu_drawer_open());
    paint(&mut wb, &mut text);
    let drawer = wb.menu_drawer.as_ref().unwrap();
    let terminal = rows_index(drawer, "view.toggle_terminal");
    let row = drawer.sub_item_rect(terminal).expect("drawn");
    let result = click(&mut wb, &mut text, center(row));
    assert_eq!(result, MouseResult::MenuCommand("view.toggle_terminal".to_owned()));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The row of the third panel running `id`.
fn rows_index(drawer: &ui_workbench::MenuDrawer, id: &str) -> usize {
    let (_, rows) = drawer.nav().sub_entries().unwrap();
    rows.iter()
        .position(|e| matches!(e, ui_workbench::MenuEntry::Item { id: i, .. } if i == id))
        .unwrap()
}
