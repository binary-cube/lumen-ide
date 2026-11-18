use super::*;

/// A widget of a fixed size (logical pixels), or none (nothing to show).
#[derive(Debug)]
struct Fixed {
    id: &'static str,
    place: WidgetPlace,
    size: Option<(f32, f32)>,
}

struct FixedView((f32, f32));

impl WidgetView for FixedView {
    fn size(&mut self, _: &mut TextSystem, ds: &Ctx<'_>, _: f32) -> Option<(f32, f32)> {
        Some((ds.px(self.0.0), ds.px(self.0.1)))
    }

    fn paint(&mut self, p: WidgetPaint<'_, '_>) {
        glass_pane(p.layer, p.ds, p.rect, p.look);
    }
}

impl ScreenWidget for Fixed {
    fn id(&self) -> &'static str {
        self.id
    }

    fn place(&self) -> WidgetPlace {
        self.place
    }

    fn open(&self, _: &WidgetContext) -> Option<Box<dyn WidgetView>> {
        Some(Box::new(FixedView(self.size?)))
    }
}

fn widget(id: &'static str, place: WidgetPlace, size: Option<(f32, f32)>) -> Arc<dyn ScreenWidget> {
    Arc::new(Fixed { id, place, size })
}

/// Each place's widgets side by side from its corner, along the bottom; one with nothing
/// to show isn't there; one too wide for what's left is left out.
#[test]
fn widgets_are_laid_along_the_bottom_from_their_corners() {
    let offered = [
        widget("mark", WidgetPlace::BottomLeft, Some((200.0, 60.0))),
        widget("none", WidgetPlace::BottomLeft, None),
        widget("cards", WidgetPlace::BottomRight, Some((300.0, 90.0))),
        widget("more", WidgetPlace::BottomRight, Some((100.0, 40.0))),
        widget("huge", WidgetPlace::BottomRight, Some((5000.0, 40.0))),
    ];
    let mut widgets = ScreenWidgets::open_from(&offered, &WidgetContext::default());
    assert_eq!(widgets.len(), 4, "the one with nothing to show isn't made");
    let palette = Palette::dark();
    let ds = Ctx::new(&palette, 1.0);
    let (mut scene, mut text) = (Scene::default(), TextSystem::new());
    let icons = Icons::builtin();
    let look = GlassLook { tint: Rgba::WHITE, opacity: 1.0, night: true };
    let pen = ScreenPen { scene: &mut scene, text: &mut text, icons: &icons, ds: &ds, look };
    let drawn = widgets.paint(pen, Rect::new(0.0, 0.0, 1200.0, 800.0), 20.0);
    assert_eq!(
        drawn,
        [
            Rect::new(20.0, 720.0, 200.0, 60.0),
            Rect::new(880.0, 690.0, 300.0, 90.0),
            Rect::new(768.0, 740.0, 100.0, 40.0),
        ]
    );
}

/// The theme's palette, its words and edges in the glass's tint: the rest stays the theme's.
#[test]
fn a_screen_s_palette_is_the_theme_s_in_the_glass_s_tint() {
    let theme = Palette::dark();
    let screen = screen_palette(&theme, Rgba::WHITE);
    assert_eq!(screen.text, Rgba::WHITE.with_alpha(0.94));
    assert_eq!(screen.text_muted, Rgba::WHITE.with_alpha(0.62));
    assert_eq!((screen.accent, screen.danger), (theme.accent, theme.danger));
}
