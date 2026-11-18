use super::*;

/// An editor group on the left, a panel on the right, the status bar under both.
fn regions() -> Vec<Region> {
    let region = |kind, x, y, w, h| Region { kind, rect: Rect::new(x, y, w, h), radius: 0.0 };
    vec![
        region(RegionKind::EditorGroup, 0.0, 0.0, 100.0, 100.0),
        region(RegionKind::SidePanel, 100.0, 0.0, 50.0, 100.0),
        region(RegionKind::StatusBar, 0.0, 100.0, 150.0, 20.0),
    ]
}

fn kind(r: Option<Region>) -> Option<RegionKind> {
    r.map(|r| r.kind)
}

#[test]
fn the_keyboard_follows_the_focus_whatever_the_pointer() {
    let (regions, mut focus) = (regions(), RegionFocus::default());
    focus.pointer_moved(Point::new(120.0, 50.0));
    let chosen = focus.choose(&regions, Some(0), Follow::Keyboard, false);
    assert_eq!(kind(chosen), Some(RegionKind::EditorGroup));
}

#[test]
fn the_pointer_follows_the_region_under_it_else_the_keyboard() {
    let (regions, mut focus) = (regions(), RegionFocus::default());
    assert_eq!(
        kind(focus.choose(&regions, Some(0), Follow::Pointer, false)),
        Some(RegionKind::EditorGroup)
    );
    focus.pointer_moved(Point::new(10.0, 110.0));
    assert_eq!(
        kind(focus.choose(&regions, Some(0), Follow::Pointer, false)),
        Some(RegionKind::StatusBar)
    );
    // Typing doesn't bring it back: only the pointer counts.
    focus.key_pressed();
    assert_eq!(
        kind(focus.choose(&regions, Some(0), Follow::Pointer, false)),
        Some(RegionKind::StatusBar)
    );
    // Off any region (a gap), or out of the window: the keyboard's.
    focus.pointer_moved(Point::new(300.0, 300.0));
    assert_eq!(
        kind(focus.choose(&regions, Some(1), Follow::Pointer, false)),
        Some(RegionKind::SidePanel)
    );
    focus.pointer_moved(Point::new(10.0, 10.0));
    focus.pointer_left();
    assert_eq!(
        kind(focus.choose(&regions, Some(1), Follow::Pointer, false)),
        Some(RegionKind::SidePanel)
    );
}

#[test]
fn both_hover_wins_until_a_key_then_the_keyboard_until_the_pointer_moves() {
    let (regions, mut focus) = (regions(), RegionFocus::default());
    focus.pointer_moved(Point::new(120.0, 50.0));
    assert_eq!(
        kind(focus.choose(&regions, Some(0), Follow::Both, false)),
        Some(RegionKind::SidePanel)
    );
    focus.key_pressed();
    assert_eq!(
        kind(focus.choose(&regions, Some(0), Follow::Both, false)),
        Some(RegionKind::EditorGroup)
    );
    focus.pointer_moved(Point::new(121.0, 50.0));
    assert_eq!(
        kind(focus.choose(&regions, Some(0), Follow::Both, false)),
        Some(RegionKind::SidePanel)
    );
    // Out of the window: the keyboard's.
    focus.pointer_left();
    assert_eq!(
        kind(focus.choose(&regions, Some(0), Follow::Both, false)),
        Some(RegionKind::EditorGroup)
    );
}

#[test]
fn frozen_while_something_is_open_over_the_window() {
    let (mut regions, mut focus) = (regions(), RegionFocus::default());
    focus.pointer_moved(Point::new(120.0, 50.0));
    focus.choose(&regions, Some(0), Follow::Both, false);
    // A modal opened: the pointer and the keyboard change nothing behind it…
    focus.pointer_moved(Point::new(10.0, 10.0));
    let chosen = focus.choose(&regions, Some(0), Follow::Both, true);
    assert_eq!(kind(chosen), Some(RegionKind::SidePanel));
    // …but it follows its region's place (the window resized under it).
    regions[1].rect = Rect::new(90.0, 0.0, 60.0, 100.0);
    assert_eq!(
        focus.choose(&regions, Some(0), Follow::Both, true).map(|r| r.rect),
        Some(regions[1].rect)
    );
    // Closed: as usual again.
    assert_eq!(
        kind(focus.choose(&regions, Some(0), Follow::Both, false)),
        Some(RegionKind::EditorGroup)
    );
}

#[test]
fn a_frame_is_needed_only_when_the_pointer_reaches_another_region() {
    let (regions, mut focus) = (regions(), RegionFocus::default());
    focus.pointer_moved(Point::new(10.0, 10.0));
    focus.choose(&regions, Some(0), Follow::Both, false);
    let moves = |p| focus.moves_to_another(&regions, Some(0), Follow::Both, p);
    assert!(!moves(Point::new(50.0, 50.0)));
    assert!(moves(Point::new(120.0, 50.0)));
    assert!(!moves(Point::new(500.0, 50.0)), "over none: the keyboard's, the same");
    let moves = |p| focus.moves_to_another(&regions, Some(1), Follow::Both, p);
    assert!(moves(Point::new(500.0, 50.0)), "over none: the keyboard's, another");
    assert_eq!(kind(focus.chosen()), Some(RegionKind::EditorGroup));
    assert_eq!(region_at(&regions, Point::new(120.0, 110.0)), Some(2));
}
