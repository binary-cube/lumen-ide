use super::*;
use syntax_ts::tree_sitter::Parser;

fn unused(src: &str) -> Vec<(String, &'static str)> {
    let mut parser = Parser::new();
    let _ = parser.set_language(&tree_sitter_php::LANGUAGE_PHP.into());
    let tree = parser.parse(src, None).expect("parsed");
    find(tree.root_node(), src).into_iter().map(|u| (u.name, u.kind)).collect()
}

#[test]
fn imports_and_private_members() {
    let src = r#"<?php
namespace App\Dto;

use App\Entity\Device;
use http\Env\Request;
use App\Other\{Used, Unused as Gone};

/** @param Device $d */
final class X
{
    private int $count = 0;
    private int $seen = 0;
    #[ORM\Column]
    private string $mapped;

    public function a(Used $u): int
    {
        $this->helper();
        return $this->count;
    }

    private function helper(): void {}
    private function never(): void {}
    private function callback(): void {}

    public function b(): array { return [$this, 'callback']; }
}
"#;
    let found = unused(src);
    assert_eq!(
        found,
        [
            ("Request".to_owned(), "import"),
            ("Gone".to_owned(), "import"),
            ("seen".to_owned(), "property"),
            ("never".to_owned(), "method"),
        ]
    );
}

#[test]
fn variables_assigned_and_never_read() {
    let src = r#"<?php
function f($param) {
    $unused = 42;
    $used = 1;
    echo $used;
    $twice = 1;
    $twice = 2;
    $counter = 0;
    $counter++;
    $text = 'a';
    $text .= 'b';
    foreach ([1] as $k => $v) {}
    $captured = 1;
    $closure = function () use ($captured) { $inner = 1; };
    $arrow = 2;
    $g = fn () => $arrow;
    echo "{$closure} {$g}";
}
function g() { $a = 1; return compact('a'); }
function h() { $a = 1; $$name = 2; }
function i() { $a = 1; include 'x.php'; }
function j() { $a = 1; $b = &$a; }
"#;
    let found = unused(src);
    let variables: Vec<&str> =
        found.iter().filter(|(_, k)| *k == "variable").map(|(n, _)| n.as_str()).collect();
    assert_eq!(variables, ["$unused", "$twice", "$inner"]);
}

/// A class including a template (its code may use the private members), in any case of the
/// keyword: its members aren't reported; the class next to it, without one, has its own.
#[test]
fn a_class_including_a_file_keeps_its_private_members_whatever_the_case() {
    let src = r#"<?php
class View
{
    private function title(): string { return 'x'; }
    public function render(): void { INCLUDE __DIR__ . '/page.php'; }
}
class Plain
{
    private function unusedOne(): void {}
    public function run(): void {}
}
"#;
    let found = unused(src);
    assert!(!found.iter().any(|(n, _)| n == "title"), "{found:?}");
    assert!(found.contains(&("unusedOne".to_owned(), "method")), "{found:?}");
}
