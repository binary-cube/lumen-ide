use super::*;

/// It offers its view, its command and its row in a folder's menu (only there).
#[test]
fn contributes_its_view_and_a_folder_s_menu_row() {
    let mut registry = extensions::ModuleRegistry::default();
    registry.register(Box::new(FolderSizesModule));
    let (mut contributions, reports) = registry.activate(&Default::default());
    assert!(reports.iter().all(|r| r.state == extensions::ModuleState::Active));
    assert_eq!(contributions.take::<Arc<dyn ViewerProvider>>().len(), 1);
    let commands = contributions.take::<CommandContribution>();
    assert_eq!(commands.iter().map(|c| c.spec.id).collect::<Vec<_>>(), [ANALYZE]);
    let rows = contributions.take::<ContextMenuItem>();
    assert_eq!(rows.len(), 1);
    assert!(Provider.kind(Path::new("x.view-folder-sizes")).is_some());
}
