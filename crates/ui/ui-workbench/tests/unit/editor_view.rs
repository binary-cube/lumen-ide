use super::*;

#[test]
fn tab_expansion_and_offset_mapping() {
    let mut out = String::new();
    expand_tabs("\tab", 80, 4, &mut out);
    assert_eq!(out, "    ab");
    expand_tabs("abcdef", 3, 4, &mut out);
    assert_eq!(out, "abc");

    assert_eq!(source_to_display("\tab", 1, 4), 4);
    assert_eq!(source_to_display("ăb", 2, 4), 2);
    assert_eq!(display_to_source("\tab", 4, 4), 1);
    assert_eq!(display_to_source("\tab", 1, 4), 0);
    assert_eq!(display_to_source("\tab", 3, 4), 1);
    assert_eq!(display_to_source("ab", 99, 4), 2);

    let mut map = Vec::new();
    build_display_map("a\tb", 4, &mut map);
    assert_eq!(map, vec![0, 1, 1, 1, 2]);
}

/// Warnings moved in the middle of a list at the same document version (same length, same
/// first and last offsets) still change the strip's key: the lists' generation is in it.
#[test]
fn the_strip_marks_follow_lists_recomputed_at_the_same_version() {
    let before = [0..1, 10..12, 40..41];
    let after = [0..1, 25..27, 40..41];
    let key = |generation, list: &[Range<usize>]| mark_key(7, generation, [list, &[], &[], &[]]);
    assert_eq!(key(1, &before), key(1, &after), "the summary alone can't tell them apart");
    assert_ne!(key(1, &before), key(2, &after));
}

/// After a jump, a line off screen comes to the middle; a line already well in view stays
/// where it is (the screen doesn't move for nothing).
#[test]
fn a_jump_brings_the_line_to_the_middle_unless_it_s_in_view() {
    let mut text = TextSystem::new();
    let palette = Palette::dark();
    let mut doc = Document::new(&"line\n".repeat(400));
    let folds = Folds::default();
    let mut view = EditorView::new(FontFamily::Monospace);
    view.set_bounds(Rect::new(0.0, 0.0, 800.0, 600.0), 1.0);
    let line_height = view.layout(&mut text, &doc, &palette).line_height;
    let row_of_cursor = |view: &EditorView| {
        let middle = view.scroll_y + 300.0;
        (middle / line_height).floor() as usize
    };
    let at = |doc: &mut Document, line: usize| {
        let offset = doc.buffer().line_start(line);
        document::commands::click(doc, offset, false, false);
    };
    at(&mut doc, 300);
    view.center_cursor(&doc, &folds, &mut text, &palette);
    assert!(row_of_cursor(&view).abs_diff(300) <= 1, "off screen: in the middle");
    let scrolled = view.scroll_y;
    at(&mut doc, 297);
    view.center_cursor(&doc, &folds, &mut text, &palette);
    assert_eq!(view.scroll_y, scrolled, "in view: not moved");
    // An ordinary move keeps the old behavior: just enough to show it, at the edge.
    at(&mut doc, 350);
    view.reveal_cursor(&doc, &folds, &mut text, &palette);
    assert!(row_of_cursor(&view) < 350, "revealed at the bottom edge, not centered");
}

/// Decorations with nothing on them but `folds`.
fn plain(folds: &Folds) -> Decorations<'_> {
    Decorations {
        colors: &[],
        matches: &[],
        current_match: None,
        folds,
        errors: &[],
        deprecated: &[],
        warnings: &[],
        unused: &[],
        link: None,
        occurrences: &[],
        swatches: &[],
        statements: &[],
        highlight: None,
        current: None,
        focus: None,
        vcs: &[],
        annotations: &[],
        marks_generation: 0,
        bulb: None,
        lenses: &[],
        concealed: &[],
    }
}

fn paint_once(view: &mut EditorView, doc: &Document, folds: &Folds, text: &mut TextSystem) {
    let mut scene = Scene::default();
    view.paint(doc, plain(folds), true, text, &mut scene, &Palette::dark());
}

