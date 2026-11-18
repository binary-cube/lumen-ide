use super::*;

#[test]
fn a_service_and_a_parameter_without_their_signs() {
    assert_eq!(place_at("  arguments: ['@mai", 19), Some((Place::Service, 16..19)));
    assert_eq!(place_at("  a: @mai", 9), Some((Place::Service, 6..9)));
    assert_eq!(place_at("  a: %kernel.pr", 15), Some((Place::Parameter, 6..15)));
}

#[test]
fn a_key_is_alone_at_the_start_of_its_line() {
    assert_eq!(place_at("ser", 3), Some((Place::Key { nested: false }, 0..3)));
    assert_eq!(place_at("    aut", 7), Some((Place::Key { nested: true }, 4..7)));
    assert_eq!(place_at("  a: aut", 8), None, "a value");
    assert_eq!(place_at("# aut", 5), None, "a comment");
}

fn labels(items: Vec<CompletionItem>) -> Vec<String> {
    items.into_iter().map(|i| i.label).collect()
}

fn has(items: Vec<CompletionItem>, word: &str) -> bool {
    labels(items).iter().any(|l| l == word)
}

#[test]
fn each_file_has_its_keys() {
    assert!(has(keys(File::Services, true, Some("services"), "autow"), "autowire"));
    assert!(has(keys(File::Services, false, None, "serv"), "services"));
    assert!(has(keys(File::Routes, true, Some("home"), "contr"), "controller"));
}

#[test]
fn a_package_s_keys_are_its_bundle_s() {
    assert!(has(keys(File::Package, false, None, "doct"), "doctrine"));
    assert!(has(keys(File::Package, true, Some("framework"), "sec"), "secret"));
    assert!(has(keys(File::Package, true, Some("twig"), "default"), "default_path"));
    assert!(!has(keys(File::Package, true, Some("twig"), "sec"), "secret"));
    assert!(keys(File::Package, true, None, "sec").is_empty(), "deeper in");
}

#[test]
fn the_parent_is_only_for_its_direct_children() {
    assert_eq!(parent_at("twig:\n    pa", 10), Some("twig"));
    assert_eq!(parent_at("twig:\n    debug: true\n    pa", 26), Some("twig"));
    assert_eq!(parent_at("twig:\n    paths:\n        a", 25), None);
}

#[test]
fn route_names_and_other_files_have_none() {
    assert!(keys(File::Routes, false, None, "serv").is_empty());
    assert!(keys(File::Other, true, None, "pat").is_empty());
}

#[test]
fn a_package_file_is_told_from_the_rest() {
    assert_eq!(File::of("config/packages/twig.yaml", false), File::Package);
    assert_eq!(File::of("config/services.yaml", false), File::Services);
    assert_eq!(File::of("config/routes.yaml", true), File::Routes);
}

#[test]
fn only_the_configuration() {
    assert!(is_config("config/services.yaml"));
    assert!(!is_config("translations/messages.yaml"));
}
