use super::*;

/// The declaration found for the `nth` `$name` of `text` (0-based), as a line of `text`.
fn declared(text: &str, name: &str, nth: usize) -> Option<&'static str> {
    let at = text.match_indices(name).nth(nth).unwrap().0 + 2;
    let found = definition(text, at)?;
    let line = text[..found].rsplit('\n').next().unwrap_or("");
    let rest = text[found..].split('\n').next().unwrap_or("");
    Some(Box::leak(format!("{line}{rest}").trim().to_owned().into_boxed_str()))
}

#[test]
fn a_variable_is_declared_where_its_scope_first_binds_it() {
    let text = "<?php\nfunction f(int $n) {\n    $total = $n;\n    $total = $total * 2;\n    \
                foreach ($items as $key => $item) {\n        echo $item . $key;\n    }\n    \
                [$a, $b] = pair();\n    try {} catch (Exception $e) { echo $e; }\n    \
                echo $a . $b . $total . $n;\n}\n";
    // The first assignment, not the last one.
    assert_eq!(declared(text, "$total", 3), Some("$total = $n;"));
    // A parameter: the signature.
    assert_eq!(declared(text, "$n", 2), Some("function f(int $n) {"));
    assert_eq!(declared(text, "$item", 1), Some("foreach ($items as $key => $item) {"));
    assert_eq!(declared(text, "$key", 1), Some("foreach ($items as $key => $item) {"));
    assert_eq!(declared(text, "$b", 1), Some("[$a, $b] = pair();"));
    assert_eq!(declared(text, "$e", 1), Some("try {} catch (Exception $e) { echo $e; }"));
    // Only read, never bound in its scope: nothing.
    assert_eq!(declared(text, "$items", 0), None);
}

#[test]
fn closures_and_arrow_functions_have_their_own_bindings() {
    let text = "<?php\n$rate = 2;\n$f = function ($x) use ($rate) {\n    return $x * $rate;\n};\n\
                $g = fn ($y) => $y * $rate;\nclass A { function m() { return $this; } }\n";
    // A closure's `use`, an arrow function's parameter, the outer scope seen from it.
    assert_eq!(declared(text, "$rate", 2), Some("$f = function ($x) use ($rate) {"));
    assert_eq!(declared(text, "$x", 1), Some("$f = function ($x) use ($rate) {"));
    assert_eq!(declared(text, "$y", 1), Some("$g = fn ($y) => $y * $rate;"));
    assert_eq!(declared(text, "$rate", 3), Some("$rate = 2;"));
    assert_eq!(declared(text, "$this", 0), None);
}
