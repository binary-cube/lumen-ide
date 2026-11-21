use super::*;
use crate::palette::Palette;

fn drawn(tab: PillTab<'_>) -> (PillTabRects, Vec<Quad>) {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let icons = Icons::builtin();
    let mut text = TextSystem::new();
    let mut layer = Layer::default();
    let rects = pill_tab(&mut layer, &mut text, &icons, &ctx, (10.0, 50.0), &tab);
    (rects, layer.quads)
}

/// A tab's pill: 24 high on its middle, as wide as it measures, its fill by its state.
#[test]
fn a_tab_is_a_pill_tinted_by_its_state() {
    let p = Palette::dark();
    let ctx = Ctx::new(&p, 1.0);
    let mut text = TextSystem::new();
    let tab = PillTab::new("Terminal").on(true, true);
    let (rects, quads) = drawn(tab);
    assert_eq!((rects.pill.min_y(), rects.pill.size.height), (38.0, TAB_H));
    assert_eq!(rects.pill.size.width, tab.width(&mut text, &ctx));
    assert_eq!(quads[0].color, p.accent.with_alpha(0.14));
    assert_eq!(drawn(PillTab::new("a").on(true, false)).1[0].color, p.text.with_alpha(0.08));
    assert_eq!(drawn(PillTab::new("a").hovered(true)).1[0].color, p.text.with_alpha(0.06));
    assert_eq!(drawn(PillTab::new("a")).1[0].color, Rgba::TRANSPARENT);
}

/// Its slot: room for a × (shown or not), a pin; an icon before the label widens it.
#[test]
fn a_tab_keeps_room_for_its_slot_and_its_icon() {
    let p = Palette::dark();
    let ctx = Ctx::new(&p, 1.0);
    let mut text = TextSystem::new();
    let bare = PillTab::new("Log").width(&mut text, &ctx);
    let slot = TabSlot::Close { shown: false, lit: false };
    let with_slot = PillTab::new("Log").slot(slot).width(&mut text, &ctx);
    assert_eq!(with_slot - bare, 18.0, "the × and its gap");
    let with_icon = PillTab::new("Log").icon("terminal").width(&mut text, &ctx);
    assert_eq!(with_icon - bare, 20.0, "the icon and its gap");
    assert_eq!(drawn(PillTab::new("Log").slot(slot)).0.close, None, "hidden ×");
    let shown = drawn(PillTab::new("Log").slot(TabSlot::Close { shown: true, lit: true })).0;
    let close = shown.close.unwrap();
    assert_eq!(close.max_x(), shown.pill.max_x() - 6.0);
    assert_eq!(drawn(PillTab::new("Log").slot(TabSlot::Pin)).0.close, None, "a pin: no ×");
}

/// The strip's border: the cards' border along its bottom.
#[test]
fn the_strip_has_its_border() {
    let p = Palette::dark();
    let ctx = Ctx::new(&p, 1.0);
    let mut layer = Layer::default();
    strip_border(&mut layer, &ctx, 0.0, 300.0, 40.0);
    assert_eq!(layer.quads[0].bounds, Rect::new(0.0, 40.0, 300.0, 1.0));
    assert_eq!(layer.quads[0].color, p.card_border);
}

/// An underlined tab: its label with room each side, the one in force over an accent line
/// along the bar's bottom, the others without one.
#[test]
fn an_underlined_tab_marks_the_one_in_force() {
    let p = Palette::dark();
    let ctx = Ctx::new(&p, 1.0);
    let font = ctx.ui_font(0.0);
    let mut text = TextSystem::new();
    let mut layer = Layer::default();
    let tab = UnderlineTab::new("Headers").on(true);
    let r = underline_tab(&mut layer, &mut text, &ctx, &font, (10.0, 0.0, 30.0), &tab);
    assert_eq!((r.min_x(), r.size.height), (10.0, 30.0));
    assert_eq!(r.size.width, tab.width(&mut text, &ctx, &font));
    assert_eq!(layer.quads.len(), 1);
    assert_eq!(layer.quads[0].color, p.accent);
    assert_eq!(layer.quads[0].bounds.max_y(), 30.0);
    let mut off = Layer::default();
    let narrow = UnderlineTab::new("Headers").pad(5.0);
    let r2 = underline_tab(&mut off, &mut text, &ctx, &font, (10.0, 0.0, 30.0), &narrow);
    assert!(off.quads.is_empty(), "no line under the others");
    assert_eq!(r.size.width - r2.size.width, 10.0, "closer together");
}
