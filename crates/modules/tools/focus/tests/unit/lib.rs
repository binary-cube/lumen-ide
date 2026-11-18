use super::*;
use ui_components::{Dock, ToolContent};

#[test]
fn contributes_its_button_above_relax_modes_its_command_and_its_emphasis() {
    let mut registry = extensions::ModuleRegistry::default();
    registry.register(Box::new(FocusModule));
    let (mut contributions, _) = registry.activate(&Default::default());
    let tools = contributions.take::<Arc<dyn ToolWindowProvider>>();
    assert_eq!(tools[0].id(), button::TOOL);
    assert_eq!(tools[0].dock(), Dock::Right);
    // Relax mode's is at the strip's end with order 10: this one right above it.
    assert!(tools[0].strip_end() && tools[0].order() < 10);
    assert!(matches!(
        tools[0].content(),
        ToolContent::Command { command, tab: None } if command == TOGGLE.0
    ));
    let icons = contributions.take::<languages::IconSvg>();
    assert!(icons.iter().any(|i| i.name == ICON && i.svg.contains("<path")));
    let commands = contributions.take::<CommandContribution>();
    assert_eq!(commands[0].spec.id, TOGGLE);
    assert!(commands[0].spec.shortcuts.is_empty(), "chosen in the keymap");
    let emphasis = contributions.take::<Arc<dyn RegionEmphasis>>();
    assert_eq!(emphasis.len(), 1);
    // Off at start: nothing asked, the button dark; toggled (what the command does): asked,
    // lit; again: off.
    assert!(emphasis[0].request().is_none() && !tools[0].active());
    assert!(state::toggle());
    assert!(emphasis[0].request().is_some() && tools[0].active());
    assert!(!state::toggle());
    assert!(emphasis[0].request().is_none() && !tools[0].active());
}
