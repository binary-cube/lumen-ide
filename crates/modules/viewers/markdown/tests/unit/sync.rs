use std::collections::HashSet;
use std::path::Path;

use text_layout::TextSystem;
use ui_components::Palette;

use crate::layout::{Caches, Layout, Page};

fn layout_of(source: &str) -> Layout {
    let toggled = HashSet::new();
    let page = Page {
        source,
        version: 1,
        width: 800.0,
        scale: 1.0,
        dir: Path::new("/nowhere"),
        toggled: &toggled,
    };
    Layout::build(page, &mut Caches::inline(), &mut TextSystem::new(), &Palette::default())
}

fn top_of(l: &Layout, word: &str) -> f32 {
    l.runs.iter().find(|r| r.text.starts_with(word)).expect("the word").rect.min_y()
}

#[test]
fn each_block_starts_at_its_source_line_and_the_map_runs_both_ways() {
    let source = "# Title\n\nFirst paragraph.\n\nSecond paragraph.\n\nThird.\n";
    let l = layout_of(source);
    let m = &l.lines;
    for (word, line) in [("Title", 0.0), ("First", 2.0), ("Second", 4.0), ("Third", 6.0)] {
        let y = top_of(&l, word);
        assert!((m.line_at(y) - line).abs() < 0.01, "{word}: {}", m.line_at(y));
        // The first line is at the very top (the text at its start: the preview at its).
        let want = if line == 0.0 { 0.0 } else { y };
        assert!((m.y_of(line) - want).abs() < 0.5, "{word}");
    }
    assert_eq!(m.line_at(-10.0), 0.0);
    assert_eq!(m.line_at(l.height + 100.0), 7.0);
    assert_eq!(m.y_of(1000.0), l.height);
}

#[test]
fn a_long_block_is_spread_over_its_height() {
    let code: String = (0..40).map(|i| format!("line {i}\n")).collect();
    let source = format!("Intro.\n\n```\n{code}```\n\nAfter.\n");
    let l = layout_of(&source);
    let m = &l.lines;
    // The code's lines one under the other: line n of the code near its row.
    let row = |n: usize| top_of(&l, &format!("line {n}"));
    for n in [5usize, 20, 35] {
        let line = 3.0 + n as f32;
        assert!((m.y_of(line) - row(n)).abs() < 30.0, "line {n}: {} vs {}", m.y_of(line), row(n));
    }
    // Smooth: half a line further is between the two.
    let (a, b) = (m.y_of(10.0), m.y_of(11.0));
    assert!(m.y_of(10.5) > a && m.y_of(10.5) < b);
}

#[test]
fn a_diagram_taller_than_its_lines_takes_them_in_proportion() {
    let source =
        "Before.\n\n```mermaid\ngraph TD\n  A --> B\n  B --> C\n  C --> D\n```\n\nAfter.\n";
    let l = layout_of(source);
    let m = &l.lines;
    let (top, after) = (m.y_of(2.0), top_of(&l, "After"));
    assert!(after - top > 200.0, "a tall diagram");
    // Its lines (2..=7) share its height: the middle one about half way down.
    let mid = m.y_of(5.0);
    assert!(
        mid > top + (after - top) * 0.3 && mid < top + (after - top) * 0.8,
        "{top} {mid} {after}"
    );
    // Never back: lines grow with the height.
    let mut last = -1.0;
    for i in 0..=20 {
        let y = l.height * i as f32 / 20.0;
        let line = m.line_at(y);
        assert!(line >= last);
        last = line;
    }
}
