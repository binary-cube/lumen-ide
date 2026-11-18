use super::*;

#[test]
fn a_constructors_class_types_without_attributes_modifiers_or_scalars() {
    let detail = "(#[Inject(a: [1, 2])] private readonly Db $db, ?Clock $clock = null, int $n, A|B $x, string ...$rest)";
    assert_eq!(parameter_types(detail), ["Db", "Clock", "A", "B"]);
    assert!(parameter_types("()").is_empty());
    assert!(parameter_types("no parameters").is_empty());
}
