use super::*;

fn fold(start_line: u32, end_line: u32) -> FoldRange {
    FoldRange {
        start_byte: start_line as usize * 100,
        end_byte: end_line as usize * 100 + 99,
        start_line,
        end_line,
        placeholder: None,
    }
}

fn folds() -> Folds {
    let mut f = Folds::default();
    // 0: class {   1: fn {   2..3   4: }   5..8   9: }
    f.set_available(&[fold(0, 9), fold(1, 4), fold(5, 8)]);
    f
}

#[test]
fn collapse_maps_rows_and_lines() {
    let mut f = folds();
    assert!(f.toggle(1));
    assert!(f.is_hidden(2) && f.is_hidden(4) && !f.is_hidden(5));
    assert_eq!(f.visible_rows(10), 7);
    assert_eq!(f.line_of(2), 5);
    assert_eq!(f.row_of(5), 2);
    assert_eq!(f.row_of(3), 1); // hidden line → the region's row
    assert_eq!(f.next_visible(1, true, 10), Some(5));
    assert_eq!(f.next_visible(5, false, 10), Some(1));
}

#[test]
fn nested_and_all() {
    let mut f = folds();
    f.toggle(1);
    f.toggle(0);
    assert_eq!(f.visible_rows(10), 1);
    assert!(f.reveal(6));
    assert!(!f.is_hidden(6));
    f.collapse_all();
    assert_eq!(f.visible_rows(10), 1);
    f.expand_all();
    assert_eq!(f.visible_rows(10), 10);
}

#[test]
fn collapse_and_expand_around() {
    let mut f = folds();
    assert!(f.collapse_around(3)); // the innermost: 1..=4
    assert!(f.is_hidden(3) && !f.is_hidden(6));
    assert!(f.expand_around(3));
    assert!(!f.is_hidden(3));
}

#[test]
fn collapsed_state_survives_new_ranges_and_edits() {
    let mut f = folds();
    f.toggle(5);
    let change = TextChange {
        start: 0,
        old_end: 0,
        new_end: 10,
        start_point: kernel::TextPosition::new(0, 0),
        old_end_point: kernel::TextPosition::new(0, 0),
        new_end_point: kernel::TextPosition::new(1, 0),
    };
    f.map_changes(&[change]);
    // The new regions (one line lower), with anchors shifted by 10 bytes.
    f.set_available(&[FoldRange {
        start_byte: 510,
        end_byte: 999,
        start_line: 6,
        end_line: 9,
        placeholder: None,
    }]);
    assert!(f.is_hidden(7));
}

#[test]
fn a_hidden_line_is_shown_on_its_region_first_line() {
    let mut f = folds();
    assert_eq!(f.shown_line(3), 3);
    f.toggle(1);
    assert_eq!((f.shown_line(3), f.shown_line(1), f.shown_line(6)), (1, 1, 6));
    // Nested: the outermost collapsed region wins.
    f.toggle(0);
    assert_eq!(f.shown_line(3), 0);
}

/// The generation follows the hidden lines: it moves when a region collapses or expands,
/// not when the same regions arrive again from the syntax engine.
#[test]
fn the_generation_follows_the_hidden_lines() {
    let mut f = folds();
    let start = f.generation();
    f.set_available(&[fold(0, 9), fold(1, 4), fold(5, 8)]);
    assert_eq!(f.generation(), start, "nothing hidden changed");
    f.toggle(1);
    let collapsed = f.generation();
    assert_ne!(collapsed, start);
    assert_eq!(f.hidden_ranges(), &[(2, 4)]);
    f.set_available(&[fold(0, 9), fold(1, 4), fold(5, 8)]);
    assert_eq!(f.generation(), collapsed);
    f.expand_all();
    assert_ne!(f.generation(), collapsed);
    assert!(f.hidden_ranges().is_empty());
}

/// Collapse All on a large file, then new regions from a parse: the collapsed ones still
/// there stay, the others are forgotten — looked up, not compared each with each.
#[test]
fn many_collapsed_regions_follow_new_ranges() {
    let mut f = Folds::default();
    let all: Vec<FoldRange> = (0..20_000).map(|i| fold(i * 3, i * 3 + 2)).collect();
    f.set_available(&all);
    f.collapse_all();
    assert_eq!(f.visible_rows(60_000), 20_000);
    let half: Vec<FoldRange> = all.iter().step_by(2).cloned().collect();
    f.set_available(&half);
    assert_eq!(f.visible_rows(60_000), 40_000, "the remaining ones still collapsed");
    assert!(f.is_collapsed(&half[1]) && f.is_hidden(7) && !f.is_hidden(4));
}

