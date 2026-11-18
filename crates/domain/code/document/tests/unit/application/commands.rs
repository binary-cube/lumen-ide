use super::*;

fn doc_with(text: &str, selections: Vec<Selection>) -> Document {
    let mut d = Document::new(text);
    d.set_selections(Selections::new(selections, 0));
    d
}

fn text(d: &Document) -> String {
    d.buffer().to_string()
}

#[test]
fn multiple_cursors() {
    // All occurrences, then dropping the last one.
    let mut d = doc_with("$a = $a + $a;", vec![Selection::new(0, 2)]);
    select_all_occurrences(&mut d);
    assert_eq!(d.selections().len(), 3);
    insert_text(&mut d, "$b");
    assert_eq!(text(&d), "$b = $b + $b;", "tastarea merge pe toate cursoarele");
    unselect_occurrence(&mut d);
    assert_eq!(d.selections().len(), 2);

    // ⌥ + click: add, then remove the cursor.
    let mut d = doc_with("abc\ndef", vec![Selection::cursor(0)]);
    toggle_cursor(&mut d, 5);
    assert_eq!(d.selections().len(), 2);
    toggle_cursor(&mut d, 5);
    assert_eq!(d.selections().len(), 1);

    // Column: lines 0..=2, columns 1..3; the short line gets the end.
    let mut d = doc_with("abcd\nx\nefgh", vec![Selection::cursor(0)]);
    column_select(&mut d, (0, 1), (2, 3));
    let ranges: Vec<_> = d.selections().iter().map(|s| s.range()).collect();
    assert_eq!(ranges, [1..3, 6..6, 8..10]);
    insert_text(&mut d, "-");
    assert_eq!(text(&d), "a-d\nx-\ne-h");
}

#[test]
fn auto_closing_pairs() {
    let q = &['"', '\''];
    let mut d = doc_with("f", vec![Selection::cursor(1)]);
    type_char(&mut d, '(', q);
    assert_eq!((text(&d), d.selections().primary().head), ("f()".into(), 2));
    type_char(&mut d, ')', q);
    assert_eq!((text(&d), d.selections().primary().head), ("f()".into(), 3), "trece peste");
    let mut d = doc_with("echo ", vec![Selection::cursor(5)]);
    type_char(&mut d, '"', q);
    assert_eq!(text(&d), "echo \"\"");
    backspace(&mut d);
    assert_eq!(text(&d), "echo ", "the empty pair goes");
    let mut d = doc_with("don", vec![Selection::cursor(3)]);
    type_char(&mut d, '\'', q);
    assert_eq!(text(&d), "don'", "no doubling beside letters");
    let mut d = doc_with("ab", vec![Selection::cursor(0)]);
    type_char(&mut d, '(', q);
    assert_eq!(text(&d), "(ab", "not before a word");
    let mut d = doc_with("x = val", vec![Selection::new(4, 7)]);
    type_char(&mut d, '[', q);
    assert_eq!(text(&d), "x = [val]", "wraps the selection");
}

#[test]
fn enter_in_doc_comments() {
    let mut d = doc_with("    /**", vec![Selection::cursor(7)]);
    newline(&mut d);
    assert_eq!(text(&d), "    /**\n     * \n     */");
    assert_eq!(d.selections().primary().head, 15);
    type_char(&mut d, 'x', &[]);
    newline(&mut d);
    assert_eq!(text(&d), "    /**\n     * x\n     * \n     */", "goes on with `* `");
}

#[test]
fn enter_after_open_brace_adds_the_closing_one() {
    let mut d = doc_with("function a() {", vec![Selection::cursor(14)]);
    newline(&mut d);
    assert_eq!(text(&d), "function a() {\n    \n}");
    assert_eq!(d.selections().primary().head, 19);
    // Already closed further down: not doubled.
    let mut d = doc_with("if (x) {\n}", vec![Selection::cursor(8)]);
    newline(&mut d);
    assert_eq!(text(&d), "if (x) {\n    \n}");
    let mut d = doc_with("    $a = [", vec![Selection::cursor(10)]);
    newline(&mut d);
    assert_eq!(text(&d), "    $a = [\n        \n    ]");
}

