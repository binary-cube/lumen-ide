use super::*;
use icons::Icons;
use render_api::Scene;
use text_layout::TextSystem;

use crate::Palette;

#[test]
fn transparent_splash_draws_nothing_visible() {
    let mut scene = Scene::default();
    let art = ImageData::new(2, 1, vec![255; 8]);
    Splash.paint(
        SplashFrame {
            bounds: Rect::new(0.0, 0.0, 800.0, 600.0),
            scale: 1.0,
            phase: 0.3,
            opacity: 0.0,
        },
        "0.1.0",
        "Getting the editor ready…",
        Some(&art),
        Painter {
            text: &mut TextSystem::new(),
            scene: &mut scene,
            palette: &Palette::dark(),
            icons: &Icons::builtin(),
        },
    );
    for layer in scene.layers() {
        assert!(layer.quads.iter().all(|q| q.color.a == 0.0 && q.border_color.a == 0.0));
        assert!(layer.glyphs.iter().all(|g| g.color.a == 0.0));
        assert!(layer.images.iter().all(|i| i.opacity == 0.0));
    }
}
