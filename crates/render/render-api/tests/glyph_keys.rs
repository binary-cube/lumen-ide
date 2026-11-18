//! Glyph keys reissued (the text system and the glyph caches cleared together): a key equal
//! to the last frame's no longer proves the same pixels, so the next frame is full. In its
//! own test binary: the epoch is process-wide.

use render_api::{Damage, DamageTracker, GlyphBox, GlyphInstance, GlyphKey, Scene};
use render_api::{PhysicalSize, Point, Rgba, glyph_keys_reset};

#[test]
fn reissued_glyph_keys_make_the_next_frame_full() {
    let size = PhysicalSize::new(50, 50);
    let mut scene = Scene::new(Rgba::BLACK);
    scene.push_layer(None).glyph(GlyphInstance {
        key: GlyphKey(3),
        origin: Point::new(10.0, 20.0),
        color: Rgba::WHITE,
    });
    let mut tracker = DamageTracker::new();
    tracker.commit(&scene, size);
    let mut boxes = |_| Some(GlyphBox { left: 0, top: 8, width: 6, height: 8 });
    let mut damage = Damage::default();
    tracker.damage(&scene, size, &mut boxes, &mut damage);
    assert!(damage.is_empty());
    glyph_keys_reset();
    tracker.damage(&scene, size, &mut boxes, &mut damage);
    assert!(damage.is_full());
    tracker.commit(&scene, size);
    tracker.damage(&scene, size, &mut boxes, &mut damage);
    assert!(damage.is_empty(), "recorded again after the reset");
}
