use super::*;

const PG: Flavor = Flavor::Postgres { version: None };

/// The tokens of `text` other than whitespace, as (kind, text, unterminated).
fn toks(text: &str, flavor: Flavor) -> Vec<(TokenKind, &str, bool)> {
    lex(text, flavor)
        .into_iter()
        .filter(|t| t.kind != TokenKind::Whitespace)
        .map(|t| (t.kind, t.text(text), t.unterminated))
        .collect()
}

fn kinds(text: &str, flavor: Flavor) -> Vec<TokenKind> {
    toks(text, flavor).into_iter().map(|(k, _, _)| k).collect()
}

/// The one token `text` is.
fn one(text: &str, flavor: Flavor) -> (TokenKind, bool) {
    let all = toks(text, flavor);
    assert_eq!(all.len(), 1, "{text:?} in {flavor:?}: {all:?}");
    assert_eq!(all[0].1, text);
    (all[0].0, all[0].2)
}

const PLAIN: TokenKind = TokenKind::String(StringKind::Plain);

#[test]
fn words_numbers_and_punctuation() {
    use TokenKind::*;
    assert_eq!(
        toks("SELECT a_1, t.$x FROM t;", PG),
        [
            (Word, "SELECT", false),
            (Word, "a_1", false),
            (Comma, ",", false),
            (Word, "t", false),
            (Dot, ".", false),
            (Punct, "$", false),
            (Word, "x", false),
            (Word, "FROM", false),
            (Word, "t", false),
            (Semicolon, ";", false),
        ]
    );
    assert_eq!(one("a$b", PG).0, Word, "PostgreSQL's names may hold `$`");
    assert_eq!(one("ăîșț_名前", Flavor::Generic).0, Word);
    for n in ["42", "1.5", ".5", "1e10", "1.5E-3", "0x1F", "1_000_000", "12abc"] {
        assert_eq!(one(n, PG).0, Number, "{n}");
    }
    assert_eq!(kinds("t.5", Flavor::Generic), [Word, Dot, Number], "a dot after a name is a dot");
    assert_eq!(toks("1..2", PG).iter().map(|t| t.1).collect::<Vec<_>>(), ["1", ".", ".2"]);
    assert_eq!(kinds("(a)", PG), [OpenParen, Word, CloseParen]);
    assert_eq!(kinds("a[1]", PG), [Word, Punct, Number, Punct]);
}

#[test]
fn strings_in_every_form() {
    for flavor in [Flavor::Generic, Flavor::Sqlite, PG, Flavor::MySql] {
        assert_eq!(one("'it''s'", flavor), (PLAIN, false), "{flavor:?}");
        assert_eq!(one("''", flavor), (PLAIN, false));
        assert_eq!(one("'a;b -- c /* d'", flavor), (PLAIN, false));
        assert_eq!(one("X'00ff'", flavor), (TokenKind::String(StringKind::Binary), false));
        assert_eq!(one("x'00'", flavor), (TokenKind::String(StringKind::Binary), false));
        assert_eq!(one("N'ș'", flavor), (TokenKind::String(StringKind::National), false));
        assert_eq!(one("'multi\r\nline'", flavor), (PLAIN, false));
    }
    // A backslash is an ordinary character in standard strings...
    assert_eq!(toks("'a\\' c'", PG).len(), 3, "the string ends at the second quote");
    assert_eq!(toks("'a\\' c'", Flavor::Sqlite).len(), 3);
    // ...an escape in PostgreSQL's E'' and in MySQL.
    assert_eq!(one("E'it\\'s'", PG), (TokenKind::String(StringKind::Escape), false));
    assert_eq!(one("e'\\\\'", PG), (TokenKind::String(StringKind::Escape), false));
    assert_eq!(one("E'a''b'", PG), (TokenKind::String(StringKind::Escape), false));
    assert_eq!(one("'it\\'s'", Flavor::MySql), (PLAIN, false));
    assert_eq!(
        one("\"say \\\"hi\\\"\"", Flavor::MySql),
        (PLAIN, false),
        "MySQL's \"…\" is a string"
    );
    // Elsewhere `E` is a name before a string.
    assert_eq!(kinds("E'x'", Flavor::Sqlite), [TokenKind::Word, PLAIN]);
    assert_eq!(one("B'0101'", PG), (TokenKind::String(StringKind::Bit), false));
    assert_eq!(one("U&'d\\0061t'", PG), (TokenKind::String(StringKind::Unicode), false));
    assert_eq!(one("'ș 名 🙂'", PG), (PLAIN, false));
}

