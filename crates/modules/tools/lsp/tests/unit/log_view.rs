use super::*;

/// The log's lines past the view: the bar's thumb pressed and dragged down scrolls them,
/// through the view's own press, drag and release.
#[test]
fn the_thumb_dragged_scrolls_the_lines() {
    let bounds = Rect::new(0.0, 0.0, 400.0, 300.0);
    let content = 3000.0;
    let mut view = LogView { max_scroll: content - bounds.size.height, ..LogView::default() };
    let palette = ui_components::Palette::dark();
    view.bar.paint(&mut render_api::Scene::default(), &palette, 1.0, bounds, content, 0.0);
    let edge = bounds.max_x() - 4.0;
    let thumb = Point::new(edge, bounds.min_y() + 6.0);
    view.hover(thumb);
    assert!(view.click(thumb).is_none());
    assert!(view.drag(Point::new(edge, bounds.min_y() + 120.0)), "the thumb moved");
    assert!(view.scroll > 0.0, "the lines scrolled");
    assert!(!view.follow, "away from the end: new lines don't pull it back");
    assert!(view.release());
    assert!(!view.drag(Point::new(edge, 250.0)), "let go: the moves don't reach it");
}