#[test]
fn typing_with_multiple_cursors() {
    let mut d = doc_with("ab\ncd", vec![Selection::cursor(1), Selection::cursor(4)]);
    insert_text(&mut d, "X");
    assert_eq!(text(&d), "aXb\ncXd");
    assert_eq!(d.selections().as_slice(), &[Selection::cursor(2), Selection::cursor(6)]);
}

#[test]
fn newline_keeps_indent_and_splits_braces() {
    let mut d = doc_with("    if (x) {}", vec![Selection::cursor(12)]);
    newline(&mut d);
    assert_eq!(text(&d), "    if (x) {\n        \n    }");
    assert_eq!(d.selections().primary(), Selection::cursor(21));
}

#[test]
fn backspace_removes_indent_level() {
    let mut d = doc_with("        x", vec![Selection::cursor(8)]);
    backspace(&mut d);
    assert_eq!(text(&d), "    x");
    backspace(&mut d);
    assert_eq!(text(&d), "x");
}

#[test]
fn indent_and_outdent_selected_lines() {
    let mut d = doc_with("a\nb\n\nc", vec![Selection::new(0, 6)]);
    indent(&mut d);
    assert_eq!(text(&d), "    a\n    b\n\n    c");
    outdent(&mut d);
    assert_eq!(text(&d), "a\nb\n\nc");
}

#[test]
fn duplicate_line_and_delete_line() {
    let mut d = doc_with("one\ntwo", vec![Selection::cursor(1)]);
    duplicate(&mut d);
    assert_eq!(text(&d), "one\none\ntwo");
    assert_eq!(d.selections().primary(), Selection::cursor(5));
    delete_lines(&mut d);
    assert_eq!(text(&d), "one\ntwo");
    d.set_selections(Selections::single(Selection::cursor(5)));
    delete_lines(&mut d);
    assert_eq!(text(&d), "one");
}

#[test]
fn a_group_of_lines_acts_as_one() {
    // Lines 1..=3 collapsed in the editor: one row.
    let text_of = "a\n<div>\n  x\n</div>\nb";
    let at = |d: &mut Document| {
        d.set_line_groups(vec![(1, 3)]);
        d.set_selections(Selections::single(Selection::cursor(3)));
    };
    let mut d = Document::new(text_of);
    at(&mut d);
    assert_eq!(copy(&d), "<div>\n  x\n</div>\n", "copied whole");
    assert_eq!(selected_lines(&d), 1..4);
    duplicate(&mut d);
    assert_eq!(text(&d), "a\n<div>\n  x\n</div>\n<div>\n  x\n</div>\nb");
    let mut d = Document::new(text_of);
    at(&mut d);
    delete_lines(&mut d);
    assert_eq!(text(&d), "a\nb", "deleted whole");
    let mut d = Document::new(text_of);
    at(&mut d);
    assert!(move_lines(&mut d, true));
    assert_eq!(text(&d), "<div>\n  x\n</div>\na\nb", "moved whole");
    // Without groups: each line on its own, as always.
    let mut d = Document::new(text_of);
    d.set_selections(Selections::single(Selection::cursor(3)));
    delete_lines(&mut d);
    assert_eq!(text(&d), "a\n  x\n</div>\nb");
}

#[test]
fn copy_without_selection_copies_line() {
    let d = doc_with("one\ntwo", vec![Selection::cursor(5)]);
    assert_eq!(copy(&d), "two\n");
}

#[test]
fn paste_distributes_lines_to_cursors() {
    let mut d = doc_with("a\nb", vec![Selection::cursor(1), Selection::cursor(3)]);
    paste(&mut d, "1\n2\n");
    assert_eq!(text(&d), "a1\nb2");
}

#[test]
fn select_next_occurrence_adds_cursor() {
    let mut d = doc_with("foo bar foo", vec![Selection::cursor(1)]);
    select_next_occurrence(&mut d);
    assert_eq!(d.selections().primary(), Selection::new(0, 3));
    select_next_occurrence(&mut d);
    assert_eq!(d.selections().len(), 2);
    insert_text(&mut d, "x");
    assert_eq!(text(&d), "x bar x");
}

#[test]
fn shift_arrow_extends_and_arrow_collapses() {
    let mut d = doc_with("hello", vec![Selection::cursor(0)]);
    move_cursors(&mut d, Motion::WordRight, true);
    assert_eq!(d.selections().primary(), Selection::new(0, 5));
    move_cursors(&mut d, Motion::Left, false);
    assert_eq!(d.selections().primary(), Selection::cursor(0));
}