/// The widest line in display columns: tabs to their stops, wide characters as two, the
/// lines of collapsed regions left out.
#[test]
fn the_longest_line_counts_display_columns() {
    let cols =
        |text: &str, hidden: &[(u32, u32)]| longest_line_columns([text].into_iter(), 4, hidden);
    assert_eq!(cols("ab\n\tx\nabcde", &[]), 5);
    assert_eq!(cols("a\tb", &[]), 5, "the tab goes to column 4");
    assert_eq!(cols("中文", &[]), 4, "wide characters take two columns");
    assert_eq!(cols("ăîș", &[]), 3);
    assert_eq!(cols("a\nlong line here\nb", &[(1, 1)]), 1, "a hidden line doesn't count");
    assert_eq!(cols("", &[]), 0);
    assert_eq!(cols("a中\tb", &[]), 5, "a wide character moves the tab stop");
    // Over chunks split anywhere between characters.
    assert_eq!(longest_line_columns(["ab", "c\nd", "efgh"].into_iter(), 4, &[]), 5);
}

/// Scrolling down past a long line keeps the horizontal range (the bar doesn't come and go):
/// it's the document's widest line, not the visible rows'.
#[test]
fn scrolling_past_a_long_line_keeps_the_horizontal_range() {
    let mut text = TextSystem::new();
    let src = format!("{}\n{}", "x".repeat(400), "short\n".repeat(400));
    let doc = Document::new(&src);
    let folds = Folds::default();
    let mut view = EditorView::new(FontFamily::Monospace);
    view.set_bounds(Rect::new(0.0, 0.0, 800.0, 600.0), 1.0);
    paint_once(&mut view, &doc, &folds, &mut text);
    let range = view.max_scroll_x;
    assert!(range > 0.0, "the long line is wider than the view");
    // Far below it, only short lines in view.
    view.scroll_by(0.0, 4000.0, &doc, &folds);
    paint_once(&mut view, &doc, &folds, &mut text);
    assert!(view.scroll_y > 1000.0);
    assert_eq!(view.max_scroll_x, range, "the same range with the long line out of view");
    // And it scrolls right from there.
    assert!(view.scroll_by(200.0, 0.0, &doc, &folds));
    assert_eq!(view.scroll_x, 200.0);
}

/// Deleting the long line shrinks the range, the scroll brought back within it.
#[test]
fn deleting_the_long_line_shrinks_the_range() {
    let mut text = TextSystem::new();
    let src = format!("{}\n{}", "x".repeat(400), "short\n".repeat(50));
    let mut doc = Document::new(&src);
    let folds = Folds::default();
    let mut view = EditorView::new(FontFamily::Monospace);
    view.set_bounds(Rect::new(0.0, 0.0, 800.0, 600.0), 1.0);
    paint_once(&mut view, &doc, &folds, &mut text);
    view.scroll_by(10_000.0, 0.0, &doc, &folds);
    assert!((view.scroll_x - view.max_scroll_x).abs() < 1.0, "scrolled to the end");
    assert!(view.scroll_x > 0.0);
    document::commands::replace_range(&mut doc, 0..401, "");
    paint_once(&mut view, &doc, &folds, &mut text);
    assert_eq!(view.max_scroll_x, 0.0, "every line fits now");
    assert_eq!(view.scroll_x, 0.0, "the scroll is back within the range");
}

/// The widest line is worked out once per text (and tab width, folds), not on every frame.
#[test]
fn the_widest_line_is_not_worked_out_per_frame() {
    let mut text = TextSystem::new();
    let mut doc = Document::new(&format!("{}\nb\n", "a".repeat(300)));
    let folds = Folds::default();
    let mut view = EditorView::new(FontFamily::Monospace);
    view.set_bounds(Rect::new(0.0, 0.0, 800.0, 600.0), 1.0);
    for _ in 0..5 {
        paint_once(&mut view, &doc, &folds, &mut text);
        view.scroll_by(30.0, 10.0, &doc, &folds);
    }
    assert_eq!(view.width_scans, 1, "five frames and scrolls, one count");
    let widest = |view: &EditorView| view.longest_line.value().copied();
    // Typing on the widest line, or elsewhere: only the changed lines measured.
    document::commands::click(&mut doc, 0, false, false);
    document::commands::insert_text(&mut doc, "z");
    paint_once(&mut view, &doc, &folds, &mut text);
    paint_once(&mut view, &doc, &folds, &mut text);
    assert_eq!(widest(&view), Some((301, 0)));
    let end = doc.buffer().len();
    document::commands::click(&mut doc, end, false, false);
    document::commands::insert_text(&mut doc, &format!("{}\n", "c".repeat(400)));
    paint_once(&mut view, &doc, &folds, &mut text);
    assert_eq!(widest(&view), Some((400, 2)), "a new widest line");
    assert_eq!(view.width_scans, 1, "edits: never the whole text counted");
    // The widest line made shorter: the whole text counted again, once.
    let start = doc.buffer().line_start(2);
    document::commands::replace_range(&mut doc, start..start + 400, "c");
    paint_once(&mut view, &doc, &folds, &mut text);
    assert_eq!(widest(&view), Some((301, 0)));
    assert_eq!(view.width_scans, 2, "counted again once");
}

