use super::*;

#[test]
fn a_mask_keeps_the_files_it_names_and_leaves_out_the_others() {
    let php = FileMask::parse("*.php, *.twig");
    assert!(php.accepts("src/User.php") && php.accepts("templates/base.html.twig"));
    assert!(php.accepts("SRC/USER.PHP"), "any case");
    assert!(!php.accepts("assets/app.js"));
    let out = FileMask::parse("*.js !*.min.js");
    assert!(out.accepts("a/app.js") && !out.accepts("a/app.min.js"));
    let only_out = FileMask::parse("!vendor/**");
    assert!(only_out.accepts("src/a.php") && !only_out.accepts("vendor/x/a.php"));
    assert!(FileMask::parse("  ").is_empty() && FileMask::parse("").accepts("anything"));
}

#[test]
fn a_pattern_with_a_slash_is_matched_on_the_whole_path() {
    let mask = FileMask::parse("src/**/*.php");
    assert!(mask.accepts("src/a.php") && mask.accepts("src/Domain/Order/Order.php"));
    assert!(!mask.accepts("tests/a.php"));
    // `*` stays within a folder; `?` is one character.
    assert!(!FileMask::parse("src/*.php").accepts("src/x/a.php"));
    assert!(
        FileMask::parse("?.php").accepts("lib/a.php")
            && !FileMask::parse("?.php").accepts("ab.php")
    );
}
