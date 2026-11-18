use super::*;
use crate::sql::lex;

const PG: Flavor = Flavor::Postgres { version: None };

fn spans(text: &str, flavor: Flavor, options: &SplitOptions) -> Vec<StatementSpan> {
    split(text, &lex(text, flavor), flavor, options)
}

/// The statements' texts.
fn texts(text: &str, flavor: Flavor) -> Vec<&str> {
    spans(text, flavor, &SplitOptions::default()).iter().map(|s| s.text(text)).collect()
}

#[test]
fn statements_end_at_top_level_semicolons() {
    assert_eq!(texts("select 1; select 2;", PG), ["select 1", "select 2"]);
    assert_eq!(texts("  select 1  ;\n\n  select 2  ", PG), ["select 1", "select 2"]);
    assert_eq!(texts("select 1", PG), ["select 1"], "the last needs no `;`");
}

#[test]
fn semicolons_that_dont_end_a_statement() {
    let cases: [(&str, Flavor); 9] = [
        ("select ';'", PG),
        ("select \";\" from t", PG),
        ("select 1 -- ;\n+ 2", PG),
        ("select /* ; */ 1", PG),
        ("select (1; 2)", PG),
        ("select $$ ; $$", PG),
        ("create function f() returns int as $f$ begin; return 1; end $f$ language plpgsql", PG),
        ("select [a;b] from t", Flavor::Sqlite),
        ("select `a;b` from t", Flavor::MySql),
    ];
    for (text, flavor) in cases {
        assert_eq!(texts(text, flavor), [text], "{text:?}");
    }
    assert_eq!(texts("select E'\\';' ; select 2", PG), ["select E'\\';'", "select 2"]);
    assert_eq!(texts("select '\\'; select 2", PG), ["select '\\'", "select 2"]);
    assert_eq!(texts("select '\\'; select 2'", Flavor::MySql).len(), 1, "MySQL escapes the quote");
}

#[test]
fn a_sqlite_trigger_keeps_its_body_whole() {
    let trigger = "CREATE TRIGGER audit AFTER UPDATE ON users\n\
        WHEN new.name <> old.name\n\
        BEGIN\n  INSERT INTO log VALUES (new.id, 'renamed');\n  \
        UPDATE users SET n = CASE WHEN n > 1 THEN n END WHERE id = new.id;\nEND";
    let text = format!("{trigger};\nselect 1;");
    assert_eq!(texts(&text, Flavor::Sqlite), [trigger, "select 1"]);
    let temp = "create temp trigger if not exists t before delete on x begin select raise(abort, 'no'); end";
    assert_eq!(texts(&format!("{temp}; select 2"), Flavor::Sqlite), [temp, "select 2"]);
    // A transaction's `BEGIN` is a statement of its own.
    assert_eq!(
        texts("BEGIN; update t set a = 1; COMMIT;", Flavor::Sqlite),
        ["BEGIN", "update t set a = 1", "COMMIT"]
    );
    // A trigger being typed: to the end.
    let open = spans(
        "create trigger t after insert on x begin select 1;",
        Flavor::Sqlite,
        &SplitOptions::default(),
    );
    assert_eq!(open.len(), 1);
    assert!(open[0].incomplete);
}

#[test]
fn begin_atomic_bodies_on_postgres_14_and_later() {
    let function = "CREATE FUNCTION add(a int, b int) RETURNS int LANGUAGE sql\n\
        BEGIN ATOMIC\n  SELECT CASE WHEN a > 0 THEN a + b ELSE b END;\nEND";
    let text = format!("{function};\nSELECT add(1, 2);");
    for version in [None, Some(140_000), Some(170_002)] {
        assert_eq!(texts(&text, Flavor::Postgres { version }), [function, "SELECT add(1, 2)"]);
    }
    // An older server has no such body: it splits as the server reads it.
    assert_eq!(texts(&text, Flavor::Postgres { version: Some(130_000) }).len(), 3);
    let procedure =
        "create procedure p() begin atomic insert into t values (1); insert into t values (2); end";
    assert_eq!(texts(procedure, PG), [procedure]);
}

#[test]
fn mysql_compound_statements_close_their_own_blocks() {
    let text = "CREATE TRIGGER t BEFORE INSERT ON x FOR EACH ROW BEGIN\n\
        IF NEW.a < 0 THEN SET NEW.a = 0; END IF;\n\
        CASE NEW.b WHEN 1 THEN SET NEW.c = 2; ELSE BEGIN END; END CASE;\nEND";
    assert_eq!(texts(&format!("{text}; select 1"), Flavor::MySql), [text, "select 1"]);
}

#[test]
fn empty_statements_and_comments_alone_are_skipped() {
    assert!(texts(";;  ;\n", PG).is_empty());
    assert!(texts("-- nothing\n/* at all */", PG).is_empty());
    assert_eq!(texts("; -- x\n; select 1;;", PG), ["select 1"]);
    assert!(texts("", PG).is_empty());
}

