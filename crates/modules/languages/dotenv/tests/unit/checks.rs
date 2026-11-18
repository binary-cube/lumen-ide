use super::*;

#[test]
fn a_key_defined_again_is_flagged_after_its_first_definition() {
    let text = "A=1\nB=2\nA=3\nexport A=4\n";
    let found: Vec<(usize, &str)> =
        duplicates(text, &EnvFile::parse(text)).into_iter().map(|r| (r.start, &text[r])).collect();
    assert_eq!(found, [(8, "A"), (19, "A")]);
    assert!(duplicates("A=1\nB=1\n", &EnvFile::parse("A=1\nB=1\n")).is_empty());
}

#[test]
fn reads_no_env_file_defines() {
    let text = "getenv('A'); getenv('B'); getenv('NODE_ENV'); getenv('A');";
    let reads: Vec<Range<usize>> = ["'A'", "'B'", "'NODE_ENV'"]
        .iter()
        .flat_map(|n| {
            text.match_indices(n).map(|(i, m)| i + 1..i + m.len() - 1).collect::<Vec<_>>()
        })
        .collect();
    let found: Vec<&str> = undefined(text, reads, &|k| k == "A", &["NODE_ENV"])
        .into_iter()
        .map(|r| &text[r])
        .collect();
    assert_eq!(found, ["B"]);
}

#[test]
fn keys_nothing_reads() {
    let text = "HOST=db\nURL=http://$HOST\nPORT=5432\nNAME=app\n";
    let found: Vec<&str> = unused(text, &EnvFile::parse(text), &|k| k == "URL")
        .into_iter()
        .map(|r| &text[r])
        .collect();
    assert_eq!(found, ["PORT", "NAME"], "HOST is interpolated, URL read");
}

#[test]
fn a_position_in_characters() {
    assert_eq!(position("ab\năbc", 6), (1, 2));
    assert_eq!(position("x", 0), (0, 0));
}
