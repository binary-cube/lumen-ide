use super::*;

fn parse(text: &str) -> EnvFile {
    EnvFile::parse(text)
}

/// The entries as `(key, value content)`.
fn pairs(text: &str) -> Vec<(&str, &str)> {
    let file = parse(text);
    file.entries()
        .map(|(_, e)| (&text[e.key.clone()], &text[e.value.content.clone()]))
        .collect::<Vec<_>>()
        .into_iter()
        .collect()
}

#[test]
fn reads_entries_comments_and_blank_lines() {
    let text = "# Database\nDB_HOST=localhost\n\n  # indented\nexport DB_PORT = 5432\nwords only\n";
    let file = parse(text);
    let kinds: Vec<&str> = file
        .lines
        .iter()
        .map(|l| match &l.kind {
            LineKind::Blank => "blank",
            LineKind::Comment { .. } => "comment",
            LineKind::Entry(_) => "entry",
            LineKind::Invalid => "invalid",
        })
        .collect();
    assert_eq!(kinds, ["comment", "entry", "blank", "comment", "entry", "invalid"]);
    let LineKind::Comment { text: comment } = &file.lines[0].kind else { panic!() };
    assert_eq!(&text[comment.clone()], "Database");
    assert_eq!(pairs(text), [("DB_HOST", "localhost"), ("DB_PORT", "5432")]);
    let (line, entry) = file.entries().nth(1).unwrap();
    assert_eq!(entry.export.clone().map(|r| &text[r]), Some("export"));
    assert_eq!((line.first, line.last), (4, 4));
}

#[test]
fn unquoted_values_end_at_a_comment_after_whitespace() {
    let text = "A=value # note\nB=a#b\nC=\nD=#comment\nE= spaced out  \n";
    assert_eq!(
        pairs(text),
        [("A", "value"), ("B", "a#b"), ("C", ""), ("D", ""), ("E", "spaced out")]
    );
    let file = parse(text);
    let comments: Vec<Option<&str>> =
        file.entries().map(|(_, e)| e.comment.clone().map(|c| &text[c])).collect();
    assert_eq!(comments, [Some("# note"), None, None, Some("#comment"), None]);
}

#[test]
fn quoted_values_keep_their_text_and_may_span_lines() {
    let text =
        "S='a # b ${X}'\nD=\"say \\\"hi\\\" # not a comment\" # a comment\nM=\"one\ntwo\"\nN=1\n";
    assert_eq!(
        pairs(text),
        [
            ("S", "a # b ${X}"),
            ("D", "say \\\"hi\\\" # not a comment"),
            ("M", "one\ntwo"),
            ("N", "1")
        ]
    );
    let file = parse(text);
    let lines: Vec<(u32, u32)> = file.entries().map(|(l, _)| (l.first, l.last)).collect();
    assert_eq!(lines, [(0, 0), (1, 1), (2, 3), (4, 4)]);
    let (_, d) = file.entries().nth(1).unwrap();
    assert_eq!(d.comment.clone().map(|c| &text[c]), Some("# a comment"));
    assert_eq!(d.value.quote, Some(Quote::Double));
}

#[test]
fn an_unclosed_quote_runs_to_its_lines_end() {
    let text = "A=\"open\nB=2\n";
    assert_eq!(pairs(text), [("A", "open"), ("B", "2")]);
}

#[test]
fn finds_the_variables_a_value_interpolates() {
    let text = "U=${HOST}:$PORT/x\nD=\"${A:-fallback} ${B-x} \\$NOT $C\"\nS='$LITERAL'\n";
    let file = parse(text);
    let names: Vec<Vec<&str>> = file
        .entries()
        .map(|(_, e)| e.value.interpolations.iter().map(|i| &text[i.name.clone()]).collect())
        .collect();
    assert_eq!(names, [vec!["HOST", "PORT"], vec!["A", "B", "C"], vec![]]);
    let (_, d) = file.entries().nth(1).unwrap();
    let defaults: Vec<Option<&str>> =
        d.value.interpolations.iter().map(|i| i.default.clone().map(|r| &text[r])).collect();
    assert_eq!(defaults, [Some("fallback"), Some("x"), None]);
    assert_eq!(&text[d.value.interpolations[0].span.clone()], "${A:-fallback}");
}

#[test]
fn names_definitions_and_interpolations() {
    let text = "HOST=db\nURL=http://$HOST/\nHOST=other\n";
    let file = parse(text);
    let places: Vec<(&str, KeyPlace)> = file.names().map(|(r, p)| (&text[r], p)).collect();
    assert_eq!(
        places,
        [
            ("HOST", KeyPlace::Definition),
            ("URL", KeyPlace::Definition),
            ("HOST", KeyPlace::Interpolation),
            ("HOST", KeyPlace::Definition),
        ]
    );
    assert_eq!(file.definitions(text, "HOST").count(), 2);
    let at = text.find("$HOST").unwrap() + 3;
    assert_eq!(
        file.name_at(at).map(|(r, p)| (&text[r], p)),
        Some(("HOST", KeyPlace::Interpolation))
    );
    assert!(file.name_at(text.find("http").unwrap()).is_none());
}

#[test]
fn handles_crlf_and_a_missing_last_line_break() {
    let text = "A=1\r\nB=\"two\"\r\nC=3";
    assert_eq!(pairs(text), [("A", "1"), ("B", "two"), ("C", "3")]);
}
