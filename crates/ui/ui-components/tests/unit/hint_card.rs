use super::*;

const AREA: Rect = Rect::new(0.0, 0.0, 1000.0, 800.0);
const GAPS: (f32, f32) = (10.0, 12.0);

/// Under its anchor, centered on it, the notch on its top edge at the anchor's middle.
#[test]
fn a_card_goes_under_its_anchor() {
    let anchor = Rect::new(400.0, 40.0, 100.0, 30.0);
    let (rect, notch) = place((300.0, 150.0), Some(anchor), AREA, GAPS);
    assert_eq!(rect, Rect::new(300.0, 80.0, 300.0, 150.0));
    assert_eq!(notch, Some(Notch { top: true, x: 450.0 }));
}

/// No room under it: above it, the notch on its bottom edge.
#[test]
fn a_card_flips_above_when_there_s_no_room_below() {
    let anchor = Rect::new(400.0, 700.0, 100.0, 30.0);
    let (rect, notch) = place((300.0, 150.0), Some(anchor), AREA, GAPS);
    assert_eq!(rect.max_y(), 690.0);
    assert_eq!(notch.map(|n| n.top), Some(false));
}

/// Near an edge: kept inside the area, the notch still toward the anchor (off the corners).
#[test]
fn a_card_stays_inside_the_area() {
    let anchor = Rect::new(0.0, 40.0, 40.0, 30.0);
    let (rect, notch) = place((300.0, 150.0), Some(anchor), AREA, GAPS);
    assert_eq!(rect.min_x(), 12.0);
    assert_eq!(notch.map(|n| n.x), Some(32.0), "off the rounded corner");
    let anchor = Rect::new(980.0, 40.0, 20.0, 30.0);
    let (rect, _) = place((300.0, 150.0), Some(anchor), AREA, GAPS);
    assert_eq!(rect.max_x(), 988.0);
    // Wider than the area: as wide as it allows.
    let (rect, _) = place((3000.0, 150.0), Some(anchor), AREA, GAPS);
    assert_eq!(rect.size.width, 976.0);
}

/// Without an anchor: the area's top-right corner, no notch.
#[test]
fn a_card_without_an_anchor_takes_the_top_right_corner() {
    let (rect, notch) = place((300.0, 150.0), None, AREA, GAPS);
    assert_eq!(rect, Rect::new(688.0, 12.0, 300.0, 150.0));
    assert_eq!(notch, None);
}

/// Drawn: its ×, its buttons and the card itself are found under the mouse; a hovered
/// button says so; hidden, nothing is.
#[test]
fn a_painted_card_answers_the_mouse() {
    let palette = crate::palette::Palette::dark();
    let icons = icons::Icons::builtin();
    let mut text = TextSystem::new();
    let mut scene = render_api::Scene::default();
    let buttons = [
        CardButton { label: "Don't show again", main: false, aside: true },
        CardButton { label: "Turn on", main: true, aside: false },
    ];
    let spec = HintCardSpec {
        icon: Some("moon"),
        eyebrow: "Tip",
        place: Some("Settings › Terminal › Sleeping sessions"),
        title: "Sessions can sleep",
        body: "Sessions left alone for a while stop and resume when you come back to them.",
        counter: None,
        buttons: &buttons,
        focus: Some(1),
    };
    let mut card = HintCard::default();
    let place = HintPlace {
        anchor: Some(Rect::new(100.0, 40.0, 120.0, 30.0)),
        area: AREA,
        scale: 1.0,
        spotlight: true,
    };
    let painter = Painter { text: &mut text, scene: &mut scene, palette: &palette, icons: &icons };
    card.paint(painter, &spec, place);
    let bounds = card.bounds().unwrap();
    assert!(bounds.min_y() > 70.0, "under its anchor");
    let center =
        |r: Rect| Point::new(r.min_x() + r.size.width / 2.0, r.min_y() + r.size.height / 2.0);
    let button = |i| card.hits.iter().find(|(_, h)| *h == CardHit::Button(i)).unwrap().0;
    let (aside, main) = (button(0), button(1));
    assert!(aside.min_x() < main.min_x(), "set aside on the left");
    assert_eq!(card.hit(center(main)), Some(CardHit::Button(1)));
    assert_eq!(
        card.hit(Point::new(bounds.min_x() + 20.0, bounds.min_y() + 60.0)),
        Some(CardHit::Card)
    );
    assert_eq!(card.hit(Point::new(5.0, 5.0)), None);
    assert!(card.hover(Some(center(main))));
    assert!(card.over_button());
    assert!(!card.hover(Some(center(main))), "the same");
    card.hide();
    assert_eq!(card.hit(center(main)), None);
}

