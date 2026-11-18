use super::*;

fn rows() -> Vec<PopoverRow> {
    vec![
        PopoverRow::Value {
            label: "Language".into(),
            value: "JSON".into(),
            action: None,
            icon: None,
        },
        PopoverRow::Value {
            label: "Engine".into(),
            value: "vscode-json".into(),
            action: Some("choose-engine"),
            icon: None,
        },
    ]
}

/// Open over its item, above the bar, its right edge on the item's: a row with a command
/// runs it, one without only tells; it stays while the mouse is on it or its item.
#[test]
fn it_opens_over_its_item_and_its_rows_run_their_commands() {
    let (mut ts, palette) = (TextSystem::new(), Palette::dark());
    let viewport = Rect::new(0.0, 0.0, 1200.0, 800.0);
    let item = Rect::new(900.0, 772.0, 200.0, 20.0);
    let mut popover = StatusPopover::default();
    popover.open("status.file", item);
    popover.paint(
        &mut Scene::default(),
        &mut ts,
        &Icons::builtin(),
        &palette,
        1.0,
        viewport,
        768.0,
        &rows(),
        None,
        false,
    );
    let panel = popover.bounds().expect("drawn");
    assert!(panel.max_y() <= 768.0 - ds::POPOVER_GAP + 0.5, "{panel:?} above the bar");
    assert!((panel.max_x() - item.max_x()).abs() < 1.0, "its right edge on the item's");
    let engine = Point::new(panel.center().x, panel.min_y() + PAD + ROW_H * 1.5);
    assert_eq!(popover.hit(engine), Some(PopoverHit::Run("choose-engine")));
    let language = Point::new(panel.center().x, panel.min_y() + PAD + ROW_H * 0.5);
    assert_eq!(popover.hit(language), Some(PopoverHit::Inside), "a value only tells");
    assert!(popover.keeps(Point::new(950.0, 780.0)) && popover.keeps(engine));
    assert!(!popover.keeps(Point::new(100.0, 100.0)), "elsewhere: it goes");
}

/// Pinned, it stays wherever the mouse goes; closed, it's unpinned.
#[test]
fn a_pinned_one_stays() {
    let (mut ts, palette) = (TextSystem::new(), Palette::dark());
    let mut popover = StatusPopover::default();
    popover.open("status.resources", Rect::new(900.0, 772.0, 120.0, 20.0));
    popover.paint(
        &mut Scene::default(),
        &mut ts,
        &Icons::builtin(),
        &palette,
        1.0,
        Rect::new(0.0, 0.0, 1200.0, 800.0),
        768.0,
        &rows(),
        Some("Resources"),
        true,
    );
    let pin = popover.pin.expect("a pin");
    assert_eq!(popover.hit(pin.center()), Some(PopoverHit::Pin));
    popover.set_pinned(true);
    assert!(popover.keeps(Point::new(10.0, 10.0)));
    popover.close();
    assert!(!popover.pinned() && !popover.is_open());
}

/// On the way from the item up to the window (the gap over the bar), it stays.
#[test]
fn the_way_from_its_item_keeps_it() {
    let (mut ts, palette) = (TextSystem::new(), Palette::dark());
    let item = Rect::new(900.0, 772.0, 200.0, 20.0);
    let mut popover = StatusPopover::default();
    popover.open("status.file", item);
    popover.paint(
        &mut Scene::default(),
        &mut ts,
        &Icons::builtin(),
        &palette,
        1.0,
        Rect::new(0.0, 0.0, 1200.0, 800.0),
        768.0,
        &rows(),
        None,
        false,
    );
    let panel = popover.bounds().unwrap();
    let between = Point::new(item.center().x, (panel.max_y() + item.min_y()) / 2.0);
    assert!(popover.keeps(between));
    assert!(!popover.keeps(Point::new(item.min_x() - 40.0, between.y)), "beside the way: not");
}

