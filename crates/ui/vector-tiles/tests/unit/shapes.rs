use super::*;
use crate::tiles::{Shape, tile};
use kernel::Rgba;

const ALL: [ShapeKind; 22] = [
    ShapeKind::Rect { radius: 8.0 },
    ShapeKind::Ellipse,
    ShapeKind::Rhombus,
    ShapeKind::Parallelogram { slant: 20.0 },
    ShapeKind::Hexagon { inset: 20.0 },
    ShapeKind::Triangle,
    ShapeKind::Cylinder { depth: 15.0 },
    ShapeKind::Cloud,
    ShapeKind::Document { wave: 0.3 },
    ShapeKind::Process { inset: 0.1 },
    ShapeKind::Actor,
    ShapeKind::Note { fold: 20.0 },
    ShapeKind::Arrow { shaft: 0.3, head: 0.3 },
    ShapeKind::DoubleArrow { shaft: 0.3, head: 0.25 },
    ShapeKind::Trapezoid { inset: 20.0 },
    ShapeKind::Step { depth: 20.0 },
    ShapeKind::Card { cut: 30.0 },
    ShapeKind::Callout { tail: 30.0, position: 0.5, tip: 0.5, base: 20.0 },
    ShapeKind::Tape { wave: 0.4 },
    ShapeKind::Delay,
    ShapeKind::OffPage { depth: 37.5 },
    ShapeKind::ManualInput { rise: 30.0 },
];

/// The points the outline goes through (a curve's control points may lie outside the box
/// while the curve doesn't: draw.io's document wave).
fn points(segs: &[Seg]) -> Vec<Point> {
    segs.iter()
        .filter_map(|s| match s {
            Seg::Move(p) | Seg::Line(p) | Seg::Cubic(_, _, p) => Some(*p),
            Seg::Close => None,
        })
        .collect()
}

/// Every shape is drawn in its box: an outline there, none of its points outside it.
#[test]
fn every_shape_stays_in_its_box() {
    let r = Rect::new(10.0, 20.0, 120.0, 80.0);
    for kind in ALL {
        let o = outline(kind, r);
        assert!(!o.body.is_empty(), "{kind:?}");
        for p in points(&o.body).into_iter().chain(points(&o.details)) {
            assert!(
                p.x >= r.min_x() - 0.01
                    && p.x <= r.max_x() + 0.01
                    && p.y >= r.min_y() - 0.01
                    && p.y <= r.max_y() + 0.01,
                "{kind:?}: {p:?}"
            );
        }
    }
    // The ones with details over their body.
    for kind in [
        ShapeKind::Cylinder { depth: 15.0 },
        ShapeKind::Process { inset: 0.1 },
        ShapeKind::Note { fold: 20.0 },
    ] {
        assert!(!outline(kind, r).details.is_empty(), "{kind:?}");
    }
    // A box with no room: no panic, nothing outside it.
    for kind in ALL {
        outline(kind, Rect::new(0.0, 0.0, 0.0, 0.0));
    }
}

/// Filled, each covers its middle; the shapes that leave a corner empty leave it.
#[test]
fn each_fills_its_middle_and_leaves_its_corners() {
    let r = Rect::new(0.0, 0.0, 100.0, 100.0);
    let white = Rgba::rgb(0xffffff);
    for kind in ALL {
        let o = outline(kind, r);
        let masks = tile(
            (100.0, 100.0),
            [Shape { segs: &o.body, fill: Some(white), line: None }],
            1.0,
            0,
            0,
        );
        let at = |x, y| crate::tiles::tests::color_at(&masks, x, y)[0];
        assert!(at(50, 55) > 0.99, "{kind:?}: its middle");
        let empty_corner = !matches!(
            kind,
            ShapeKind::Rect { .. }
                | ShapeKind::Process { .. }
                | ShapeKind::Document { .. }
                | ShapeKind::Note { .. }
                | ShapeKind::Triangle
                | ShapeKind::Step { .. }
                | ShapeKind::Callout { .. }
                | ShapeKind::Delay
                | ShapeKind::OffPage { .. }
        );
        if empty_corner {
            assert!(at(1, 1) < 0.01, "{kind:?}: its corner");
        }
    }
}