/// The widest line after edits, worked out from the changed lines, is the whole count's:
/// over lines added, removed, joined and split, before and after the widest one.
#[test]
fn the_widest_line_after_edits_is_the_whole_count_s() {
    let mut doc =
        Document::new(&(0..60).map(|i| "x".repeat(i * 7 % 50)).collect::<Vec<_>>().join("\n"));
    let mut seed: u64 = 3;
    let mut next = |n: usize| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) as usize % n.max(1)
    };
    let pieces =
        ["", "\n", "yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy", "a\nb", "\t\t"];
    let mut widest = widest_line(doc.buffer().chunks(), 4, &[]);
    for step in 0..400 {
        let version = doc.version();
        let len = doc.buffer().len();
        let a = next(len + 1);
        let b = (a + next(30)).min(len);
        document::commands::replace_range(&mut doc, a..b, pieces[next(pieces.len())]);
        let changes = doc.changes_since(version).unwrap();
        let whole = widest_line(doc.buffer().chunks(), 4, &[]);
        widest = match widest_after(doc.buffer(), &changes, widest, 4, &[]) {
            Some(w) => {
                assert_eq!(w.0, whole.0, "step {step}");
                let line = doc.buffer().line_text(w.1 as usize);
                assert_eq!(longest_line_columns([line.as_str()].into_iter(), 4, &[]), w.0);
                w
            }
            None => whole,
        };
    }
}

/// A message or a code written over lines (a PHP parameter split after its `$`) is drawn
/// on one line: exactly as the same texts with the newline made a space, nothing drawn
/// back at the start over the first part.
#[test]
fn the_error_tooltip_is_drawn_on_one_line() {
    let mut text = TextSystem::new();
    let palette = Palette::dark();
    let mut view = EditorView::new(FontFamily::Monospace);
    view.set_bounds(Rect::new(0.0, 0.0, 1400.0, 600.0), 1.0);
    let paint = |message: &str, code: &str, text: &mut TextSystem| {
        let mut scene = Scene::default();
        let tip = ErrorTip { message, code: Some(code), warning: true, weak: false };
        view.paint_error_tooltip(Point::new(20.0, 20.0), tip, text, &mut scene, &palette);
        scene
    };
    let split = paint(
        "Optional parameter $\n    a comes before a required one",
        "$\n        a = 1",
        &mut text,
    );
    let flat = paint("Optional parameter $ a comes before a required one", "$ a = 1", &mut text);
    let layers = |s: &Scene| {
        s.layers().iter().map(|l| (l.quads.clone(), l.glyphs.clone())).collect::<Vec<_>>()
    };
    assert_eq!(layers(&split), layers(&flat));
    // Along one baseline, left to right.
    let glyphs: Vec<_> = split.layers().iter().flat_map(|l| l.glyphs.iter()).collect();
    assert!(glyphs.len() > 20);
    let message: Vec<_> = glyphs.iter().filter(|g| g.color == palette.text).collect();
    assert!(message.windows(2).all(|w| w[1].origin.x > w[0].origin.x), "no text drawn back over");
    assert!(message.windows(2).all(|w| w[1].origin.y == w[0].origin.y));
}

/// Working out the widest line stays cheap on a large file (it's done once per edit).
#[test]
fn the_widest_line_of_a_large_file_is_cheap() {
    let line = "    $value = $this->compute($a, $b); // ăîș comment\n";
    let doc = Document::new(&line.repeat(1_000_000 / line.len()));
    let start = std::time::Instant::now();
    let columns = longest_line_columns(doc.buffer().chunks(), 4, &[]);
    let took = start.elapsed();
    assert_eq!(columns, line.chars().count() - 1);
    // About 1 ms in release; the limit leaves room for debug builds and a loaded machine.
    assert!(took < std::time::Duration::from_millis(150), "{took:?}");
}

