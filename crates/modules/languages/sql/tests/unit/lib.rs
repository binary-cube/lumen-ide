use super::*;
use document::Document;
use languages::LanguageRegistry;
use std::path::Path;
use syntax_ts::{Grammars, Highlighter, SyntaxConfig, SyntaxSnapshot, parse_document};

const SQLITE: &str = include_str!("../data/sqlite.sql");
const POSTGRES: &str = include_str!("../data/postgres.sql");

fn config() -> SyntaxConfig {
    let mut grammars = Grammars::default();
    grammars.register(grammar());
    SyntaxConfig::new(grammars)
}

fn parse(config: &SyntaxConfig, text: &str) -> SyntaxSnapshot {
    let doc = Document::new(text);
    parse_document(config, &LANGUAGE, &doc.buffer().snapshot(), None, &[], 0).unwrap()
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
fn files_are_sql_by_their_extension() {
    let mut registry = LanguageRegistry::default();
    registry.register(descriptor());
    for name in ["a.sql", "B.SQL", "schema.ddl", "f.pgsql", "f.psql", "dump.sqlite.sql"] {
        assert_eq!(registry.detect(Path::new(name), "").as_str(), "sql", "{name}");
    }
    assert_ne!(registry.detect(Path::new("a.sqlx"), "").as_str(), "sql");
    let d = descriptor();
    assert_eq!(
        (d.line_comment, d.block_comment, d.icon),
        (Some("--"), Some(("/*", "*/")), "database")
    );
}

#[test]
fn sqlite_sample_is_highlighted() {
    let c = config();
    let s = SQLITE;
    assert_eq!(scope_at(&c, s, "-- A shop", 0).as_deref(), Some("comment"));
    assert_eq!(scope_at(&c, s, "/* The orders", 0).as_deref(), Some("comment"));
    assert_eq!(scope_at(&c, s, "CREATE", 0).as_deref(), Some("keyword"));
    assert_eq!(scope_at(&c, s, "SELECT", 0).as_deref(), Some("keyword"));
    assert_eq!(scope_at(&c, s, "INTEGER", 0).as_deref(), Some("keyword.type"));
    assert_eq!(scope_at(&c, s, "AND", 0).as_deref(), Some("keyword.operator"));
    assert_eq!(scope_at(&c, s, "users", 0).as_deref(), Some("type"));
    assert_eq!(scope_at(&c, s, "email", 0).as_deref(), Some("property"));
    assert_eq!(scope_at(&c, s, "count", 0).as_deref(), Some("function.call"));
    assert_eq!(scope_at(&c, s, "'%@example.com'", 0).as_deref(), Some("string"));
    assert_eq!(scope_at(&c, s, "0.5", 0).as_deref(), Some("number"));
    assert_eq!(scope_at(&c, s, "10;", 0).as_deref(), Some("number"));
    assert_eq!(scope_at(&c, s, "TRUE", 0).as_deref(), Some("boolean"));
    assert_eq!(scope_at(&c, s, "NULL)", 0).as_deref(), Some("null"));
    assert_eq!(scope_at(&c, s, "?", 0).as_deref(), Some("constant.parameter"));
    assert_eq!(scope_at(&c, s, "?1", 0).as_deref(), Some("constant.parameter"));
    assert_eq!(scope_at(&c, s, "n DESC", 0).as_deref(), Some("property"));
    assert_eq!(scope_at(&c, s, "u\nJOIN", 0).as_deref(), Some("variable"));
    assert_eq!(scope_at(&c, s, "=", 0).as_deref(), Some("operator"));
}

#[test]
fn postgres_sample_is_highlighted() {
    let c = config();
    let s = POSTGRES;
    assert_eq!(scope_at(&c, s, "recent", 0).as_deref(), Some("variable"));
    assert_eq!(scope_at(&c, s, "now()", 0).as_deref(), Some("function.call"));
    assert_eq!(scope_at(&c, s, "'30 days'", 0).as_deref(), Some("string"));
    assert_eq!(scope_at(&c, s, "::", 0).as_deref(), Some("operator"));
    assert_eq!(scope_at(&c, s, "CASE", 0).as_deref(), Some("keyword"));
    assert_eq!(scope_at(&c, s, "$1", 0).as_deref(), Some("constant.parameter"));
    assert_eq!(scope_at(&c, s, "CONFLICT", 0).as_deref(), Some("keyword"));
    assert_eq!(scope_at(&c, s, "RETURNING", 0).as_deref(), Some("keyword"));
    assert_eq!(scope_at(&c, s, "$$", 0).as_deref(), Some("punctuation.special"));
    // The function's `$$` body is read as SQL: its statements are colored.
    assert_eq!(scope_at(&c, s, "UPDATE users SET score", 0).as_deref(), Some("keyword"));
    assert_eq!(scope_at(&c, s, "score + 1", 0).as_deref(), Some("property"));
}

#[test]
fn a_double_quoted_name_is_not_a_string() {
    let c = config();
    let s = "SELECT \"Order\" FROM t WHERE a = 'x';\n";
    assert_eq!(scope_at(&c, s, "\"Order\"", 0).as_deref(), Some("variable.quoted"));
    assert_eq!(scope_at(&c, s, "'x'", 0).as_deref(), Some("string"));
}

#[test]
fn grammar_errors_are_never_reported() {
    let c = config();
    // SQL the grammar doesn't know (PostgreSQL's named parameter, SQLite's `[x]`): valid.
    let snapshot = parse(&c, "SELECT * FROM [order] WHERE a = :name;\nDO $$ BEGIN END $$;\n");
    assert!(snapshot.errors().is_empty());
}

#[test]
fn statements_subqueries_and_bodies_fold() {
    let c = config();
    let line =
        |text: &str, needle: &str| text[..text.find(needle).unwrap()].matches('\n').count() as u32;
    let starts =
        |text: &str| -> Vec<u32> { parse(&c, text).folds().iter().map(|f| f.start_line).collect() };
    let sqlite = starts(SQLITE);
    for needle in ["CREATE TABLE IF", "CREATE TABLE orders", "SELECT u.email"] {
        assert!(sqlite.contains(&line(SQLITE, needle)), "{needle}: {sqlite:?}");
    }
    // A statement or a block comment on one line doesn't fold.
    assert!(!sqlite.contains(&line(SQLITE, "/* The orders")));
    assert!(!sqlite.contains(&line(SQLITE, "UPDATE users")));
    let postgres = starts(POSTGRES);
    // The statement, its CTE, the function and its `$$` body (from `AS $$`).
    for needle in ["WITH recent", "SELECT user_id", "CREATE OR REPLACE", "LANGUAGE plpgsql AS"] {
        assert!(postgres.contains(&line(POSTGRES, needle)), "{needle}: {postgres:?}");
    }
    let case = parse(&c, "SELECT\n  CASE\n    WHEN a THEN 1\n    ELSE 2\n  END\nFROM t;\n");
    assert!(
        case.folds().iter().any(|f| f.start_line == 1 && f.end_line == 4),
        "{:?}",
        case.folds()
    );
}

#[test]
fn lines_are_commented_with_dashes() {
    let token = descriptor().line_comment.unwrap();
    let text = "SELECT 1;\n  SELECT 2;\n";
    let mut doc = Document::new(text);
    let all = document::Selection::new(0, text.len() - 1);
    doc.set_selections(document::Selections::new(vec![all], 0));
    document::commands::toggle_line_comment(&mut doc, token);
    assert_eq!(doc.buffer().to_string(), "-- SELECT 1;\n--   SELECT 2;\n");
    document::commands::toggle_line_comment(&mut doc, token);
    assert_eq!(doc.buffer().to_string(), text);
}

#[test]
fn new_lines_indent_inside_parentheses() {
    let c = config();
    let text = "CREATE TABLE t (\n    id INT,\n    name TEXT\n);\n";
    let doc = Document::new(text);
    let snap = doc.buffer().snapshot();
    let snapshot = parse_document(&c, &LANGUAGE, &snap, None, &[], 0).unwrap();
    let inside = text.find("    name").unwrap();
    assert_eq!(snapshot.indent_anchor(&snap, inside).map(|a| a.line), Some(0));
}

/// Comment is in a SQL file's editor menu only (its own language), in the code's section.
#[test]
fn comment_is_offered_in_sql_files_only() {
    let item = comment_item();
    assert_eq!(item.command, workbench_model::COMMENT_LINE);
    assert_eq!(item.places, &[workbench_model::MenuPlace::Editor]);
    assert_eq!(item.section, workbench_model::MenuSection::Code);
    assert_eq!(item.when, workbench_model::MenuWhen::Language("sql"));
    assert_eq!(LANGUAGE.as_str(), "sql", "the language the item names is the module's");
}

/// SQL offers none of the editor's name features (no Rename, no Find Usages).
#[test]
fn sql_offers_no_name_features() {
    use code_intelligence::LanguageIntelligence;
    assert!(SqlIntelligence.features().is_empty());
}

/// "Reformat Code" (Code quality's command) and "Reformat Statement" in a SQL file's menu
/// only, after its line comment.
#[test]
fn the_format_rows_are_in_a_sql_file_s_menu() {
    let items = format_items();
    let commands: Vec<&str> = items.iter().map(|i| i.command.0).collect();
    assert_eq!(commands, ["quality.format_file", "sql.format_statement"]);
    assert!(items.iter().all(|i| i.when == workbench_model::MenuWhen::Language("sql")));
    assert!(
        items.iter().all(|i| i.section == comment_item().section && i.order > comment_item().order)
    );
}
