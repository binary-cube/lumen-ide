use super::*;

#[test]
fn contributes_the_preview_and_the_refactor_menu() {
    let mut registry = extensions::ModuleRegistry::default();
    registry.register(Box::new(RefactoringModule));
    let (mut contributions, reports) = registry.activate(&Default::default());
    assert!(reports.iter().all(|r| r.state == extensions::ModuleState::Active));
    assert_eq!(contributions.take::<Arc<dyn ViewerProvider>>().len(), 4);
    let commands = contributions.take::<CommandContribution>();
    let ids: Vec<&str> = commands.iter().map(|c| c.spec.id.0).collect();
    assert_eq!(ids, [FIND_USAGES.0, RENAME.0, CHANGE_SIGNATURE.0, MOVE.0, UNDO.0]);
    assert!(MoveProvider.kind(Path::new("/p/Move User.view-move")).is_some());
    assert!(commands.iter().all(|c| c.spec.menu.is_some_and(|m| {
        [sections::REFACTOR_CHANGE, sections::REFACTOR_UNDO].contains(&m.section)
    }) || c.spec.id == FIND_USAGES));
    assert!(RenameProvider.kind(Path::new("/p/Rename User.view-rename")).is_some());
    assert!(RenameProvider.kind(Path::new("/p/User.php")).is_none());
}
