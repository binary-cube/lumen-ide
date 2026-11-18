use super::*;
use crate::view::tests::{editing_view, screen};
use text_layout::TextSystem;
use ui_components::{PressMods, Viewer, ViewerKey};

/// Dragged from diagram point `from` to `to`, with `mods` at the press.
fn drag(view: &mut DiagramView, from: (f32, f32), to: (f32, f32), mods: PressMods) {
    let (from, to) = (screen(view, from.0, from.1), screen(view, to.0, to.1));
    view.pointer_down(from, 1, mods);
    view.drag(Point::new((from.x + to.x) / 2.0, (from.y + to.y) / 2.0));
    view.drag(to);
    view.release();
}

fn chosen(view: &DiagramView) -> diagram_model::Cell {
    let id = view.selection[0].clone();
    view.diagram.as_ref().unwrap().doc.pages[0].cell(&id).unwrap().clone()
}

fn bounds(cell: &diagram_model::Cell) -> (f64, f64, f64, f64) {
    cell.geometry.as_ref().unwrap().bounds()
}

/// A box dragged: from its first corner, a square on ⇧ (its side the larger).
#[test]
fn a_dragged_box_is_a_square_on_shift() {
    let (a, b) = (Point::new(10.0, 10.0), Point::new(-30.0, 25.0));
    assert_eq!(dragged_box(a, b, false), Rect::new(-30.0, 10.0, 40.0, 15.0));
    assert_eq!(dragged_box(a, b, true), Rect::new(-30.0, 10.0, 40.0, 40.0));
}

/// The rectangle tool draws the box dragged (its corners on the grid), then Select is back;
/// kept (a double click on it), it draws again; Esc lets it go.
#[test]
fn a_shape_is_drawn_by_dragging_on_the_sheet() {
    let mut text = TextSystem::new();
    let (mut view, _f) = editing_view(&mut text);
    view.choose_draw(Draw::Rect, false);
    drag(&mut view, (482.0, 300.0), (598.0, 361.0), PressMods::default());
    let made = chosen(&view);
    assert_eq!(bounds(&made), (480.0, 300.0, 120.0, 60.0));
    assert_eq!(made.style.get("rounded"), Some("0"));
    assert_eq!(view.draw, Draw::Select, "back to Select");
    // Kept: an ellipse, then another, on ⇧ a circle.
    view.choose_draw(Draw::Ellipse, true);
    drag(
        &mut view,
        (480.0, 400.0),
        (540.0, 430.0),
        PressMods { shift: true, ..Default::default() },
    );
    let circle = chosen(&view);
    assert!(circle.style.has_name("ellipse") || circle.style.shape() == Some("ellipse"));
    assert_eq!(bounds(&circle), (480.0, 400.0, 60.0, 60.0));
    assert_eq!(view.draw, Draw::Ellipse, "kept");
    assert!(view.edit_key_with(ViewerKey::Escape, Default::default()));
    assert_eq!(view.draw, Draw::Select);
}

/// A click with a shape tool makes it its usual size there; the text tool opens its field.
#[test]
fn a_click_makes_the_usual_size_and_a_text_is_typed_at_once() {
    let mut text = TextSystem::new();
    let (mut view, _f) = editing_view(&mut text);
    view.choose_draw(Draw::Rhombus, false);
    drag(&mut view, (520.0, 330.0), (520.0, 330.0), PressMods::default());
    let (_, _, w, h) = bounds(&chosen(&view));
    assert!(w >= 60.0 && h >= 60.0, "its usual size: {w} × {h}");
    view.choose_draw(Draw::Text, false);
    drag(&mut view, (480.0, 500.0), (600.0, 530.0), PressMods::default());
    assert!(view.editing.is_some(), "its field open");
}

/// The line tool: from a shape to another joins them; on the sheet a loose line.
#[test]
fn the_line_tool_joins_or_draws_loose() {
    let mut text = TextSystem::new();
    let (mut view, _f) = editing_view(&mut text);
    view.choose_draw(Draw::Line, false);
    drag(&mut view, (400.0, 70.0), (630.0, 200.0), PressMods::default());
    let joined = chosen(&view);
    assert_eq!((joined.source.as_deref(), joined.target.as_deref()), (Some("start"), Some("save")));
    assert_eq!(view.draw, Draw::Select);
    view.choose_draw(Draw::Line, false);
    drag(&mut view, (480.0, 600.0), (640.0, 640.0), PressMods::default());
    let loose = chosen(&view);
    let g = loose.geometry.unwrap();
    assert_eq!(
        (g.source_point, g.target_point),
        (
            Some(diagram_model::Point::new(480.0, 600.0)),
            Some(diagram_model::Point::new(640.0, 640.0))
        )
    );
}
