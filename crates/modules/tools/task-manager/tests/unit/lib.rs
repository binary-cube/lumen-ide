use super::*;

#[test]
fn contributes_its_tab_its_button_and_its_command() {
    let mut registry = extensions::ModuleRegistry::default();
    registry.register(Box::new(TaskManagerModule));
    let (mut contributions, _) = registry.activate(&Default::default());
    // Its button opens the tab (no side panel).
    let tools = contributions.take::<Arc<dyn ToolWindowProvider>>();
    assert_eq!(tools[0].id(), TOOL);
    assert!(matches!(
        tools[0].content(),
        ToolContent::Command { command, tab: Some(EXTENSION) } if command == OPEN.0
    ));
    assert!(tools[0].create().is_none(), "no panel");
    let viewers = contributions.take::<Arc<dyn ViewerProvider>>();
    let tab = Path::new("/p/Task Manager.view-task-manager");
    assert_eq!(viewers[0].kind(tab), Some(ViewerKind::Replace));
    assert_eq!(viewers[0].kind(Path::new("/p/a.php")), None);
    let icons = contributions.take::<languages::FileIconRule>();
    assert_eq!(icons[0].icon, "cpu");
    // It counts itself (its collector's thread).
    let reporters = contributions.take::<Arc<dyn workbench_model::ResourceReporter>>();
    assert!(reporters[0].report()[0].runs_on("task-manager"));
    let commands = contributions.take::<CommandContribution>();
    let ids: Vec<&str> = commands.iter().map(|c| c.spec.id.0).collect();
    assert_eq!(&ids[..2], [OPEN.0, SAVE.0]);
    // Its question's modal, a view of its own.
    let modal = Path::new("/p/Stop.view-task-confirm");
    assert_eq!(viewers[1].kind(modal), Some(ViewerKind::Replace));
    // Its actions: in the palette, their keys only in its tab.
    assert_eq!(ids[2..].len(), ACTIONS.len());
    for command in &commands[4..] {
        assert_eq!(command.handler.when(), command_model::When::View(VIEW), "{}", command.spec.id);
    }
}
