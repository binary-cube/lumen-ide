//! The module's contributions: its window at the bottom (a panel, after the terminal), its
//! toggle with ⌘8 / Alt+8, its button's menu, its icon in the app's set.

use super::*;

#[test]
fn its_window_is_a_panel_at_the_bottom_toggled_by_its_command() {
    let tool = ServicesTool;
    assert_eq!(tool.id(), TOOL);
    assert_eq!(tool.dock(), Dock::Bottom);
    assert_eq!(tool.content(), ToolContent::Panel);
    assert_eq!(tool.toggle_command(), Some(TOGGLE.0));
    assert_eq!(tool.icon(), "services");
    assert!(tool.order() > 0, "after the terminal");
    assert!(tool.create().is_some(), "its panel, made when it's first shown");
    assert!(icons::Icons::builtin().id("services").is_some(), "its icon is in the app's set");
    for name in ["text-wrap", "arrow-down-to-line"] {
        assert!(icons::Icons::builtin().id(name).is_some(), "{name}");
    }
}

#[test]
fn the_module_contributes_its_window_command_and_menu() {
    let mut registry = extensions::ModuleRegistry::default();
    registry.register(Box::new(ServicesModule));
    let (mut contributions, reports) = registry.activate(&extensions::ModuleSelection::default());
    assert!(reports.iter().all(|r| r.state == extensions::ModuleState::Active));
    let tools = contributions.take::<Arc<dyn ToolWindowProvider>>();
    assert_eq!(tools.iter().map(|t| t.id()).collect::<Vec<_>>(), [TOOL]);
    let commands = contributions.take::<CommandContribution>();
    let toggle = commands.iter().find(|c| c.spec.id == TOGGLE).unwrap();
    assert_eq!(toggle.spec.shortcuts, ["CmdOrCtrl+8", "Alt+8"]);
    assert!(commands.iter().any(|c| c.spec.id == EXPAND_ALL));
    assert!(commands.iter().any(|c| c.spec.id == COLLAPSE_ALL));
    let menus = contributions.take::<MenuContribution>();
    assert_eq!(menus.iter().map(|m| m.id).collect::<Vec<_>>(), [TOOL], "its button's menu");
    let presets = contributions.take::<command_model::bindings::PresetShortcuts>();
    assert!(presets.iter().any(|p| p.preset == "jetbrains" && p.command == TOGGLE));
}
