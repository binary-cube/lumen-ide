use super::*;

const SYMFONY: &str = include_str!("../data/symfony.env");
const LARAVEL: &str = include_str!("../data/laravel.env");
const FLEX: EnvSections = EnvSections::Fenced { open: "###> ", close: "###< ", trail: " ###" };

#[test]
fn keys_are_inside_their_sections() {
    let file = symbols(SYMFONY, &[FLEX]);
    let mailer = file.symbols.iter().find(|s| s.name == ("symfony/mailer").into()).unwrap();
    assert_eq!(mailer.kind, SymbolKind::Section);
    let dsn = file.symbols.iter().find(|s| s.name == "MAILER_DSN".into()).unwrap();
    assert_eq!(dsn.kind, SymbolKind::Constant);
    assert!(mailer.range.start <= dsn.range.start && dsn.range.end <= mailer.range.end);
    assert!(dsn.detail.is_none(), "a value is never shown: it may be a secret");
    // Outside every section.
    let proxies = file.symbols.iter().find(|s| s.name == "TRUSTED_PROXIES".into()).unwrap();
    assert!(
        file.symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Section)
            .all(|s| !s.range.contains(&proxies.offset))
    );
    assert_eq!(&*file.language, crate::ID);
}

#[test]
fn the_outline_and_the_breadcrumb_follow_the_sections() {
    let file = symbols(SYMFONY, &[FLEX]);
    let rows: Vec<(usize, &str)> = code_intelligence::outline(&file)
        .iter()
        .map(|r| (r.depth, file.symbols[r.index].name.as_ref()))
        .take(4)
        .collect();
    assert_eq!(
        rows,
        [(0, "symfony/framework-bundle"), (1, "APP_ENV"), (1, "APP_SECRET"), (1, "APP_DEBUG")]
    );
    let at = SYMFONY.find("s3cret").unwrap();
    let chain: Vec<&str> = code_intelligence::scope_at(&file, at)
        .iter()
        .map(|&i| file.symbols[i].name.as_ref())
        .collect();
    assert_eq!(chain, ["symfony/mailer", "MAILER_DSN"]);
}

#[test]
fn sections_fold_showing_their_keys() {
    let found = folds(LARAVEL, &[EnvSections::KeyPrefix]);
    let first = &found[0];
    assert_eq!((first.start_line, first.end_line), (0, 7));
    assert_eq!(first.placeholder, Some(FoldCount::Keys(7)));
    assert_eq!(first.start_byte, 0);
    // A one-line section has nothing to fold.
    let one = "# Only\nA=1\n";
    assert_eq!(folds(one, &[]).len(), 1);
    assert!(folds("# Only\n", &[]).is_empty());
}