#[test]
fn quoted_names_by_dialect() {
    let quoted = TokenKind::QuotedIdent;
    for flavor in [Flavor::Generic, Flavor::Sqlite, PG] {
        assert_eq!(one("\"a \"\"b\"\" ;c\"", flavor), (quoted, false), "{flavor:?}");
    }
    assert_eq!(one("`a``b`", Flavor::MySql), (quoted, false));
    assert_eq!(one("`a`", Flavor::Sqlite), (quoted, false), "SQLite takes MySQL's too");
    assert_eq!(one("[order details]", Flavor::Sqlite), (quoted, false));
    assert_eq!(one("[a;b]", Flavor::Sqlite), (quoted, false));
    assert_eq!(one("U&\"d\\0061t\"", PG), (quoted, false));
    assert_ne!(kinds("[x]", PG)[0], quoted, "PostgreSQL's brackets are subscripts");
    assert_ne!(kinds("`x`", PG)[0], quoted);
}

#[test]
fn dollar_quoted_bodies() {
    let body = TokenKind::DollarBody;
    assert_eq!(one("$$ SELECT 'x'; -- $$", PG), (body, false));
    assert_eq!(one("$fn$ a $$ b $ c $fn$", PG), (body, false), "another tag inside is text");
    assert_eq!(one("$_1$x$_1$", PG), (body, false));
    assert_eq!(one("$ș$ body $ș$", PG), (body, false));
    assert_eq!(
        toks("$a$ x $b$ y $a$z", PG).iter().map(|t| t.1).collect::<Vec<_>>(),
        ["$a$ x $b$ y $a$", "z"]
    );
    // `$1` is a parameter, never a tag.
    assert_eq!(kinds("$1$", PG)[0], TokenKind::Parameter(ParamKind::Numbered));
    assert_eq!(one("$$ open", PG), (body, true), "unterminated: to the end");
    assert_eq!(one("$tag$ open $tag", PG), (body, true));
    assert_ne!(kinds("$$x$$", Flavor::Sqlite)[0], body, "only PostgreSQL has them");
}

#[test]
fn comments_line_block_and_nested() {
    use TokenKind::{BlockComment, LineComment};
    for flavor in [Flavor::Generic, Flavor::Sqlite, PG, Flavor::MySql] {
        assert_eq!(toks("-- a ; 'b\nc", flavor)[0], (LineComment, "-- a ; 'b", false));
        assert_eq!(toks("-- a\r\nc", flavor)[0], (LineComment, "-- a", false), "\\r\\n ends it");
        assert_eq!(one("/* a ; 'b */", flavor), (BlockComment, false));
        assert_eq!(one("/* open", flavor), (BlockComment, true));
    }
    // Nested in PostgreSQL only.
    assert_eq!(one("/* a /* b */ c */", PG), (BlockComment, false));
    assert_eq!(one("/* a /* b */", PG), (BlockComment, true));
    let sqlite = toks("/* a /* b */ c */", Flavor::Sqlite);
    assert_eq!(sqlite[0].1, "/* a /* b */");
    // `#` is MySQL's.
    assert_eq!(toks("# x\ny", Flavor::MySql)[0].0, LineComment);
    assert_ne!(toks("# x", PG)[0].0, LineComment);
    // MySQL needs a space after `--`.
    assert_ne!(toks("--x", Flavor::MySql)[0].0, LineComment);
    assert_eq!(toks("-- x", Flavor::MySql)[0].0, LineComment);
    assert_eq!(toks("--\n", Flavor::MySql)[0].0, LineComment);
    // A comment ends an operator.
    assert_eq!(kinds("a=--x", PG), [TokenKind::Word, TokenKind::Operator, LineComment]);
    assert_eq!(
        kinds("a+/*x*/b", PG),
        [TokenKind::Word, TokenKind::Operator, BlockComment, TokenKind::Word]
    );
}