/// A multi-cursor edit's many changes move the regions and the collapsed anchors as the
/// changes one by one do.
#[test]
fn many_changes_move_the_regions_as_one_by_one() {
    let text = "fn {\n  body\n}\n".repeat(60);
    let mut buffer = document::TextBuffer::new(&text);
    let mut f = Folds::default();
    let regions: Vec<FoldRange> = (0..60)
        .map(|i| FoldRange {
            start_byte: i * 14 + 3,
            end_byte: i * 14 + 12,
            start_line: i as u32 * 3,
            end_line: i as u32 * 3 + 2,
            placeholder: None,
        })
        .collect();
    f.set_available(&regions);
    f.collapse_all();
    // Text typed at the start of every line (40 cursors), in one transaction.
    let edits: Vec<document::Edit> =
        (0..40).map(|i| document::Edit::new(i * 14..i * 14, "x\n")).collect();
    let (_, changes) = document::Transaction::new(edits).apply(&mut buffer);
    assert!(changes.len() > MANY_CHANGES);
    let mut one_by_one = f.clone();
    for change in &changes {
        one_by_one.map_changes(std::slice::from_ref(change));
    }
    f.map_changes(&changes);
    assert_eq!(f.collapsed, one_by_one.collapsed);
    let bytes =
        |f: &Folds| f.available.iter().map(|r| (r.start_byte, r.end_byte)).collect::<Vec<_>>();
    assert_eq!(bytes(&f), bytes(&one_by_one));
}

#[test]
fn a_selection_reaching_a_collapsed_region_takes_it_whole() {
    use document::{Selection, Selections, TextBuffer};
    // 0: a   1: <div class="card">   2: x   3: </div>   4: b
    let buffer = TextBuffer::new("a\n<div class=\"card\">\nx\n</div>\nb");
    let mut f = Folds::default();
    f.set_available(&[FoldRange {
        start_byte: 2,
        end_byte: 32,
        start_line: 1,
        end_line: 3,
        placeholder: None,
    }]);
    let one = |f: &Folds, s: Selection| f.widen(&buffer, &Selections::single(s)).primary();
    let (tail, row_end) = (buffer.line_end(1), buffer.line_end(3));
    let row = Selection::new(buffer.line_start(1), tail);
    assert_eq!(one(&f, row), row, "expanded: as it was");
    assert!(f.toggle(1));
    assert_eq!(f.folded_lines(), vec![(1, 3)]);
    // The row's text selected up to its `⋯`: through `</div>`.
    assert_eq!(one(&f, row), Selection::new(row.anchor, row_end));
    // Backwards too, its direction kept.
    let back = Selection::new(tail, buffer.line_start(1));
    assert_eq!(one(&f, back), Selection::new(row_end, buffer.line_start(1)));
    // Short of the `⋯`, a cursor, or past the region: unchanged.
    let word = Selection::new(buffer.line_start(1), buffer.line_start(1) + 4);
    assert_eq!(one(&f, word), word);
    assert_eq!(one(&f, Selection::cursor(tail)), Selection::cursor(tail));
    let after = Selection::new(row_end + 1, buffer.len());
    assert_eq!(one(&f, after), after);
}

/// A row's line and a line's row, found by a binary search, are what walking every hidden
/// range gives: for many ranges, every line and every row, and back again.
#[test]
fn rows_and_lines_match_a_walk_over_the_hidden_ranges() {
    let walk_row = |hidden: &[(u32, u32)], line: u32| {
        let mut row = line;
        for (a, b) in hidden {
            if *b < line {
                row -= b - a + 1;
            } else if *a <= line {
                row -= line - a + 1;
                break;
            } else {
                break;
            }
        }
        row
    };
    let walk_line = |hidden: &[(u32, u32)], row: u32| {
        let mut line = row;
        for (a, b) in hidden {
            if *a <= line {
                line += b - a + 1;
            } else {
                break;
            }
        }
        line
    };
    // Regions of 1 to 5 hidden lines, every 7 lines, some next to each other.
    let folds: Vec<FoldRange> = (0..40u32)
        .map(|i| {
            let start = i * 7 + (i % 3);
            fold(start, start + 1 + i % 5)
        })
        .collect();
    let mut folds_state = Folds::default();
    folds_state.set_available(&folds);
    assert_eq!(folds_state.row_of(5), 5, "nothing collapsed");
    folds_state.collapse_all();
    let hidden = folds_state.hidden_ranges().to_vec();
    assert!(!hidden.is_empty());
    for line in 0..320 {
        assert_eq!(folds_state.row_of(line), walk_row(&hidden, line), "line {line}");
    }
    for row in 0..folds_state.visible_rows(320) {
        let line = folds_state.line_of(row);
        assert_eq!(line, walk_line(&hidden, row), "row {row}");
        assert_eq!(folds_state.row_of(line), row, "back to row {row}");
    }
    folds_state.expand_all();
    assert_eq!((folds_state.row_of(9), folds_state.line_of(9)), (9, 9));
}