#[test]
fn double_and_triple_click_selection() {
    let mut d = doc_with("$user->name;\nnext", vec![Selection::cursor(0)]);
    select_word_at(&mut d, 3, "");
    assert_eq!(d.selections().primary(), Selection::new(1, 5));
    // With `$` as a word character (PHP), the variable is selected whole.
    select_word_at(&mut d, 3, "$");
    assert_eq!(d.selections().primary(), Selection::new(0, 5));
    // At the end of a word the word is grabbed, not the punctuation after it.
    select_word_at(&mut d, 5, "");
    assert_eq!(d.selections().primary(), Selection::new(1, 5));
    select_word_at(&mut d, 6, "");
    assert_eq!(d.selections().primary(), Selection::new(5, 7));
    select_line_at(&mut d, 2);
    assert_eq!(d.selections().primary(), Selection::new(0, 13));
}

#[test]
fn dragging_after_double_click_keeps_whole_words() {
    let mut d = doc_with("const MAX_TERMS = 10;", vec![Selection::cursor(0)]);
    let origin = word_range(&d, 9, "");
    assert_eq!(origin, 6..15);
    // The mouse jitters a bit within the same word: the selection stays whole.
    let target = word_range(&d, 8, "");
    drag_extend(&mut d, origin.clone(), target);
    assert_eq!(d.selections().primary(), Selection::new(6, 15));
    // Dragged right / left: whole words are added.
    let target = word_range(&d, 19, "");
    drag_extend(&mut d, origin.clone(), target);
    assert_eq!(d.selections().primary(), Selection::new(6, 20));
    let target = word_range(&d, 1, "");
    drag_extend(&mut d, origin, target);
    assert_eq!(d.selections().primary(), Selection::new(15, 0));
}

#[test]
fn click_variants() {
    let mut d = doc_with("abcdef", vec![Selection::cursor(1)]);
    click(&mut d, 4, true, false);
    assert_eq!(d.selections().primary(), Selection::new(1, 4));
    click(&mut d, 2, false, false);
    click(&mut d, 5, false, true);
    assert_eq!(d.selections().len(), 2);
    drag_to(&mut d, 6);
    assert_eq!(d.selections().as_slice(), &[Selection::new(5, 6)]);
}

#[test]
fn cleanup_trims_except_cursor_line_and_adds_final_newline() {
    use crate::domain::document::SaveCleanup;
    let mut d = doc_with("a  \nb\t\n    ", vec![Selection::cursor(10)]);
    d.set_save_cleanup(SaveCleanup {
        trim_trailing_whitespace: true,
        insert_final_newline: Some(true),
    });
    cleanup_for_save(&mut d);
    assert_eq!(text(&d), "a\nb\n    \n");
    d.undo();
    assert_eq!(text(&d), "a  \nb\t\n    ");
}

#[test]
fn line_comment_toggles_at_min_indent() {
    let mut d = doc_with("    a\n\n  b", vec![Selection::new(0, 10)]);
    toggle_line_comment(&mut d, "//");
    assert_eq!(text(&d), "  //   a\n\n  // b");
    toggle_line_comment(&mut d, "//");
    assert_eq!(text(&d), "    a\n\n  b");
}

#[test]
fn block_comment_wraps_and_unwraps() {
    let mut d = doc_with("  <div></div>", vec![Selection::cursor(4)]);
    toggle_block_comment(&mut d, "<!--", "-->");
    assert_eq!(text(&d), "  <!-- <div></div> -->");
    d.set_selections(Selections::single(Selection::cursor(5)));
    toggle_block_comment(&mut d, "<!--", "-->");
    assert_eq!(text(&d), "  <div></div>");
}

#[test]
fn vertical_move_uses_target_lines_and_goal() {
    let mut d = doc_with("abcdef\nx\ny\nabcdef", vec![Selection::cursor(4)]);
    // Skips lines 1–2 (like a collapsed region).
    move_vertical(&mut d, true, false, &|l| (l == 0).then_some(3));
    assert_eq!(d.selections().primary().head, 15);
    move_vertical(&mut d, true, false, &|_| None);
    assert_eq!(d.selections().primary().head, d.buffer().len());
}

