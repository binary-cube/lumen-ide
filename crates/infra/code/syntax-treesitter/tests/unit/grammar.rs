use super::*;
use crate::{SyntaxConfig, parse_document};
use document::Document;

fn grammars() -> Grammars {
    let mut grammars = Grammars::default();
    grammars.register(Grammar {
        language: LanguageId::new_static("javascript"),
        ts: tree_sitter_javascript::LANGUAGE.into(),
        highlights: "(identifier) @variable".into(),
        injections: "".into(),
        folds: "".into(),
        indents: "".into(),
        string_content_nodes: &[],
        embedded_errors: true,
        injected_errors: true,
        own_errors: true,
    });
    grammars
}

#[test]
fn queries_stay_while_a_file_uses_them_and_go_after() {
    let js = LanguageId::new_static("javascript");
    let config = SyntaxConfig::new(grammars());
    let grammars = config.grammars();
    assert!(!grammars.is_compiled(&js), "nothing compiled before a file needs it");
    assert_eq!(grammars.memory(&js), 0);
    let text = Document::new("let a = 1;").buffer().snapshot();
    let parsed = parse_document(&config, &js, &text, None, &[], 0).unwrap();
    assert!(grammars.is_compiled(&js));
    assert!(grammars.memory(&js) > 0, "the compiled queries are counted");
    // An open file (its tree) holds them.
    assert_eq!(grammars.release_idle(Duration::ZERO, |_| false), 0);
    assert!(grammars.is_compiled(&js));
    drop(parsed);
    // Kept on purpose (chosen "at startup"), or not idle long enough: they stay.
    assert_eq!(grammars.release_idle(Duration::ZERO, |l| *l == js), 0);
    assert_eq!(grammars.release_idle(Duration::from_secs(3600), |_| false), 0);
    assert_eq!(grammars.release_idle(Duration::ZERO, |_| false), 1);
    assert!(!grammars.is_compiled(&js));
    assert_eq!(grammars.memory(&js), 0, "let go");
    // Needed again: compiled again.
    let again = parse_document(&config, &js, &text, None, &[], 0).unwrap();
    assert!(grammars.is_compiled(&js));
    assert!(again.folds().is_empty());
}

/// A grammar not trusted with its own errors (a permissive SQL's) keeps them quiet: told
/// only when asked for (a console showing them faint).
#[test]
fn quiet_errors_are_those_a_grammar_doesn_t_report() {
    let js = LanguageId::new_static("javascript");
    let mut quiet = Grammars::default();
    quiet.register(Grammar {
        language: js.clone(),
        ts: tree_sitter_javascript::LANGUAGE.into(),
        highlights: "(identifier) @variable".into(),
        injections: "".into(),
        folds: "".into(),
        indents: "".into(),
        string_content_nodes: &[],
        embedded_errors: true,
        injected_errors: true,
        own_errors: false,
    });
    let config = SyntaxConfig::new(quiet);
    let text = Document::new("let = ;").buffer().snapshot();
    let parsed = parse_document(&config, &js, &text, None, &[], 0).unwrap();
    assert!(parsed.errors().is_empty(), "not reported");
    assert!(!parsed.quiet_errors().is_empty(), "there when asked for");
    // A trusted grammar's are its errors, none quiet.
    let config = SyntaxConfig::new(grammars());
    let parsed = parse_document(&config, &js, &text, None, &[], 0).unwrap();
    assert!(!parsed.errors().is_empty());
    assert!(parsed.quiet_errors().is_empty());
}
