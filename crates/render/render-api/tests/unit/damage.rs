use super::*;
use crate::{GlyphInstance, GlyphKey, ImageData, Quad, Scene};
use kernel::{PhysicalSize, Point, Rect, Rgba};

const SIZE: PhysicalSize = PhysicalSize::new(200, 100);

/// Every glyph is 4 × 6 pixels, 1 left of and 5 above its pen position; key 0 has no pixels.
fn glyph_box(key: GlyphKey) -> Option<GlyphBox> {
    Some(if key.0 == 0 {
        GlyphBox::EMPTY
    } else {
        GlyphBox { left: -1, top: 5, width: 4, height: 6 }
    })
}

fn glyph(key: u64, x: f32, y: f32) -> GlyphInstance {
    GlyphInstance { key: GlyphKey(key), origin: Point::new(x, y), color: Rgba::WHITE }
}

fn scene(caret: bool) -> Scene {
    let mut scene = Scene::new(Rgba::BLACK);
    scene.push_layer(None).quad(Quad::filled(Rect::new(0.0, 0.0, 200.0, 100.0), Rgba::BLACK));
    let text = scene.push_layer(Some(Rect::new(10.0, 10.0, 150.0, 80.0)));
    text.glyph(glyph(1, 20.0, 30.0)).glyph(glyph(2, 26.0, 30.0)).glyph(glyph(0, 32.0, 30.0));
    if caret {
        text.quad(Quad::filled(Rect::new(40.2, 22.0, 2.0, 12.0), Rgba::WHITE));
    }
    scene
}

fn damage_of(tracker: &DamageTracker, scene: &Scene) -> Damage {
    let mut damage = Damage::default();
    tracker.damage(scene, SIZE, &mut glyph_box, &mut damage);
    damage
}

fn committed(scene: &Scene) -> DamageTracker {
    let mut tracker = DamageTracker::new();
    tracker.commit(scene, SIZE);
    tracker
}

#[test]
fn the_first_frame_and_an_invalidated_one_are_full() {
    let mut tracker = DamageTracker::new();
    assert!(damage_of(&tracker, &scene(true)).is_full());
    tracker.commit(&scene(true), SIZE);
    assert!(damage_of(&tracker, &scene(true)).is_empty());
    tracker.invalidate();
    assert!(damage_of(&tracker, &scene(true)).is_full());
}

#[test]
fn an_unchanged_scene_has_no_damage() {
    let tracker = committed(&scene(true));
    assert!(damage_of(&tracker, &scene(true)).is_empty());
}

#[test]
fn a_caret_blinking_damages_only_its_pixels() {
    let tracker = committed(&scene(true));
    let damage = damage_of(&tracker, &scene(false));
    assert_eq!(damage.rects(), &[PixelRect::new(40, 22, 43, 34)], "covering the fractional x");
}

#[test]
fn a_glyph_changing_damages_both_boxes() {
    let tracker = committed(&scene(false));
    let mut moved = scene(false);
    moved.layer_mut(1).unwrap().glyphs[0].origin.x = 60.4;
    let damage = damage_of(&tracker, &moved);
    let mut rects = damage.rects().to_vec();
    rects.sort_by_key(|r| r.x0);
    assert_eq!(rects, vec![PixelRect::new(19, 25, 23, 31), PixelRect::new(59, 25, 63, 31)]);
}

#[test]
fn damage_is_clipped_by_its_layer() {
    let tracker = committed(&scene(false));
    let mut changed = scene(false);
    // Half outside the layer's clip (which starts at x = 10).
    changed.layer_mut(1).unwrap().quad(Quad::filled(Rect::new(4.0, 40.0, 10.0, 4.0), Rgba::WHITE));
    assert_eq!(damage_of(&tracker, &changed).rects(), &[PixelRect::new(10, 40, 14, 44)]);
}

#[test]
fn a_space_changing_damages_nothing() {
    let tracker = committed(&scene(false));
    let mut changed = scene(false);
    changed.layer_mut(1).unwrap().glyphs[2].origin.x = 90.0;
    assert!(damage_of(&tracker, &changed).is_empty());
}