#[test]
fn undo_restores_selection() {
    let mut d = doc_with("abc", vec![Selection::new(0, 3)]);
    insert_text(&mut d, "x");
    d.undo();
    assert_eq!(text(&d), "abc");
    assert_eq!(d.selections().primary(), Selection::new(0, 3));
}

#[test]
fn final_newline_follows_the_file_setting_then_the_default() {
    use crate::domain::document::SaveCleanup;
    let unset = SaveCleanup::default();
    assert_eq!(unset.or_final_newline(true).insert_final_newline, Some(true));
    let off = SaveCleanup { insert_final_newline: Some(false), ..unset };
    assert_eq!(off.or_final_newline(true).insert_final_newline, Some(false));
    let mut d = doc_with("x", vec![Selection::cursor(0)]);
    d.set_save_cleanup(unset);
    cleanup_for_save(&mut d);
    assert_eq!(text(&d), "x", "unset and not resolved: nothing added");
    // A cursor at the end stays on the last line, before the added newline.
    let mut d = doc_with("ab", vec![Selection::cursor(2)]);
    d.set_save_cleanup(unset.or_final_newline(true));
    cleanup_for_save(&mut d);
    assert_eq!(text(&d), "ab\n");
    assert_eq!(d.selections().as_slice(), &[Selection::cursor(2)]);
}

#[test]
fn lines_move_past_their_neighbours_with_the_cursor() {
    let mut d = doc_with("a\nb\nc", vec![Selection::cursor(2)]);
    assert!(move_lines(&mut d, true));
    assert_eq!(text(&d), "b\na\nc");
    assert_eq!(d.selections().primary().head, 0, "the cursor moved with its line");
    assert!(!move_lines(&mut d, true), "the first line stays");
    assert!(move_lines(&mut d, false));
    assert!(move_lines(&mut d, false));
    assert_eq!(text(&d), "a\nc\nb", "the last line, without a newline after it");
    assert_eq!(d.selections().primary().head, 4);
    d.undo();
    assert_eq!(text(&d), "a\nb\nc", "one undo step each");
}

#[test]
fn a_block_swaps_with_a_block_keeping_the_gap() {
    let src = "f1 {\n  x\n}\n\nf2 {\n  y\n}\n";
    let mut d = doc_with(src, vec![Selection::cursor(src.find("f2").unwrap())]);
    assert!(swap_lines(&mut d, 4..7, 0..3));
    assert_eq!(text(&d), "f2 {\n  y\n}\n\nf1 {\n  x\n}\n");
    assert_eq!(d.selections().primary().head, 0);
    assert!(!swap_lines(&mut d, 0..3, 2..5), "overlapping: nothing");
}

/// Enter with the tree's base: one level in from it, the closer at it; between a pair (a
/// bracket's, a tag's) the closer gets its own line.
#[test]
fn enter_follows_the_tree_s_base() {
    let at = |text: &str, offset: usize| doc_with(text, vec![Selection::cursor(offset)]);
    let base = |b: &'static str| move |_: usize| Some(b.to_owned());
    // Inside a block: its base plus a level, whatever the current line's indentation.
    let mut d = at("    foo(a,b);", 10);
    newline_with(&mut d, &base("  "));
    assert_eq!(text(&d), "    foo(a,\n      b);");
    // `<div>|</div>`: the content one level in, `</div>` at the base.
    let mut d = at("  <div></div>", 7);
    newline_with(&mut d, &base("  "));
    assert_eq!(text(&d), "  <div>\n      \n  </div>");
    assert_eq!(d.selections().primary().head, 14);
    // Right before a closer: the closer's line at the base.
    let mut d = at("  f(x)", 5);
    newline_with(&mut d, &base(""));
    assert_eq!(text(&d), "  f(x\n)");
}

/// A closer typed first on its line goes back to its opener's indentation.
#[test]
fn a_closer_goes_back_to_its_opener_s_indentation() {
    let typed = |before: &str, c: char| {
        let mut d = doc_with(before, vec![Selection::cursor(before.len())]);
        type_char(&mut d, c, &[]);
        text(&d)
    };
    let block = typed("function a() {\n    if (x) {\n        y();\n        ", '}');
    assert!(block.ends_with("\n    }"), "{block:?}");
    let list = typed("x = [\n  1,\n      ", ']');
    assert!(list.ends_with("\n]"), "{list:?}");
    // In the middle of a line: as typed.
    assert_eq!(typed("f(a ", ')'), "f(a )");
}
