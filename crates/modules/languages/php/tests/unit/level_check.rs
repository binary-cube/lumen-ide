use super::*;
use syntax_ts::tree_sitter::Parser;

fn issues(src: &str, level: &str) -> Vec<&'static str> {
    let mut parser = Parser::new();
    let _ = parser.set_language(&tree_sitter_php::LANGUAGE_PHP.into());
    let Some(tree) = parser.parse(src, None) else { return Vec::new() };
    check(tree.root_node(), src, level).into_iter().map(|i| i.feature).collect()
}

const SRC: &str = r#"<?php
enum Status { case On; }
class Car {
    private int $speed = 1_000;
    public function __construct(private readonly string $name) {}
    public function info(?int $x, int|string $y): string {
        $a = $x ?? 0;
        $b = match ($y) { 1 => 'a', default => 'b' };
        $c = fn($v) => $v * 2;
        $d = $this?->name;
        $e = [...$list];
        return str_contains($this->name, needle: 'a') ? 'y' : 'n';
    }
}
"#;

#[test]
fn finds_features_newer_than_the_level() {
    let old = issues(SRC, "5.6");
    for feature in [
        "enum",
        "typed_property",
        "numeric_separator",
        "promotion",
        "readonly",
        "nullable_type",
        "union_type",
        "return_type",
        "null_coalescing",
        "match",
        "arrow_function",
        "nullsafe",
        "array_spread",
        "function",
        "named_arguments",
    ] {
        assert!(old.contains(&feature), "{feature} missing in {old:?}");
    }
    // PHP 8.1 supports all of it.
    assert!(issues(SRC, "8.1").is_empty(), "{:?}", issues(SRC, "8.1"));
    // 7.4: only what came in 8.x.
    let mid = issues(SRC, "7.4");
    assert!(mid.contains(&"match") && !mid.contains(&"null_coalescing"), "{mid:?}");
}

/// Each issue says what it's written as: a keyword or operator, a function as `name()`.
#[test]
fn issues_carry_their_code() {
    let mut parser = Parser::new();
    let _ = parser.set_language(&tree_sitter_php::LANGUAGE_PHP.into());
    let tree = parser.parse(SRC, None).unwrap();
    let found = check(tree.root_node(), SRC, "5.6");
    let code = |feature: &str| found.iter().find(|i| i.feature == feature).map(|i| i.code.clone());
    assert_eq!(code("match"), Some(Some("match".into())));
    assert_eq!(code("null_coalescing"), Some(Some("??".into())));
    let function = found.iter().find(|i| i.name.as_deref() == Some("str_contains")).unwrap();
    assert_eq!(function.code.as_deref(), Some("str_contains()"));
}

/// An optional parameter before a required one: deprecated from PHP 8.0 (a warning, on the
/// optional one's name); a typed `= null` (the old nullable) and trailing optionals are fine.
#[test]
fn an_optional_parameter_before_a_required_one_is_deprecated() {
    let src = r#"<?php
class A {
    public static function ceva($a = 1, int $b) {}
    function fine(?Foo $c = null, int $d, $e = 2, ...$rest) {}
    function __construct(private int $f = 0, string $g) {}
}
"#;
    let mut parser = Parser::new();
    let _ = parser.set_language(&tree_sitter_php::LANGUAGE_PHP.into());
    let tree = parser.parse(src, None).unwrap();
    let found: Vec<LevelIssue> = check(tree.root_node(), src, "8.3")
        .into_iter()
        .filter(|i| i.feature == "deprecated_optional_parameter")
        .collect();
    let names: Vec<&str> = found.iter().filter_map(|i| i.name.as_deref()).collect();
    assert_eq!(names, ["$a", "$f"]);
    assert!(found.iter().all(LevelIssue::is_warning));
    assert_eq!(&src[found[0].range.clone()], "$a");
    assert_eq!(found[0].code.as_deref(), Some("$a = 1"));
    // Before 8.0 it was allowed.
    assert!(!issues(src, "7.4").contains(&"deprecated_optional_parameter"));
}

/// A parameter written over two lines (`$` on one, its name on the next) is reported on one
/// line: its name and its code never carry the newline into the message or the chip.
#[test]
fn an_optional_parameter_over_two_lines_is_reported_on_one_line() {
    let src = "<?php\nclass A {\n    public static function ceva($\n        a = 1, int $b, int $c) {}\n}\n";
    let mut parser = Parser::new();
    let _ = parser.set_language(&tree_sitter_php::LANGUAGE_PHP.into());
    let tree = parser.parse(src, None).unwrap();
    let found: Vec<LevelIssue> = check(tree.root_node(), src, "8.3")
        .into_iter()
        .filter(|i| i.feature == "deprecated_optional_parameter")
        .collect();
    assert_eq!(found.len(), 1, "{found:?}");
    let name = found[0].name.as_deref().unwrap();
    let code = found[0].code.as_deref().unwrap();
    assert!(!name.contains('\n') && !code.contains('\n'), "{name:?} / {code:?}");
    assert_eq!(name, "$ a");
    assert_eq!(code, "$ a = 1");
}
