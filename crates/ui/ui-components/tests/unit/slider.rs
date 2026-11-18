use super::*;

#[test]
fn a_click_picks_the_nearest_stop() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut layer = Layer::default();
    let mut text = TextSystem::new();
    let style = SliderStyle::from_palette(&palette);
    let area = slider(
        &mut layer,
        &mut text,
        &ctx,
        SliderSpec {
            x: 0.0,
            top: 0.0,
            width: 216.0,
            labels: &["1", "2", "4", "8"],
            current: 1,
            hovered: false,
            style: &style,
        },
    );
    // Four stops, evenly from the thumb's half-width to the end.
    assert_eq!(area.stops, vec![8.0, 75.0, 141.0, 208.0]);
    assert_eq!(area.nearest(0.0), Some(0));
    assert_eq!(area.nearest(100.0), Some(1));
    assert_eq!(area.nearest(120.0), Some(2));
    assert_eq!(area.nearest(500.0), Some(3));
    assert!(area.rect.contains(kernel::Point::new(141.0, 30.0)), "the labels take clicks too");
    // Never wider than its style says.
    let wide = slider(
        &mut layer,
        &mut text,
        &ctx,
        SliderSpec {
            x: 0.0,
            top: 0.0,
            width: 2000.0,
            labels: &["a", "b"],
            current: 0,
            hovered: true,
            style: &style,
        },
    );
    assert_eq!(wide.rect.size.width, 360.0);
    // Customized: no values under it, only as tall as its thumb.
    let compact = SliderStyle { labels: false, ticks: false, ..style };
    let area = slider(
        &mut layer,
        &mut text,
        &ctx,
        SliderSpec {
            x: 0.0,
            top: 0.0,
            width: 200.0,
            labels: &["a", "b"],
            current: 1,
            hovered: false,
            style: &compact,
        },
    );
    assert_eq!(area.rect.size.height, height(&compact));
    assert_eq!(height(&compact), 20.0);
}
