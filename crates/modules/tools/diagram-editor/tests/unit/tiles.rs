use super::*;

/// The scale's steps: half an octave apart.
#[test]
fn scales_round_to_their_steps() {
    assert_eq!(step(1.0), (0, 1.0));
    assert_eq!(step(2.0).0, 2);
    assert!((step(1.3).1 - std::f32::consts::SQRT_2).abs() < 1e-4);
}

/// Without a waker (a snapshot): made here, drawn at once.
#[test]
fn without_a_waker_tiles_are_made_at_once() {
    let drawing = Arc::new(crate::drawing::tests::flow());
    let mut tiles = Tiles::default();
    let at =
        Place { corner: Point::new(0.0, 0.0), k: 1.0, view: Rect::new(0.0, 0.0, 800.0, 600.0) };
    let (drawn, whole) = tiles.visible(&drawing, 1, at);
    assert!(whole && drawn.iter().map(|(m, _, _)| m.len()).sum::<usize>() > 0);
}

/// With a waker (the app): the tiles in view are asked for, made on the worker, given at
/// the next poll (the app woken); drawn from then on.
#[test]
fn tiles_in_view_are_made_and_kept() {
    let drawing = Arc::new(crate::drawing::tests::flow());
    let mut tiles = Tiles::default();
    let woken = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = Arc::clone(&woken);
    let waker: Waker = Arc::new(move || flag.store(true, std::sync::atomic::Ordering::SeqCst));
    tiles.set_waker(Some(&waker));
    let at =
        Place { corner: Point::new(0.0, 0.0), k: 1.0, view: Rect::new(0.0, 0.0, 800.0, 600.0) };
    let (first, whole) = tiles.visible(&drawing, 1, at);
    assert!(first.is_empty() && !whole, "asked, not made yet");
    let started = std::time::Instant::now();
    while !tiles.poll() && started.elapsed() < std::time::Duration::from_secs(10) {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let mut whole = false;
    let mut drawn = Vec::new();
    while !whole && started.elapsed() < std::time::Duration::from_secs(10) {
        tiles.poll();
        (drawn, whole) = tiles.visible(&drawing, 1, at);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(whole && woken.load(std::sync::atomic::Ordering::SeqCst), "the app woken");
    let masks: usize = drawn.iter().map(|(m, _, _)| m.len()).sum();
    assert!(masks > 0, "the shapes drawn");
    assert!(tiles.bytes() > 0);
}

/// A drawing changed (an edit): while its tiles are made, the last whole set is drawn (the
/// drawing before), never a blank and never a mix.
#[test]
fn an_edit_shows_the_drawing_before_until_its_tiles_come() {
    let drawing = Arc::new(crate::drawing::tests::flow());
    let mut tiles = Tiles::default();
    let at =
        Place { corner: Point::new(0.0, 0.0), k: 1.0, view: Rect::new(0.0, 0.0, 800.0, 600.0) };
    let (before, whole) = tiles.visible(&drawing, 1, at);
    assert!(whole && !before.is_empty());
    // Generation 2, its tiles on the worker (a waker known now).
    let waker: Waker = Arc::new(|| {});
    tiles.set_waker(Some(&waker));
    let (shown, whole) = tiles.visible(&drawing, 2, at);
    assert!(!whole);
    assert_eq!(shown.len(), before.len(), "the whole set before, only");
}

/// An edit (a shape moved a little): the tiles it doesn't reach are the new drawing's too —
/// each the same, pixel for pixel, as made anew — and the others are made again.
#[test]
fn an_edit_keeps_the_tiles_it_does_not_reach() {
    use diagram_model::{Diagram, Document, ops};
    const FLOW: &str =
        include_str!("../../../../../domain/tools/diagram-model/tests/data/flow.drawio");
    let theme = crate::drawing::tests::theme();
    let mut d = Diagram::new(Document::read(FLOW).unwrap());
    let before = Arc::new(crate::drawing::draw(&d.doc.pages[0], &theme));
    let tx = ops::move_by(&d.doc, 0, &["save"], 0.0, 4.0);
    assert!(d.edit(tx, None));
    let now = Arc::new(crate::drawing::draw(&d.doc.pages[0], &theme));
    assert_eq!(before.bounds, now.bounds, "the page's box is the same");
    let k = 2.0;
    let mut tiles = Tiles::default();
    let view = Rect::new(0.0, 0.0, 4000.0, 4000.0);
    let at = Place { corner: Point::new(0.0, 0.0), k, view };
    let (_, whole) = tiles.visible(&before, 1, at);
    assert!(whole);
    let all = tiles.made.len();
    tiles.carry(&before, 1, &now, 2);
    let carried: Vec<Key> = tiles.made.keys().filter(|k| k.0 == 2).copied().collect();
    assert!(!carried.is_empty() && carried.len() < all, "{} of {all} carried", carried.len());
    let size = (now.bounds.size.width, now.bounds.size.height);
    let pixels = |masks: &[Mask]| -> Vec<(Rect, Vec<u8>)> {
        masks.iter().map(|(i, r)| (*r, i.with_pixels(<[u8]>::to_vec).unwrap())).collect()
    };
    for key in carried {
        let fresh = vector_tiles::tile(size, now.shapes(), step(k).1, key.2, key.3);
        assert_eq!(pixels(&tiles.made[&key].0), pixels(&fresh), "tile {key:?}");
    }
    // Drawn: the rest made again, then the old drawing's tiles let go.
    let (_, whole) = tiles.visible(&now, 2, at);
    assert!(whole);
    assert!(tiles.made.keys().all(|k| k.0 == 2), "the old ones let go");
}

/// Tiles carried across an edit count once, and a purge right after frees them all (each is
/// under two keys: the last to go frees it), as a closed tab's do.
#[test]
fn a_purge_after_an_edit_frees_the_carried_tiles() {
    use diagram_model::{Diagram, Document, ops};
    const FLOW: &str =
        include_str!("../../../../../domain/tools/diagram-model/tests/data/flow.drawio");
    let theme = crate::drawing::tests::theme();
    let mut d = Diagram::new(Document::read(FLOW).unwrap());
    let before = Arc::new(crate::drawing::draw(&d.doc.pages[0], &theme));
    assert!(d.edit(ops::move_by(&d.doc, 0, &["save"], 0.0, 4.0), None));
    let now = Arc::new(crate::drawing::draw(&d.doc.pages[0], &theme));
    let at =
        Place { corner: Point::new(0.0, 0.0), k: 2.0, view: Rect::new(0.0, 0.0, 4000.0, 4000.0) };
    let mut tiles = Tiles::default();
    assert!(tiles.visible(&before, 1, at).1);
    let held = tiles.memory();
    tiles.carry(&before, 1, &now, 2);
    assert_eq!(tiles.memory(), held, "a carried tile counted once");
    let images: Vec<_> =
        tiles.made.values().flat_map(|(m, _)| m.iter().map(|(i, _)| i.clone())).collect();
    tiles.clear();
    assert!(images.iter().all(|i| i.released()), "every texture freed");
    // A closed tab's too.
    let mut tiles = Tiles::default();
    assert!(tiles.visible(&now, 2, at).1);
    let images: Vec<_> =
        tiles.made.values().flat_map(|(m, _)| m.iter().map(|(i, _)| i.clone())).collect();
    drop(tiles);
    assert!(images.iter().all(|i| i.released()));
}
