use super::*;

fn sample() -> Scene {
    let mut s = Scene { width: 40.0, height: 20.0, ..Scene::default() };
    s.rect(
        Rect::new(2.0, 2.0, 36.0, 16.0),
        4.0,
        Some(Rgba::rgb(0xff0000)),
        Some(Line::solid(Rgba::BLACK, 1.0)),
    );
    s.text(Text {
        at: Point::new(20.0, 10.0),
        text: "a<b".into(),
        size: 10.0,
        bold: false,
        italic: false,
        mono: false,
        math: false,
        color: Rgba::WHITE,
        anchor: Anchor::Middle,
    });
    s
}

#[test]
fn svg_holds_shapes_and_escaped_text() {
    let svg = svg(&sample(), Some(Rgba::BLACK), "Inter");
    assert!(svg.starts_with("<svg") && svg.ends_with("</svg>"));
    assert!(
        svg.contains("<path") && svg.contains("a&lt;b") && svg.contains("text-anchor=\"middle\"")
    );
}

struct NoGlyphs;

impl GlyphPainter for NoGlyphs {
    fn glyphs(&mut self, _: &Text, _: f32) -> ShapedText {
        ShapedText {
            width: 10.0,
            ascent: 8.0,
            descent: 2.0,
            glyphs: vec![(0, -8, 2, 2, vec![255; 4], false)],
        }
    }
}

#[test]
fn a_picture_has_its_texts_and_a_png() {
    let (w, h, rgba, png) = picture(&sample(), 1.0, Rgba::BLACK, &mut NoGlyphs).expect("picture");
    assert_eq!((w, h, rgba.len()), (40, 20, 40 * 20 * 4));
    assert_eq!(&png[1..4], b"PNG");
}
