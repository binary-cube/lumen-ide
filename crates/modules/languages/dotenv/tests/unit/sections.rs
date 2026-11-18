use super::*;

const SYMFONY: &str = include_str!("../data/symfony.env");
const LARAVEL: &str = include_str!("../data/laravel.env");
const FLEX: EnvSections = EnvSections::Fenced { open: "###> ", close: "###< ", trail: " ###" };

/// The sections as `(label, keys, first line, last line)`.
fn read(text: &str, rules: &[EnvSections]) -> Vec<(String, usize, u32, u32)> {
    sections(text, &EnvFile::parse(text), rules)
        .into_iter()
        .map(|s| (s.label, s.keys, s.first_line, s.last_line))
        .collect()
}

#[test]
fn a_comment_names_the_keys_below_it() {
    let text = "# Database\nDB_HOST=db\nDB_PORT=5432\n\n# ---- Mail ----\n# MAIL_FROM=x@y\nMAIL_HOST=smtp\n\nLOOSE=1\n";
    assert_eq!(
        read(text, &[]),
        [("Database".into(), 2, 0, 2), ("Mail".into(), 1, 4, 6)],
        "decorations trimmed; a commented-out entry names nothing; an unnamed group is none"
    );
}

#[test]
fn a_headings_own_punctuation_is_kept() {
    let text = "## Database (PostgreSQL): ##\nDB_HOST=db\n";
    assert_eq!(read(text, &[]), [("Database (PostgreSQL)".into(), 1, 0, 1)]);
}

#[test]
fn a_comment_with_no_keys_is_no_section() {
    let text = "# Just notes\n# more notes\n\nA=1\n";
    assert!(read(text, &[]).is_empty());
}

#[test]
fn flex_blocks_are_symfonys_sections() {
    let sections = read(SYMFONY, &[FLEX]);
    let labels: Vec<(&str, usize)> = sections.iter().map(|(l, k, ..)| (l.as_str(), *k)).collect();
    assert_eq!(
        labels,
        [
            ("symfony/framework-bundle", 3),
            ("doctrine/doctrine-bundle", 1),
            ("symfony/messenger", 1),
            ("symfony/mailer", 1),
            ("lexik/jwt-authentication-bundle", 3),
        ]
    );
    // From its opening marker to its closing one; the comment inside names nothing more.
    assert_eq!((sections[0].2, sections[0].3), (8, 12));
    let label_at = super::sections(SYMFONY, &EnvFile::parse(SYMFONY), &[FLEX])[1].label_at.clone();
    assert_eq!(&SYMFONY[label_at], "doctrine/doctrine-bundle");
}

#[test]
fn without_the_rule_flex_markers_are_plain_comments() {
    let text = "###> symfony/mailer ###\nMAILER_DSN=null://null\n###< symfony/mailer ###\n";
    assert_eq!(read(text, &[]), [("symfony/mailer".into(), 1, 0, 2)]);
}

#[test]
fn an_unclosed_block_is_no_section() {
    let text = "###> a/b ###\nA=1\n\nB=2\n";
    assert_eq!(read(text, &[FLEX]), [("a/b".into(), 1, 0, 1)], "the comment above names it still");
}

#[test]
fn laravels_groups_are_named_by_their_keys_prefix() {
    let labels: Vec<(String, usize)> =
        read(LARAVEL, &[EnvSections::KeyPrefix]).into_iter().map(|(l, k, ..)| (l, k)).collect();
    let expected = [
        ("APP_", 7),
        ("LOG_", 2),
        ("DB_", 6),
        ("SESSION_", 2),
        ("REDIS_", 3),
        ("MAIL_", 3),
        ("AWS_", 3),
    ];
    assert_eq!(
        labels,
        expected.map(|(l, k)| (l.to_owned(), k)),
        "APP_ joined across a blank line; one VITE_ key is none"
    );
}

#[test]
fn key_prefixes_give_way_to_fenced_blocks_and_comments() {
    let text = "# Cache\nREDIS_HOST=r\nREDIS_PORT=1\n\nDB_A=1\nDB_B=2\n";
    assert_eq!(
        read(text, &[EnvSections::KeyPrefix]),
        [("Cache".into(), 2, 0, 2), ("DB_".into(), 2, 4, 5)]
    );
    let fenced = "###> a/b ###\nX_A=1\n###< a/b ###\n\nDB_A=1\nDB_B=2\n";
    assert_eq!(read(fenced, &[FLEX, EnvSections::KeyPrefix]), [("a/b".into(), 1, 0, 2)]);
}

#[test]
fn a_group_of_mixed_prefixes_is_split_into_runs() {
    let text = "APP_A=1\nAPP_B=2\nPHP_WORKERS=4\nBCRYPT_ROUNDS=12\n";
    assert_eq!(read(text, &[EnvSections::KeyPrefix]), [("APP_".into(), 2, 0, 1)]);
}
