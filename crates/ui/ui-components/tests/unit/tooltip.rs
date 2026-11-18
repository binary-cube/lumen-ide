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
