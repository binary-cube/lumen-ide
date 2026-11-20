use super::*;

/// A tooltip shown says where it is (its pill and its arrow toward the element), what must
/// show over a page of the system's web engine; hidden, nowhere.
#[test]
fn a_shown_tooltip_says_where_it_is() {
    let (mut text, mut scene, palette) = (TextSystem::new(), Scene::default(), Palette::dark());
    let window = Rect::new(0.0, 0.0, 800.0, 600.0);
    let anchor = Rect::new(700.0, 100.0, 30.0, 30.0);
    let mut tips = Tooltips::default();
    assert!(tips.areas().is_empty());
    tips.set(Some(Tip::new(anchor, "Diagrams").with_delay(Duration::ZERO)));
    tips.paint(&mut text, &mut scene, &palette, 1.0, window);
    let areas = tips.areas().to_vec();
    assert_eq!(areas.len(), 2, "{areas:?}");
    let (pill, arrow) = (areas[0].0, areas[1].0);
    assert!(pill.min_y() >= anchor.max_y() && window.contains(pill.center()));
    assert!(arrow.min_y() >= anchor.max_y() - 0.5 && arrow.max_y() <= pill.min_y() + 0.5);
    assert!((arrow.center().x - anchor.center().x).abs() <= 0.5, "under the element");
    tips.hide();
    assert!(tips.areas().is_empty());
}

/// A wide item (a field across the window): its tooltip by the mouse, where it was when it
/// appeared (it doesn't chase it after); a narrow one at its middle whatever the mouse.
#[test]
fn a_wide_item_s_tooltip_shows_by_the_mouse() {
    let (mut text, mut scene, palette) = (TextSystem::new(), Scene::default(), Palette::dark());
    let window = Rect::new(0.0, 0.0, 1600.0, 600.0);
    let field = Rect::new(20.0, 100.0, 1500.0, 30.0);
    let mut tips = Tooltips::default();
    tips.pointer(kernel::Point::new(300.0, 115.0));
    tips.set(Some(Tip::new(field, "SQL condition").with_delay(Duration::ZERO)));
    tips.paint(&mut text, &mut scene, &palette, 1.0, window);
    let arrow = tips.areas()[1].0;
    assert!((arrow.center().x - 300.0).abs() <= 1.0, "by the mouse: {arrow:?}");
    // The mouse moves on over the same field: it stays where it showed.
    tips.pointer(kernel::Point::new(900.0, 115.0));
    tips.paint(&mut text, &mut scene, &palette, 1.0, window);
    assert!((tips.areas()[1].0.center().x - 300.0).abs() <= 1.0);
    // A narrow item: at its middle.
    let button = Rect::new(700.0, 100.0, 30.0, 30.0);
    tips.set(None);
    tips.pointer(kernel::Point::new(702.0, 115.0));
    tips.set(Some(Tip::new(button, "Pause").with_delay(Duration::ZERO)));
    tips.paint(&mut text, &mut scene, &palette, 1.0, window);
    assert!((tips.areas()[1].0.center().x - button.center().x).abs() <= 0.5);
}
