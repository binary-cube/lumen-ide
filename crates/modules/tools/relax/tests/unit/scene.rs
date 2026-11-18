use super::*;

fn window() -> Rect {
    Rect::new(0.0, 0.0, 2400.0, 1400.0)
}

fn colors() -> SceneColors {
    SceneColors::from_palette(&Palette::dark())
}

#[test]
fn the_registry_lists_each_scene_once_and_cycles_through_them() {
    let ids: Vec<&str> = SCENES.iter().map(|s| s.id).collect();
    assert_eq!(ids, ["stars", "waves", "ribbon"]);
    assert!(SCENES.iter().filter(|s| s.three_d).count() >= 1, "at least one is 3D");
    for info in &SCENES {
        assert_eq!(find(info.id).map(|s| s.id), Some(info.id));
        assert_eq!((info.make)().id(), info.id, "made as listed");
        assert_eq!(previous_before(next_after(info.id).id).id, info.id);
    }
    assert_eq!(next_after("ribbon").id, "stars", "after the last, the first");
    assert_eq!(next_after("gone").id, "stars", "an unknown one: the first");
    assert_eq!(find("gone").map(|s| s.id), None);
}

#[test]
fn every_scene_draws_the_same_at_the_same_time_and_moves_with_it() {
    for info in &SCENES {
        let picture = |t: f32| {
            let mut scene = (info.make)();
            scene.resize(window(), 2.0);
            scene.set_density(1.0);
            scene.set_colors(&colors());
            scene.update(t);
            let mut out = render_api::Scene::default();
            scene.paint(&mut out, window());
            let quads: Vec<Quad> = out.layers().iter().flat_map(|l| l.quads.clone()).collect();
            let images: Vec<Rect> =
                out.layers().iter().flat_map(|l| l.images.iter().map(|i| i.bounds)).collect();
            (quads, images)
        };
        let (a, b) = (picture(12.5), picture(12.5));
        assert_eq!(a, b, "{}: deterministic", info.id);
        assert!(a.0.len() + a.1.len() > 20, "{}: something drawn", info.id);
        assert_ne!(a, picture(14.0), "{}: it moves", info.id);
    }
}

#[test]
fn the_theme_gives_the_colors() {
    let dark = SceneColors::from_palette(&Palette::dark());
    assert!(dark.dark);
    assert!(luminance(dark.sky_top) < luminance(dark.sky_bottom) + 0.05);
    // The lights are bright on a dark sky.
    assert!(dark.lights.iter().all(|c| c.to_hsl().2 >= 0.6));
    let mut light = Palette::dark();
    light.window_background = Rgba::rgb(0xf6f7fa);
    let pale = SceneColors::from_palette(&light);
    assert!(!pale.dark);
    assert!(pale.lights.iter().all(|c| c.to_hsl().2 <= 0.5), "deep lights on a pale sky");
    // A green accent (Matrix) gives green lights.
    let mut matrix = Palette::dark();
    matrix.accent = Rgba::rgb(0x00ff41);
    let (hue, _, _) = SceneColors::from_palette(&matrix).lights[0].to_hsl();
    assert!((100.0..150.0).contains(&hue), "{hue}");
}

#[test]
fn layouts_are_fixed_and_follow_the_density() {
    assert_eq!(stars::layout(1.0), stars::layout(1.0));
    assert!(stars::layout(1.5).len() > stars::layout(1.0).len());
    for i in 0..500 {
        let r = rand(i, 7);
        assert!((0.0..1.0).contains(&r));
    }
}

#[test]
fn a_star_comes_nearer_and_turns_slowly() {
    let star = stars::Star { x: 0.5, y: 0.0, z0: 0.5, magnitude: 1.0, twinkle: 0.0, color: 0 };
    let (depth, x, _) = stars::star_at(&star, 0.0);
    let sway = stars::sway(0.0).0;
    assert!((depth - 0.5).abs() < 1e-6 && (x - (0.5 - sway)).abs() < 1e-6);
    // Ten seconds on: nearer by the flight's pace, turned by the roll's.
    let (later, x, y) = stars::star_at(&star, 10.0);
    assert!((later - (0.5 - 10.0 * stars::FLIGHT)).abs() < 1e-5);
    let (sx, sy) = stars::sway(10.0);
    let a = 10.0 * stars::ROLL;
    assert!((x + sx - 0.5 * a.cos()).abs() < 1e-5 && (y + sy - 0.5 * a.sin()).abs() < 1e-5);
    // Past the nearest it starts again far away.
    let (wrapped, _, _) = stars::star_at(&star, 0.6 / stars::FLIGHT);
    assert!(wrapped > 0.85, "{wrapped}");
}

#[test]
fn the_swells_roll_toward_the_camera() {
    // The water at a place a while later is what was farther before.
    let (x, z, t) = (1.0, 5.0, 3.0);
    let h = waves::height(x, z, t);
    assert!(h.abs() <= 0.61);
    assert_ne!(h, waves::height(x, z, t + 0.5));
}

