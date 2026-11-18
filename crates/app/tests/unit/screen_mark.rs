use super::*;

/// The app's mark: nothing until its logo is decoded (on a worker, at the size it's drawn),
/// then its logo, name and version on glass.
#[test]
fn the_mark_shows_once_its_logo_is_decoded() {
    let mut view = ScreenMark.open(&WidgetContext::default()).expect("always offered");
    let palette = ui_workbench::Palette::dark();
    let ds = Ctx::new(&palette, 2.0);
    let mut text = TextSystem::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let size = loop {
        if let Some(size) = view.size(&mut text, &ds, 2000.0) {
            break size;
        }
        assert!(std::time::Instant::now() < deadline, "the logo wasn't decoded");
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    assert_eq!(size.1, 2.0 * WIDGET_HEIGHT, "as tall as every widget");
    assert!(size.0 > size.1, "{size:?}");
    let mut scene = render_api::Scene::default();
    let layer = scene.push_layer(None);
    let icons = icons::Icons::builtin();
    let rect = kernel::Rect::new(10.0, 10.0, size.0, size.1);
    let look = ui_workbench::screen_widgets::GlassLook {
        tint: kernel::Rgba::WHITE,
        opacity: 1.0,
        night: true,
    };
    view.paint(WidgetPaint { layer, text: &mut text, icons: &icons, ds: &ds, rect, look });
    let logo = scene.layers()[0].images.first().expect("the logo");
    // 64 logical pixels at scale 2: decoded at 128, drawn at 128.
    assert_eq!((logo.image.width, logo.bounds.size.width), (128, 128.0));
}
