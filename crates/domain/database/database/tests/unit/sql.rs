use super::*;
use crate::dialect::{Dialect, GenericSql};
use crate::postgres::Postgres;
use crate::sqlite::Sqlite;
use proptest::prelude::*;
use proptest::test_runner::RngSeed;
use std::time::{Duration, Instant};

const PG: Flavor = Flavor::Postgres { version: None };
const FLAVORS: [Flavor; 5] = [
    Flavor::Generic,
    Flavor::Sqlite,
    Flavor::Postgres { version: None },
    Flavor::Postgres { version: Some(130_000) },
    Flavor::MySql,
];

#[test]
fn each_dialect_says_its_flavor() {
    assert_eq!(GenericSql.flavor(), Flavor::Generic);
    assert_eq!(Sqlite.flavor(), Flavor::Sqlite);
    assert_eq!(Postgres::default().flavor(), PG);
    assert_eq!(Postgres::new(170_002).flavor(), Flavor::Postgres { version: Some(170_002) });
    assert!(Flavor::Postgres { version: Some(140_000) }.begin_atomic());
    assert!(!Flavor::Postgres { version: Some(130_011) }.begin_atomic());
    assert!(PG.begin_atomic(), "an unknown version may have them");
    assert!(!Flavor::Sqlite.begin_atomic());
}

#[test]
fn a_text_is_parsed_once_into_tokens_and_statements() {
    let text = "select 1; -- one\nselect 2";
    let parsed = parse(text, Flavor::Sqlite, &SplitOptions::default());
    assert_eq!(parsed.statements.len(), 2);
    let second = &parsed.statements[1];
    let words: Vec<&str> =
        parsed.tokens_of(second).iter().filter(|t| !t.is_trivia()).map(|t| t.text(text)).collect();
    assert_eq!(words, ["select", "2"]);
}

#[test]
fn quoted_names_lose_their_quotes() {
    let name = |text: &str, flavor| {
        let tokens = lex(text, flavor);
        identifier(text, &tokens[0]).into_owned()
    };
    assert_eq!(name("\"My \"\"Table\"\"\"", PG), "My \"Table\"");
    assert_eq!(name("[order details]", Flavor::Sqlite), "order details");
    assert_eq!(name("`a``b`", Flavor::MySql), "a`b");
    assert_eq!(name("U&\"d\\0061t\"", PG), "d\\0061t", "escapes are the engine's to read");
    assert_eq!(name("users", PG), "users");
    assert_eq!(name("\"open", PG), "open", "unterminated: what was typed");
}

#[test]
fn qualified_names_have_their_schema() {
    let read = |text: &str| {
        let tokens = lex(text, PG);
        let sig = significant(&tokens);
        qualified_name(text, &sig, 0)
    };
    assert_eq!(read("users x").unwrap(), (TableRef::new("users"), 1));
    assert_eq!(read("public.users").unwrap(), (TableRef::in_schema("public", "users"), 3));
    assert_eq!(
        read("db.\"My Schema\".t").unwrap().0,
        TableRef::in_schema("My Schema", "t"),
        "a database before the schema is left out"
    );
    assert_eq!(read("u.* ").unwrap(), (TableRef::new("u"), 1), "a dot without a name stops");
    assert!(read("(select 1)").is_none());
    assert!(read("").is_none());
}

#[test]
fn significant_tokens_know_their_depth() {
    let text = "a (b (c)) d";
    let tokens = lex(text, Flavor::Generic);
    let depths: Vec<(&str, usize)> =
        significant(&tokens).iter().map(|(t, d)| (t.text(text), *d)).collect();
    assert_eq!(
        depths,
        [("a", 0), ("(", 0), ("b", 1), ("(", 1), ("c", 2), (")", 1), (")", 0), ("d", 0)]
    );
}

/// Four statements of every kind.
const PIECE: &str = "-- a comment\nSELECT u.id, 'it''s' AS s, \"Name\" FROM users u WHERE u.id = $1;\n\
        INSERT INTO t (a, b) VALUES (1, E'\\\\x'), (2, 'b'); /* block /* nested */ */\n\
        CREATE FUNCTION f() RETURNS int AS $body$ SELECT 1; $body$ LANGUAGE sql;\n\
        UPDATE t SET a = a + 1 WHERE b IN (SELECT b FROM u WHERE c > 0.5e3);\n";

#[test]
#[allow(clippy::disallowed_methods)] // The budget is measured by the test, not the domain.
fn a_megabyte_is_lexed_and_split_within_budget() {
    let copies = 1_000_000 / PIECE.len() + 1;
    let text = PIECE.repeat(copies);
    assert!(text.len() >= 1_000_000);
    // Release builds must stay under 30 ms; debug builds (the tests') get room.
    let budget = if cfg!(debug_assertions) {
        Duration::from_millis(3000)
    } else {
        Duration::from_millis(30)
    };
    // The best of a few runs: the budget is the work's, not a busy machine's.
    let mut best = Duration::MAX;
    let mut statements = 0;
    for _ in 0..3 {
        let started = Instant::now();
        let parsed = parse(&text, PG, &SplitOptions::default());
        best = best.min(started.elapsed());
        statements = parsed.statements.len();
    }
    assert_eq!(statements, copies * 4);
    assert!(best < budget, "lexing and splitting 1 MB took {best:?}");
}

