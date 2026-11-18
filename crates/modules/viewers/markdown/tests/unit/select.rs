use std::collections::HashSet;
use std::path::Path;

use text_layout::TextSystem;
use ui_components::Palette;

use super::*;
use crate::layout::{Caches, Layout, Page};

fn layout_of(source: &str, width: f32) -> Layout {
    let toggled = HashSet::new();
    let page = Page {
        source,
        version: 1,
        width,
        scale: 1.0,
        dir: Path::new("/nowhere"),
        toggled: &toggled,
    };
    Layout::build(page, &mut Caches::inline(), &mut TextSystem::new(), &Palette::default())
}

fn all(source: &str) -> String {
    let l = layout_of(source, 800.0);
    text(&l.runs, 0, total(&l.runs))
}

/// The offset where `word` starts in the runs' text (its first run holding it).
fn offset_of(runs: &[TextRun], word: &str) -> usize {
    let r = runs.iter().find(|r| r.text.contains(word)).expect("a run with the word");
    r.start + r.text.find(word).unwrap_or(0)
}

#[test]
fn blocks_are_copied_apart_by_empty_lines_and_wrapped_lines_joined() {
    let long = "word ".repeat(80);
    let source = format!("# Title\n\nFirst paragraph here.\n\n{long}\n\nlast  \nline");
    let l = layout_of(&source, 400.0);
    // The long paragraph took several lines.
    let tops: HashSet<i32> =
        l.runs.iter().filter(|r| r.text == "word ").map(|r| r.rect.min_y() as i32).collect();
    assert!(tops.len() > 3, "{tops:?}");
    let copied = text(&l.runs, 0, total(&l.runs));
    assert_eq!(
        copied,
        format!("Title\n\nFirst paragraph here.\n\n{}\n\nlast\nline", long.trim_end())
    );
}

#[test]
fn lists_keep_their_markers_and_depth() {
    assert_eq!(all("- one\n- two\n  - nested\n"), "• one\n• two\n  ◦ nested");
    assert_eq!(all("1. first\n2. second\n"), "1. first\n2. second");
    assert_eq!(all("- [x] done\n- [ ] to do\n"), "[x] done\n[ ] to do");
}

#[test]
fn table_cells_are_tab_separated_and_empty_ones_kept() {
    assert_eq!(all("| a | b |\n|---|---|\n| 1 | 2 |\n|   | 4 |\n"), "a\tb\n1\t2\n\t4");
}

#[test]
fn code_is_copied_verbatim() {
    let code = "fn main() {\n    let x = 1;\n\n    x\n}";
    assert_eq!(
        all(&format!("Before\n\n```rust\n{code}\n```\n\nAfter")),
        format!("Before\n\n{code}\n\nAfter")
    );
    // Scrolling across, it's the same text.
    let wide = format!("```\n{}\nend\n```", "y".repeat(300));
    assert_eq!(all(&wide), format!("{}\nend", "y".repeat(300)));
}

#[test]
fn a_range_across_blocks_copies_its_parts() {
    let l = layout_of("Alpha beta gamma.\n\nDelta epsilon.", 800.0);
    let a = offset_of(&l.runs, "beta");
    let b = offset_of(&l.runs, "epsilon");
    assert_eq!(text(&l.runs, a, b), "beta gamma.\n\nDelta");
    // The offsets under the points the words are drawn at.
    let at = |word: &str| {
        let r = l.runs.iter().find(|r| r.text.starts_with(word)).expect("run");
        Point::new(r.rect.min_x() + 1.0, r.rect.center().y)
    };
    let none = |_| 0.0;
    assert_eq!(offset_at(&l.runs, at("beta"), &none), a);
    assert_eq!(offset_at(&l.runs, at("epsilon"), &none), b);
    // Left of a line: its start; between the blocks: the next one's start; below: the end.
    let first = &l.runs[0];
    assert_eq!(offset_at(&l.runs, Point::new(0.0, first.rect.center().y), &none), 0);
    let delta = l.runs.iter().find(|r| r.text.starts_with("Delta")).expect("run");
    let gap = Point::new(first.rect.min_x(), (first.rect.max_y() + delta.rect.min_y()) / 2.0);
    assert_eq!(offset_at(&l.runs, gap, &none), delta.start);
    assert_eq!(offset_at(&l.runs, Point::new(10.0, 10_000.0), &none), total(&l.runs));
}

#[test]
fn a_double_click_takes_the_word_a_triple_the_block() {
    let l = layout_of("Hello **wor**ld, friend.\n\nOther block here.", 800.0);
    let inside = offset_of(&l.runs, "wor") + 1;
    let (a, b) = word_at(&l.runs, inside);
    assert_eq!(text(&l.runs, a, b), "world");
    let comma = offset_of(&l.runs, ",");
    let (a, b) = word_at(&l.runs, comma);
    assert_eq!(text(&l.runs, a, b), ",");
    let (a, b) = block_at(&l.runs, offset_of(&l.runs, "block"));
    assert_eq!(text(&l.runs, a, b), "Other block here.");
    let (a, b) = block_at(&l.runs, inside);
    assert_eq!(text(&l.runs, a, b), "Hello world, friend.");
}

#[test]
fn copy_as_markdown_takes_the_source_lines() {
    let source = "Intro.\n\n- **one**\n- two\n\nEnd.";
    let l = layout_of(source, 800.0);
    let a = offset_of(&l.runs, "one");
    let b = offset_of(&l.runs, "two") + 1;
    assert_eq!(markdown(&l.runs, source, a, b).as_deref(), Some("- **one**\n- two"));
    assert_eq!(markdown(&l.runs, source, 0, total(&l.runs)).as_deref(), Some(source));
}

#[test]
fn formulas_and_diagrams_are_selected_whole_and_copy_their_source() {
    let source = "Area $\\pi r^2$ here.\n\n```mermaid\ngraph LR\n  A --> B\n```\n\nAfter.";
    let l = layout_of(source, 800.0);
    let copied = text(&l.runs, 0, total(&l.runs));
    assert_eq!(copied, "Area $\\pi r^2$ here.\n\ngraph LR\n  A --> B\n\nAfter.");
    let whole = l.runs.iter().find(|r| r.copy.starts_with("graph")).expect("the diagram");
    assert_eq!(whole.len(), 1);
    assert_eq!(word_at(&l.runs, whole.start), (whole.start, whole.end()));
}

#[test]
fn selection_boxes_join_a_line_and_follow_code_scroll() {
    let l = layout_of("one two three\n\n```\nabc\n```", 800.0);
    let lines = boxes(&l.runs, 0, total(&l.runs), |_| true);
    // One box for the line of words, one for the code line.
    assert_eq!(lines.len(), 2, "{lines:?}");
    let words: f32 = l.runs.iter().take(3).map(|r| r.rect.size.width).sum();
    assert!((lines[0].0.size.width - words).abs() < 1.0);
    // Part of a word: from its first letter to the cut.
    let half = boxes(&l.runs, 1, 2, |_| true);
    assert_eq!(half.len(), 1);
    assert!(half[0].0.min_x() > l.runs[0].rect.min_x() && half[0].0.size.width > 0.0);
}
