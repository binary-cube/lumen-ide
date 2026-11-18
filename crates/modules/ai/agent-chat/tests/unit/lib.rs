use super::*;

/// It shows its tabs and answers the request to open one.
#[test]
fn it_shows_conversations_and_answers_for_them() {
    let mut registry = extensions::ModuleRegistry::default();
    registry.register(Box::new(ChatModule));
    let (mut contributions, _) = registry.activate(&Default::default());
    let viewers = contributions.take::<Arc<dyn ViewerProvider>>();
    let tab = std::path::PathBuf::from(format!("matching-system.{EXTENSION}"));
    assert_eq!(viewers[0].kind(&tab), Some(ViewerKind::Replace));
    let answers = contributions.take::<workbench_model::Answers>();
    assert_eq!(answers.len(), 1);
}
