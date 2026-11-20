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
    let row = view.code.band(1, 7, (100.0, 400.0)).unwrap();
    assert_eq!((row.min_x(), row.size.width), (140.0, 40.0));
    let last = view.code.band(2, 3, (100.0, 400.0)).unwrap();
    assert_eq!(last.size.width, 20.0);
    assert!(view.code.band(0, 3, (100.0, 400.0)).is_none());
}
