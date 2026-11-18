use super::*;
use crate::view::tests::{editing_view, paint_view};
use text_layout::TextSystem;
use ui_components::{PressMods, Viewer};

fn cell(view: &DiagramView, id: &str) -> Cell {
    view.diagram.as_ref().unwrap().doc.pages[0].cell(id).unwrap().clone()
}

/// The bar shows over what's chosen, its edge buttons only with an edge; a press on a color
/// pair gives a shape its fill and line, an edge its line — one step.
#[test]
fn a_color_pair_reaches_shapes_and_edges_as_one_step() {
    let mut text = TextSystem::new();
    let (mut view, _f) = editing_view(&mut text);
    view.selection = vec!["start".into()];
    paint_view(&mut view, &mut text);
    let (bar, buttons) = view.quick_layout().expect("shown over the chosen shape");
    assert!(!buttons.iter().any(|(_, q)| matches!(q, Quick::Route(_))), "no edge chosen");
    assert!(bar.max_y() <= view.sheet().max_y());
    view.selection = vec!["start".into(), "e1".into()];
    // The mouse reads the bar as painted.
    paint_view(&mut view, &mut text);
    let (_, buttons) = view.quick_layout().unwrap();
    let (r, _) = buttons.iter().find(|(_, q)| *q == Quick::Scheme(5)).copied().unwrap();
    view.pointer_down(r.center(), 1, PressMods::default());
    view.release();
    assert_eq!(cell(&view, "start").style.get("fillColor"), Some("#f8cecc"));
    assert_eq!(cell(&view, "start").style.get("strokeColor"), Some("#b85450"));
    assert_eq!(cell(&view, "e1").style.get("strokeColor"), Some("#b85450"));
    assert_eq!(cell(&view, "e1").style.get("fillColor"), None);
    assert_eq!(view.selection.len(), 2, "what's chosen stays");
    assert!(view.command("undo"));
    assert_eq!(cell(&view, "start").style.get("fillColor"), Some("#d5e8d4"), "one step");
    // The theme's: the colors taken away.
    assert!(view.quick(Quick::Scheme(0)));
    assert_eq!(cell(&view, "start").style.get("fillColor"), None);
}

/// Dashed turns on for all, then off; an edge's way and its arrow change.
#[test]
fn lines_ways_and_arrows_change() {
    let mut text = TextSystem::new();
    let (mut view, _f) = editing_view(&mut text);
    view.selection = vec!["e1".into(), "start".into()];
    assert!(view.quick(Quick::Dashed));
    assert_eq!(cell(&view, "e1").style.get("dashed"), Some("1"));
    assert_eq!(cell(&view, "start").style.get("dashed"), Some("1"));
    assert!(view.quick(Quick::Dashed));
    assert_eq!(cell(&view, "e1").style.get("dashed"), None, "all dashed: off");
    assert!(view.quick(Quick::Route(Route::Curved)));
    assert_eq!(route_of(&cell(&view, "e1")), Route::Curved);
    assert_eq!(cell(&view, "start").style.get("curved"), None, "a shape has no way");
    assert!(view.quick(Quick::Route(Route::Elbow)));
    assert_eq!(route_of(&cell(&view, "e1")), Route::Elbow);
    assert!(view.quick(Quick::EndArrow));
    assert_eq!(cell(&view, "e1").style.get("endArrow"), Some("none"));
    assert!(view.quick(Quick::EndArrow));
    assert_eq!(cell(&view, "e1").style.get("endArrow"), None, "back to its arrow");
}
