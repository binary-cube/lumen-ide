use super::*;

/// Its actions do what its keys did (rotate, flip, zoom…), on the image's keys in its
/// view only; a key alone does nothing (the keymap sends the action).
#[test]
fn the_actions_do_what_the_keys_did() {
    let dir = std::env::temp_dir().join(format!("image-actions-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("a.svg");
    std::fs::write(&path, "<svg xmlns='http://www.w3.org/2000/svg' width='4' height='2'/>")
        .unwrap();
    let mut viewer = ImageProvider.open(&path).unwrap();
    assert!(!viewer.is_dirty());
    assert!(viewer.command("rotate_right"));
    assert!(viewer.is_dirty());
    assert!(viewer.command("rotate_left"));
    assert!(!viewer.is_dirty(), "back as it was");
    assert!(viewer.command("flip_horizontal") && viewer.is_dirty());
    assert!(!viewer.command("save_as"), "not one of its actions");
    assert!(!viewer.key('r'));

    let mut registry = extensions::ModuleRegistry::default();
    registry.register(Box::new(ImageViewerModule));
    let (mut contributions, _) = registry.activate(&Default::default());
    let commands = contributions.take::<CommandContribution>();
    assert_eq!(commands.len(), ACTIONS.len());
    assert!(commands.iter().all(|c| c.handler.when() == command_model::When::View(VIEW)));
    let rotate_left = commands.iter().find(|c| c.spec.id.0 == "image.rotate_left").unwrap();
    assert_eq!(rotate_left.spec.shortcuts, ["Shift+R"], "`R`, as before");
    let _ = std::fs::remove_dir_all(dir);
}

/// A view of `path` painted once at `bounds` and `scale`.
fn paint(viewer: &mut ImageViewer, bounds: Rect, scale: f32) {
    let mut text = text_layout::TextSystem::new();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    let mut scene = render_api::Scene::default();
    viewer.paint(ViewerPaint {
        bounds,
        scale,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
}

/// Painted until no decode is on its way.
fn settle(viewer: &mut ImageViewer, bounds: Rect, scale: f32) {
    let started = std::time::Instant::now();
    paint(viewer, bounds, scale);
    while viewer.decoding.is_some() {
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "decoded meanwhile");
        std::thread::sleep(std::time::Duration::from_millis(5));
        paint(viewer, bounds, scale);
    }
}

/// An SVG is drawn as large as it's shown in physical pixels (a Retina screen's, a zoom's),
/// and zooming in by little doesn't draw it again.
#[test]
fn an_svg_stays_sharp_on_retina_and_when_zoomed() {
    let dir = std::env::temp_dir().join(format!("image-svg-sharp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("a.svg");
    std::fs::write(
        &path,
        "<svg xmlns='http://www.w3.org/2000/svg' width='100' height='50'>\
         <rect width='100' height='50' fill='red'/></svg>",
    )
    .unwrap();
    let mut viewer = ImageViewer::open(&path).unwrap();
    let bounds = Rect::new(0.0, 0.0, 2000.0, 1600.0);
    settle(&mut viewer, bounds, 2.0);
    let side = |v: &ImageViewer| v.shown.as_ref().map(|s| s.side).unwrap_or(0);
    // Fitted at its size on a Retina screen: 200 × 100 physical pixels, drawn at least so.
    let rect = viewer.rect();
    assert_eq!(rect.size.width, 200.0);
    assert!(side(&viewer) >= 200, "{}", side(&viewer));
    // Zoomed in eight times: drawn again, as large.
    viewer.zoom(8.0, viewer.center());
    settle(&mut viewer, bounds, 2.0);
    let rect = viewer.rect();
    assert!(side(&viewer) as f32 >= rect.size.width, "{} < {}", side(&viewer), rect.size.width);
    // A little more: what's drawn still covers it, nothing decoded again.
    let before = viewer.shown.as_ref().map(|s| s.image.id);
    viewer.zoom(1.1, viewer.center());
    paint(&mut viewer, bounds, 2.0);
    assert!(viewer.decoding.is_none());
    assert_eq!(viewer.shown.as_ref().map(|s| s.image.id), before);
    // A raster image keeps its rule: never decoded over its own size.
    let target = ImageViewer { svg: false, ..ImageViewer::open(&path).unwrap() }
        .target_side(Rect::new(0.0, 0.0, 4000.0, 2000.0));
    assert_eq!(target.ask, 100);
    let _ = std::fs::remove_dir_all(dir);
}

/// A decode asked before a later one, still queued, is let go without reading the file.
#[test]
fn a_superseded_decode_is_skipped() {
    let dir = std::env::temp_dir().join(format!("image-superseded-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("a.svg");
    std::fs::write(&path, "<svg xmlns='http://www.w3.org/2000/svg' width='4' height='2'/>")
        .unwrap();
    let latest = AtomicU64::new(2);
    assert!(decode_job(&latest, 1, &path, 64, true).is_none(), "superseded");
    assert!(decode_job(&latest, 2, &path, 64, true).is_some_and(|r| r.is_ok()));
    // Asked twice: only the second counts; a purge lets it go too.
    let mut viewer = ImageViewer::open(&path).unwrap();
    viewer.decode(64, None);
    viewer.decode(128, None);
    assert_eq!(viewer.generation.load(Ordering::Relaxed), 2);
    viewer.purge();
    assert_eq!(viewer.generation.load(Ordering::Relaxed), 3);
    let _ = std::fs::remove_dir_all(dir);
}

/// A turned image is saved on a worker (the file read, turned, encoded and written off the
/// app's thread), told at a poll: meanwhile nothing is unsaved and the turning tools wait;
/// then the file is the new base, turned once.
#[test]
fn a_turned_image_is_saved_on_a_worker() {
    let dir = std::env::temp_dir().join(format!("image-save-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("wide.png");
    // A red picture 4 × 2 (PNG).
    const PNG: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 4, 0, 0, 0, 2, 8, 6,
        0, 0, 0, 127, 168, 125, 99, 0, 0, 0, 18, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 240,
        31, 25, 51, 160, 11, 0, 0, 15, 33, 15, 241, 4, 55, 198, 159, 0, 0, 0, 0, 73, 69, 78, 68,
        174, 66, 96, 130,
    ];
    std::fs::write(&path, PNG).unwrap();
    let mut viewer = ImageViewer::open(&path).unwrap();
    let woken = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = Arc::clone(&woken);
    viewer.waker = Some(Arc::new(move || flag.store(true, Ordering::SeqCst)));
    assert!(viewer.run(Tool::RotateRight));
    assert!(viewer.is_dirty());
    viewer.save().unwrap();
    assert!(viewer.saving.is_some(), "written on the worker");
    assert!(!viewer.is_dirty(), "a save on its way is no unsaved change");
    assert!(!viewer.run(Tool::RotateRight), "the image stays as it's saved meanwhile");
    let started = std::time::Instant::now();
    while viewer.saving.is_some() {
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "saved meanwhile");
        std::thread::sleep(std::time::Duration::from_millis(5));
        viewer.poll();
    }
    let (size, _) = image_decode::probe(&path).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(woken.load(Ordering::SeqCst), "the app woken for it");
    assert_eq!(size, (2, 4), "turned once, in the file");
    assert_eq!(viewer.original, (2, 4));
    assert!(viewer.transform.is_identity() && !viewer.is_dirty());
}