#[test]
fn a_frame_allocates_nothing_new() {
    for info in &SCENES {
        let mut scene = (info.make)();
        scene.resize(window(), 2.0);
        scene.set_density(1.0);
        scene.set_colors(&colors());
        scene.update(1.0);
        let held = scene.memory();
        for i in 0..50 {
            scene.update(1.0 + i as f32 * 0.016);
        }
        assert_eq!(scene.memory(), held, "{}: its buffers keep their size", info.id);
    }
}

/// One picture per scene (`EDITOR_RELAX_SHOTS=<dir>`), to look at: the theme's colors and
/// the scene's own, through the software renderer.
#[test]
fn shots() {
    let Some(dir) = std::env::var_os("EDITOR_RELAX_SHOTS") else { return };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    let size = render_api::PhysicalSize::new(1600, 1000);
    let bounds = Rect::new(0.0, 0.0, 1600.0, 1000.0);
    let mut text = text_layout::TextSystem::new();
    for info in &SCENES {
        for own in [false, true] {
            let mut scene = (info.make)();
            scene.resize(bounds, 1.0);
            scene.set_density(1.0);
            let c = if own { scene.own_colors() } else { colors() };
            scene.set_colors(&c);
            scene.update(37.0);
            let mut out = render_api::Scene::new(Rgba::BLACK);
            scene.paint(&mut out, bounds);
            let mut renderer = render_software::SoftwareRenderer::new(size).unwrap();
            use render_api::Renderer;
            renderer.render(&out, &mut text).unwrap();
            let name = format!("{}-{}.png", info.id, if own { "own" } else { "theme" });
            std::fs::write(dir.join(name), renderer.encode_png().unwrap()).unwrap();
        }
    }
}

/// What a frame costs on the CPU (`EDITOR_RELAX_BENCH=1`, in release): placing and pushing
/// each scene at 2400 × 1400, density 1 and 1.5.
#[test]
fn frame_cost() {
    if std::env::var_os("EDITOR_RELAX_BENCH").is_none() {
        return;
    }
    for info in &SCENES {
        for density in [1.0, 1.5] {
            let mut scene = (info.make)();
            scene.resize(window(), 2.0);
            scene.set_density(density);
            scene.set_colors(&colors());
            let mut out = render_api::Scene::default();
            let frames = 600;
            let started = std::time::Instant::now();
            for i in 0..frames {
                out.reset(Rgba::BLACK);
                scene.update(i as f32 / 60.0);
                scene.paint(&mut out, window());
            }
            let each = started.elapsed() / frames;
            let quads: usize = out.layers().iter().map(|l| l.quads.len()).sum();
            let images: usize = out.layers().iter().map(|l| l.images.len()).sum();
            println!(
                "{:9} density {density}: {each:?} a frame, {quads} quads, {images} images, {} KB held",
                info.id,
                scene.memory() / 1024
            );
        }
    }
}

#[test]
fn the_ribbon_twists() {
    // The ribbon thins to nothing where it folds, and moves.
    let thin = (0..200).map(|i| ribbon::ribbon_at(0, i as f32 / 200.0, 3.0).1.abs());
    assert!(thin.fold(f32::MAX, f32::min) < 0.003, "it folds somewhere");
    assert_ne!(ribbon::ribbon_at(0, 0.5, 3.0), ribbon::ribbon_at(0, 0.5, 4.0));
}

/// A row's heights turned on by their step are the water's, computed exactly (in `f64`):
/// within a thousandth of a unit (a crest is 0.6) across a whole row, even after an hour
/// of the scene (its phases in the thousands of radians, as precise in `f32` as that).
#[test]
fn a_row_s_heights_are_the_water_s() {
    let exact = |x: f64, z: f64, t: f64| -> f64 {
        let x = x + (t * 0.021).sin() * 2.0;
        let z = z + t * f64::from(waves::FLOW);
        waves::SWELLS
            .iter()
            .map(|&(a, kx, kz, w, p)| {
                let [a, kx, kz, w, p] = [a, kx, kz, w, p].map(f64::from);
                a * (kx * x + kz * z + w * t + p).sin()
            })
            .sum()
    };
    let mut row = Vec::new();
    for (z, t) in [(3.0_f32, 0.0_f32), (11.5, 37.2), (24.0, 4000.0)] {
        let (x0, dx, n) = (-0.8 * z, 0.0135 * z, 121);
        waves::row_heights(x0, dx, n, z, t, &mut row);
        assert_eq!(row.len(), n);
        for (j, h) in row.iter().enumerate() {
            let x = f64::from(x0) + j as f64 * f64::from(dx);
            let e = exact(x, f64::from(z), f64::from(t));
            assert!((f64::from(*h) - e).abs() < 1e-3, "z {z} t {t} point {j}: {h} vs {e}");
        }
    }
}
