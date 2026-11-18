use super::*;
use document::Document;
use languages::LanguageRegistry;
use std::path::Path;
use syntax_ts::{Grammars, Highlighter, SyntaxConfig, parse_document};

const SYMFONY: &str = include_str!("../data/symfony.env");
const LARAVEL: &str = include_str!("../data/laravel.env");

fn config() -> SyntaxConfig {
    let mut grammars = Grammars::default();
    grammars.register(grammar());
    SyntaxConfig::new(grammars)
}

/// The scope drawn at the start of `needle`'s `nth` occurrence (the last span applied wins).
fn scope_at(config: &SyntaxConfig, text: &str, needle: &str, nth: usize) -> Option<String> {
    let doc = Document::new(text);
    let snapshot = parse_document(config, &LANGUAGE, &doc.buffer().snapshot(), None, &[], 0)?;
    let mut spans = Vec::new();
    Highlighter::new().highlight(&snapshot, &doc.buffer().snapshot(), 0..text.len(), &mut spans);
    let at = text.match_indices(needle).nth(nth).unwrap_or_else(|| panic!("{needle}")).0;
    spans
        .iter()
        .rev()
        .find(|sp| sp.range.contains(&at))
        .and_then(|sp| config.scope(sp.scope))
        .map(|(_, name)| name)
}

#[test]
fn queries_compile() {
    let mut g = Grammars::default();
    g.register(grammar());
    assert_eq!(g.validate(), Vec::<String>::new());
}

#[test]
fn env_files_are_found_by_name() {
    let mut registry = LanguageRegistry::default();
    registry.register(descriptor());
    registry.register(LanguageDescriptor::new("php", "PHP").extensions(&["php"]));
    for name in [
        ".env",
        ".env.local",
        ".env.prod",
        ".env.example",
        ".env.test",
        ".env.dist",
        ".env.dev.local",
        "docker.env",
        "app/.env.staging",
    ] {
        assert_eq!(registry.detect(Path::new(name), "").as_str(), ID, "{name}");
    }
    // Symfony's compiled values are PHP.
    assert_eq!(registry.detect(Path::new(".env.local.php"), "").as_str(), "php");
    assert_ne!(registry.detect(Path::new("env"), "").as_str(), ID);
    assert_eq!(descriptor().line_comment, Some("#"));
}

#[test]
fn a_symfony_file_is_highlighted() {
    let (c, s) = (config(), SYMFONY);
    let at = |needle: &str, nth: usize| scope_at(&c, s, needle, nth);
    assert_eq!(at("###> symfony/framework-bundle", 0).as_deref(), Some("comment"));
    assert_eq!(at("APP_ENV=dev", 0).as_deref(), Some("property"));
    assert_eq!(at("=dev", 0).as_deref(), Some("operator"));
    assert_eq!(at("dev\n", 0).as_deref(), Some("string"));
    assert_eq!(at("true", 0).as_deref(), Some("boolean"));
    assert_eq!(at("postgresql://", 0).as_deref(), Some("function"));
    assert_eq!(at("doctrine://", 0).as_deref(), Some("function"));
    assert_eq!(at("%kernel.project_dir%", 0).as_deref(), Some("type"));
    assert_eq!(at("./var/uploads", 0).as_deref(), Some("type"));
    assert_eq!(at("20\n", 0).as_deref(), Some("number"));
    assert_eq!(at("# rotate", 0).as_deref(), Some("comment"));
    assert_eq!(at("export", 0).as_deref(), Some("keyword"));
    // Interpolated in double quotes (the name as a key, its `${` `}` marked, its default a
    // string); literal in single quotes.
    assert_eq!(at("APP_NAME:-", 0).as_deref(), Some("property"));
    assert_eq!(at("${APP_NAME", 0).as_deref(), Some("punctuation.special"));
    assert_eq!(at(":-the", 0).as_deref(), Some("operator"));
    assert_eq!(at("the app}", 0).as_deref(), Some("string"));
    assert_eq!(at("\\\"", 0).as_deref(), Some("string.escape"));
    assert_eq!(at("${APP_ENV}'", 0).as_deref(), Some("string"));
    // A value going on past its line, and the entry after it.
    assert_eq!(at("on $APP_ENV", 0).as_deref(), Some("string"));
    assert_eq!(at("APP_ENV\"", 0).as_deref(), Some("property"));
}

#[test]
fn a_laravel_file_is_highlighted() {
    let (c, s) = (config(), LARAVEL);
    let at = |needle: &str, nth: usize| scope_at(&c, s, needle, nth);
    assert_eq!(at("APP_NAME=", 0).as_deref(), Some("property"));
    assert_eq!(at("http://localhost", 0).as_deref(), Some("function"));
    assert_eq!(at("3306", 0).as_deref(), Some("number"));
    assert_eq!(at("base64:", 0).as_deref(), Some("string"));
    assert_eq!(at("APP_NAME}", 0).as_deref(), Some("property"));
    // An empty value leaves the next entry alone.
    assert_eq!(at("AWS_SECRET_ACCESS_KEY", 0).as_deref(), Some("property"));
}

#[test]
fn a_multiline_value_folds() {
    let text = "CERT=\"-----BEGIN-----\nabc\n-----END-----\"\nNEXT=1\n";
    let doc = Document::new(text);
    let snapshot =
        parse_document(&config(), &LANGUAGE, &doc.buffer().snapshot(), None, &[], 0).unwrap();
    let folds: Vec<(u32, u32)> =
        snapshot.folds().iter().map(|f| (f.start_line, f.end_line)).collect();
    assert_eq!(folds, vec![(0, 2)]);
}

#[test]
fn the_sections_fold_beside_the_grammars_regions() {
    let mut config = config();
    config.add_fold_provider(std::sync::Arc::new(intelligence::SectionFolds));
    let text = "# Certificates\nCERT=\"-----BEGIN-----\nabc\n-----END-----\"\nKEY=1\n";
    let doc = Document::new(text);
    let snapshot =
        parse_document(&config, &LANGUAGE, &doc.buffer().snapshot(), None, &[], 0).unwrap();
    let folds: Vec<(u32, u32)> =
        snapshot.folds().iter().map(|f| (f.start_line, f.end_line)).collect();
    assert_eq!(folds, vec![(0, 4), (1, 3)], "the section, and the value spanning lines in it");
}
