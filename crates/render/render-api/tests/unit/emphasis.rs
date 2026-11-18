use super::*;
use crate::{Quad, Scene};
use kernel::{Rect, Rgba};

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-5
}

#[test]
fn gray_keeps_the_luma_and_dim_darkens() {
    let red = [1.0, 0.0, 0.0];
    let gray = mute(red, 1.0, 0.0);
    assert!(gray.iter().all(|c| close(*c, LUMA[0])), "{gray:?}");
    // Half way: between the color and its luma.
    let half = mute(red, 0.5, 0.0);
    assert!(close(half[0], (1.0 + LUMA[0]) / 2.0) && close(half[1], LUMA[0] / 2.0));
    // A gray stays the same gray; dimmed, darker by as much.
    let mid = [0.5, 0.5, 0.5];
    assert!(mute(mid, 1.0, 0.0).iter().all(|c| close(*c, 0.5)));
    assert!(mute(mid, 0.0, 0.2).iter().all(|c| close(*c, 0.4)));
}

#[test]
fn the_operator_is_linear_so_blending_commutes_with_it() {
    // Muting each primitive before blending gives the muted blend (what lets the GPU apply
    // it per primitive instead of in a pass over the frame).
    let (src, dst, a) = ([0.6, 0.1, 0.3], [0.2, 0.8, 0.4], 0.7);
    let blend = |s: [f32; 3], d: [f32; 3]| [0, 1, 2].map(|i| s[i] * a + d[i] * (1.0 - a));
    let (g, d) = (0.75, 0.1);
    let after = mute(blend(src, dst), g, d);
    let before = blend(mute(src, g, d), mute(dst, g, d));
    assert!((0..3).all(|i| close(after[i], before[i])), "{after:?} {before:?}");
}

#[test]
fn kept_areas_stay_with_a_soft_edge() {
    let mut e = Emphasis::new(1.0, 0.0, 10.0);
    assert!(e.keep(Keep::new(Rect::new(100.0, 100.0, 50.0, 50.0), 0.0)));
    assert_eq!(e.mask(120.0, 120.0), 1.0);
    let edge = e.mask(155.0, 120.0);
    assert!(edge > 0.0 && edge < 1.0, "{edge}");
    assert_eq!(e.mask(170.0, 120.0), 0.0);
    assert_eq!(e.apply([1.0, 0.0, 0.0], 120.0, 120.0), [1.0, 0.0, 0.0]);
    let far = e.apply([1.0, 0.0, 0.0], 10.0, 10.0);
    assert!(close(far[1], LUMA[0]));
}

#[test]
fn a_hard_edge_without_feather_and_weights_add_up() {
    let mut e = Emphasis::new(1.0, 0.0, 0.0);
    let r = Rect::new(0.0, 0.0, 10.0, 10.0);
    e.keep(Keep { rect: r, radius: 0.0, weight: 0.5 });
    assert_eq!(e.mask(5.5, 5.5), 0.5);
    assert_eq!(e.mask(10.5, 5.5), 0.0);
    e.keep(Keep { rect: r, radius: 0.0, weight: 0.75 });
    assert_eq!(e.mask(5.5, 5.5), 1.0);
}

#[test]
fn at_most_max_keeps_and_none_empty() {
    let mut e = Emphasis::new(1.0, 0.0, 0.0);
    assert!(!e.keep(Keep::new(Rect::new(0.0, 0.0, 0.0, 10.0), 0.0)));
    for i in 0..MAX_KEEPS {
        assert!(e.keep(Keep::new(Rect::new(i as f32, 0.0, 1.0, 1.0), 0.0)));
    }
    assert!(!e.keep(Keep::new(Rect::new(0.0, 0.0, 1.0, 1.0), 0.0)));
    assert_eq!(e.keeps().len(), MAX_KEEPS);
}

#[test]
fn the_scene_mutes_the_layers_pushed_before_it_only() {
    let mut scene = Scene::new(Rgba::BLACK);
    // Nothing under it: left out.
    scene.emphasize(Emphasis::new(1.0, 0.0, 0.0));
    assert!(scene.emphasis().is_none());
    scene.push_layer(None).quad(Quad::filled(Rect::new(0.0, 0.0, 4.0, 4.0), Rgba::WHITE));
    scene.push_layer(None);
    scene.emphasize(Emphasis::new(1.0, 0.0, 0.0));
    scene.push_layer(None);
    assert_eq!(scene.emphasis().map(Emphasis::below), Some(2));
    // One that changes nothing is left out; a new frame has none.
    scene.emphasize(Emphasis::new(0.0, 0.0, 4.0));
    assert!(scene.emphasis().is_none());
    scene.emphasize(Emphasis::new(0.5, 0.0, 0.0));
    scene.reset(Rgba::BLACK);
    assert!(scene.emphasis().is_none());
}

#[test]
fn outside_its_reach_everything_is_muted_fully() {
    let mut e = Emphasis::new(1.0, 0.0, 10.0);
    assert_eq!(e.reach(), None);
    e.keep(Keep::new(Rect::new(100.0, 100.0, 50.0, 50.0), 8.0));
    e.keep(Keep::new(Rect::new(300.0, 100.0, 50.0, 50.0), 0.0));
    let reach = e.reach().unwrap();
    assert_eq!(reach, Rect::new(90.0, 90.0, 270.0, 70.0));
    // Just outside it, and every pixel row by row inside it, as the mask says.
    assert_eq!(e.mask(89.5, 120.0), 0.0);
    for x in (85..365).map(|x| x as f32 + 0.5) {
        let m = e.mask(x, 125.0);
        assert!((0.0..=1.0).contains(&m));
        if !(90.0..360.0).contains(&x) {
            assert_eq!(m, 0.0, "{x}");
        }
    }
    // The corners rounded: the very corner of a rounded area is on its soft edge.
    let corner = e.mask(100.5, 100.5);
    assert!(corner > 0.0 && corner < 1.0, "{corner}");
}