#[test]
fn comments_right_before_a_statement_go_with_it() {
    let text = "select 1; -- about one\n-- about two\n/* more */\nselect 2;";
    let s = spans(text, PG, &SplitOptions::default());
    assert_eq!(s[0].text(text), "select 1");
    assert_eq!(&text[s[0].with_comments.clone()], "select 1", "its line's comment stays out");
    assert_eq!(&text[s[1].with_comments.clone()], "-- about two\n/* more */\nselect 2");
    assert_eq!(s[1].first_word(text), Some("select"));
    // The first statement of a text takes its leading comments.
    let s = spans("-- first\nselect 1", PG, &SplitOptions::default());
    assert_eq!(s[0].with_comments, 0..17);
}

#[test]
fn spans_know_their_separator_tokens_and_first_word() {
    let text = "(select 1) union select 2;\nvalues (1)";
    let tokens = lex(text, PG);
    let s = split(text, &tokens, PG, &SplitOptions::default());
    assert_eq!(s[0].terminator, Some(25..26));
    assert_eq!(s[0].end(), 26);
    assert_eq!(s[0].first_word(text), Some("select"), "after a parenthesis");
    assert_eq!(tokens[s[0].tokens.start].text(text), "(");
    assert_eq!(tokens[s[0].tokens.end - 1].text(text), "2");
    assert_eq!(s[1].terminator, None);
    assert_eq!(s[1].end(), text.len());
    assert_eq!(s[1].first_word(text), Some("values"));
    let s = spans("'x' || 'y'", PG, &SplitOptions::default());
    assert_eq!(s[0].first_word, None);
}

#[test]
fn unfinished_text_is_one_incomplete_statement() {
    for text in ["select 'open; select 2", "select (1; select 2", "select $$ ; "] {
        let s = spans(text, PG, &SplitOptions::default());
        assert_eq!(s.len(), 1, "{text:?}");
        assert!(s[0].incomplete);
        assert_eq!(s[0].range.end, text.len());
    }
    assert!(!spans("select 1", PG, &SplitOptions::default())[0].incomplete);
    // A comment left open after it isn't part of it.
    let text = "select /* ; select 2";
    let s = spans(text, PG, &SplitOptions::default());
    assert_eq!(s.iter().map(|s| s.text(text)).collect::<Vec<_>>(), ["select"]);
}

#[test]
fn go_and_slash_lines_split_only_when_asked() {
    let text = "select 1\nGO\nselect 2\n  /  \nselect 3 / 2\ngo";
    let on = SplitOptions { line_separators: true };
    let split_on: Vec<&str> =
        spans(text, Flavor::Generic, &on).iter().map(|s| s.text(text)).collect();
    assert_eq!(split_on, ["select 1", "select 2", "select 3 / 2"]);
    assert_eq!(texts(text, Flavor::Generic).len(), 1, "off: one statement");
    // `GO` in a statement's line is a name.
    let s = spans("select go from t", Flavor::Generic, &on);
    assert_eq!(s.len(), 1);
    let crlf = "select 1\r\nGO\r\nselect 2";
    assert_eq!(spans(crlf, Flavor::Generic, &on).len(), 2);
}

#[test]
fn windows_line_ends_change_nothing() {
    let unix = "-- c\nselect 1;\n\nselect\n2;";
    let windows = unix.replace('\n', "\r\n");
    let a: Vec<String> = texts(unix, PG).iter().map(|s| s.replace('\n', " ")).collect();
    let b: Vec<String> = texts(&windows, PG).iter().map(|s| s.replace("\r\n", " ")).collect();
    assert_eq!(a, b);
}

#[test]
fn the_statement_at_the_cursor() {
    let text = "select 1;\n\nselect 2; select 3;\n-- c\nselect\n  4;\n\n\nselect 5";
    let s = spans(text, PG, &SplitOptions::default());
    let at = |offset: usize| statement_at(text, &s, offset).map(|s| s.text(text));
    let pos = |needle: &str| text.find(needle).unwrap();
    assert_eq!(at(0), Some("select 1"));
    assert_eq!(at(3), Some("select 1"));
    assert_eq!(at(9), Some("select 1"), "right after its `;`");
    assert_eq!(at(10), Some("select 1"), "on the empty line right after it");
    assert_eq!(at(pos("select 2")), Some("select 2"));
    assert_eq!(at(pos("select 2") + 9), Some("select 2"), "after `;`, before the next on the line");
    assert_eq!(at(pos("select 3")), Some("select 3"));
    assert_eq!(at(pos("-- c") + 2), Some("select\n  4"), "in its comment");
    assert_eq!(at(pos("4;") + 3), Some("select\n  4"), "the line right after");
    assert_eq!(at(pos("4;") + 4), None, "two lines away: none");
    assert_eq!(at(text.len()), Some("select 5"));
    // Touching: the next one.
    let text = "select 1;select 2";
    let s = spans(text, PG, &SplitOptions::default());
    assert_eq!(statement_at(text, &s, 9).map(|s| s.text(text)), Some("select 2"));
    assert_eq!(statement_at(text, &s, 8).map(|s| s.text(text)), Some("select 1"));
    // Before every statement, or none at all.
    let text = "\n\n\nselect 1";
    let s = spans(text, PG, &SplitOptions::default());
    assert_eq!(statement_at(text, &s, 0), None);
    assert_eq!(statement_at("", &[], 0), None);
    // `\r\n` is one line break.
    let text = "select 1;\r\n";
    let s = spans(text, PG, &SplitOptions::default());
    assert_eq!(statement_at(text, &s, text.len()).map(|s| s.text(text)), Some("select 1"));
}
