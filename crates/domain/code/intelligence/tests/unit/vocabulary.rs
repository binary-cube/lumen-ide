use super::*;

const DATA: &str =
    "# a comment\ntag\tif\nfilter\tdate\tdate(¦)\tformat, timezone\nfilter\tupper\n\nbroken\n";

#[test]
fn the_lines_become_words_and_the_rest_is_skipped() {
    assert_eq!(Vocabulary::parse(DATA).len(), 3);
    assert!(Vocabulary::parse("# only a comment\n").is_empty());
}

#[test]
fn only_the_groups_asked_for_are_suggested() {
    let items = Vocabulary::parse(DATA).suggest(&[("tag", CompletionKind::Keyword)], "");
    assert_eq!(items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["if"]);
    assert_eq!(items[0].kind, CompletionKind::Keyword);
}

#[test]
fn the_cursor_ends_where_the_data_says() {
    let items = Vocabulary::parse(DATA).suggest(&[("filter", CompletionKind::Function)], "dat");
    assert_eq!(items[0].label, "date");
    assert_eq!(items[0].insert, "date()");
    assert_eq!(items[0].cursor_back, 1);
    assert_eq!(items[0].detail.as_deref(), Some("format, timezone"));
}

#[test]
fn a_word_inserted_as_it_is_leaves_the_cursor_after_it() {
    let items = Vocabulary::parse(DATA).suggest(&[("filter", CompletionKind::Function)], "upp");
    assert_eq!((items[0].insert.as_str(), items[0].cursor_back), ("upper", 0));
}
