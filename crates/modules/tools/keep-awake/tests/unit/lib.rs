use super::*;
use ui_components::{Dock, ToolContent};

/// Its button sits right above Focus mode's (order 5) at the right strip's end; its command
/// has no shortcut of its own; its page is in the settings.
#[test]
fn contributes_its_button_above_focus_modes_its_command_and_its_page() {
    let mut registry = extensions::ModuleRegistry::default();
    registry.register(Box::new(KeepAwakeModule));
    let (mut contributions, _) = registry.activate(&Default::default());
    let tools = contributions.take::<Arc<dyn ToolWindowProvider>>();
    assert_eq!(tools[0].id(), button::TOOL);
    assert_eq!(tools[0].dock(), Dock::Right);
    assert!(tools[0].strip_end() && tools[0].order() < 5);
    assert!(matches!(
        tools[0].content(),
        ToolContent::Command { command, tab: None } if command == TOGGLE.0
    ));
    let icons = contributions.take::<languages::IconSvg>();
    assert!(icons.iter().any(|i| i.name == ICON && i.svg.contains("<path")));
    let commands = contributions.take::<CommandContribution>();
    assert_eq!(commands[0].spec.id, TOGGLE);
    assert!(commands[0].spec.shortcuts.is_empty(), "chosen in the keymap");
    let pages = contributions.take::<Arc<dyn workbench_model::SettingsContribution>>();
    assert_eq!(pages[0].page_info().map(|p| p.id), Some("keep_awake"));
    // Off by default: the button dark, nothing in the status bar.
    assert!(!tools[0].active());
    let indicators = contributions.take::<Arc<dyn StatusIndicator>>();
    assert!(indicators[0].indicator().is_none());
    // Held: the status bar says so, and a click turns it off.
    keeper::keeper().set(Some(power::Keep::default())).unwrap();
    let shown = indicators[0].indicator().expect("said while held");
    assert_eq!(shown.command, Some(TOGGLE));
    keeper::keeper().set(None).unwrap();
    assert!(indicators[0].indicator().is_none());
    // Outside the windows too (by the clock, on the app's icon), only while held; its menu
    // turns it off.
    let signs = contributions.take::<Arc<dyn SystemIndicator>>();
    assert!(signs[0].sign().is_none());
    keeper::keeper().set(Some(power::Keep::default())).unwrap();
    let sign = signs[0].sign().expect("shown while held");
    assert_eq!((sign.icon, sign.action.map(|a| a.1)), (ICON, Some(TOGGLE)));
    assert!(!sign.tooltip.contains("{app}"), "the app named");
    keeper::keeper().set(None).unwrap();
    assert!(signs[0].sign().is_none());
    // Its button's menu: the same commands, Settings… last.
    let menus = contributions.take::<MenuContribution>();
    let menu = menus.iter().find(|m| m.id == button::TOOL).expect("its menu");
    let rows = menu.rows_in(MenuPlace::ToolStrip, &|_| true);
    assert_eq!(rows.first(), Some(&MenuRow::Command(TOGGLE, &[MenuPlace::ToolStrip])));
    assert_eq!(rows.last(), Some(&MenuRow::Command(SETTINGS, &[MenuPlace::ToolStrip])));
}

/// The screen dims after the minutes chosen only while kept awake; the dim command is greyed
/// otherwise.
#[test]
fn the_screen_dims_only_while_kept_awake() {
    assert_eq!(IdleDim.after(), None, "not kept awake: the system's own screen saver");
    keeper::keeper().set(Some(power::Keep::default())).unwrap();
    assert_eq!(IdleDim.after(), Some(Duration::from_secs(600)), "10 minutes by default");
    assert_eq!(IdleDim.command(), DIM_NOW);
    keeper::keeper().set(None).unwrap();
}
