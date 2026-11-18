use super::*;
use code_intelligence::{CompletionKind, Project, SymbolIndex};

fn kind_at(text: &str) -> ContextKind {
    let offset = text.find('|').unwrap();
    let text = text.replace('|', "");
    context(&text, offset).kind
}

#[test]
fn keywords_fit_where_a_word_is_written() {
    assert_eq!(kind_at("SEL|"), ContextKind::Name);
    assert_eq!(kind_at("SELECT * FR|"), ContextKind::Name);
    assert_eq!(kind_at("SELECT *\nFROM t\nWH|"), ContextKind::Name);
    let c = context("SELECT co", 9);
    assert_eq!((c.prefix.as_str(), c.replace), ("co", 7..9));
}

#[test]
fn nothing_is_offered_in_comments_strings_and_quoted_names() {
    for text in [
        "-- sel|",
        "SELECT 1 -- a com|",
        "SELECT 'na|",
        "SELECT \"Na|",
        "SELECT `na|",
        "/* a com|",
        "/* a\n long com|",
        "SELECT /* x */ 'it''s a|",
        "SELECT |",
        "SELECT 12|",
        "WHERE id = $1|",
        "WHERE id = :na|",
        "WHERE id = @na|",
        "SELECT u.na|",
    ] {
        assert_eq!(kind_at(text), ContextKind::None, "{text}");
    }
    // Closed before the cursor: words again.
    for text in ["SELECT 'a' FR|", "/* x */ SEL|", "/* a\n b */\nSEL|", "SELECT \"A\" FR|"] {
        assert_eq!(kind_at(text), ContextKind::Name, "{text}");
    }
}

#[test]
fn a_heredoc_in_php_is_read_from_its_own_lines() {
    // The PHP around it (its strings, its `//`) doesn't count.
    let php = "<?php\n$a = 'x'; // note\n$sql = <<<SQL\n    SELECT * FR|\n    SQL;\n";
    assert_eq!(kind_at(php), ContextKind::Name);
}

#[test]
fn functions_are_inserted_as_calls_values_as_written() {
    let i = SqlIntelligence;
    assert_eq!(i.keyword_insert("COUNT"), KeywordInsert::Call);
    assert_eq!(i.keyword_insert("NULL"), KeywordInsert::Exact);
    assert_eq!(i.keyword_insert("CURRENT_TIMESTAMP"), KeywordInsert::Exact);
    assert_eq!(i.keyword_insert("SELECT"), KeywordInsert::Spaced);
    assert_eq!(i.keyword_insert("REPLACE"), KeywordInsert::Spaced);
}

#[test]
fn the_words_follow_the_files_dialect() {
    let i = SqlIntelligence;
    let words = |name: &str| i.keywords_for(Path::new(name)).into_owned();
    assert!(words("app.sqlite.sql").contains(&"PRAGMA"));
    assert!(!words("app.pgsql").contains(&"PRAGMA"));
    assert!(words("app.pgsql").contains(&"ILIKE"));
    assert!(!words("app.sql").contains(&"ILIKE"));
    assert!(words("app.sql").contains(&"SELECT"));
    assert_eq!(i.keywords(), STANDARD);
    assert!(i.extensions().is_empty());
    assert_eq!(i.completion_triggers(), ".\"`'");
    assert_eq!(i.word_chars(), "$");
    assert!(i.naming().names_ignore_case);
}

#[test]
fn completion_offers_the_dialects_words_and_calls() {
    let i = SqlIntelligence;
    let index = SymbolIndex::default();
    let project = Project::none();
    let complete = |name: &str, text: &str| {
        code_intelligence::complete(&i, &index, Path::new(name), text, text.len(), &project).1
    };
    let items = complete("a.sqlite.sql", "SELECT cou");
    let count = items.iter().find(|c| c.label == "COUNT").expect("COUNT");
    assert_eq!((count.insert.as_str(), count.cursor_back), ("COUNT()", 1));
    assert_eq!(count.kind, CompletionKind::Keyword);
    let items = complete("a.sqlite.sql", "PRAG");
    assert_eq!(items.first().map(|c| c.insert.as_str()), Some("PRAGMA "));
    assert!(complete("a.pgsql", "PRAG").iter().all(|c| c.label != "PRAGMA"));
    assert!(complete("a.sql", "SELECT 'cou").is_empty());
}

/// A known function's signature by the file's dialect (the parameter help's card).
#[test]
fn a_known_function_s_signature_follows_the_file_s_dialect() {
    let i = SqlIntelligence;
    let code = i.call_signature(Path::new("q.sql"), "UPPER");
    assert_eq!(code.as_deref(), Some("upper(text) → text"));
    assert!(i.call_signature(Path::new("q.pgsql"), "date_format").is_none(), "MySQL's");
    assert!(i.call_signature(Path::new("q.mysql.sql"), "date_format").is_some());
    assert!(i.call_signature(Path::new("q.sql"), "nothing_like_it").is_none());
    assert!(i.hover("SELECT upper(x)", 8).is_none(), "no hover card (nor inlay hints)");
}
