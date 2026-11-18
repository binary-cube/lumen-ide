use super::*;

#[test]
fn contributes_the_service_the_view_the_command_and_its_menu() {
    let mut registry = extensions::ModuleRegistry::default();
    registry.register(Box::new(LocalHistoryModule));
    let (mut contributions, reports) = registry.activate(&Default::default());
    assert!(reports.iter().all(|r| r.state == extensions::ModuleState::Active));
    assert_eq!(contributions.take::<Arc<dyn ProjectService>>().len(), 1);
    assert_eq!(contributions.take::<Arc<dyn ViewerProvider>>().len(), 1);
    let commands = contributions.take::<CommandContribution>();
    assert_eq!(
        commands.iter().map(|c| c.spec.id).collect::<Vec<_>>(),
        [CURRENT, SHOW_HISTORY, RELOAD]
    );
    // Reloading works in the history's view, on its key.
    assert_eq!(commands[2].handler.when(), command_model::When::View(MODULE_ID));
    assert_eq!(commands[2].spec.shortcuts, ["r"]);
    let menus = contributions.take::<MenuContribution>();
    assert_eq!(menus[0].rows_in(MenuPlace::TreeFile, &|_| true).len(), 1);
    assert!(menus[0].rows_in(MenuPlace::ToolStrip, &|_| true).is_empty());
}