/// Inlay hints get room in their row (spaces in the displayed text, mapped to the byte
/// they're before): a byte an inlay is before is shown after it, a point on the inlay is
/// that byte, tabs still go to their stops.
#[test]
fn inlays_make_room_without_moving_the_text_around_them() {
    let line = "f(1, 2)";
    // `a:` (3 columns) before `1`, `b:` before `2`.
    let inlays = [(2, 3), (5, 3)];
    let mut out = String::new();
    expand_tabs_with(line, 80, 4, &inlays, &mut out);
    assert_eq!(out, "f(   1,    2)");
    assert_eq!(source_to_display_with(line, 2, 4, &inlays), 5, "the `1` after its hint");
    assert_eq!(source_to_display_with(line, 1, 4, &inlays), 1);
    assert_eq!(source_to_display_with(line, 7, 4, &inlays), 13, "the end");
    for disp in 2..5 {
        assert_eq!(display_to_source_with(line, disp, 4, &inlays), 2, "on the hint: {disp}");
    }
    assert_eq!(display_to_source_with(line, 6, 4, &inlays), 3);
    let mut map = Vec::new();
    build_display_map_with(line, 4, &inlays, &mut map);
    assert_eq!(map.len(), out.len());
    assert_eq!(&map[2..6], &[2, 2, 2, 2]);
    // At the line's end, and with a tab after it.
    expand_tabs_with("ab", 80, 4, &[(2, 2)], &mut out);
    assert_eq!(out, "ab  ");
    expand_tabs_with("a\tb", 80, 4, &[(0, 2)], &mut out);
    assert_eq!(out, "  a b", "the tab still to its stop");
}

/// Hints, a lens and the bulb painted: the caret's place counts the hint before it, the
/// lens and the bulb are where a click finds them.
#[test]
fn hints_lenses_and_the_bulb_are_painted_where_clicks_find_them() {
    let mut text = TextSystem::new();
    let palette = Palette::dark();
    let mut view = EditorView::new(FontFamily::Monospace);
    view.set_bounds(Rect::new(0.0, 0.0, 800.0, 400.0), 1.0);
    let mut doc = Document::new("total(1, 2);\nnext();\n");
    let folds = Folds::default();
    document::commands::click(&mut doc, 6, false, false);
    let before = view.caret_rect(&doc, &folds, &mut text, &palette);
    view.set_inlays(std::sync::Arc::from(vec![Inlay { offset: 6, label: "a:".into() }]));
    let after = view.caret_rect(&doc, &folds, &mut text, &palette);
    assert!(after.min_x() > before.min_x() + 10.0, "the caret after the hint");
    let lenses = [LineLens { line: 1, parts: vec!["2 usages".into(), "1 implementation".into()] }];
    let deco =
        Decorations { bulb: Some(Bulb { line: 0, fix: true }), lenses: &lenses, ..plain(&folds) };
    let mut scene = Scene::default();
    view.paint(&doc, deco, true, &mut text, &mut scene, &palette);
    let row1 = view.line_height() * 1.5;
    let parts: Vec<(usize, usize)> =
        (0..800).filter_map(|x| view.lens_at(Point::new(x as f32, row1))).collect();
    assert_eq!(parts.first(), Some(&(0, 0)));
    assert_eq!(parts.last(), Some(&(0, 1)), "each part clicked on its own");
    let half = view.line_height() / 2.0;
    assert!((0..100).any(|x| view.bulb_at(Point::new(x as f32, half))), "the bulb in the gutter");
    // A click on the hint is the byte after it.
    let hint_x = after.min_x() - 4.0;
    let at = view.offset_at(Point::new(hint_x, half), &doc, &folds, &mut text, &palette);
    assert_eq!(at, 6);
}