/// SQL-ish text: the pieces where lexers go wrong, in any order.
fn fragment() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        "select", " ", "\n", "\r\n", "\t", ";", "'", "''", "\"", "`", "[", "]", "$$", "$a$", "$1",
        "?", "?2", ":x", "@y", "$z", "::", "--", "#", "/*", "*/", "(", ")", ",", ".", "E'", "X'",
        "U&'", "1.5e3", ".5", "BEGIN", "END", "CASE", "ATOMIC", "CREATE", "TRIGGER", "GO", "/",
        "\\", "ș", "名", "🙂", "\u{a0}", "=", "<>", "a", "b1",
    ])
    .prop_map(str::to_owned)
}

fn text() -> impl Strategy<Value = String> {
    prop::collection::vec(fragment(), 0..60).prop_map(|parts| parts.concat())
}

proptest! {
    // A fixed seed: the same cases on every run and every machine.
    #![proptest_config(ProptestConfig {
        cases: 2000,
        rng_seed: RngSeed::Fixed(39),
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn tokens_cover_the_text_without_overlapping(text in text(), f in 0usize..5) {
        let tokens = lex(&text, FLAVORS[f]);
        let mut at = 0;
        for t in &tokens {
            prop_assert_eq!(t.start, at, "no gap, no overlap");
            prop_assert!(t.end > t.start, "never empty");
            prop_assert!(text.is_char_boundary(t.start) && text.is_char_boundary(t.end));
            at = t.end;
        }
        prop_assert_eq!(at, text.len());
        let joined: String = tokens.iter().map(|t| t.text(&text)).collect();
        prop_assert_eq!(joined, text);
    }

    #[test]
    fn statements_and_what_is_between_them_give_the_text_back(
        text in text(),
        f in 0usize..5,
        separators in any::<bool>(),
    ) {
        let options = SplitOptions { line_separators: separators };
        let flavor = FLAVORS[f];
        let parsed = parse(&text, flavor, &options);
        let mut at = 0;
        let mut rebuilt = String::new();
        for s in &parsed.statements {
            prop_assert!(at <= s.with_comments.start, "in order, apart");
            prop_assert!(s.with_comments.start <= s.range.start);
            prop_assert_eq!(s.with_comments.end, s.range.end);
            prop_assert!(s.range.start < s.range.end, "never empty");
            // Between statements: only whitespace, comments and separators.
            let gap = &text[at..s.with_comments.start];
            prop_assert!(lex(gap, flavor).iter().all(|t| t.is_trivia()
                || t.kind == TokenKind::Semicolon
                || (separators && (t.is_word(gap, "GO") || t.text(gap) == "/"))));
            rebuilt.push_str(&text[at..s.range.start]);
            rebuilt.push_str(s.text(&text));
            at = s.range.end;
            // Its tokens are exactly its range.
            let tokens = parsed.tokens_of(s);
            prop_assert_eq!(tokens.first().map(|t| t.start), Some(s.range.start));
            prop_assert_eq!(tokens.last().map(|t| t.end), Some(s.range.end));
        }
        rebuilt.push_str(&text[at..]);
        prop_assert_eq!(rebuilt, text);
    }

    #[test]
    fn a_statement_read_alone_is_one_statement(text in text(), f in 0usize..5) {
        let flavor = FLAVORS[f];
        let parsed = parse(&text, flavor, &SplitOptions::default());
        for s in parsed.statements.iter().filter(|s| !s.incomplete) {
            let alone = s.text(&text);
            let again = parse(alone, flavor, &SplitOptions::default());
            prop_assert_eq!(again.statements.len(), 1, "{:?}", alone);
            prop_assert_eq!(again.statements[0].range.clone(), 0..alone.len());
        }
    }

    #[test]
    fn nothing_panics_on_any_text(text in "\\PC{0,80}", f in 0usize..5, offset in 0usize..100) {
        let flavor = FLAVORS[f];
        let parsed = parse(&text, flavor, &SplitOptions { line_separators: true });
        for s in &parsed.statements {
            let tokens = parsed.tokens_of(s);
            let _ = classify(&text, tokens, flavor);
            let _ = crate::danger::statement_dangers(&text, tokens, flavor);
            let _ = params(&text, tokens, &ParamStyles::all());
        }
        let _ = statement_at(&text, &parsed.statements, offset.min(text.len()));
        let _ = mask_secrets(&text, flavor);
    }
}
