use super::*;

fn of(style: &str) -> Look {
    look(&Style::parse(style), 120.0, 60.0)
}

/// The shapes by their names, with their proportions; an unknown one is its box.
#[test]
fn shapes_by_their_names() {
    assert_eq!(
        of("rounded=0;whiteSpace=wrap;html=1;").shape,
        Some(ShapeKind::Rect { radius: 0.0 })
    );
    assert_eq!(of("rounded=1;").shape, Some(ShapeKind::Rect { radius: 9.0 }), "15% of 60");
    assert_eq!(of("rounded=1;arcSize=50;").shape, Some(ShapeKind::Rect { radius: 30.0 }));
    assert_eq!(
        of("rounded=1;absoluteArcSize=1;arcSize=10;").shape,
        Some(ShapeKind::Rect { radius: 5.0 })
    );
    assert_eq!(of("ellipse;whiteSpace=wrap;").shape, Some(ShapeKind::Ellipse));
    assert_eq!(of("ellipse;shape=cloud;").shape, Some(ShapeKind::Cloud));
    assert_eq!(of("rhombus;").shape, Some(ShapeKind::Rhombus));
    assert_eq!(of("shape=cylinder3;size=15;").shape, Some(ShapeKind::Cylinder { depth: 15.0 }));
    assert_eq!(
        of("shape=parallelogram;").shape,
        Some(ShapeKind::Parallelogram { slant: 24.0 }),
        "0.2 of 120"
    );
    assert_eq!(
        of("shape=parallelogram;fixedSize=1;size=20;").shape,
        Some(ShapeKind::Parallelogram { slant: 20.0 })
    );
    assert_eq!(of("shape=note;size=20;").shape, Some(ShapeKind::Note { fold: 20.0 }));
    assert_eq!(
        of("shape=parallelogram;fixedSize=1;").shape,
        Some(ShapeKind::Parallelogram { slant: 20.0 }),
        "fixed, unset: 20 pixels (not 0.2)"
    );
    assert_eq!(
        of("shape=mxgraph.aws4.lambda;").shape,
        Some(ShapeKind::Rect { radius: 0.0 }),
        "not drawn here yet: its box"
    );
    assert_eq!(of("text;html=1;").shape, None);
    assert_eq!(of("group").shape, None);
    let lane = of("swimlane;startSize=30;");
    assert_eq!((lane.header, lane.valign), (Some(30.0), VAlign::Top));
}

/// Colors: given, none, or the theme's default; a text draws no box unless it's given
/// colors; the font's size, weight, alignment.
#[test]
fn colors_and_text() {
    let l = of(
        "fillColor=#dae8fc;strokeColor=none;fontColor=#F00;fontSize=14;fontStyle=3;align=left;verticalAlign=top;whiteSpace=wrap;html=1;",
    );
    assert_eq!(l.fill, Paint::Color(Rgba::rgb(0xdae8fc)));
    assert_eq!(l.stroke, Paint::None);
    assert_eq!(l.font_color, Paint::Color(Rgba::rgb(0xff0000)));
    assert_eq!(
        (l.font_size, l.bold, l.italic, l.align, l.valign, l.wrap, l.html),
        (14.0, true, true, Align::Left, VAlign::Top, true, true)
    );
    let d = of("rounded=0;");
    assert_eq!(
        (d.fill, d.stroke, d.font_color, d.font_size),
        (Paint::Default, Paint::Default, Paint::Default, 12.0)
    );
    assert_eq!(Paint::Default.or(Rgba::BLACK), Some(Rgba::BLACK));
    assert_eq!(Paint::None.or(Rgba::BLACK), None);
    let text = of("text;html=1;");
    assert_eq!((text.fill, text.stroke), (Paint::None, Paint::None));
    assert_eq!(of("text;fillColor=#fff;").fill, Paint::Color(Rgba::rgb(0xffffff)));
    assert_eq!(color("#12345"), None);
    assert_eq!(color("red"), None);
}

/// An edge: a classic arrow at its end by default; the ends, the routing, the line.
#[test]
fn an_edge_s_look() {
    let e = edge_look(&Style::parse("edgeStyle=orthogonalEdgeStyle;rounded=0;html=1;"));
    assert_eq!(
        (e.start, e.end, e.orthogonal),
        ((Marker::None, true), (Marker::Classic, true), true)
    );
    let e = edge_look(&Style::parse(
        "endArrow=block;endFill=0;startArrow=diamond;dashed=1;strokeWidth=2;",
    ));
    assert_eq!(
        (e.start.0, e.end, e.dashed, e.width, e.orthogonal),
        (Marker::Diamond, (Marker::Block, false), true, 2.0, false)
    );
    assert_eq!(edge_look(&Style::parse("endArrow=none;")).end.0, Marker::None);
}
