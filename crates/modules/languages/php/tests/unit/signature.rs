use super::*;

#[test]
fn php_parameters_read_and_written_back() {
    for text in [
        "int $a",
        "?string $name = null",
        "array $list = [1, 2]",
        "private readonly Clock $clock",
        "#[SensitiveParameter] string $password",
        "int &$count",
        "string ...$parts",
        "callable $f = fn() => 1",
    ] {
        assert_eq!(PhpParams.render(&PhpParams.parse(text)), text);
    }
    let p = PhpParams.parse("#[Attr(1)] protected ?int &$n = 0");
    assert_eq!(
        (p.prefix.as_str(), p.type_hint.as_str(), p.name.as_str()),
        ("#[Attr(1)] protected", "?int", "n")
    );
    assert!(p.by_ref && !p.variadic);
    assert_eq!(PhpParams.named_argument("limit: 10"), Some(("limit", "10")));
    assert_eq!(PhpParams.named_argument("A::B"), None);
    assert_eq!(PhpParams.named_argument("$a ? 1 : 2"), None);
}
