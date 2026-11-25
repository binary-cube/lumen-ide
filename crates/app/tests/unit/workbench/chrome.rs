use super::*;

/// A window without a project, laid out and painted.
fn painted() -> (Workbench, TextSystem) {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    let mut text = TextSystem::new();
    wb.paint(&mut text, &mut render_api::Scene::default());
    (wb, text)
}

/// Where the top bar's button `id` is (a point on it), as last painted.
fn button(wb: &Workbench, id: &'static str) -> Option<Point> {
    (0..1200)
        .map(|x| Point::new(x as f32, 20.0))
        .find(|p| wb.chrome.top_bar.hit(*p) == Some(TopBarHit::Action(id)))
}

/// Each window is kept on top on its own: off at first, the toggle turns it on and off,
/// and the command's state (its ✓ in a menu, the button lit) follows it.
#[test]
fn keeping_a_window_on_top_is_a_switch_of_its_own() {
    let (mut wb, _) = painted();
    let (other, _) = painted();
    let id = crate::commands::KEEP_ON_TOP;
    assert_eq!(wb.command_switched(id), Some(false), "a switch, off at first");
    assert!(wb.toggle_on_top());
    assert_eq!(wb.command_switched(id), Some(true));
    assert_eq!(other.command_switched(id), Some(false), "another window: its own");
    assert!(!wb.toggle_on_top());
    assert_eq!(wb.command_switched(id), Some(false));
    assert_eq!(wb.command_switched(crate::commands::SAVE), None, "a plain action");
}

/// The command: in the Window menu, with its label and its icon.
#[test]
fn keep_on_top_is_a_command_of_the_window_menu() {
    let id = crate::commands::KEEP_ON_TOP;
    assert_eq!(crate::commands::by_id("window.keep_on_top"), Some(id));
    assert!(!crate::commands::label(id).is_empty());
    let path = crate::commands::menu_path(id);
    assert_eq!(path.len(), 1, "a row of a menu of the bar: {path:?}");
    assert_eq!(crate::commands::menu_icon(id.0), Some("pin"));
}

/// The top bar's right side, as registered: search, settings, a line, the pin — the pin
/// lit while the window is kept on top, its tooltip saying how it stands, a click running
/// its command.
#[test]
fn the_pin_comes_after_search_and_settings_past_a_line() {
    let (mut wb, mut text) = painted();
    let ids: Vec<&str> = wb.docked(ui_workbench::Dock::Top).iter().map(|t| t.id()).collect();
    assert_eq!(ids, ["search", "settings", "keep-on-top"]);
    let [Some(search), Some(settings), Some(pin)] =
        ["search", "settings", "keep-on-top"].map(|id| button(&wb, id))
    else {
        panic!("every button drawn");
    };
    let lines = wb.chrome.top_bar.separators();
    assert_eq!(lines.len(), 1, "one line, before the pin");
    assert!(search.x < settings.x && settings.x < lines[0].min_x() && lines[0].max_x() < pin.x);
    let tip = |wb: &Workbench| wb.tooltip_at(pin).map(|t| t.label).unwrap_or_default();
    let off = tip(&wb);
    wb.toggle_on_top();
    wb.paint(&mut text, &mut render_api::Scene::default());
    let on = tip(&wb);
    assert_ne!(on, off, "the tooltip says how it stands");
    assert_eq!(on, i18n::tr!("tooltip.switched_on", name = i18n::t("tooltip.keep_on_top")));
    let pin_tool = wb.tool("keep-on-top").cloned().expect("registered");
    assert_eq!(wb.tool_switched(pin_tool.as_ref()), Some(true), "drawn lit");
    assert_eq!(
        wb.mouse_down(&mut text, pin, false, false, false, 1),
        MouseResult::Command(crate::commands::KEEP_ON_TOP.0),
        "a click runs its command (the app tells the system)"
    );
}

/// Where the system can't keep a window on top (Wayland): no pin, and its command greyed
/// out with why — nothing offered that does nothing.
#[test]
fn without_the_system_s_help_there_is_no_pin() {
    let (mut wb, mut text) = painted();
    wb.set_keeps_on_top(false);
    wb.paint(&mut text, &mut render_api::Scene::default());
    let ids: Vec<&str> = wb.docked(ui_workbench::Dock::Top).iter().map(|t| t.id()).collect();
    assert_eq!(ids, ["search", "settings"]);
    assert!(wb.chrome.top_bar.separators().is_empty(), "no line left alone");
    let id = crate::commands::KEEP_ON_TOP;
    assert!(!wb.command_applies(id));
    let context = wb.menu_context();
    assert_eq!(
        wb.row_state(&context, id.0),
        RowState::Disabled(Some(command_model::Requires::KeepOnTop))
    );
    wb.set_keeps_on_top(true);
    assert!(wb.command_applies(id));
}
