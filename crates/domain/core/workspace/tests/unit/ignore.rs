use super::*;

#[test]
fn follows_gitignore_rules() {
    let mut rules = IgnoreRules::default();
    rules.add("", "# build\n/target\n.DS_Store\n*.log\n!keep.log\ncache/\ndocs/**/*.tmp\n");
    rules.add("web", "/build\n[Tt]humbs.db\n");
    assert!(rules.is_ignored("target", true));
    assert!(!rules.is_ignored("crates/target", true));
    assert!(rules.is_ignored("a/b/.DS_Store", false));
    assert!(rules.is_ignored("x/error.log", false));
    assert!(!rules.is_ignored("x/keep.log", false));
    assert!(rules.is_ignored("src/cache", true));
    assert!(!rules.is_ignored("src/cache", false));
    assert!(rules.is_ignored("docs/a/b/x.tmp", false));
    assert!(rules.is_ignored("docs/x.tmp", false));
    assert!(rules.is_ignored("web/build", true));
    assert!(!rules.is_ignored("build", true));
    assert!(rules.is_ignored("web/img/thumbs.db", false));
    assert!(!rules.is_ignored("src/main.rs", false));
}