#[test]
fn parameters_in_every_form() {
    use ParamKind::*;
    let param = |text: &str, flavor| match one(text, flavor).0 {
        TokenKind::Parameter(kind) => Some(kind),
        _ => None,
    };
    assert_eq!(param("?", Flavor::Sqlite), Some(Positional));
    assert_eq!(param("?12", Flavor::Sqlite), Some(Numbered));
    assert_eq!(param("$3", PG), Some(Numbered));
    assert_eq!(param(":name", PG), Some(Named));
    assert_eq!(param(":ș", Flavor::Sqlite), Some(Named));
    assert_eq!(param("@name", Flavor::Sqlite), Some(Named));
    assert_eq!(param("$name", Flavor::Sqlite), Some(Named));
    assert_eq!(param("?", PG), Some(Positional), "lexed as one; the styles decide");
    // What looks like one and isn't.
    assert_eq!(kinds("a::int", PG), [TokenKind::Word, TokenKind::Cast, TokenKind::Word]);
    assert_eq!(kinds("x := 1", PG)[1], TokenKind::Operator);
    assert_eq!(kinds("a[1:2]", PG)[3], TokenKind::Operator, "a slice's colon");
    assert_eq!(kinds("j ?| array['a']", PG)[1], TokenKind::Operator);
    assert_eq!(kinds("j ?& b", PG)[1], TokenKind::Operator);
    assert_eq!(kinds("@ -5", PG)[0], TokenKind::Operator, "PostgreSQL's absolute value");
    assert_eq!(kinds("a @> b", PG)[1], TokenKind::Operator);
    assert_eq!(kinds("@x", PG)[0], TokenKind::Operator);
    // In a string, a comment, a name or a body: text.
    for text in ["':x ? $1'", "\":x\"", "/* ? */", "-- :x"] {
        assert!(kinds(text, Flavor::Sqlite).iter().all(|k| !matches!(k, TokenKind::Parameter(_))));
    }
    assert_eq!(one("$$ select :x, $1 $$", PG).0, TokenKind::DollarBody);
}

#[test]
fn operators_and_casts() {
    use TokenKind::*;
    assert_eq!(
        toks("a<>b||c->>'k'", PG).iter().map(|t| (t.0, t.1)).collect::<Vec<_>>(),
        [
            (Word, "a"),
            (Operator, "<>"),
            (Word, "b"),
            (Operator, "||"),
            (Word, "c"),
            (Operator, "->>"),
            (PLAIN, "'k'")
        ]
    );
    assert_eq!(kinds("'1'::int::text", PG), [PLAIN, Cast, Word, Cast, Word]);
    assert_eq!(kinds("a ~* b", PG)[1], Operator);
}

#[test]
fn unterminated_text_runs_to_the_end_without_panicking() {
    assert_eq!(one("'open", PG), (PLAIN, true));
    assert_eq!(one("'open\nmore; lines", Flavor::Sqlite), (PLAIN, true));
    assert_eq!(one("E'open\\'", PG), (TokenKind::String(StringKind::Escape), true));
    assert_eq!(one("\"open", PG), (TokenKind::QuotedIdent, true));
    assert_eq!(one("[open", Flavor::Sqlite), (TokenKind::QuotedIdent, true));
    assert_eq!(one("`open", Flavor::MySql), (TokenKind::QuotedIdent, true));
    assert_eq!(one("'a\\", Flavor::MySql), (PLAIN, true), "a backslash at the very end");
    assert_eq!(one("'a\\ș", Flavor::MySql), (PLAIN, true), "a backslash before a wide char");
    assert!(lex("", PG).is_empty());
}

#[test]
fn whitespace_of_every_kind_and_windows_line_ends() {
    let text = "select\r\n\t1\u{a0}\u{2003};\r\n";
    let tokens = lex(text, Flavor::Sqlite);
    let shapes: Vec<(TokenKind, &str)> = tokens.iter().map(|t| (t.kind, t.text(text))).collect();
    assert_eq!(
        shapes,
        [
            (TokenKind::Word, "select"),
            (TokenKind::Whitespace, "\r\n\t"),
            (TokenKind::Number, "1"),
            (TokenKind::Whitespace, "\u{a0}\u{2003}"),
            (TokenKind::Semicolon, ";"),
            (TokenKind::Whitespace, "\r\n"),
        ]
    );
    // An unknown character is one token of its own.
    assert_eq!(kinds("a • b", PG), [TokenKind::Word, TokenKind::Punct, TokenKind::Word]);
}

#[test]
fn token_helpers() {
    let text = "Select /* c */ x";
    let tokens = lex(text, PG);
    assert!(tokens[0].is_word(text, "SELECT"));
    assert!(!tokens[0].is_word(text, "SELECTED"));
    assert!(tokens[1].is_trivia() && !tokens[1].is_comment());
    assert!(tokens[2].is_comment() && tokens[2].is_trivia());
    assert!(tokens[4].is_name());
    assert_eq!(tokens[4].range(), 15..16);
}
