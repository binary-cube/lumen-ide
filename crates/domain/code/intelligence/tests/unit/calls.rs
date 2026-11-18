use super::*;

#[test]
fn a_registered_call_names_its_results_class_argument() {
    register(ClassArgumentCall::new("Test\\Locator", "get", 0).names("test.services"));
    let rule = class_argument("\\Test\\Locator", "GET").expect("as PHP compares names");
    assert_eq!((rule.argument, rule.names), (0, Some("test.services")));
    assert!(class_argument("Test\\Locator", "has").is_none());
    assert!(class_argument("Other\\Locator", "get").is_none());
}

/// A table's names are kept when the symbols are built again (they're a framework's, not the
/// files'); an empty one goes.
#[test]
fn a_projects_named_types_survive_a_rebuild() {
    let mut index = crate::SymbolIndex::default();
    let names = std::collections::HashMap::from([("mailer".to_owned(), "App\\Mailer".to_owned())]);
    index.set_named("test.services", names);
    index.replace_symbols(crate::SymbolIndex::default());
    assert_eq!(index.named_type("test.services", "mailer"), Some("App\\Mailer"));
    assert_eq!(index.named_type("test.services", "other"), None);
    index.set_named("test.services", std::collections::HashMap::new());
    assert_eq!(index.named_type("test.services", "mailer"), None);
}