#[test]
fn an_unknown_glyph_makes_the_frame_full() {
    let tracker = committed(&scene(false));
    let mut changed = scene(false);
    changed.layer_mut(1).unwrap().glyphs[0].key = GlyphKey(7);
    let mut damage = Damage::default();
    tracker.damage(&changed, SIZE, &mut |_| None, &mut damage);
    assert!(damage.is_full());
}

#[test]
fn background_size_and_corners_make_the_frame_full() {
    let tracker = committed(&scene(false));
    let mut other = scene(false);
    other.clear_color = Rgba::WHITE;
    assert!(damage_of(&tracker, &other).is_full(), "another background");
    let mut other = scene(false);
    other.window_corners = 8.0;
    assert!(damage_of(&tracker, &other).is_full(), "the corners cut");
    let mut damage = Damage::default();
    tracker.damage(&scene(false), PhysicalSize::new(201, 100), &mut glyph_box, &mut damage);
    assert!(damage.is_full(), "another size");
    // Glyph keys reissued: `tests/glyph_keys.rs` (its own process: the epoch is global).
}

#[test]
fn a_layer_added_or_reclipped_damages_what_it_draws() {
    let tracker = committed(&scene(false));
    let mut added = scene(false);
    added
        .push_layer(Some(Rect::new(100.0, 50.0, 20.0, 20.0)))
        .quad(Quad::filled(Rect::new(90.0, 40.0, 50.0, 50.0), Rgba::WHITE));
    assert_eq!(damage_of(&tracker, &added).rects(), &[PixelRect::new(100, 50, 120, 70)]);

    let tracker = committed(&added);
    let mut moved = added.clone();
    moved.layer_mut(2).unwrap().clip = Some(Rect::new(130.0, 50.0, 5.0, 5.0));
    let mut rects = damage_of(&tracker, &moved).rects().to_vec();
    rects.sort_by_key(|r| r.x0);
    assert_eq!(rects, vec![PixelRect::new(100, 50, 120, 70), PixelRect::new(130, 50, 135, 55)]);
}

#[test]
fn an_image_changing_damages_its_bounds() {
    let image = ImageData::new(2, 2, vec![255; 16]);
    let mut first = scene(false);
    first.push_layer(None).image(&image, Rect::new(150.0, 70.0, 8.0, 8.0));
    let tracker = committed(&first);
    let mut faded = first.clone();
    faded.layer_mut(2).unwrap().images[0].opacity = 0.5;
    assert_eq!(damage_of(&tracker, &faded).rects(), &[PixelRect::new(150, 70, 158, 78)]);
    let other = ImageData::new(2, 2, vec![0; 16]);
    let mut replaced = scene(false);
    replaced.push_layer(None).image(&other, Rect::new(150.0, 70.0, 8.0, 8.0));
    assert!(!damage_of(&tracker, &replaced).is_empty(), "another image, same place");
}

/// A live image's new frame (its texture filled again) damages where it's drawn.
#[test]
fn a_live_images_new_frame_damages_its_bounds() {
    let page = ImageData::live();
    assert!(!page.has_frame());
    let mut first = scene(false);
    first.push_layer(None).image(&page, Rect::new(150.0, 70.0, 8.0, 8.0));
    let tracker = committed(&first);
    assert!(damage_of(&tracker, &first).is_empty(), "nothing new");
    page.frame_copied(crate::CopiedFrame { size: (8, 8), changed: None, bytes: 0 });
    assert!(page.has_frame());
    assert_eq!(damage_of(&tracker, &first).rects(), &[PixelRect::new(150, 70, 158, 78)]);
}

