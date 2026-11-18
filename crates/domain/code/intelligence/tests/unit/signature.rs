use super::*;

#[test]
fn finds_the_call_and_the_argument() {
    let src = "<?php foo(1, bar('a,b', [1, 2]), ";
    let call = enclosing_call(src, src.len(), "$\\").unwrap();
    assert_eq!(&src[call.name.clone()], "foo");
    assert_eq!(call.argument, 2);
    let inner = src.find("[1").unwrap();
    let call = enclosing_call(src, inner, "$\\").unwrap();
    assert_eq!(&src[call.name.clone()], "bar");
    assert_eq!(call.argument, 1, "the comma in the string doesn't count");
    // Closed, or not a call.
    assert!(enclosing_call("f(1); ", 6, "$\\").is_none());
    assert!(enclosing_call("x = [1, ", 8, "$\\").is_none());
    assert_eq!(enclosing_call("$o->run(", 8, "$\\").map(|c| c.name), Some(4..7));
    assert!(enclosing_call("f(1 // (\n", 9, "$\\").is_some());
}

#[test]
fn splits_the_signature() {
    let code = "f(array $a = [1, 2], string $s = ',', int ...$rest): int";
    let params = parameters(code).unwrap();
    let names: Vec<_> = params.iter().map(|r| &code[r.clone()]).collect();
    assert_eq!(names, ["array $a = [1, 2]", "string $s = ','", "int ...$rest"]);
    assert_eq!(active_parameter(code, &params, 5), Some(2), "variadic takes the rest");
    assert_eq!(parameters("g(): void").unwrap(), Vec::<Range<usize>>::new());
    let ts = "map(callbackfn: (value: T, index: number) => U, thisArg?: Map<K, V>): U[]";
    assert_eq!(parameters(ts).unwrap().len(), 2);
    assert_eq!(active_parameter("g()", &[], 0), None);
}

/// The named arguments written so far: at the call's own level, not `A::B`, not a nested
/// call's.
#[test]
fn named_arguments_written_so_far() {
    let text = "f(name: 'x', A::B, g(inner: 1), [k: 2], other : 3, ";
    assert_eq!(named_so_far(text, 1, text.len()), ["name", "other"]);
    // Inside strings nothing is an argument, escaped quotes included.
    let quoted = r#"f('a, b: c', "it\"s, d: e", real: 1, "#;
    assert_eq!(named_so_far(quoted, 1, quoted.len()), ["real"]);
}

/// A signature is laid out one parameter per line; a code already on several lines (a
/// column's card: `t.c` then its definition) is kept as written.
#[test]
fn a_code_already_on_lines_is_kept() {
    let laid = multiline_signature("f(int $a, int $b)");
    assert!(laid.contains("\n    int $a,"), "{laid}");
    let card = "orders.amount\nnumeric(10, 2) NOT NULL";
    assert_eq!(multiline_signature(card), card);
}

/// In SQL a `--` comment is skipped (its quote opens no string); elsewhere `--` is code
/// (JavaScript's `i--`).
#[test]
fn dash_comments_are_skipped_only_when_asked() {
    let sql = "-- the customers' names\nSELECT substr(name, ";
    assert!(enclosing_call(sql, sql.len(), "").is_none(), "the quote opened a string");
    let call = enclosing_call_in(sql, sql.len(), "", true).expect("the call");
    assert_eq!(&sql[call.name], "substr");
    assert_eq!(call.argument, 1);
    let js = "f(i--, ";
    assert_eq!(enclosing_call_in(js, js.len(), "", false).map(|c| c.argument), Some(1));
}
