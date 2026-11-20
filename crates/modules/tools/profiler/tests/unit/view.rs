use super::*;
use page::Cell;
use profiling::{Busy, Io, Owners, Sample, Summary, System, ThreadCpu, Trigger};

#[test]
fn a_recording_s_file_is_known_by_its_name() {
    assert!(is_recording("profile-20261120-101500.json"));
    assert!(!is_recording("profile-2026-11-20.json"));
    assert!(!is_recording("profile-20261120-101500.json.tmp"));
    assert!(!is_recording("notes.json"));
}

/// A recording read on its worker, summed up, painted: the figures, the charts and the bars
/// (the module keeping the app busy first).
#[test]
fn a_recording_is_read_and_shown() {
    let dir = std::env::temp_dir().join(format!("profile-view-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut recording = Recording::new("2026-11-20T10:00:00Z".into(), 100, System::default());
    recording.owners = Owners(vec![("indexer".into(), "code.index".into())]);
    for i in 0..20 {
        recording.push(Sample {
            memory_peak: 0,
            gpu_peak: None,
            at_ms: i * 100,
            span_ms: 100,
            cpu_us: 60_000,
            memory: 1 << 20,
            gpu: Some(1 << 20),
            frames: 6,
            frame_us: 24_000,
            frame_longest_us: 6_000,
            threads: vec![ThreadCpu { thread: "indexer".into(), us: 60_000 }],
            triggers: vec![Trigger { owner: "caret".into(), cause: "deadline".into(), count: 6 }],
            busy: vec![Busy {
                owner: "editor".into(),
                what: "paint".into(),
                calls: 6,
                us: 9_000,
                longest_us: 2_000,
            }],
            io: vec![Io {
                owner: "http-client".into(),
                sent: 1000,
                received: 9000,
                ..Io::default()
            }],
            disk_read: 4096,
            disk_written: 0,
            counters: Vec::new(),
            levels: Vec::new(),
            spans: Vec::new(),
        });
    }
    let path = dir.join("profile-20261120-100000.json");
    std::fs::write(&path, recording.to_json()).unwrap();
    let mut view = ProfileView::new(&path);
    let icons = icons::Icons::builtin();
    let palette = ui_components::Palette::dark();
    let mut text = text_layout::TextSystem::new();
    let mut paint = |view: &mut ProfileView| {
        let mut scene = render_api::Scene::default();
        view.paint(ViewerPaint {
            bounds: Rect::new(0.0, 0.0, 1200.0, 400.0),
            scale: 1.0,
            text: &mut text,
            scene: &mut scene,
            palette: &palette,
            icons: &icons,
            focused: true,
            waker: None,
        });
        scene.layers().iter().map(|l| l.quads.len()).sum::<usize>()
    };
    paint(&mut view);
    let since = std::time::Instant::now();
    while view.read.is_none() && since.elapsed() < std::time::Duration::from_secs(5) {
        view.poll();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let Some(Ok(prepared)) = &view.read else { panic!("not read") };
    // Made on the worker: the module keeping the app busy first, a value a sample.
    let Cell::Text(first) = &prepared.before_flame[0].rows[0][0] else { panic!("a name") };
    assert_eq!(first, "code.index");
    assert_eq!(prepared.series[prepared::CPU].values.len(), 20);
    assert!(
        prepared.series[prepared::NETWORK].values.iter().all(|v| *v > 0.0),
        "the network over time"
    );
    assert!(prepared.text.contains("code.index"), "its summary to copy");
    assert!(paint(&mut view) > 12, "its figures, charts and bars drawn");
    assert!(view.content_h > 400.0, "taller than the window: it scrolls");
    assert!(view.scroll(0.0, 200.0));
    // Its scrollbar's thumb taken and dragged down: the page follows (as the wheel does).
    view.scroll = 0.0;
    paint(&mut view);
    let thumb = Point::new(1200.0 - 4.0, 8.0);
    assert!(view.click(thumb).is_none(), "the scrollbar takes the press");
    assert!(view.drag(Point::new(1200.0 - 4.0, 120.0)));
    assert!(view.scroll > 0.0, "dragged down");
    view.release();
    // The keys scroll it: End to the bottom, Home back.
    assert!(view.edit_key(ViewerKey::End));
    assert!(view.scroll > 0.0);
    assert!(view.edit_key(ViewerKey::Home));
    assert_eq!(view.scroll, 0.0);
    // Narrower than its labels: drawn, no panic.
    let mut scene = render_api::Scene::default();
    view.paint(ViewerPaint {
        bounds: Rect::new(0.0, 0.0, 90.0, 300.0),
        scale: 1.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
    let _ = std::fs::remove_dir_all(dir);
}

/// The flame graph's nodes: each way down a node under the way one step shorter, its own
/// part what its children don't cover; a way whose parent wasn't kept is left out.
#[test]
fn the_flame_graph_nests_the_ways_down() {
    let way = |steps: &[&str], us: u64| {
        (steps.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>(), 1, us)
    };
    let summary = Summary {
        spans: vec![
            way(&["app·frame", "log-viewer·paint"], 300),
            way(&["app·frame"], 1000),
            way(&["app·frame", "editor·paint"], 200),
            way(&["lost·parent", "x·y"], 50),
        ],
        ..Summary::default()
    };
    let nodes = flame::nodes(&summary);
    assert_eq!(nodes.len(), 3, "the orphan left out");
    assert_eq!((nodes[0].label.as_str(), nodes[0].parent), ("app·frame", None));
    assert_eq!(nodes[0].self_value, 500.0, "what its children don't cover");
    assert!(nodes[1..].iter().all(|n| n.parent == Some(0)));
    assert_eq!(nodes[1].label, "log-viewer·paint", "the costlier child first");
}