/// A frame of a live image (40 × 20 pixels, drawn twice as large) changed in one place:
/// only that is drawn again, where it shows, with a texel more around it (the filtering).
#[test]
fn a_live_images_frame_damages_only_where_it_changed() {
    let page = ImageData::live();
    let copied = |changed| crate::CopiedFrame { size: (40, 20), changed, bytes: 0 };
    page.frame_copied(copied(None));
    let mut drawn = scene(false);
    drawn.push_layer(None).image(&page, Rect::new(100.0, 50.0, 80.0, 40.0));
    let tracker = committed(&drawn);
    page.frame_copied(copied(Some(PixelRect::new(10, 5, 12, 6))));
    // Texels 9..13 × 4..7, two pixels each, from (100, 50).
    assert_eq!(damage_of(&tracker, &drawn).rects(), &[PixelRect::new(118, 58, 126, 64)]);
    // Two frames since: both their areas.
    page.frame_copied(copied(Some(PixelRect::new(0, 0, 1, 1))));
    assert_eq!(damage_of(&tracker, &drawn).bounds(), Some(PixelRect::new(100, 50, 126, 64)));
    // A frame changed all over: all of it.
    page.frame_copied(copied(None));
    assert_eq!(damage_of(&tracker, &drawn).rects(), &[PixelRect::new(100, 50, 180, 90)]);
}

/// More frames than are remembered since the last drawn: all of the image.
#[test]
fn a_live_image_many_frames_behind_is_drawn_whole() {
    let page = ImageData::live();
    let copied =
        crate::CopiedFrame { size: (40, 20), changed: Some(PixelRect::new(0, 0, 1, 1)), bytes: 0 };
    page.frame_copied(copied);
    let mut drawn = scene(false);
    drawn.push_layer(None).image(&page, Rect::new(100.0, 50.0, 40.0, 20.0));
    let tracker = committed(&drawn);
    for _ in 0..20 {
        page.frame_copied(copied);
    }
    assert_eq!(damage_of(&tracker, &drawn).rects(), &[PixelRect::new(100, 50, 140, 70)]);
}

/// A frame that comes after the damage was found but before it's recorded isn't taken as
/// drawn: the frame drawn is recorded with the frame compared, and the next one draws it.
#[test]
fn a_frame_put_in_while_drawing_is_drawn_by_the_next() {
    let page = ImageData::live();
    let copied = |changed| crate::CopiedFrame { size: (40, 20), changed, bytes: 0 };
    page.frame_copied(copied(None));
    let mut drawn = scene(false);
    drawn.push_layer(None).image(&page, Rect::new(100.0, 50.0, 40.0, 20.0));
    let mut tracker = committed(&drawn);
    // Nothing changed when compared…
    let damage = damage_of(&tracker, &drawn);
    assert!(damage.is_empty());
    // …a frame comes, then the frame drawn is recorded.
    page.frame_copied(copied(Some(PixelRect::new(5, 5, 6, 6))));
    tracker.commit_drawn(&drawn, SIZE, &damage);
    assert!(!damage_of(&tracker, &drawn).is_empty(), "the new frame drawn next");
}

/// A frame that comes while a whole frame is drawn — widened to full by a change elsewhere,
/// or never compared — is drawn by the next one too: a blur over the page whose layer
/// didn't change isn't made again by the whole frame, the next one finds the page's layer
/// changed and makes it again.
#[test]
fn a_frame_put_in_while_drawing_a_whole_frame_is_drawn_by_the_next() {
    let page = ImageData::live();
    let copied = |changed| crate::CopiedFrame { size: (40, 20), changed, bytes: 0 };
    page.frame_copied(copied(None));
    let mut drawn = scene(false);
    drawn.push_layer(None).image(&page, Rect::new(100.0, 50.0, 40.0, 20.0));
    drawn.push_layer(None).backdrop_blur(Rect::new(100.0, 50.0, 40.0, 20.0), 8.0, 1.0);
    let veil = Rect::new(0.0, 0.0, 200.0, 100.0);
    drawn.push_layer(None).quad(Quad::filled(veil, Rgba::TRANSPARENT));
    let mut tracker = committed(&drawn);
    // A veil over all of it changes (a full frame, above the blur), the page's layer doesn't…
    let mut changed = drawn.clone();
    changed.layer_mut(4).unwrap().quads[0].color = Rgba::WHITE;
    let damage = damage_of(&tracker, &changed);
    assert!(damage.is_full());
    assert_eq!(damage.first_layer(), Some(4), "the blur under it holds");
    // …a frame comes, then the frame drawn is recorded.
    page.frame_copied(copied(Some(PixelRect::new(5, 5, 6, 6))));
    tracker.commit_drawn(&changed, SIZE, &damage);
    let next = damage_of(&tracker, &changed);
    assert!(!next.is_empty(), "the new frame drawn next");
    assert_eq!(next.first_layer(), Some(2), "from the page's layer: the blur over it remade");

    // Not compared at all (no frame kept): the same.
    let mut whole = Damage::default();
    whole.set_full_of(&changed);
    page.frame_copied(copied(Some(PixelRect::new(5, 5, 6, 6))));
    tracker.commit_drawn(&changed, SIZE, &whole);
    assert!(!damage_of(&tracker, &changed).is_empty(), "the frame come since, drawn next");
}

