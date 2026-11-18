use super::*;

#[test]
fn it_contributes_the_view_its_actions_and_its_icon() {
    let mut registry = extensions::ModuleRegistry::default();
    registry.register(Box::new(DatabaseDiagramModule));
    let (mut contributions, _) = registry.activate(&Default::default());
    let viewers = contributions.take::<Arc<dyn ViewerProvider>>();
    assert_eq!(viewers.len(), 1);
    let path = Path::new(&format!("shop.{}", database::diagram::VIEW_EXTENSION)).to_path_buf();
    assert_eq!(viewers[0].kind(&path), Some(ViewerKind::Replace));
    assert_eq!(viewers[0].kind(Path::new("shop.sqlite")), None);
    let commands = contributions.take::<CommandContribution>();
    let ids: Vec<&str> = commands.iter().map(|c| c.spec.id.0).collect();
    assert_eq!(ids, ACTIONS.map(|a| a.0.0));
    assert_eq!(contributions.take::<languages::IconSvg>()[0].name, "relations");
    // Its tab (and breadcrumb) shows it, by the view's extension.
    let rules = contributions.take::<languages::FileIconRule>();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].icon, "relations");
    assert_eq!(
        rules[0].matcher,
        languages::FileMatcher::Extension(database::diagram::VIEW_EXTENSION)
    );
}
