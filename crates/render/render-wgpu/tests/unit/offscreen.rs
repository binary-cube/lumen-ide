use super::*;
use kernel::{Rect, Rgba};
use render_api::{GlyphKey, Quad, RasterizedGlyph};

/// No glyphs (the scenes here have none).
struct NoGlyphs;

impl GlyphRasterizer for NoGlyphs {
    fn rasterize(&mut self, _key: GlyphKey) -> Option<RasterizedGlyph> {
        None
    }
}

fn scene(color: Rgba) -> Scene {
    let mut scene = Scene::default();
    scene.reset(Rgba::rgb(0x000000));
    scene.push_layer(None).quad(Quad::filled(Rect::new(4.0, 4.0, 8.0, 8.0), color));
    scene
}

/// A scene drawn without a window: its draw calls counted, its pixels read back as RGBA
/// (the quad's red where it is); drawn again unchanged, nothing is drawn.
#[test]
fn a_scene_is_drawn_measured_and_read_back_without_a_window() {
    let Ok(mut gpu) = Offscreen::new(PhysicalSize::new(16, 16)) else { return };
    let red = scene(Rgba::rgb(0xFF0000));
    let first = gpu.draw(&red, &mut NoGlyphs, true);
    assert!(first.stats.draw_calls > 0 && first.stats.drawn_pixels == 256);
    let (w, h, pixels) = gpu.pixels().expect("a frame drawn");
    assert_eq!((w, h, pixels.len()), (16, 16, 16 * 16 * 4));
    let at = |x: usize, y: usize| &pixels[(y * 16 + x) * 4..][..4];
    assert_eq!(at(8, 8), [255, 0, 0, 255]);
    assert_eq!(at(1, 1), [0, 0, 0, 255]);
    let again = gpu.draw(&red, &mut NoGlyphs, false);
    assert_eq!(again.stats.draw_calls, 0, "nothing changed: nothing drawn");
}

/// Many layers, each with its own clip (panels, lists, popovers), are drawn in one draw:
/// each layer's rectangles cut where its clip ends, a later layer over an earlier one, as
/// separate draws with scissors drew them.
#[test]
fn clipped_layers_are_drawn_in_one_draw_in_their_order() {
    let Ok(mut gpu) = Offscreen::new(PhysicalSize::new(64, 16)) else { return };
    let mut scene = Scene::default();
    scene.reset(Rgba::rgb(0x000000));
    // Eight columns, each a layer clipped to its 8 pixels, its quad wider than that.
    for i in 0..8u32 {
        let x = i as f32 * 8.0;
        let layer = scene.push_layer(Some(Rect::new(x, 0.0, 8.0, 16.0)));
        layer
            .quad(Quad::filled(Rect::new(x - 4.0, 0.0, 16.0, 16.0), Rgba::rgb(0x100000 * (i + 1))));
    }
    // One over them all, unclipped, at the bottom.
    scene.push_layer(None).quad(Quad::filled(Rect::new(0.0, 12.0, 64.0, 4.0), Rgba::rgb(0x00FF00)));
    let drawn = gpu.draw(&scene, &mut NoGlyphs, true);
    assert_eq!(drawn.stats.draw_calls, 1, "nine layers, one draw");
    let (_, _, pixels) = gpu.pixels().expect("a frame drawn");
    let at = |x: usize, y: usize| pixels[(y * 64 + x) * 4..][..3].to_vec();
    for i in 0..8usize {
        let red = (0x10 * (i + 1)) as u8;
        assert_eq!(at(i * 8, 4), [red, 0, 0], "column {i}, its left edge: its own");
        assert_eq!(at(i * 8 + 7, 4), [red, 0, 0], "column {i}, its right edge: not the next's");
    }
    assert_eq!(at(30, 14), [0, 255, 0], "the last layer over them");
}

/// A glyph wider than the atlas can ever be.
struct HugeGlyph;

impl GlyphRasterizer for HugeGlyph {
    fn rasterize(&mut self, _key: GlyphKey) -> Option<RasterizedGlyph> {
        let width = crate::ATLAS_MAX + 8;
        Some(RasterizedGlyph {
            left: 0,
            top: 1,
            width,
            height: 1,
            format: render_api::GlyphFormat::Mask,
            data: vec![255; width as usize],
        })
    }
}

/// A glyph that finds no room in the atlas, even made as large as it goes, is left out:
/// the rest of the frame is drawn all the same.
#[test]
fn a_glyph_without_room_leaves_the_rest_drawn() {
    let Ok(mut gpu) = Offscreen::new(PhysicalSize::new(16, 16)) else { return };
    let mut scene = scene(Rgba::rgb(0xFF0000));
    scene.push_layer(None).glyphs.push(render_api::GlyphInstance {
        key: GlyphKey(7),
        origin: kernel::Point::new(0.0, 14.0),
        color: Rgba::WHITE,
    });
    gpu.draw(&scene, &mut HugeGlyph, true);
    let (_, _, pixels) = gpu.pixels().expect("a frame drawn");
    assert_eq!(&pixels[(8 * 16 + 8) * 4..][..4], [255, 0, 0, 255], "the quad drawn");
}
