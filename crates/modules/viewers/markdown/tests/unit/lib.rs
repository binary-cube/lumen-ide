use std::time::{Duration, SystemTime};

use text_layout::TextSystem;
use ui_components::Palette;

use super::*;

#[test]
fn purge_drops_the_layout_and_its_images() {
    let dir = std::env::temp_dir()
        .join(format!("viewer-markdown-tests-{}", std::process::id()))
        .join("purge");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"2\" height=\"1\">\
               <rect width=\"2\" height=\"1\" fill=\"red\"/></svg>";
    let image = dir.join("a.svg");
    std::fs::write(&image, svg).expect("write image");
    let file = std::fs::File::options().write(true).open(&image).expect("open image");
    file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000)).expect("mtime");

    let mut viewer = MarkdownViewer::new(&dir.join("a.md"));
    viewer.caches = Some(layout::Caches::inline());
    viewer.set_source("![a](a.svg)");
    let page = Page {
        source: &viewer.source,
        version: viewer.version,
        width: 600.0,
        scale: 1.0,
        dir: &viewer.dir,
        toggled: &viewer.toggled,
    };
    let caches = viewer.caches.as_mut().expect("caches");
    let layout = Layout::build(page, caches, &mut TextSystem::new(), &Palette::default());
    assert!(layout.items.iter().any(|item| matches!(item, Item::Picture { .. })));
    viewer.layout = Some(layout);

    viewer.purge();
    assert!(viewer.layout.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A heavy document's cost, measured (run with `--ignored --nocapture`): 20 diagrams and
/// 20 large photos, laid out and scrolled through, the renderer simulated (each image drawn
/// is uploaded once and kept, as the GPU's cache does for a while).
#[test]
#[ignore]
fn measure_a_heavy_document() {
    use std::collections::HashMap;
    let dir = std::env::temp_dir().join(format!("viewer-markdown-measure-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let mut source = String::from("# Heavy\n\n");
    let n: usize = std::env::var("MEASURE_IMAGES").ok().and_then(|v| v.parse().ok()).unwrap_or(20);
    for i in 0..n {
        let path = dir.join(format!("photo{i}.png"));
        let mut pixmap = tiny_skia::Pixmap::new(4000, 3000).expect("pixmap");
        pixmap.fill(tiny_skia::Color::from_rgba8((i * 12) as u8, 120, 200, 255));
        pixmap.save_png(&path).expect("png");
        source.push_str(&format!("![photo {i}](photo{i}.png)\n\n"));
        source.push_str(&format!(
            "```mermaid\nflowchart TD\n  A{i}[Start {i}] --> B{{Check}}\n  B -->|yes| C[Do it]\n  B -->|no| D[(Store)]\n  C --> E((End))\n  D --> E\n  subgraph S\n    F --> G --> H\n  end\n  E --> F\n```\n\n"
        ));
    }
    let mut viewer = MarkdownViewer::new(&dir.join("heavy.md"));
    viewer.set_source(&source);
    let mut text = TextSystem::new();
    let palette = Palette::dark();
    let icons = icons::Icons::builtin();
    let bounds = Rect::new(0.0, 0.0, 1600.0, 1800.0);
    let mut uploaded: HashMap<u64, u64> = HashMap::new();
    let mut peak_ram = 0u64;
    let started = std::time::Instant::now();
    let paint = |viewer: &mut MarkdownViewer,
                 text: &mut TextSystem,
                 uploaded: &mut HashMap<u64, u64>| {
        let mut scene = render_api::Scene::default();
        viewer.paint(ViewerPaint {
            bounds,
            scale: 2.0,
            text,
            scene: &mut scene,
            palette: &palette,
            icons: &icons,
            focused: true,
            waker: None,
        });
        for layer in scene.layers() {
            for i in &layer.images {
                if let std::collections::hash_map::Entry::Vacant(e) = uploaded.entry(i.image.id) {
                    let bytes = u64::from(i.image.width)
                        * u64::from(i.image.height)
                        * i.image.bytes_per_pixel();
                    e.insert(bytes);
                    i.image.uploaded();
                }
            }
        }
    };
    // Settled: everything it waits for arrived.
    loop {
        paint(&mut viewer, &mut text, &mut uploaded);
        peak_ram = peak_ram.max(viewer.memory().ram);
        if !viewer.is_loading() || started.elapsed() > Duration::from_secs(60) {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let first = started.elapsed();
    let settled = !viewer.is_loading();
    // Scrolled through, a screen at a time.
    let height = viewer.layout.as_ref().map_or(0.0, |l| l.height);
    let mut y = 0.0;
    while y < height {
        viewer.scroll(0.0, bounds.size.height);
        y += bounds.size.height;
        loop {
            paint(&mut viewer, &mut text, &mut uploaded);
            peak_ram = peak_ram.max(viewer.memory().ram);
            if !viewer.is_loading() || started.elapsed() > Duration::from_secs(120) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    let total = started.elapsed();
    let gpu: u64 = uploaded.values().sum();
    let budget = viewer_gpu_live();
    eprintln!(
        "MEASURE (settled: {settled}): first screen {first:?}, all scrolled {total:?}; GPU uploaded {:.1} MB in {} textures (live under the budget: {:.1} MB); peak RAM {:.1} MB",
        gpu as f64 / 1e6,
        uploaded.len(),
        budget as f64 / 1e6,
        peak_ram as f64 / 1e6
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// What the module's budget counts on the GPU now.
fn viewer_gpu_live() -> u64 {
    crate::budget::live()
}

#[test]
fn a_wide_code_block_scrolls_across_under_the_mouse() {
    let mut viewer = MarkdownViewer::new(Path::new("/tmp/wide.md"));
    viewer.set_source(&format!("```\n{}\n```\n\ntext\n", "y".repeat(500)));
    let mut text = TextSystem::new();
    let palette = Palette::dark();
    let icons = icons::Icons::builtin();
    let bounds = Rect::new(0.0, 0.0, 800.0, 600.0);
    let mut paint = |viewer: &mut MarkdownViewer| {
        let mut scene = render_api::Scene::default();
        viewer.paint(ViewerPaint {
            bounds,
            scale: 1.0,
            text: &mut text,
            scene: &mut scene,
            palette: &palette,
            icons: &icons,
            focused: true,
            waker: None,
        });
    };
    paint(&mut viewer);
    let view = viewer
        .layout
        .as_ref()
        .and_then(|l| {
            l.items.iter().find_map(|i| match i {
                Item::Scroll { view, .. } => Some(*view),
                _ => None,
            })
        })
        .expect("a block scrolling across");
    // Across, away from it: the page doesn't move.
    viewer.hover(Point::new(view.min_x() + 10.0, view.max_y() + 200.0));
    assert!(!viewer.scroll(40.0, 0.0));
    // Over it: it scrolls, within its width.
    viewer.hover(Point::new(view.min_x() + 10.0, view.min_y() + 10.0));
    assert!(viewer.scroll(40.0, 0.0));
    assert_eq!(viewer.code_scroll.get(&0), Some(&40.0));
    assert!(viewer.scroll(100_000.0, 0.0));
    let most = viewer.code_scroll[&0];
    assert!(most > 1000.0 && most < 100_000.0);
    // Down: the page.
    assert!(!viewer.scroll(0.0, 30.0) || viewer.scroll > 0.0);
    paint(&mut viewer);
}

/// The bar across of the code block under the mouse: its thumb, pressed and dragged, scrolls
/// the block (not the page, and no text is selected).
#[test]
fn a_code_block_s_bar_across_is_dragged_by_its_thumb() {
    let mut viewer = MarkdownViewer::new(Path::new("/tmp/wide.md"));
    viewer.set_source(&format!("```\n{}\n```\n\ntext\n", "y".repeat(500)));
    let mut text = TextSystem::new();
    let palette = Palette::dark();
    let icons = icons::Icons::builtin();
    let bounds = Rect::new(0.0, 0.0, 800.0, 600.0);
    let mut paint = |viewer: &mut MarkdownViewer| {
        let mut scene = render_api::Scene::default();
        viewer.paint(ViewerPaint {
            bounds,
            scale: 1.0,
            text: &mut text,
            scene: &mut scene,
            palette: &palette,
            icons: &icons,
            focused: true,
            waker: None,
        });
    };
    paint(&mut viewer);
    let view = viewer
        .layout
        .as_ref()
        .and_then(|l| {
            l.items.iter().find_map(|i| match i {
                Item::Scroll { view, .. } => Some(*view),
                _ => None,
            })
        })
        .expect("a block scrolling across");
    // Over the block: its bar shows at its bottom.
    let thickness = ui_components::Scrollbar::track_thickness(1.0);
    let thumb = Point::new(view.min_x() + 5.0, view.max_y() - thickness / 2.0);
    viewer.hover(thumb);
    paint(&mut viewer);
    assert!(viewer.pointer_down(thumb, 1, PLAIN).is_none());
    assert!(viewer.selecting.is_none(), "no text selection started");
    assert!(viewer.drag(Point::new(thumb.x + 100.0, thumb.y)));
    assert!(viewer.code_scroll.get(&0).is_some_and(|s| *s > 0.0), "the block scrolled");
    assert_eq!(viewer.scroll, 0.0, "the page stayed");
    assert!(viewer.release());
}

/// A viewer of `source` painted once in a view away from the window's corner.
fn painted(source: &str) -> (MarkdownViewer, Rect) {
    let mut viewer = MarkdownViewer::new(Path::new("/tmp/select.md"));
    viewer.caches = Some(layout::Caches::inline());
    viewer.drawn = layout::Drawn::inline();
    viewer.set_source(source);
    let bounds = Rect::new(100.0, 50.0, 800.0, 600.0);
    let mut text = TextSystem::new();
    let palette = Palette::dark();
    let icons = icons::Icons::builtin();
    let mut scene = render_api::Scene::default();
    viewer.paint(ViewerPaint {
        bounds,
        scale: 1.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
    (viewer, bounds)
}

/// Where `word` is drawn (window coordinates), `dx` into it.
fn at_word(viewer: &MarkdownViewer, bounds: Rect, word: &str, dx: f32) -> Point {
    let layout = viewer.layout.as_ref().expect("laid out");
    let r = layout.runs.iter().find(|r| r.text.starts_with(word)).expect("the word");
    Point::new(
        bounds.min_x() + r.rect.min_x() + dx,
        bounds.min_y() + r.rect.center().y - viewer.scroll,
    )
}

const PLAIN: ui_components::PressMods =
    ui_components::PressMods { shift: false, primary: false, alt: false };

#[test]
fn dragging_selects_across_blocks_and_copies_plain_text() {
    let (mut v, b) = painted("First paragraph.\n\n- item one\n- item two\n\nLast words.");
    assert!(v.pointer_down(at_word(&v, b, "paragraph", 0.0), 1, PLAIN).is_none());
    assert!(v.drag(at_word(&v, b, "two", 100.0)));
    assert!(v.release());
    assert_eq!(v.selected_text().as_deref(), Some("paragraph.\n• item one\n• item two"));
    // ⇧ + click extends it.
    v.pointer_down(
        at_word(&v, b, "Last", 13.0),
        1,
        ui_components::PressMods { shift: true, ..PLAIN },
    );
    v.release();
    let text = v.selected_text().unwrap_or_default();
    assert!(text.starts_with("paragraph.") && text.ends_with("two\n\nLa"), "{text:?}");
    // A click without moving clears it.
    v.pointer_down(at_word(&v, b, "First", 1.0), 1, PLAIN);
    v.release();
    assert_eq!(v.selected_text(), None);
}

#[test]
fn double_and_triple_clicks_select_a_word_and_a_paragraph() {
    let (mut v, b) = painted("Some **bold**er words here.\n\nNext.");
    v.pointer_down(at_word(&v, b, "bold", 2.0), 2, PLAIN);
    v.release();
    assert_eq!(v.selected_text().as_deref(), Some("bolder"));
    v.pointer_down(at_word(&v, b, "words", 2.0), 3, PLAIN);
    v.release();
    assert_eq!(v.selected_text().as_deref(), Some("Some bolder words here."));
    // Dragging after a double click grows by words.
    v.pointer_down(at_word(&v, b, "Some", 1.0), 2, PLAIN);
    v.drag(at_word(&v, b, "words", 3.0));
    v.release();
    assert_eq!(v.selected_text().as_deref(), Some("Some bolder words"));
}

#[test]
fn select_all_copy_and_the_menu() {
    let (mut v, _) = painted("# Title\n\nBody with `code`.\n\n| a | b |\n|---|---|\n| 1 | 2 |");
    assert_eq!(v.selected_text(), None);
    let menu = v.context_menu(Point::new(120.0, 60.0)).expect("a menu");
    let ids = |menu: &[ui_components::ViewMenuEntry]| -> Vec<String> {
        menu.iter()
            .filter_map(|e| match e {
                ui_components::ViewMenuEntry::Item { id, .. } => Some(id.clone()),
                _ => None,
            })
            .collect()
    };
    assert_eq!(ids(&menu), ["markdown.select_all"]);
    assert!(v.command("markdown.select_all"));
    assert_eq!(v.selected_text().as_deref(), Some("Title\n\nBody with code.\n\na\tb\n1\t2"));
    let menu = v.context_menu(Point::new(120.0, 60.0)).expect("a menu");
    assert_eq!(ids(&menu), ["markdown.copy", "markdown.copy_markdown", "markdown.select_all"]);
    assert!(v.command("markdown.copy"));
    assert!(matches!(
        v.take_action(),
        Some(ViewerAction::Request(AppRequest::CopyText(t))) if t.starts_with("Title\n\nBody")
    ));
    assert!(v.command("markdown.copy_markdown"));
    assert!(matches!(
        v.take_action(),
        Some(ViewerAction::Request(AppRequest::CopyText(t))) if t.starts_with("# Title\n\nBody with `code`.")
    ));
    assert!(v.clear_selection());
    assert_eq!(v.selected_text(), None);
}

#[test]
fn a_link_follows_on_a_click_and_selects_on_a_drag() {
    let (mut v, b) = painted("See [the docs](https://example.com/docs) now.");
    let on = at_word(&v, b, "docs", 2.0);
    // Pressed and released in place: followed.
    assert!(v.pointer_down(on, 1, PLAIN).is_none());
    assert!(v.release());
    assert!(
        matches!(v.take_action(), Some(ViewerAction::OpenLink(u)) if u == "https://example.com/docs")
    );
    // Pressed and dragged: text selected, nothing opened.
    let the = at_word(&v, b, "the", 0.0);
    v.pointer_down(the, 1, PLAIN);
    assert!(!v.drag(Point::new(the.x + 1.0, the.y)), "a tremble isn't a drag");
    assert!(v.drag(at_word(&v, b, "now", 0.0)));
    v.release();
    assert!(v.take_action().is_none());
    assert_eq!(v.selected_text().as_deref(), Some("the docs"));
}

#[test]
fn dragging_past_the_bottom_scrolls_and_selects_on() {
    let source: String = (0..80).map(|i| format!("Paragraph number {i}.\n\n")).collect();
    let (mut v, b) = painted(&source);
    v.pointer_down(at_word(&v, b, "Paragraph", 0.0), 1, PLAIN);
    v.drag(Point::new(b.min_x() + 200.0, b.max_y() + 10.0));
    assert!(v.is_animating());
    let before = v.selected_text().unwrap_or_default().len();
    v.edge_tick = Some(std::time::Instant::now() - std::time::Duration::from_millis(50));
    v.edge_scroll();
    assert!(v.scroll > 0.0);
    assert!(v.selected_text().unwrap_or_default().len() > before);
    v.release();
    assert!(!v.is_animating());
}

#[test]
fn the_scrollbar_is_dragged_and_takes_the_press_before_the_text() {
    let source: String = (0..120).map(|i| format!("Paragraph number {i}.\n\n")).collect();
    let (mut v, b) = painted(&source);
    let lane = |y: f32| Point::new(b.max_x() - 3.0, y);
    // Hidden at rest: a press there selects text as anywhere else.
    assert!(v.vbar.press(lane(b.min_y() + 300.0)).is_none());
    // The mouse to its lane doesn't show it (only a scroll does).
    v.hover(lane(b.min_y() + 300.0));
    assert!(v.vbar.press(lane(b.min_y() + 300.0)).is_none(), "still hidden");
    // Scrolled (and back): shown; a press on the track jumps there, a drag follows.
    v.scroll = 1.0;
    let mut text = TextSystem::new();
    let palette = Palette::dark();
    let icons = icons::Icons::builtin();
    let mut scene = render_api::Scene::default();
    v.paint(ViewerPaint {
        bounds: b,
        scale: 1.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
    assert!(v.pointer_down(lane(b.min_y() + 300.0), 1, PLAIN).is_none());
    assert!(v.scroll > 1.0, "jumped down the track");
    assert!(v.selection.is_none() && v.selecting.is_none(), "no text selection started");
    let jumped = v.scroll;
    assert!(v.drag(lane(b.min_y() + 400.0)));
    assert!(v.scroll > jumped);
    assert!(v.release());
    assert!(!v.vbar.is_dragging());
    // Painted again, as the app does after the release.
    v.paint(ViewerPaint {
        bounds: b,
        scale: 1.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
    assert!(v.frame_deadline().is_some(), "it fades out after a while");
}

#[test]
fn the_preview_follows_its_text_and_tells_where_it_is() {
    let source: String = (0..200).map(|i| format!("Paragraph number {i}.\n\n")).collect();
    let (mut v, _) = painted(&source);
    assert_eq!(v.source_line_at_top(), Some(0.0));
    // Brought to line 100 (paragraph 50), it says it's there.
    assert!(v.scroll_to_source(100.0));
    let top = v.source_line_at_top().expect("follows");
    assert!((top - 100.0).abs() < 0.05, "{top}");
    let layout = v.layout.as_ref().expect("laid out");
    let para = layout.runs.iter().find(|r| r.text == "50.").expect("paragraph 50");
    assert!((para.rect.min_y() - v.scroll).abs() < 2.0);
    // The caret on a line in view: nothing moves; far away: it comes into view.
    assert!(!v.reveal_source_line(104.0));
    assert!(v.reveal_source_line(300.0));
    let y = v.layout.as_ref().unwrap().lines.y_of(300.0);
    assert!(y >= v.scroll && y <= v.scroll + v.bounds.size.height);
    // Turned off: it neither follows nor tells.
    settings::set_sync_scroll(false);
    let off = (v.source_line_at_top(), v.scroll_to_source(10.0), v.reveal_source_line(0.0));
    settings::set_sync_scroll(true);
    assert_eq!(off, (None, false, false));
}

/// A view painted once at `bounds` (scale 1, the dark palette).
fn paint_once(viewer: &mut MarkdownViewer, text: &mut TextSystem, bounds: Rect) {
    paint_woken(viewer, text, bounds, None);
}

/// Painted once with `waker` (the app's: its work on the workers).
fn paint_woken(
    viewer: &mut MarkdownViewer,
    text: &mut TextSystem,
    bounds: Rect,
    waker: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
) {
    let palette = Palette::dark();
    let icons = icons::Icons::builtin();
    let mut scene = render_api::Scene::default();
    viewer.paint(ViewerPaint {
        bounds,
        scale: 1.0,
        text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker,
    });
}

/// Painted until its layout is of its text's version (built on a worker).
/// Paints until the text's layout is in place and nothing more is being built (a build the
/// caches asked for after it, under load, comes too).
fn settle(viewer: &mut MarkdownViewer, text: &mut TextSystem, bounds: Rect) {
    let version = viewer.version;
    let started = std::time::Instant::now();
    while viewer.layout.as_ref().is_none_or(|l| l.version != version) || viewer.building.is_some() {
        assert!(started.elapsed() < Duration::from_secs(20), "laid out meanwhile");
        std::thread::sleep(Duration::from_millis(2));
        paint_once(viewer, text, bounds);
    }
}

/// A big text is laid out on a worker: the paint doesn't wait for it, the layout there is
/// drawn meanwhile, and the line at the top stays there once the new one is in place.
#[test]
fn a_big_text_is_laid_out_off_the_paint_and_nothing_jumps() {
    let source: String = (0..1500).map(|i| format!("Paragraph number {i}.\n\n")).collect();
    assert!(source.len() > build::FIRST_INLINE);
    let mut viewer = MarkdownViewer::new(Path::new("/tmp/big.md"));
    viewer.set_source(&source);
    let mut text = TextSystem::new();
    let bounds = Rect::new(0.0, 0.0, 800.0, 600.0);
    paint_once(&mut viewer, &mut text, bounds);
    assert!(viewer.building.is_some() || viewer.layout.is_some(), "asked for");
    settle(&mut viewer, &mut text, bounds);
    // Scrolled to line 1000 (paragraph 500), then edited above it.
    viewer.scroll = viewer.layout.as_ref().expect("laid out").lines.y_of(1000.0);
    let line = viewer.source_line_at_top().expect("follows");
    let shown = viewer.layout.as_ref().map(|l| l.version);
    viewer.set_source(&source.replacen("Paragraph number 3.", "Paragraph number three.", 1));
    paint_once(&mut viewer, &mut text, bounds);
    // The paint didn't wait: the old layout is still the one drawn.
    assert_eq!(viewer.layout.as_ref().map(|l| l.version), shown);
    assert!(viewer.building.is_some());
    assert!(viewer.is_loading(), "the app keeps polling while it's built");
    settle(&mut viewer, &mut text, bounds);
    let top = viewer.source_line_at_top().expect("follows");
    assert!((top - line).abs() < 0.05, "{top} vs {line}");
    assert!(viewer.building.is_none() && viewer.caches.is_some(), "the caches came back");
}

/// A small text with nothing shown yet is laid out right away (no empty first frame).
#[test]
fn a_small_text_is_shown_at_the_first_frame() {
    let mut viewer = MarkdownViewer::new(Path::new("/tmp/small.md"));
    viewer.set_source("# Title\n\nSome text.");
    let mut text = TextSystem::new();
    paint_once(&mut viewer, &mut text, Rect::new(0.0, 0.0, 800.0, 600.0));
    assert!(viewer.layout.is_some());
    assert!(viewer.building.is_none());
}

/// Purged while a layout is built: the build is let go, the next paint builds anew.
#[test]
fn a_purge_lets_a_build_on_its_way_go() {
    let source: String = (0..1500).map(|i| format!("Paragraph number {i}.\n\n")).collect();
    let mut viewer = MarkdownViewer::new(Path::new("/tmp/purged.md"));
    viewer.set_source(&source);
    let mut text = TextSystem::new();
    let bounds = Rect::new(0.0, 0.0, 800.0, 600.0);
    paint_once(&mut viewer, &mut text, bounds);
    viewer.purge();
    assert!(viewer.building.is_none() && viewer.caches.is_some() && viewer.layout.is_none());
    settle(&mut viewer, &mut text, bounds);
}

/// Built and shown, a document nothing changes in asks for nothing more: not loading (the
/// app would keep drawing it), no build started by the next paints.
#[test]
fn a_settled_document_asks_for_no_more_frames() {
    let source: String = (0..1500)
        .map(|i| {
            format!(
                "## Section {i}\n\nSome **bold** text and `code` with a [link](https://example.com) in paragraph {i}.\n\n- item one\n- item two\n\n"
            )
        })
        .collect();
    let mut viewer = MarkdownViewer::new(Path::new("/tmp/settled.md"));
    viewer.set_source(&source);
    let mut text = TextSystem::new();
    // A width in fractions of a pixel (a split pane's), as the app's may be.
    let bounds = Rect::new(0.5, 0.0, 812.25, 600.0);
    // As the app paints it: a waker, its work on the workers, painted while it loads.
    let waker: std::sync::Arc<dyn Fn() + Send + Sync> = std::sync::Arc::new(|| {});
    let started = std::time::Instant::now();
    paint_woken(&mut viewer, &mut text, bounds, Some(waker.clone()));
    while viewer.is_loading() {
        assert!(started.elapsed() < Duration::from_secs(20), "still loading");
        std::thread::sleep(Duration::from_millis(5));
        paint_woken(&mut viewer, &mut text, bounds, Some(waker.clone()));
    }
    for _ in 0..3 {
        paint_woken(&mut viewer, &mut text, bounds, Some(waker.clone()));
        assert!(viewer.building.is_none(), "no build for nothing changed");
        assert!(!viewer.is_loading(), "nothing loading");
        assert!(!viewer.is_animating() && viewer.frame_deadline().is_none());
    }
}
