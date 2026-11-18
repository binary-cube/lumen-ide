use super::*;

fn item(label: &str) -> CompletionItem {
    CompletionItem {
        label: label.into(),
        kind: CompletionKind::Method,
        detail: None,
        insert: label.into(),
        cursor_back: 0,
        score: 0,
        ..Default::default()
    }
}

#[test]
fn selection_wraps_and_survives_filtering() {
    let mut popup = CompletionPopup::default();
    let caret = Rect::new(100.0, 100.0, 2.0, 20.0);
    let viewport = Rect::new(0.0, 0.0, 1000.0, 800.0);
    popup.show(vec![item("save"), item("saveAll"), item("size")], "s".into(), caret, viewport, 1.0);
    popup.move_selection(1);
    assert_eq!(popup.selected().map(|i| i.label.as_str()), Some("saveAll"));
    popup.show(vec![item("saveAll"), item("save")], "sa".into(), caret, viewport, 1.0);
    assert_eq!(popup.selected().map(|i| i.label.as_str()), Some("saveAll"));
    // One step wraps past the start; a page stops at the end.
    popup.move_selection(-1);
    assert_eq!(popup.selected().map(|i| i.label.as_str()), Some("save"));
    popup.move_selection(-8);
    assert_eq!(popup.selected().map(|i| i.label.as_str()), Some("saveAll"));
    assert_eq!(matched_chars("gn", "getName"), [0, 3]);
    popup.show(Vec::new(), String::new(), caret, viewport, 1.0);
    assert!(!popup.is_open());
}

/// A list longer than the popup, painted once (its scrollbar knows where it is).
fn long_popup() -> CompletionPopup {
    let mut popup = CompletionPopup::default();
    let caret = Rect::new(100.0, 100.0, 2.0, 20.0);
    let viewport = Rect::new(0.0, 0.0, 1000.0, 800.0);
    let items = (0..30).map(|i| item(&format!("item{i}"))).collect();
    popup.show(items, String::new(), caret, viewport, 1.0);
    popup.paint(&mut TextSystem::new(), &mut Scene::default(), &Palette::dark());
    popup
}

/// Hidden at rest (no frames asked); the wheel scrolls whole rows and shows it, which then
/// asks frames only until it has faded out again.
#[test]
fn scrolling_the_list_shows_its_scrollbar() {
    let mut popup = long_popup();
    let now = std::time::Instant::now();
    assert_eq!(popup.bar.opacity_at(now), 0.0, "hidden at rest");
    assert_eq!(popup.frame_deadline(), None, "no frames at rest");
    // Less than a row waits for more; three rows' worth scrolls three.
    assert!(!popup.scroll_by(popup.row_h / 2.0));
    assert!(popup.scroll_by(popup.row_h * 2.5));
    assert_eq!(popup.first, 3);
    popup.paint(&mut TextSystem::new(), &mut Scene::default(), &Palette::dark());
    assert!(popup.frame_deadline().is_some(), "fading in");
    let later = now + std::time::Duration::from_secs(10);
    assert!(popup.bar.opacity_at(now + std::time::Duration::from_millis(500)) > 0.9);
    assert_eq!(popup.bar.opacity_at(later), 0.0, "faded out");
    assert_eq!(popup.bar.next_frame_at(later), None, "at rest again: no frames");
    // At the end the wheel does nothing.
    assert!(popup.scroll_by(popup.row_h * 100.0));
    assert_eq!(popup.first, 30 - VISIBLE);
    assert!(!popup.scroll_by(popup.row_h * 5.0));
}

/// Hidden, the pointer at its lane doesn't bring it (a scroll does); shown, its thumb
/// dragged moves the list, a row at a time.
#[test]
fn dragging_the_scrollbar_moves_the_list() {
    let mut popup = long_popup();
    let lane = Point::new(popup.rect.max_x() - 6.0, popup.rect.min_y() + 10.0);
    assert!(!popup.hover(lane), "hidden: not brought by the pointer");
    assert!(!popup.scrollbar_press(lane), "nor pressed there");
    popup.hover(Point::new(popup.rect.min_x() + 40.0, popup.rect.min_y() + 10.0));
    // A scroll shows it (and back to the top, for the drag below).
    assert!(popup.scroll_by(popup.row_h * 2.0));
    popup.paint(&mut TextSystem::new(), &mut Scene::default(), &Palette::dark());
    assert!(popup.scroll_by(-popup.row_h * 2.0));
    popup.paint(&mut TextSystem::new(), &mut Scene::default(), &Palette::dark());
    assert!(popup.hover(lane), "shown: it widens under the pointer");
    assert!(popup.scrollbar_press(lane));
    assert!(popup.scrollbar_held());
    assert!(popup.scrollbar_drag(Point::new(lane.x, lane.y + 120.0)));
    assert!(popup.first > 0);
    let dragged = popup.first;
    assert!(popup.scrollbar_drag(Point::new(lane.x, lane.y + 10_000.0)));
    assert_eq!(popup.first, 30 - VISIBLE, "{dragged}");
    assert!(popup.scrollbar_release());
    assert!(!popup.scrollbar_held());
    // A press on the rows isn't the scrollbar's.
    let row = Point::new(popup.rect.min_x() + 40.0, popup.rect.min_y() + 10.0);
    assert!(!popup.scrollbar_press(row));
}
