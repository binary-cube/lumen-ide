use super::*;

fn sexp(text: &str) -> String {
    crate::intelligence::parse_from(text, None).map(|t| t.root_node().to_sexp()).unwrap_or_default()
}

#[test]
fn an_edit_parses_like_the_whole_text() {
    let before = "<?php\nclass A {\n    public function f() { $a = 1; }\n}\n";
    let after = "<?php\nclass A {\n    public function f() { $a = new B(); $a->x(); }\n}\n";
    let first = parsed(before).expect("a tree");
    let second = parsed(after).expect("a tree");
    assert_eq!(second.tree.root_node().to_sexp(), sexp(after), "the incremental parse is right");
    assert_eq!(first.tree.root_node().to_sexp(), sexp(before));
    // Multi-byte text around the edit.
    let unicode = "<?php\n// ăîș\n$s = 'țară';\n";
    let edited = "<?php\n// ăîș\n$s = 'țară' . 'ş';\n";
    parsed(unicode);
    assert_eq!(parsed(edited).map(|p| p.tree.root_node().to_sexp()), Some(sexp(edited)));
}

#[test]
fn the_edit_between_two_texts() {
    let e = edit("abcXYZdef", "abcQdef");
    assert_eq!((e.start_byte, e.old_end_byte, e.new_end_byte), (3, 6, 4));
    let e = edit("a\nb\nc", "a\nbb\nc");
    assert_eq!((e.start_position.row, e.start_position.column), (1, 1));
    assert_eq!(e.new_end_position, Point { row: 1, column: 2 });
    let e = edit("same", "same");
    assert_eq!((e.start_byte, e.old_end_byte, e.new_end_byte), (4, 4, 4));
}

#[test]
fn the_symbols_come_with_the_tree_and_go_on_release() {
    let text = "<?php\nnamespace App;\nuse Foo\\Bar;\nclass Probe {}\n";
    let p = parsed(text).expect("a tree");
    assert_eq!(p.symbols(text).namespace.as_deref(), Some("App"));
    // (Other tests parse at the same time: only that releasing works is checked.)
    release();
    assert!(parsed(text).is_some(), "parsed again after");
}
