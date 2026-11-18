use super::*;

/// It follows the windows, has its settings, and offers the editor's tools.
#[test]
fn it_offers_its_service_settings_and_tools() {
    let mut registry = extensions::ModuleRegistry::default();
    registry.register(Box::new(McpModule));
    let (mut contributions, _) = registry.activate(&Default::default());
    assert_eq!(contributions.take::<Arc<dyn ProjectService>>().len(), 1);
    assert_eq!(contributions.take::<Arc<dyn workbench_model::SettingsContribution>>().len(), 1);
    let mut names: Vec<&str> =
        contributions.take::<Arc<dyn mcp::McpTool>>().iter().map(|t| t.name()).collect();
    names.sort_unstable();
    assert_eq!(names, ["get_file_text", "get_open_files", "get_selection", "open_file"]);
}
