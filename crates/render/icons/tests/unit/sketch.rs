use super::*;

const SQUARE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="2"><rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="12" cy="12" r="1" fill="#000" stroke="none"/><path d="M7 7l10 10"/></svg>"##;

/// An icon drawn by hand: readable again, its lines traced twice, its big closed shape
/// hatched, its dot kept filled; the same each time, another icon another way.
#[test]
fn an_icon_is_drawn_by_hand() {
    let drawn = sketch(SQUARE).expect("drawn");
    assert!(usvg::Tree::from_str(&drawn, &usvg::Options::default()).is_ok(), "{drawn}");
    let strokes = drawn.matches("stroke-opacity=\"1\"").count();
    assert_eq!(strokes, 2, "the square and the line, each traced firmly once");
    assert!(drawn.matches("stroke-opacity=\"0.65\"").count() >= 2, "and again, lighter");
    assert!(drawn.matches("stroke-opacity=\"0.5\"").count() >= 5, "the square hatched");
    assert!(drawn.contains("fill=\"#000\""), "the dot filled");
    assert_eq!(sketch(SQUARE), Some(drawn.clone()), "the same each time");
    let other = SQUARE.replace("M7 7l10 10", "M7 17l10 -10");
    assert_ne!(sketch(&other), Some(drawn));
    assert_eq!(sketch("not svg"), None);
}

/// The hatching stays inside its shape.
#[test]
fn the_hatching_stays_inside() {
    let square = [
        Point::from_xy(0.0, 0.0),
        Point::from_xy(10.0, 0.0),
        Point::from_xy(10.0, 10.0),
        Point::from_xy(0.0, 10.0),
    ];
    let strokes = hatch(&square);
    assert!(strokes.len() >= 4);
    let inside = |p: Point| (-0.01..=10.01).contains(&p.x) && (-0.01..=10.01).contains(&p.y);
    assert!(strokes.iter().all(|(a, b)| inside(*a) && inside(*b)));
}
