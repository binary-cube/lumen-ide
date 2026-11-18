use super::*;
use diagram_model::Document;

const FLOW: &str = include_str!("../../../../../domain/tools/diagram-model/tests/data/flow.drawio");

pub(crate) fn theme() -> Theme {
    Theme {
        canvas: Rgba::rgb(0x1e1e1e),
        fill: Rgba::rgb(0x2d2d2d),
        stroke: Rgba::rgb(0x999999),
        text: Rgba::rgb(0xeeeeee),
    }
}

pub(crate) fn flow() -> Drawing {
    let doc = Document::read(FLOW).unwrap();
    draw(&doc.pages[0], &theme())
}

fn box_of(d: &Drawing, id: &str) -> Rect {
    d.boxes.iter().find(|(i, _)| i == id).map(|(_, r)| *r).unwrap_or_else(|| panic!("{id}"))
}

/// The shapes at their absolute places (a group's children placed by it), their texts.
#[test]
fn shapes_are_placed_and_written() {
    let d = flow();
    assert_eq!(box_of(&d, "check"), Rect::new(350.0, 160.0, 100.0, 80.0));
    assert_eq!(
        box_of(&d, "g-b"),
        Rect::new(200.0, 380.0, 100.0, 80.0),
        "in its group at (40, 380)"
    );
    let texts: Vec<&str> = d.texts.iter().map(|t| t.text.as_str()).collect();
    for want in
        ["Start", "Valid?", "Save & notify\nthe owner", "yes", "checked", "A", "B", "Docs", "Pool"]
    {
        assert!(texts.contains(&want), "{want}: {texts:?}");
    }
    let start = d.texts.iter().find(|t| t.text == "Start").unwrap();
    assert_eq!(start.color, Rgba::rgb(0x1a1a1a), "on its light fill: dark");
    let a = d.texts.iter().find(|t| t.text == "A").unwrap();
    assert_eq!(a.color, theme().text, "on the theme's fill: the theme's");
    assert!(!d.items.is_empty());
    // Relative to its corner: nothing before it.
    for item in &d.items {
        for s in &item.segs {
            if let Seg::Move(p) | Seg::Line(p) = s {
                assert!(p.x >= 0.0 && p.y >= 0.0, "{p:?}");
            }
        }
    }
}

/// An edge between two shapes starts and ends on their borders; at right angles when
/// orthogonal; a marker at its end.
#[test]
fn edges_meet_their_shapes() {
    let way = route(
        Some((Rect::new(0.0, 0.0, 100.0, 50.0), false)),
        Some((Rect::new(300.0, 100.0, 100.0, 50.0), false)),
        &[],
        (None, None),
        true,
    );
    assert_eq!(
        way,
        [
            Point::new(100.0, 25.0),
            Point::new(200.0, 25.0),
            Point::new(200.0, 125.0),
            Point::new(300.0, 125.0)
        ]
    );
    let straight = route(
        Some((Rect::new(0.0, 0.0, 100.0, 100.0), true)),
        Some((Rect::new(300.0, 0.0, 100.0, 100.0), false)),
        &[],
        (None, None),
        false,
    );
    assert_eq!(straight.first(), Some(&Point::new(100.0, 50.0)), "the ellipse's border");
    assert_eq!(straight.last(), Some(&Point::new(300.0, 50.0)));
    let loose =
        route(None, None, &[], (Some(Point::new(1.0, 2.0)), Some(Point::new(5.0, 2.0))), false);
    assert_eq!(loose.len(), 2);
    let (segs, stop, filled) =
        marker(Marker::Classic, Point::new(0.0, 0.0), Point::new(100.0, 0.0), 6.0).unwrap();
    assert!(filled && stop.x < 100.0 && !segs.is_empty());
}

/// A hidden layer's cells aren't drawn.
#[test]
fn a_hidden_layer_isn_t_drawn() {
    let doc = Document::read(FLOW).unwrap();
    let d = draw(&doc.pages[1], &theme());
    assert!(d.texts.is_empty() && d.items.is_empty());
}

/// 500 shapes and 600 edges: the drawing made, its tiles rasterized (plan 80's measure: run
/// with `--ignored --nocapture`).
#[test]
#[ignore]
fn measure_a_large_drawing() {
    use diagram_model::{CellKind, Geometry, Style};
    let mut doc = Document::new("Big", "big");
    for i in 0..500 {
        let mut c = diagram_model::Cell::new(format!("v{i}"), CellKind::Vertex, "1");
        c.value = format!("Shape {i}");
        c.style = Style::parse(if i % 3 == 0 {
            "ellipse;whiteSpace=wrap;html=1;"
        } else {
            "rounded=1;whiteSpace=wrap;html=1;fillColor=#dae8fc;"
        });
        c.geometry = Some(Geometry {
            x: f64::from(i % 25) * 160.0,
            y: f64::from(i / 25) * 100.0,
            width: 120.0,
            height: 60.0,
            ..Default::default()
        });
        doc.pages[0].cells.push(c);
    }
    for i in 0..600 {
        let mut e = diagram_model::Cell::new(format!("e{i}"), CellKind::Edge, "1");
        e.style = Style::parse("edgeStyle=orthogonalEdgeStyle;html=1;");
        e.source = Some(format!("v{}", i % 500));
        e.target = Some(format!("v{}", (i * 7 + 1) % 500));
        e.geometry = Some(Geometry { relative: true, ..Default::default() });
        doc.pages[0].cells.push(e);
    }
    let started = std::time::Instant::now();
    let d = draw(&doc.pages[0], &theme());
    let drawn = started.elapsed();
    let size = (d.bounds.size.width, d.bounds.size.height);
    let (cols, rows) = vector_tiles::grid(size, 1.0);
    let started = std::time::Instant::now();
    let mut bytes = 0usize;
    for ty in 0..rows {
        for tx in 0..cols {
            for (image, _) in vector_tiles::tile(size, d.shapes(), 1.0, tx, ty) {
                bytes += (image.width * image.height) as usize;
            }
        }
    }
    let tiled = started.elapsed();
    eprintln!(
        "MEASURE drawing {drawn:?} ({} items, {} texts); {} tiles at 100% in {tiled:?}, {bytes} mask bytes",
        d.items.len(),
        d.texts.len(),
        cols * rows
    );
}

/// A curved edge is a smooth curve through its points, its ends where the straight one's are.
#[test]
fn a_curved_edge_is_smooth() {
    let way = [Point::new(0.0, 0.0), Point::new(50.0, 40.0), Point::new(100.0, 0.0)];
    let segs = smooth(&way);
    assert_eq!(segs.len(), 3);
    assert_eq!(segs[0], Seg::Move(Point::new(0.0, 0.0)));
    assert!(matches!(segs[2], Seg::Cubic(_, _, p) if p == Point::new(100.0, 0.0)));
    assert_eq!(smooth(&way[..2]).len(), 2, "two points: a line");
}
