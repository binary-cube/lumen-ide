use super::*;

#[test]
fn templates_are_read_with_what_they_are_written_over() {
    let engines = Engines::load();
    for file in ["a.html.twig", "a.blade.php"] {
        assert!(engines.is_hosted(Path::new(file)), "{file}");
        assert_eq!(engines.of(Path::new(file)).unwrap().language().as_str(), "html");
    }
    assert!(!engines.is_hosted(Path::new("a.php")));
    assert_eq!(engines.of(Path::new("a.php")).unwrap().language().as_str(), "php");
}

#[test]
fn a_language_answers_by_its_own_engine() {
    let engines = Engines::load();
    assert_eq!(engines.of(Path::new("a.sql")).unwrap().language().as_str(), "sql");
    assert_eq!(engines.of(Path::new("a.css")).unwrap().language().as_str(), "css");
    assert!(engines.of(Path::new("a.unknown")).is_none());
}
