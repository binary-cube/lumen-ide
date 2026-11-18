use super::*;

#[test]
fn a_new_name_is_spelled_as_the_old_one_with_or_without_its_sigil() {
    assert_eq!(spelled_like("$tax", "vat", Some('$')).as_deref(), Some("$vat"));
    assert_eq!(spelled_like("$tax", "$vat", Some('$')).as_deref(), Some("$vat"));
    assert_eq!(spelled_like("total", "$sum", Some('$')).as_deref(), Some("sum"));
}

#[test]
fn what_is_no_name_is_refused() {
    for wanted in ["", "$", "a b", "1a", "a-b"] {
        assert_eq!(spelled_like("total", wanted, Some('$')), None, "{wanted:?}");
    }
}

#[test]
fn a_local_changes_only_inside_its_scope_and_as_a_whole_word() {
    let text = "$a = 1; $ab = $a; $a;";
    let places = local_places(text, "$a", Some('$'), 0..17);
    assert_eq!(places, vec![0..2, 14..16]);
}

#[test]
fn the_word_under_the_cursor_is_found() {
    assert_eq!(word_at("x total y", 4, "total", None), Some(2..7));
    assert_eq!(word_at("x total y", 0, "total", None), None);
}
