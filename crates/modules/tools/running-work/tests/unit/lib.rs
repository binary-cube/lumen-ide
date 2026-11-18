use super::*;

#[test]
fn contributes_the_question_s_view() {
    let mut registry = extensions::ModuleRegistry::default();
    registry.register(Box::new(RunningWorkModule));
    let (mut contributions, _) = registry.activate(&Default::default());
    let viewers = contributions.take::<Arc<dyn ViewerProvider>>();
    assert_eq!(viewers.len(), 1);
    let path = std::path::PathBuf::from(format!("modal.{PROMPT_VIEW}"));
    assert_eq!(viewers[0].kind(&path), Some(ViewerKind::Replace));
    assert_eq!(viewers[0].kind(std::path::Path::new("a.php")), None);
}