/// What a damage notes of the live images is this frame's only: one used again for a frame
/// not compared forgets the last one's (an image gone, an older frame).
#[test]
fn a_whole_frame_notes_only_its_own_live_images() {
    let page = ImageData::live();
    let other = ImageData::live();
    let copied = crate::CopiedFrame { size: (40, 20), changed: None, bytes: 0 };
    page.frame_copied(copied);
    other.frame_copied(copied);
    let mut first = scene(false);
    first.push_layer(None).image(&page, Rect::new(100.0, 50.0, 40.0, 20.0));
    let tracker = committed(&first);
    let mut damage = damage_of(&tracker, &first);
    assert_eq!(damage.seen_generation(page.id), Some(page.generation()));
    page.frame_copied(copied);
    let mut second = scene(false);
    second.push_layer(None).image(&other, Rect::new(100.0, 50.0, 40.0, 20.0));
    damage.set_full_of(&second);
    assert_eq!(damage.seen_generation(page.id), None, "not in this scene");
    assert_eq!(damage.seen_generation(other.id), Some(other.generation()));
}

/// Live images are noted once each, however often a scene draws them, and each is found.
#[test]
fn live_images_drawn_twice_are_noted_once() {
    let pages: Vec<ImageData> = (0..5).map(|_| ImageData::live()).collect();
    let copied = crate::CopiedFrame { size: (40, 20), changed: None, bytes: 0 };
    let mut drawn = scene(false);
    // Backwards, then again: not in the order of their ids.
    for page in pages.iter().rev().chain(pages.iter()) {
        page.frame_copied(copied);
        drawn.push_layer(None).image(page, Rect::new(100.0, 50.0, 40.0, 20.0));
    }
    let mut damage = Damage::full();
    damage.set_full_of(&drawn);
    assert_eq!(damage.seen.len(), pages.len(), "once each");
    for page in &pages {
        assert_eq!(damage.seen_generation(page.id), Some(page.generation()));
    }
}

#[test]
fn most_of_the_surface_changing_is_a_full_frame() {
    let tracker = committed(&scene(false));
    let mut changed = scene(false);
    changed.layer_mut(0).unwrap().quads[0].color = Rgba::WHITE;
    assert!(damage_of(&tracker, &changed).is_full());
}

#[test]
fn many_rects_are_merged_to_a_few() {
    let mut damage = Damage::default();
    for i in 0..40 {
        damage.add(PixelRect::new(i * 10, (i % 3) * 20, i * 10 + 2, (i % 3) * 20 + 2));
    }
    assert!(damage.rects().len() <= MAX_RECTS);
    // Whatever was added is covered.
    for i in 0..40 {
        let r = PixelRect::new(i * 10, (i % 3) * 20, i * 10 + 2, (i % 3) * 20 + 2);
        assert!(damage.rects().iter().any(|d| d.contains(&r)), "{r:?}");
    }
    // Touching rects become one.
    let mut damage = Damage::default();
    damage.add(PixelRect::new(0, 0, 10, 10));
    damage.add(PixelRect::new(10, 0, 20, 10));
    damage.add(PixelRect::new(2, 2, 4, 4));
    assert_eq!(damage.rects(), &[PixelRect::new(0, 0, 20, 10)]);
}

