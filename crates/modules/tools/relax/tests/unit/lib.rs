use super::*;

#[test]
fn contributes_its_button_above_the_task_managers_its_view_and_its_command() {
    let mut registry = extensions::ModuleRegistry::default();
    registry.register(Box::new(RelaxModule));
    let (mut contributions, _) = registry.activate(&Default::default());
    let tools = contributions.take::<Arc<dyn ToolWindowProvider>>();
    assert_eq!(tools[0].id(), TOOL);
    assert_eq!(tools[0].dock(), Dock::Right);
    // The Task Manager's is at the strip's end with order 20: this one right above it.
    assert!(tools[0].strip_end() && tools[0].order() < 20);
    assert!(matches!(
        tools[0].content(),
        ToolContent::Command { command, tab: None } if command == START.0
    ));
    assert_eq!(tools[0].icon(), ICON);
    let icons = contributions.take::<languages::IconSvg>();
    assert!(icons.iter().any(|i| i.name == ICON && i.svg.contains("<path")));
    let viewers = contributions.take::<Arc<dyn ViewerProvider>>();
    assert_eq!(viewers[0].kind(Path::new("modal.view-relax")), Some(ViewerKind::Replace));
    assert_eq!(viewers[0].kind(Path::new("/p/a.php")), None);
    let view = viewers[0].open(Path::new("modal.view-relax")).unwrap();
    assert!(view.covers_window(), "drawn over everything: nothing behind is painted");
    let commands = contributions.take::<CommandContribution>();
    assert_eq!(commands[0].spec.id, START);
}

#[test]
fn it_starts_by_itself_only_when_asked() {
    let mut registry = extensions::ModuleRegistry::default();
    registry.register(Box::new(RelaxModule));
    let (mut contributions, _) = registry.activate(&Default::default());
    let idle = contributions.take::<Arc<dyn IdleAction>>();
    assert_eq!(idle[0].command(), START);
    let before = settings::config();
    settings::set_config(settings::Config { idle_minutes: Some(10), ..before });
    assert_eq!(idle[0].after(), Some(Duration::from_secs(600)));
    settings::set_config(settings::Config { idle_minutes: None, ..before });
    assert_eq!(idle[0].after(), None, "off");
    settings::set_config(before);
}