/// Buttons that don't fit beside each other: the one set aside goes on a row of its own,
/// under the others, never over them.
#[test]
fn a_crowded_footer_takes_two_rows() {
    let palette = crate::palette::Palette::dark();
    let icons = icons::Icons::builtin();
    let mut text = TextSystem::new();
    let mut scene = render_api::Scene::default();
    let buttons = [
        CardButton { label: "Don't show again", main: false, aside: true },
        CardButton { label: "Open every one of the settings…", main: false, aside: false },
        CardButton { label: "Turn it on right now, please", main: true, aside: false },
    ];
    let spec = HintCardSpec {
        icon: None,
        eyebrow: "Tip",
        title: "T",
        body: "B",
        place: None,
        counter: None,
        buttons: &buttons,
        focus: None,
    };
    let mut card = HintCard::default();
    let place = HintPlace { anchor: None, area: AREA, scale: 1.0, spotlight: true };
    let painter = Painter { text: &mut text, scene: &mut scene, palette: &palette, icons: &icons };
    card.paint(painter, &spec, place);
    let at = |i| card.hits.iter().find(|(_, h)| *h == CardHit::Button(i)).unwrap().0;
    let (aside, other, main) = (at(0), at(1), at(2));
    assert!(aside.min_y() >= main.max_y(), "on a row under the others");
    assert!(aside.max_x() <= other.min_x() || aside.min_y() >= other.max_y(), "no overlap");
    assert!(card.bounds().unwrap().contains(Point::new(aside.min_x(), aside.max_y() - 1.0)));
}

/// The card drawn for `spec` (scale 1, no anchor), the places of its parts.
fn painted(spec: &HintCardSpec<'_>) -> (HintCard, render_api::Scene) {
    let palette = crate::palette::Palette::dark();
    let icons = icons::Icons::builtin();
    let mut text = TextSystem::new();
    let mut scene = render_api::Scene::default();
    let mut card = HintCard::default();
    let place = HintPlace { anchor: None, area: AREA, scale: 1.0, spotlight: true };
    let painter = Painter { text: &mut text, scene: &mut scene, palette: &palette, icons: &icons };
    card.paint(painter, spec, place);
    (card, scene)
}

/// A usual hint's footer: one row ("Don't show again" left, the buttons right), the buttons
/// as wide as their labels (compact), the place line and the icon's circle making the card
/// taller only by their own room.
#[test]
fn a_usual_footer_is_one_row_of_compact_buttons() {
    let buttons = [
        CardButton { label: "Don't show again", main: false, aside: true },
        CardButton { label: "Settings…", main: false, aside: false },
        CardButton { label: "Turn on", main: true, aside: false },
    ];
    let spec = |place, icon| HintCardSpec {
        icon,
        eyebrow: "Tip",
        title: "Idle sessions can sleep",
        body: "A session left alone for a while stops and frees its memory.",
        place,
        counter: None,
        buttons: &buttons,
        focus: None,
    };
    let (card, _) = painted(&spec(Some("Settings › Terminal"), Some("moon")));
    let at = |i| card.hits.iter().find(|(_, h)| *h == CardHit::Button(i)).unwrap().0;
    assert_eq!(at(0).min_y(), at(2).min_y(), "one row");
    assert!(at(0).max_x() < at(1).min_x(), "the link left of the buttons");
    assert!(at(1).size.width < 96.0, "compact: not the dialogs' minimum ({})", at(1).size.width);
    let (bare, _) = painted(&spec(None, None));
    let (h, h_bare) = (card.bounds().unwrap().size.height, bare.bounds().unwrap().size.height);
    assert!(h > h_bare, "the place line takes its room: {h} > {h_bare}");
    assert!(card.bounds().unwrap().size.width <= WIDTH);
}
