use super::*;

#[test]
fn explains_built_ins() {
    let src = "<?php if (str_contains($a, 'x')) { new ArrayObject(); $o->strlen(); }";
    let at = |w: &str| src.find(w).unwrap() + 1;
    let f = hover(src, at("str_contains")).unwrap();
    assert!(f.code.starts_with("str_contains(") && f.code.contains("$needle"), "{}", f.code);
    assert_eq!(f.notes, [HoverNote::Since("8.0")]);
    assert_eq!(hover(src, at("ArrayObject")).unwrap().code, "class ArrayObject");
    // A method call with a built-in function's name is not the function.
    assert!(hover(src, src.find("strlen").unwrap() + 1).is_none());
    assert!(hover(src, at("$a")).is_none());
}