/// The pin is one item's window's: opened for another, that one isn't pinned (it closes
/// when the mouse leaves).
#[test]
fn another_item_s_window_has_its_own_pin() {
    let mut popover = StatusPopover::default();
    popover.open("status.resources", Rect::new(900.0, 772.0, 120.0, 20.0));
    popover.set_pinned(true);
    popover.open("status.file", Rect::new(600.0, 772.0, 200.0, 20.0));
    assert!(!popover.pinned());
    popover.open("status.file", Rect::new(600.0, 772.0, 200.0, 20.0));
    assert!(!popover.pinned());
}

/// Pinned, the resources are pills ending at the corner given (above the editor's
/// scrollbars), the pin last: a click on it is the pin's.
#[test]
fn pinned_pills_end_at_their_corner_with_the_pin_last() {
    let (mut ts, palette) = (TextSystem::new(), Palette::dark());
    let mut popover = StatusPopover::default();
    popover.open("status.resources", Rect::default());
    popover.set_pinned(true);
    let pill = |label: &str, value: &str| Pill {
        label: label.into(),
        value: value.into(),
        color: palette.accent,
        more: None,
    };
    let corner = Point::new(1000.0, 700.0);
    popover.paint_pills(
        &mut Scene::default(),
        &mut ts,
        &Icons::builtin(),
        &palette,
        1.0,
        corner,
        &[pill("CPU", "2,2%"), pill("RAM", "500 MB"), pill("GPU", "136 MB")],
    );
    let panel = popover.bounds().expect("drawn");
    assert!((panel.max_x() - corner.x).abs() <= 1.0 && (panel.max_y() - corner.y).abs() <= 1.0);
    assert!(panel.size.height <= 26.0, "small: {panel:?}");
    let pin = popover.pin.expect("a pin");
    assert!((pin.max_x() - panel.max_x()).abs() <= 1.0, "the pin last");
    assert_eq!(popover.hit(pin.center()), Some(PopoverHit::Pin));
    assert_eq!(
        popover.hit(Point::new(panel.min_x() + 4.0, panel.center().y)),
        Some(PopoverHit::Inside)
    );
    // Its shapes (what shows through a page under them): each pill and the pin, rounded
    // as drawn, within it; gone with it.
    let shapes = popover.shapes().to_vec();
    assert_eq!(shapes.len(), 4, "{shapes:?}");
    assert_eq!(shapes[3].0, pin);
    assert!(shapes.iter().all(|(r, radius)| panel.contains(r.center()) && *radius > 0.0));
    popover.close();
    assert!(popover.shapes().is_empty());
}

/// A window kept left of its item (off the side's buttons): the way from the item up and
/// left to it — its pin — keeps it; far from the way, it goes.
#[test]
fn the_way_to_a_window_left_of_its_item_keeps_it() {
    let (mut ts, palette) = (TextSystem::new(), Palette::dark());
    let item = Rect::new(940.0, 772.0, 50.0, 20.0);
    let mut popover = StatusPopover::default();
    popover.open("status.file", item);
    popover.paint(
        &mut Scene::default(),
        &mut ts,
        &Icons::builtin(),
        &palette,
        1.0,
        Rect::new(0.0, 0.0, 900.0, 800.0),
        768.0,
        &rows(),
        None,
        false,
    );
    let panel = popover.bounds().unwrap();
    assert!(panel.max_x() < item.min_x(), "left of its item: {panel:?}");
    // Up from the item, beside the window, then left into it.
    for y in [item.min_y() - 2.0, panel.max_y() - 10.0, panel.min_y() + 10.0] {
        assert!(popover.keeps(Point::new(item.center().x, y)), "at {y}");
    }
    assert!(popover.keeps(Point::new(panel.max_x() + 5.0, panel.min_y() + 5.0)));
    assert!(!popover.keeps(Point::new(item.center().x, panel.min_y() - 20.0)), "above: not");
    assert!(!popover.keeps(Point::new(panel.min_x() - 20.0, item.min_y() - 2.0)), "left: not");
}