#[test]
fn glyph_boxes_round_their_pen_position() {
    let b = GlyphBox { left: 2, top: 10, width: 5, height: 12 };
    assert_eq!(b.at(Point::new(10.4, 20.6)), PixelRect::new(12, 11, 17, 23));
}

/// A frame's edges changing (a neon line round the window): its four sides stay four
/// rectangles — not merged into the whole window between them — and the frame stays
/// partial; rectangles side by side, wasting nothing, still merge.
#[test]
fn a_frame_s_edges_stay_apart() {
    let mut d = Damage::default();
    let (w, h, t) = (2400, 1400, 12);
    d.add(PixelRect::new(0, 0, w, t));
    d.add(PixelRect::new(0, h - t, w, h));
    d.add(PixelRect::new(0, t, t, h - t));
    d.add(PixelRect::new(w - t, t, w, h - t));
    assert_eq!(d.rects().len(), 4, "{:?}", d.rects());
    let area: i64 = d.rects().iter().map(PixelRect::area).sum();
    assert!(area < (w as i64 * h as i64) / 10);
    d.finish(PixelRect::new(0, 0, w, h));
    assert!(!d.is_full());
    // Side by side: one.
    let mut d = Damage::default();
    d.add(PixelRect::new(0, 0, 100, 50));
    d.add(PixelRect::new(100, 0, 200, 50));
    assert_eq!(d.rects(), [PixelRect::new(0, 0, 200, 50)]);
}

/// A blur appearing, changing or going damages what it covers; the lowest layer that
/// changed is told (a blur over layers below it that didn't change still holds).
#[test]
fn blurs_are_damage_and_the_first_layer_changed_is_told() {
    let size = PhysicalSize::new(200, 100);
    let mut boxes = |_key| Some(GlyphBox::EMPTY);
    let mut tracker = DamageTracker::new();
    let mut damage = Damage::default();
    let base = |scene: &mut Scene| {
        scene.reset(Rgba::BLACK);
        scene.push_layer(None).quad(Quad::filled(Rect::new(0.0, 0.0, 20.0, 20.0), Rgba::WHITE));
    };
    let mut scene = Scene::default();
    base(&mut scene);
    tracker.damage(&scene, size, &mut boxes, &mut damage);
    assert_eq!(damage.first_layer(), Some(0));
    tracker.commit(&scene, size);
    // A blur over the corner appears in a second layer.
    base(&mut scene);
    scene.push_layer(None).backdrop_blur(Rect::new(100.0, 0.0, 50.0, 50.0), 8.0, 0.5);
    tracker.damage(&scene, size, &mut boxes, &mut damage);
    assert_eq!(damage.rects(), [PixelRect::new(100, 0, 150, 50)]);
    assert_eq!(damage.first_layer(), Some(1));
    tracker.commit(&scene, size);
    // Nothing changed.
    tracker.damage(&scene, size, &mut boxes, &mut damage);
    assert!(damage.is_empty());
    assert_eq!(damage.first_layer(), None);
    // Its opacity changes: the blur's layer only.
    base(&mut scene);
    scene.push_layer(None).backdrop_blur(Rect::new(100.0, 0.0, 50.0, 50.0), 8.0, 0.8);
    tracker.damage(&scene, size, &mut boxes, &mut damage);
    assert_eq!(damage.first_layer(), Some(1));
    tracker.commit(&scene, size);
    // Something under it changes: the first layer.
    scene.reset(Rgba::BLACK);
    scene.push_layer(None).quad(Quad::filled(Rect::new(0.0, 0.0, 30.0, 20.0), Rgba::WHITE));
    scene.push_layer(None).backdrop_blur(Rect::new(100.0, 0.0, 50.0, 50.0), 8.0, 0.8);
    tracker.damage(&scene, size, &mut boxes, &mut damage);
    assert_eq!(damage.first_layer(), Some(0));
}