#[test]
fn the_top_line_is_read_and_set_in_lines() {
    let mut text = TextSystem::new();
    let palette = Palette::dark();
    let doc = Document::new(&"line\n".repeat(400));
    let folds = Folds::default();
    let mut view = EditorView::new(FontFamily::Monospace);
    view.set_bounds(Rect::new(0.0, 0.0, 800.0, 600.0), 1.0);
    let line_height = view.layout(&mut text, &doc, &palette).line_height;
    assert_eq!(view.top_line(&folds), 0.0);
    assert!(view.scroll_to_line(120.0, &doc, &folds));
    assert_eq!(view.scroll_y, (120.0 * line_height).round());
    assert!((view.top_line(&folds) - 120.0).abs() < 0.1);
    assert!(!view.scroll_to_line(120.0, &doc, &folds), "already there");
    // Past the end: as far as it scrolls.
    view.scroll_to_line(10_000.0, &doc, &folds);
    assert!(view.top_line(&folds) <= 400.0);
}

/// The scrollbars are hidden at rest: a scroll shows them (a frame asked until they fade),
/// at rest again they're gone and no frame is asked.
#[test]
fn the_scrollbars_show_while_scrolled_and_go_at_rest() {
    let mut text = TextSystem::new();
    let doc = Document::new(&"a line of code\n".repeat(400));
    let folds = Folds::default();
    let mut view = EditorView::new(FontFamily::Monospace);
    view.set_bounds(Rect::new(0.0, 0.0, 800.0, 600.0), 1.0);
    paint_once(&mut view, &doc, &folds, &mut text);
    let now = std::time::Instant::now();
    let held = |v: &EditorView| v.vauto.held(v.vbar.is_hovered(), v.vbar.is_dragging());
    if view.vauto.hides() {
        assert_eq!(view.vauto.opacity_at(now, held(&view)), 0.0, "hidden at rest");
        assert_eq!(view.scrollbar_deadline(), None, "no frames at rest");
        view.scroll_by(0.0, 400.0, &doc, &folds);
        paint_once(&mut view, &doc, &folds, &mut text);
        assert!(view.scrollbar_deadline().is_some(), "shown: frames until it fades");
        let later = std::time::Instant::now() + std::time::Duration::from_secs(10);
        assert_eq!(view.vauto.opacity_at(later, held(&view)), 0.0, "gone after");
    }
}

/// Bold / italic runs on several rows: each row's plain glyphs of a run are replaced in
/// place, the rows before it left whole: each row has as many glyphs as without the faces
/// (an italic glyph may sit a pixel aside, as its face places it).
#[test]
fn restyled_runs_replace_only_their_own_glyphs() {
    let mut text = TextSystem::new();
    let doc = Document::new("abc def\nghi jkl\nmno pqr\nstu");
    let folds = Folds::default();
    let span =
        |range: Range<usize>| ColorSpan { range, color: Rgba::WHITE, bold: false, italic: true };
    let colors = [span(4..7), span(16..19), span(24..27)];
    let places = |view: &mut EditorView, text: &mut TextSystem, colors: &[ColorSpan]| {
        let mut scene = Scene::default();
        let deco = Decorations { colors, ..plain(&folds) };
        view.paint(&doc, deco, true, text, &mut scene, &Palette::dark());
        let mut rows = std::collections::BTreeMap::<i64, usize>::new();
        for g in scene.layers().iter().flat_map(|l| l.glyphs.iter()) {
            *rows.entry(g.origin.y.round() as i64).or_default() += 1;
        }
        rows
    };
    let mut view = EditorView::new(FontFamily::Monospace);
    view.set_bounds(Rect::new(0.0, 0.0, 800.0, 600.0), 1.0);
    let plain_places = places(&mut view, &mut text, &[]);
    let styled_places = places(&mut view, &mut text, &colors);
    assert!(!plain_places.is_empty());
    assert_eq!(styled_places, plain_places, "every glyph once, on its row");
}

/// A caret's x on a very long line (a minified file's) is the whole line's, shaped only up
/// to a step past it.
#[test]
fn a_caret_on_a_long_line_is_placed_as_on_the_whole_line() {
    let mut text = TextSystem::new();
    let line = "a\tbc ș中 ".repeat(800);
    let doc = Document::new(&line);
    let folds = Folds::default();
    let mut view = EditorView::new(FontFamily::Monospace);
    view.set_bounds(Rect::new(0.0, 0.0, 800.0, 600.0), 1.0);
    paint_once(&mut view, &doc, &folds, &mut text);
    view.load_line(&doc, 0);
    let mut whole = String::new();
    expand_tabs_with(&view.line, usize::MAX, doc.tab_width(), &view.line_inlays, &mut whole);
    let font = Font::new(view.font_family.clone(), view.font_size_px);
    let full = text.shape_line(&whole, &font);
    for col in [1, 2, 7, 3000, 6000, line.len() - 1, line.len()] {
        let col = doc.buffer().floor_char_boundary(col);
        let disp = source_to_display_with(&view.line, col, doc.tab_width(), &view.line_inlays);
        let x = view.x_in_line(&mut text, &doc, col);
        assert!((x - full.x_at(disp)).abs() < 0.01, "column {col}: {x} / {}", full.x_at(disp));
        assert!(view.display.len() < whole.len() || col + 2500 > line.len(), "shaped in part");
    }
}

