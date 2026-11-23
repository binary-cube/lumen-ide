use super::*;

const VIEW: Rect = Rect::new(0.0, 0.0, 800.0, 600.0);

fn place(side: PopoverSide, right: bool) -> PopoverPlace {
    PopoverPlace { side, right, gap: 10.0, margin: 8.0, bottom: 20.0 }
}

/// Under its anchor when it fits, over it when it doesn't, always over it when asked.
#[test]
fn it_goes_under_or_over() {
    let anchor = Rect::new(100.0, 100.0, 60.0, 20.0);
    let (r, under) = popover_rect(anchor, (200.0, 100.0), VIEW, place(PopoverSide::Below, false));
    assert!(under);
    assert_eq!((r.min_x(), r.min_y()), (100.0, 130.0));
    let low = Rect::new(100.0, 520.0, 60.0, 20.0);
    let (r, under) = popover_rect(low, (200.0, 100.0), VIEW, place(PopoverSide::Below, false));
    assert!(!under);
    assert_eq!(r.max_y(), 510.0, "the gap over the anchor");
    let right = Rect::new(400.0, 100.0, 60.0, 20.0);
    let (r, under) = popover_rect(right, (200.0, 50.0), VIEW, place(PopoverSide::Above, true));
    assert!(!under);
    assert_eq!((r.max_x(), r.max_y()), (460.0, 90.0), "its right edge on the anchor's");
}

/// Kept inside the window: by its right side, its left, its top; a window narrower than it
/// keeps its left edge (never a panic).
#[test]
fn it_stays_in_the_window() {
    let edge = Rect::new(760.0, 10.0, 30.0, 20.0);
    let (r, _) = popover_rect(edge, (200.0, 100.0), VIEW, place(PopoverSide::Below, false));
    assert_eq!(r.max_x(), 792.0);
    let top = Rect::new(10.0, 10.0, 30.0, 20.0);
    let (r, _) = popover_rect(top, (200.0, 100.0), VIEW, place(PopoverSide::Above, true));
    assert_eq!((r.min_x(), r.min_y()), (8.0, 8.0));
    let narrow = Rect::new(0.0, 0.0, 120.0, 600.0);
    let (r, _) = popover_rect(top, (200.0, 100.0), narrow, place(PopoverSide::Below, true));
    assert_eq!(r.min_x(), 8.0);
}