#[test]
fn a_changed_emphasis_is_a_full_frame_then_partial_again() {
    let plain = scene(false);
    let mut muted = scene(false);
    let mut emphasis = crate::Emphasis::new(1.0, 0.0, 4.0);
    emphasis.keep(crate::Keep::new(Rect::new(0.0, 0.0, 50.0, 50.0), 0.0));
    muted.emphasize(emphasis);
    let tracker = committed(&plain);
    let damage = damage_of(&tracker, &muted);
    assert!(damage.is_full());
    // Blurs over it are made again (they'd read the old pixels).
    assert_eq!(damage.first_layer(), Some(0));
    // The same emphasis next frame: only what else changed.
    let tracker = committed(&muted);
    assert!(damage_of(&tracker, &muted).is_empty());
    let mut caret = scene(true);
    caret.emphasize(emphasis);
    let damage = damage_of(&tracker, &caret);
    assert!(!damage.is_full() && !damage.is_empty());
    // Turned off: full again.
    assert!(damage_of(&tracker, &plain).is_full());
}

/// An animation changing most of the surface: whole as soon as that's known — the rest of
/// the frame isn't compared (its glyphs not looked up), the outcome the same as comparing it
/// all (a frame drawn whole).
#[test]
fn a_frame_known_whole_stops_comparing() {
    let dots = |shift: f32| {
        let mut scene = Scene::new(Rgba::BLACK);
        let layer = scene.push_layer(None);
        for i in 0..400 {
            let (x, y) = ((i % 40) as f32 * 5.0 + shift, (i / 40) as f32 * 10.0);
            layer.quad(Quad::filled(Rect::new(x, y, 4.0, 4.0), Rgba::WHITE));
        }
        for i in 0..50 {
            layer.glyph(glyph(1 + i, shift + i as f32 * 3.0, 50.0));
        }
        scene
    };
    let tracker = committed(&dots(0.0));
    let mut asked = 0;
    let mut counting = |key: GlyphKey| {
        asked += 1;
        glyph_box(key)
    };
    let mut damage = Damage::default();
    tracker.damage(&dots(1.0), SIZE, &mut counting, &mut damage);
    assert!(damage.is_full());
    assert_eq!(asked, 0, "the glyphs after it weren't looked up");
    // A small change: compared to the end as before.
    let mut small = dots(0.0);
    small.push_layer(None).quad(Quad::filled(Rect::new(1.0, 1.0, 2.0, 2.0), Rgba::WHITE));
    assert!(!damage_of(&tracker, &small).is_full());
}

/// Many changes in a frame (an animation): kept in one rectangle holding them all — a
/// partial frame when it's small, a whole one once it's past the share.
#[test]
fn many_changes_are_held_in_one_rectangle() {
    let dots = |shift: f32, spread: f32| {
        let mut scene = Scene::new(Rgba::BLACK);
        let layer = scene.push_layer(None);
        for i in 0..300 {
            let (x, y) = ((i % 30) as f32 * spread + shift, (i / 30) as f32 * spread);
            layer.quad(Quad::filled(Rect::new(x, y, 1.0, 1.0), Rgba::WHITE));
        }
        scene
    };
    // In a corner: partial, one rectangle over them all.
    let tracker = committed(&dots(0.0, 1.5));
    let damage = damage_of(&tracker, &dots(0.5, 1.5));
    assert!(!damage.is_full());
    assert_eq!(damage.rects().len(), 1);
    let r = damage.rects()[0];
    assert!(r.x0 <= 0 && r.y0 <= 0 && r.x1 >= 45 && r.y1 >= 15, "{r:?}");
    // Over the whole surface: whole.
    let tracker = committed(&dots(0.0, 7.0));
    assert!(damage_of(&tracker, &dots(1.0, 7.0)).is_full());
}