/// A minified file's long row is highlighted only up to the view's right edge (in steps),
/// the short rows around it whole; an ordinary file has its rows highlighted whole.
#[test]
fn long_rows_are_highlighted_up_to_the_view_s_edge() {
    let mut text = TextSystem::new();
    let folds = Folds::default();
    let mut view = EditorView::new(FontFamily::Monospace);
    view.set_bounds(Rect::new(0.0, 0.0, 800.0, 600.0), 1.0);
    let ordinary = Document::new(&"short line\n".repeat(100));
    paint_once(&mut view, &ordinary, &folds, &mut text);
    assert_eq!(view.long_line_windows(&ordinary, &folds), None);
    let src = format!("<?php\n// head\n{}\n$a = 1;\n", "x".repeat(200_000));
    let doc = Document::new(&src);
    paint_once(&mut view, &doc, &folds, &mut text);
    let windows = view.long_line_windows(&doc, &folds).expect("a long row in view");
    let buffer = doc.buffer();
    assert_eq!(windows[0].start, 0, "the short rows before it, with its drawn part");
    assert!(windows[0].end > buffer.line_start(2) && windows[0].end < buffer.line_end(2));
    assert!(windows[0].end - buffer.line_start(2) <= LONG_LINE);
    assert_eq!(windows.last().map(|w| w.end), Some(buffer.len()), "the rows after, whole");
    // A little scroll to the right keeps them (the same step).
    view.scroll_by(40.0, 0.0, &doc, &folds);
    assert_eq!(view.long_line_windows(&doc, &folds), Some(windows));
}

/// The lane on the right: its thumb pressed holds the mouse, dragged down the text follows
/// (whole pixels), let go nothing follows and the bar stays a moment before it fades.
#[test]
fn the_lane_s_thumb_is_dragged() {
    let mut text = TextSystem::new();
    let doc = Document::new(&"a line of code\n".repeat(400));
    let folds = Folds::default();
    let mut view = EditorView::new(FontFamily::Monospace);
    view.set_bounds(Rect::new(0.0, 0.0, 800.0, 600.0), 1.0);
    paint_once(&mut view, &doc, &folds, &mut text);
    let lane = view.vbar.track_rect();
    let thumb = Point::new(lane.min_x() + lane.size.width / 2.0, lane.min_y() + 4.0);
    view.hover(thumb, &doc, &folds);
    assert!(view.scrollbar_press(thumb, &doc, &folds), "the lane takes the press");
    assert!(view.scrollbar_drag(Point::new(thumb.x, thumb.y + 120.0), &doc, &folds));
    assert!(view.scroll_y > 0.0, "the text followed");
    assert_eq!(view.scroll_y, view.scroll_y.round(), "whole pixels");
    assert!(view.scrollbar_release());
    assert!(!view.scrollbar_drag(Point::new(thumb.x, thumb.y + 300.0), &doc, &folds));
}

