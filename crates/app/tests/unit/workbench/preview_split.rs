//! A preview's Split separator: dragged, double-clicked, the narrowest halves, kept in the
//! session with its file and as the default of the viewer's next tabs.

use super::*;

const AREA: Rect = Rect::new(100.0, 50.0, 1016.0, 600.0);

#[test]
fn the_halves_follow_the_ratio_with_the_separator_between_them() {
    let (text, gap, view) = halves(AREA, 0.5, 1.0);
    assert_eq!(gap.size.width, SPLITTER_WIDTH);
    assert_eq!((text.size.width, view.size.width), (500.0, 500.0));
    assert_eq!((text.max_x(), gap.max_x()), (gap.min_x(), view.min_x()));
    assert_eq!(view.max_x(), AREA.max_x());
    let (text, _, view) = halves(AREA, 0.4, 2.0);
    assert_eq!(text.size.width + view.size.width + 2.0 * SPLITTER_WIDTH, AREA.size.width);
    assert_eq!(text.size.width, ((AREA.size.width - 32.0) * 0.4).round());
}

#[test]
fn neither_half_gets_narrower_than_the_minimum() {
    for ratio in [0.0, 0.05, 0.95, 1.0] {
        let (text, _, view) = halves(AREA, ratio, 1.0);
        assert!(text.size.width >= MIN_WIDTH && view.size.width >= MIN_WIDTH, "{ratio}");
    }
    // Dragged to the edge: stopped at the minimum.
    let ratio = ratio_at(AREA, AREA.min_x(), 1.0);
    assert_eq!(halves(AREA, ratio, 1.0).0.size.width, MIN_WIDTH);
    let ratio = ratio_at(AREA, AREA.max_x(), 1.0);
    assert_eq!(halves(AREA, ratio, 1.0).2.size.width, MIN_WIDTH);
    // Too narrow for both: half each, the ratio kept for when it widens.
    let narrow = Rect::new(0.0, 0.0, 216.0, 100.0);
    let (text, _, view) = halves(narrow, 0.8, 1.0);
    assert_eq!((text.size.width, view.size.width), (100.0, 100.0));
    let mut split = PreviewSplit::new(0.8);
    split.layout(narrow, 1.0);
    assert_eq!(split.ratio, 0.8);
}

#[test]
fn a_ratio_read_back_stays_within_the_tab() {
    assert_eq!(PreviewSplit::new(f32::NAN).ratio, DEFAULT_RATIO);
    assert_eq!(PreviewSplit::new(3.0).ratio, 1.0);
    assert_eq!(PreviewSplit::new(0.5).saved(), None, "half isn't saved");
    assert_eq!(PreviewSplit::new(0.3).saved(), Some(0.3));
}

/// While dragging, the view keeps the width it was laid out at until it's time to lay it
/// out again (not on every pixel); released, its half's width.
#[test]
fn the_view_is_laid_out_again_every_so_often_while_dragged() {
    let mut split = PreviewSplit::default();
    let (_, view) = split.layout(AREA, 1.0);
    let gap = split.splitter.bounds();
    let at = Point::new(gap.min_x() + gap.size.width / 2.0, 300.0);
    assert!(split.begin_drag(at, view.size.width));
    assert!(split.drag(Point::new(at.x + 100.0, 300.0), 1.0));
    let (_, moved) = split.layout(AREA, 1.0);
    assert_eq!(moved.size.width, view.size.width - 100.0);
    let started = std::time::Instant::now();
    split.relayout_due(moved.size.width, started);
    assert_eq!(split.live_width(), Some(view.size.width), "not yet");
    split.relayout_due(moved.size.width, started + RELAYOUT_EVERY * 2);
    assert_eq!(split.live_width(), Some(moved.size.width));
    assert!(split.end_drag());
    assert_eq!(split.live_width(), None);
}

