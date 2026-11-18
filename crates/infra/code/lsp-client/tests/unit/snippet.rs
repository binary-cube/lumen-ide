use super::*;

#[test]
fn plain_text_is_inserted_as_it_is_with_the_cursor_at_its_end() {
    assert_eq!(expand("console"), ("console".into(), 7));
}

#[test]
fn the_cursor_goes_to_the_first_tab_stop() {
    assert_eq!(expand("log($1)$0"), ("log()".into(), 4));
    assert_eq!(expand("a$2b$1c"), ("abc".into(), 2));
    assert_eq!(expand("x$0y"), ("xy".into(), 1));
}

#[test]
fn placeholders_keep_their_text() {
    assert_eq!(expand("foo(${1:a}, ${2:b})$0"), ("foo(a, b)".into(), 4));
    assert_eq!(expand("${1:outer ${2:inner}}"), ("outer inner".into(), 0));
    assert_eq!(expand("${1}x"), ("x".into(), 0));
}

#[test]
fn choices_insert_the_first() {
    assert_eq!(expand("${1|let,const,var|} x"), ("let x".into(), 0));
}

#[test]
fn escapes_are_kept_literally() {
    assert_eq!(expand(r"\$1 \} \\"), (r"$1 } \".into(), 6));
}

#[test]
fn variables_insert_their_default_or_nothing() {
    assert_eq!(expand("${TM_FILENAME:file}.ts"), ("file.ts".into(), 7));
    assert_eq!(expand("$TM_FILENAME!"), ("!".into(), 1));
    assert_eq!(expand("${TM_FILENAME/(.*)/$1/}x"), ("x".into(), 1));
    assert_eq!(expand("cost: $"), ("cost: $".into(), 7));
}

#[test]
fn the_cursor_is_counted_in_bytes() {
    assert_eq!(expand("ăț(${1:ș})"), ("ăț(ș)".into(), "ăț(".len()));
}