/// To a screen reader the editor is one text area of the lines it shows (never the whole
/// file), the caret in them, its tokens and line numbers not read loose; its text can't be
/// replaced whole.
#[test]
fn the_editor_is_a_text_area_of_the_lines_shown() {
    use ui_kit::access::Role;
    let mut text = TextSystem::new();
    let src: String = (0..400).map(|i| format!("line {i}\n")).collect();
    let mut doc = Document::new(&src);
    let folds = Folds::default();
    let mut view = EditorView::new(FontFamily::Monospace);
    view.set_bounds(Rect::new(0.0, 0.0, 800.0, 300.0), 1.0);
    // The caret after "li" on the second line.
    let offset = doc.buffer().line_start(1) + 2;
    document::commands::click(&mut doc, offset, false, false);
    let recording = ui_kit::access::begin();
    paint_once(&mut view, &doc, &folds, &mut text);
    let frame = recording.take();
    let area = frame.nodes().iter().find(|n| n.role == Role::TextArea).expect("the editor");
    let value = area.value.as_deref().expect("its text");
    assert!(value.starts_with("line 0\nline 1\n"));
    assert!(!value.contains("line 300"), "only what it shows");
    let lines = area.lines.as_ref().expect("its lines");
    assert_eq!(lines.lines.len(), value.split('\n').count());
    assert_eq!((lines.anchor, lines.focus), ("line 0\n".len() + 2, "line 0\n".len() + 2));
    assert!(lines.caret.is_some() && area.state.focused);
    assert!(!area.actions.set_value);
    assert!(
        !frame.nodes().iter().any(|n| n.role == Role::Label && n.name.starts_with("line")),
        "no loose tokens"
    );
}

/// A view wrapping lines, a narrow one (its text area about 30 columns).
fn wrapping(text: &mut TextSystem, doc: &Document, folds: &Folds) -> EditorView {
    let mut view = EditorView::new(FontFamily::Monospace);
    view.set_text_options(&ui_components::TextOptions { word_wrap: true });
    let advance = view.layout(text, doc, &Palette::dark()).advance;
    let gutter = view.layout(text, doc, &Palette::dark()).gutter_width;
    let width = gutter + advance * 32.0 + scroll::lane_width(1.0);
    view.set_bounds(Rect::new(0.0, 0.0, width, 400.0), 1.0);
    paint_once(&mut view, doc, folds, text);
    view
}

/// Word wrap on: a long line takes the rows it needs (each its part of the line, nothing
/// lost), the later ones without a number but the wrap's marks; nothing scrolls sideways.
#[test]
fn a_long_line_wraps_into_rows_with_their_marks() {
    let mut text = TextSystem::new();
    let line = "the quick brown fox jumps over the lazy dog and runs far away\nnext";
    let doc = Document::new(line);
    let folds = Folds::default();
    let mut view = wrapping(&mut text, &doc, &folds);
    assert!(view.wraps());
    let first = &line[..line.find('\n').unwrap_or(0)];
    let parts: Vec<&str> = view
        .segs
        .iter()
        .zip(&view.lines)
        .filter(|(_, l)| **l == 0)
        .map(|(s, _)| &first[s.from..s.to])
        .collect();
    assert!(parts.len() >= 2, "{parts:?}");
    assert_eq!(parts.concat(), first);
    assert!(view.segs[0].first && !view.segs[0].last && !view.segs[1].first);
    assert_eq!(view.max_scroll_x, 0.0);
    let mut scene = Scene::default();
    view.paint(&doc, plain(&folds), true, &mut text, &mut scene, &Palette::dark());
    let font = view.font(&Palette::dark());
    let mut mark = |m: &str| text.shape_line(m, &font).glyphs[0].key;
    let (end, goes_on) = (mark(text::WRAP_END), mark(text::WRAP_GOES_ON));
    let glyphs: Vec<_> =
        scene.layers().iter().flat_map(|l| l.glyphs.iter().map(|g| g.key)).collect();
    assert!(glyphs.contains(&end) && glyphs.contains(&goes_on), "both marks drawn");
    // Off again: one row a line, scrolled sideways as before.
    view.set_text_options(&ui_components::TextOptions { word_wrap: false });
    paint_once(&mut view, &doc, &folds, &mut text);
    assert_eq!(view.lines, [0, 1]);
}

