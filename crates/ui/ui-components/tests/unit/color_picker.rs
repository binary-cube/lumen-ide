use super::*;

#[test]
fn hsva_round_trip() {
    for hex in [0xe5484d, 0x3b82f6, 0x10b981, 0x777777, 0x000000, 0xffffff] {
        let c = Rgba::rgb(hex);
        assert_eq!(from_hsva(to_hsva(c)).to_rgba8(), c.to_rgba8());
    }
}

/// Hex only (what the picker needs from a language here).
#[derive(Debug)]
struct Hex;

impl ColorProvider for Hex {
    fn find(&self, _: &str, _: &str) -> Vec<code_intelligence::ColorLiteral> {
        Vec::new()
    }
    fn parse(&self, _: &str) -> Option<code_intelligence::ColorLiteral> {
        None
    }
    fn format(&self, color: Rgba, _: ColorFormat) -> String {
        format!("{:?}", color.to_rgba8())
    }
}

/// A click outside never loses the color chosen: it's kept (applied); with nothing
/// changed the picker just closes.
#[test]
fn a_click_outside_keeps_the_chosen_color() {
    let mut picker = ColorPicker::default();
    let red = Rgba::rgb(0xe5484d);
    let format = ColorFormat::Hex;
    picker.open(Rect::new(0.0, 0.0, 10.0, 10.0), red, format, &Hex);
    picker.card = Rect::new(100.0, 100.0, 200.0, 200.0);
    let outside = Point::new(5.0, 5.0);
    assert_eq!(picker.mouse_down(outside, &Hex), Some(PickerAction::Cancel), "unchanged");
    let blue = Rgba::rgb(0x3b82f6);
    picker.set_color(blue, &Hex);
    match picker.mouse_down(outside, &Hex) {
        Some(PickerAction::Apply(color, _)) => assert_eq!(color.to_rgba8(), blue.to_rgba8()),
        other => panic!("kept, not given up: {other:?}"),
    }
}
