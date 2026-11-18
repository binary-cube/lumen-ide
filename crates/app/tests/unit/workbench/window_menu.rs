use super::*;

fn workbench() -> Workbench {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    wb
}

fn ids(entries: &[ui_workbench::MenuEntry]) -> Vec<String> {
    entries
        .iter()
        .filter_map(|e| match e {
            ui_workbench::MenuEntry::Item { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect()
}

/// A bottom window's options: its module's rows first, then the app's — its tabs closed,
/// half or all the height (switches, as they stand), hide; each does what it says.
#[test]
fn a_bottom_window_s_options_end_with_the_app_s_rows() {
    let mut wb = workbench();
    assert!(wb.toggle_tool("terminal"));
    let entries = wb.window_menu_entries("terminal");
    let rows = ids(&entries);
    let app: Vec<&str> = rows.iter().filter_map(|r| r.strip_prefix(WINDOW_MENU)).collect();
    assert_eq!(app, ["close-all:terminal", "half:terminal", "full:terminal", "hide:terminal"]);
    // The module's rows come before the app's.
    let first_app = rows.iter().position(|r| r.starts_with(WINDOW_MENU)).unwrap();
    assert!(rows[..first_app].iter().all(|r| !r.starts_with(WINDOW_MENU)));
    assert!(wb.window_menu("half:terminal"));
    assert_eq!(wb.tools.bottom_size, BottomSize::Half);
    let half_on = wb.window_menu_entries("terminal").into_iter().any(|e| {
        matches!(e, ui_workbench::MenuEntry::Item { id, on: Some(true), .. } if id == "window-menu:half:terminal")
    });
    assert!(half_on, "the switch shows it's on");
    assert!(wb.window_menu("close-all:terminal"));
    assert!(!wb.bottom_shows("terminal"), "no tab left: the window goes");
    assert!(wb.toggle_tool("terminal"));
    assert!(wb.window_menu("hide:terminal"));
    assert!(!wb.bottom_shows("terminal"));
    assert!(!wb.window_menu("nothing:terminal"));
}

/// A side panel's options: no size rows (it isn't at the bottom); hide puts it away.
#[test]
fn a_side_panel_s_options_hide_it() {
    let mut wb = workbench();
    assert!(wb.show_side_panel("structure"));
    let rows = ids(&wb.window_menu_entries("structure"));
    let app: Vec<&str> = rows.iter().filter_map(|r| r.strip_prefix(WINDOW_MENU)).collect();
    assert_eq!(app, ["hide:structure"]);
    assert!(wb.window_menu("hide:structure"));
    assert!(!wb.tools.sides.iter().any(|s| s.id == "structure"));
}