/// While wrapping, a click on a later row lands in its part of the line; ↓ moves to the
/// next row at the same place (not the next line), past the last to the end.
#[test]
fn clicks_and_moves_follow_the_rows() {
    let mut text = TextSystem::new();
    let palette = Palette::dark();
    let line = "the quick brown fox jumps over the lazy dog and runs far away\nnext";
    let doc = Document::new(line);
    let folds = Folds::default();
    let mut view = wrapping(&mut text, &doc, &folds);
    let seg = view.segs[1];
    let lh = view.line_height();
    let left = view.bounds.min_x() + view.layout(&mut text, &doc, &palette).text_left;
    let at = view.offset_at(Point::new(left + 1.0, lh * 1.5), &doc, &folds, &mut text, &palette);
    assert_eq!(at, seg.from, "the second row's start");
    let (down, goal) = view.moved_by_rows(
        (&doc, &folds),
        (&mut text, &palette),
        RowMove { head: 2, goal: None, rows: 1 },
    );
    assert!(view.segs[1].holds(down) && down >= seg.from, "the next row: {down}");
    let (again, _) = view.moved_by_rows(
        (&doc, &folds),
        (&mut text, &palette),
        RowMove { head: down, goal: Some(goal), rows: 1 },
    );
    assert!(again > down);
    let rows = i64::from(view.total_rows(&doc, &folds));
    let (end, _) = view.moved_by_rows(
        (&doc, &folds),
        (&mut text, &palette),
        RowMove { head: 0, goal: None, rows: rows + 5 },
    );
    assert_eq!(end, doc.buffer().len());
}

/// While wrapping, Home / End go to the row's start and end first; on a line's first
/// row Home, on its last row End, and a second press are the line's own (`None`).
#[test]
fn home_and_end_go_to_the_row_first() {
    let mut text = TextSystem::new();
    let line = "the quick brown fox jumps over the lazy dog and runs far away\nnext";
    let doc = Document::new(line);
    let folds = Folds::default();
    let mut view = wrapping(&mut text, &doc, &folds);
    let second = view.segs[1];
    let inside = second.from + 2;
    assert_eq!(view.row_home_end(&doc, &folds, inside, false), Some(second.from));
    assert_eq!(view.row_home_end(&doc, &folds, second.from, false), None, "a second press");
    assert_eq!(view.row_home_end(&doc, &folds, 1, false), None, "the first row: the line's");
    let end = view.row_home_end(&doc, &folds, 1, true).expect("the first row's end");
    assert!(end < view.segs[0].to && end >= view.segs[0].from);
    let last_row = view.segs.iter().position(|s| s.last).expect("its last row");
    let on_last = view.segs[last_row].from + 1;
    assert_eq!(view.row_home_end(&doc, &folds, on_last, true), None, "the last row: the line's");
}

/// A very long line while wrapping: its parts are wrapped once, the cursor's row found
/// among them (never the line wrapped again per row).
#[test]
#[allow(clippy::disallowed_methods)] // The budget is measured by the test, not the view.
fn a_very_long_wrapped_line_stays_quick() {
    let mut text = TextSystem::new();
    let palette = Palette::dark();
    let line = "word ".repeat(100_000);
    let doc = Document::new(&line);
    let folds = Folds::default();
    let mut view = wrapping(&mut text, &doc, &folds);
    let started = std::time::Instant::now();
    let (row, seg) = view.row_of_offset(&doc, &folds, line.len() - 3);
    assert!(row > 1000 && seg.last, "the last of many rows: {row}");
    for _ in 0..50 {
        view.caret_rect(&doc, &folds, &mut text, &palette);
    }
    assert!(started.elapsed() < std::time::Duration::from_secs(2), "{:?}", started.elapsed());
}

/// A wrapped line's later rows start at its indentation (under its text): a click there
/// lands in the row's part, the caret is drawn where it is.
#[test]
fn wrapped_rows_hang_at_the_line_s_indentation() {
    let mut text = TextSystem::new();
    let palette = Palette::dark();
    let line = "        the quick brown fox jumps over the lazy dog and runs far away\nnext";
    let doc = Document::new(line);
    let folds = Folds::default();
    let mut view = wrapping(&mut text, &doc, &folds);
    assert!(view.segs.len() > 2 && !view.segs[1].first);
    assert_eq!(view.segs[1].hang, 8, "under the text, at its indentation");
    let layout = view.layout(&mut text, &doc, &palette);
    let seg = view.segs[1];
    let left = view.bounds.min_x() + layout.text_left;
    let lh = view.line_height();
    // Just past where the row starts (its hang): its part's start.
    let at = Point::new(left + 8.0 * layout.advance + 1.0, lh * 1.5);
    assert_eq!(view.offset_at(at, &doc, &folds, &mut text, &palette), seg.from);
    let mut doc = doc;
    document::commands::click(&mut doc, seg.from, false, false);
    let caret = view.caret_rect(&doc, &folds, &mut text, &palette);
    assert!((caret.min_x() - (left + 8.0 * layout.advance)).abs() < 1.0, "{caret:?}");
}