/// The layers a view painted at its old width are clipped to its half.
#[test]
fn a_view_painted_wider_than_its_half_is_clipped_to_it() {
    let mut scene = Scene::default();
    scene.push_layer(None);
    let first = scene.layers().len();
    scene.push_layer(None);
    scene.push_layer(Some(Rect::new(0.0, 0.0, 400.0, 400.0)));
    scene.push_layer(Some(Rect::new(900.0, 0.0, 10.0, 10.0)));
    let half = Rect::new(300.0, 0.0, 500.0, 600.0);
    clip_layers(&mut scene, first, half);
    let clips: Vec<Option<Rect>> = scene.layers().iter().map(|l| l.clip).collect();
    assert_eq!(clips[0], None, "painted before the view");
    assert_eq!(clips[1], Some(half));
    assert_eq!(clips[2], Some(Rect::new(300.0, 0.0, 100.0, 400.0)));
    assert_eq!(clips[3].map(|c| c.size.width), Some(0.0), "outside: nothing shown");
}

fn workbench() -> Workbench {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    wb
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("preview-split-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    crate::workbench::canonical(&dir)
}

fn viewer(wb: &Workbench) -> &TabViewer {
    wb.tabs[wb.active].viewer.as_ref().expect("a preview")
}

fn center(r: Rect) -> Point {
    Point::new(r.min_x() + r.size.width / 2.0, r.min_y() + r.size.height / 2.0)
}

/// Opens `name` (a Markdown file) in Split.
fn open_split(wb: &mut Workbench, dir: &Path, name: &str) {
    let path = dir.join(name);
    std::fs::write(&path, "# Notes\n\nSome text.\n").unwrap();
    wb.open_file(&path).unwrap();
    wb.set_viewer_mode_split();
    assert!(viewer(wb).splits());
}

fn drag(wb: &mut Workbench, text: &mut TextSystem, dx: f32) {
    let from = center(viewer(wb).split.splitter.bounds());
    assert_eq!(wb.mouse_down(text, from, false, false, false, 1), MouseResult::StartDrag);
    wb.mouse_drag(text, Point::new(from.x + dx / 2.0, from.y));
    wb.mouse_drag(text, Point::new(from.x + dx, from.y));
    wb.mouse_up(text);
}

#[test]
fn dragging_the_separator_resizes_the_halves_and_a_double_click_halves_them() {
    let dir = scratch("drag");
    let mut text = TextSystem::new();
    let mut wb = workbench();
    open_split(&mut wb, &dir, "a.md");
    let before = (wb.tabs[wb.active].view.bounds(), viewer(&wb).bounds);
    let at = center(viewer(&wb).split.splitter.bounds());
    // Over the separator: the resize cursor, even though it's between the halves.
    assert_eq!(wb.hover(at).1, CursorIcon::ResizeHorizontal);
    let version = wb.tabs[wb.active].doc.version();
    let selections = wb.tabs[wb.active].doc.selections().clone();
    drag(&mut wb, &mut text, -120.0);
    let (editor, view) = (wb.tabs[wb.active].view.bounds(), viewer(&wb).bounds);
    assert_eq!(editor.size.width, before.0.size.width - 120.0);
    assert_eq!(view.size.width, before.1.size.width + 120.0);
    assert_eq!(view.min_x(), before.1.min_x() - 120.0);
    assert!(viewer(&wb).split.ratio < 0.5);
    // The drag selected nothing, typed nothing.
    assert_eq!(wb.tabs[wb.active].doc.selections(), &selections);
    assert_eq!(wb.tabs[wb.active].doc.version(), version);
    // The viewer's next tabs start there.
    let key = workbench_model::sizes::preview_split(viewer(&wb).provider);
    assert_eq!(wb.data.sizes.get(&key), Some(viewer(&wb).split.ratio));
    // Far to the right: stopped with the view at its narrowest.
    drag(&mut wb, &mut text, 5000.0);
    assert_eq!(viewer(&wb).bounds.size.width, MIN_WIDTH);
    // A double click: half each again, and the default forgotten.
    let at = center(viewer(&wb).split.splitter.bounds());
    assert_eq!(wb.mouse_down(&mut text, at, false, false, false, 2), MouseResult::Redraw);
    wb.mouse_up(&mut text);
    assert_eq!(viewer(&wb).split.ratio, DEFAULT_RATIO);
    assert_eq!((wb.tabs[wb.active].view.bounds(), viewer(&wb).bounds), before);
    assert_eq!(wb.data.sizes.get(&key), None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_ratio_comes_back_with_the_file_and_starts_the_viewers_next_tabs() {
    let dir = scratch("session");
    let mut text = TextSystem::new();
    let mut scene = Scene::default();
    let ratio;
    {
        let mut wb = workbench();
        wb.open_project(&dir).unwrap();
        open_split(&mut wb, &dir, "a.md");
        drag(&mut wb, &mut text, 90.0);
        ratio = viewer(&wb).split.ratio;
        assert!(ratio > 0.5);
        // Another file: it starts at the ratio just chosen.
        open_split(&mut wb, &dir, "b.md");
        assert_eq!(viewer(&wb).split.ratio, ratio);
        drag(&mut wb, &mut text, -200.0);
        wb.paint(&mut text, &mut scene);
        wb.freeze_session();
    }
    let mut wb = workbench();
    wb.open_project(&dir).unwrap();
    assert!(wb.restore_session());
    let ratios: Vec<(String, f32)> =
        wb.tabs.iter().map(|t| (t.title(), t.viewer.as_ref().unwrap().split.ratio)).collect();
    let last = ratios[1].1;
    assert_eq!(ratios[0], ("a.md".to_owned(), ratio), "{ratios:?}");
    assert!(last < 0.5, "{ratios:?}");
    // A new tab of the viewer: the last ratio chosen in the project.
    open_split(&mut wb, &dir, "c.md");
    assert_eq!(viewer(&wb).split.ratio, last);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_halves_scroll_together_both_ways_without_echoes() {
    let t0 = Instant::now();
    let at = |ms: u64| t0 + Duration::from_millis(ms);
    let mut sync = ScrollSync::default();
    // Just side by side: the view goes where the text is.
    assert_eq!(sync.step(10.0, Some(0.0), 3, at(0)), Follow::View(10.0));
    sync.seen(10.0, Some(10.0));
    assert_eq!(sync.step(10.0, Some(10.0), 3, at(16)), Follow::None);
    // The text scrolled: the view follows; its follow seen, nothing comes back.
    assert_eq!(sync.step(20.0, Some(10.0), 3, at(32)), Follow::View(20.0));
    sync.seen(20.0, Some(19.5));
    assert_eq!(sync.step(20.0, Some(19.5), 3, at(48)), Follow::None);
    // The view moving on its own while the text drives (a layout settling): not followed.
    assert_eq!(sync.step(20.0, Some(21.0), 3, at(64)), Follow::None);
    sync.seen(20.0, Some(21.0));
    // Once the lease is over, the user scrolls the view: the text follows.
    assert_eq!(sync.step(20.0, Some(30.0), 3, at(500)), Follow::Text(30.0));
    sync.seen(30.0, Some(30.0));
    assert_eq!(sync.step(30.0, Some(30.0), 3, at(516)), Follow::None);
    // The caret moved (no scroll): the view brings its line in if it must.
    assert_eq!(sync.step(30.0, Some(30.0), 80, at(532)), Follow::Reveal(80.0));
    assert_eq!(sync.step(30.0, Some(30.0), 80, at(548)), Follow::None);
}

#[test]
fn a_view_that_doesnt_follow_its_text_is_left_alone() {
    let mut sync = ScrollSync::default();
    let now = Instant::now();
    assert_eq!(sync.step(10.0, None, 0, now), Follow::None);
    assert_eq!(sync.step(50.0, None, 9, now), Follow::None);
    // Turned on again: aligned at once, as when first shown.
    assert_eq!(sync.step(50.0, Some(0.0), 9, now), Follow::View(50.0));
}
