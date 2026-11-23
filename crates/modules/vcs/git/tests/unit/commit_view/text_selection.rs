use super::*;

fn line(kind: LineKind, text: &str) -> version_control::staging::DiffLine {
    version_control::staging::DiffLine { kind, old: Some(1), new: Some(1), text: text.into() }
}

fn view() -> CommitView {
    let lines = vec![line(LineKind::Context, "\tone\n"), line(LineKind::Added, "two\n")];
    let hunk = DiffHunk { old_start: 0, old_count: 2, new_start: 0, new_count: 2, lines };
    let mut view = CommitView {
        diff: Some(FileDiff {
            path: "f".into(),
            staged: false,
            hunks: vec![hunk],
            rows: vec![DiffRow::Hunk(0), DiffRow::Line(0, 0), DiffRow::Line(0, 1)],
            colors: Vec::new(),
            partial: true,
            id: super::diff::reading_id(),
        }),
        ..CommitView::default()
    };
    view.code.place(CodeArea {
        area: Rect::new(0.0, 0.0, 400.0, 200.0),
        left: 100.0,
        row_h: 20.0,
        column: 10.0,
        scroll: 0.0,
        sideways: 0.0,
    });
    view
}

/// A press through the view's parts: `true` if the code's text took it (a selection held).
fn press_code(view: &mut CommitView, p: Point) -> bool {
    Viewer::press(view, p, 1);
    view.code.dragging
}

fn at(row: f32, column: f32) -> Point {
    Point::new(100.0 + column * 10.0, row * 20.0 + 5.0)
}

#[test]
fn a_drag_selects_across_lines_and_a_tab_counts_as_shown() {
    let mut view = view();
    assert!(press_code(&mut view, at(1.0, 4.0)));
    assert!(view.drag(at(2.0, 2.0)));
    assert!(view.release());
    assert_eq!(view.selected_code().as_deref(), Some("one\ntw"));
}

#[test]
fn a_click_without_a_drag_selects_nothing() {
    let mut view = view();
    assert!(press_code(&mut view, at(1.0, 4.0)));
    view.code.release();
    assert_eq!(view.selected_code(), None);
}

#[test]
fn a_hunk_header_or_the_numbers_are_not_code() {
    let mut view = view();
    assert!(!press_code(&mut view, at(0.0, 2.0)), "a hunk's header");
    assert!(!press_code(&mut view, Point::new(50.0, 25.0)), "the numbers");
}

#[test]
fn a_press_elsewhere_ends_the_selection() {
    let mut view = view();
    press_code(&mut view, at(1.0, 4.0));
    view.drag(at(2.0, 2.0));
    view.code.release();
    assert!(!press_code(&mut view, Point::new(-10.0, -10.0)), "pressed elsewhere");
    assert_eq!(view.selected_code(), None);
}

#[test]
fn the_highlight_covers_what_is_selected_and_stays_between_the_edges() {
    let mut view = view();
    press_code(&mut view, at(1.0, 4.0));
    view.drag(at(2.0, 2.0));
    let mut bands = Vec::new();
    view.code.bands(1, 7, (100.0, 400.0), &mut bands);
    assert_eq!(bands.len(), 1);
    assert_eq!((bands[0].min_x(), bands[0].size.width), (140.0, 40.0));
    view.code.bands(2, 3, (100.0, 400.0), &mut bands);
    assert_eq!(bands[0].size.width, 20.0);
    view.code.bands(0, 3, (100.0, 400.0), &mut bands);
    assert!(bands.is_empty());
}

/// Wrapped: a long line takes several screen rows; a press on its second row selects from
/// past where that row starts, and the highlight is drawn on each row it touches.
#[test]
fn a_wrapped_line_is_selected_on_its_rows() {
    let mut view = view();
    let long = "a fairly long line that will not fit in ten columns here\n";
    if let Some(diff) = &mut view.diff {
        diff.hunks[0].lines[1].text = long.into();
        view.code.wrap.refresh(diff, 10);
    }
    assert!(view.code.wrap.on());
    assert!(view.code.wrap.rows(2) > 1);
    // Its second screen row: the line's row 2 starts at screen row 2.
    let second = Point::new(100.0 + 2.0 * 10.0, 3.0 * 20.0 + 5.0);
    assert!(press_code(&mut view, at(2.0, 0.0)));
    assert!(view.drag(second));
    let (_, to) = view.code.ordered().expect("a selection");
    assert_eq!(to.0, 2, "the same line");
    assert!(to.1 > 10, "past its first row: {}", to.1);
    let mut bands = Vec::new();
    let width = super::text_selection::columns(long.trim_end());
    view.code.bands(2, width, (100.0, 400.0), &mut bands);
    assert_eq!(bands.len(), 2, "on both rows it touches");
}

/// Another file's changes, as many rows: their own heights, never the last file's (each
/// reading is its own, whatever memory it lands in).
#[test]
fn another_file_s_rows_are_wrapped_anew() {
    let mut view = view();
    let mut diff = view.diff.take().expect("a diff");
    diff.hunks[0].lines[1].text =
        "a fairly long line that will not fit in ten columns here\n".into();
    view.code.wrap.refresh(&diff, 10);
    assert!(view.code.wrap.rows(2) > 1);
    let mut other = FileDiff { id: super::diff::reading_id(), ..diff };
    other.hunks[0].lines[1].text = "short\n".into();
    view.code.wrap.refresh(&other, 10);
    assert_eq!(view.code.wrap.rows(2), 1, "the other file's own heights");
}
