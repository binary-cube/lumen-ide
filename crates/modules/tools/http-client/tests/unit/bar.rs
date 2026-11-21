//! The bar over a `.http` file: the request at the cursor found from the text read — a
//! small file's at once, a big one's on a worker (the one before serving meanwhile).

use super::*;

fn bar() -> HttpBar {
    let runs = Runs::of(Some(kernel::ProjectId::new()), None);
    HttpBar::new(runs, Path::new("/p/a.http"), None, Arc::new(|| {}))
}

fn state(text: &str, version: u64, cursor: usize) -> EditorState {
    EditorState { text: Arc::from(text), version, cursor, selection: None }
}

#[test]
fn a_small_file_is_read_at_once() {
    let mut bar = bar();
    let text = "### A\nGET https://e.com/a\n\n### B\nGET https://e.com/b\n";
    bar.sync_editor(&state(text, 1, text.find("e.com/b").unwrap()));
    assert_eq!(bar.parsed_version, 1);
    let current = bar.current().unwrap();
    assert!(text[current].contains("/b"));
    assert!(!bar.parsing.is_busy());
}

#[test]
fn a_big_file_is_read_on_a_worker() {
    let mut bar = bar();
    let body = "x".repeat(PARSED_HERE + 1);
    let text = format!("### A\nPOST https://e.com/a\n\n{body}\n\n### B\nGET https://e.com/b\n");
    bar.sync_editor(&state(&text, 1, text.find("e.com/b").unwrap()));
    assert_ne!(bar.parsed_version, 1, "not read here");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while bar.parsed_version != 1 && std::time::Instant::now() < deadline {
        bar.poll();
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(bar.parsed_version, 1, "read on its worker, taken by a poll");
    let current = bar.current().unwrap();
    assert!(text[current].contains("/b"));
    // An edit meanwhile: the read before still serves, never cut where the text isn't.
    let shorter = format!("é{}", &text[1..]);
    bar.sync_editor(&state(&shorter, 2, shorter.len() - 1));
    assert_eq!(bar.parsed_version, 1, "the read before serves until the new one's read");
}

/// Its buttons are the shared ones: Stop, with nothing running, is drawn off and never lit
/// under the mouse, nor pressed; Run all is lit there.
#[test]
fn stop_is_off_until_something_runs() {
    use ui_components::editor_toolbar::{EditorToolbar, ToolbarPaint};
    let mut bar = bar();
    let mut text = text_layout::TextSystem::new();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    let bounds = Rect::new(0.0, 0.0, 800.0, HEIGHT);
    let mut paint = |bar: &mut HttpBar| {
        let mut scene = render_api::Scene::default();
        bar.paint(ToolbarPaint {
            bounds,
            scale: 1.0,
            text: &mut text,
            scene: &mut scene,
            palette: &palette,
            icons: &icons,
        });
        scene.layers().iter().map(|l| l.quads.len()).sum::<usize>()
    };
    let at_rest = paint(&mut bar);
    let area =
        |bar: &HttpBar, tool: Tool| bar.hits.iter().find(|(_, t)| *t == tool).map(|(r, _)| *r);
    let stop = area(&bar, Tool::Stop).expect("Stop drawn");
    assert!(bar.hover(Some(stop.center())));
    assert_eq!(paint(&mut bar), at_rest, "off: not lit");
    assert!(bar.pointer_down(stop.center()).is_none(), "nor pressed");
    let run = area(&bar, Tool::RunAll).expect("Run all drawn");
    assert!(bar.hover(Some(run.center())));
    assert_eq!(paint(&mut bar), at_rest + 1, "lit under the mouse");
}
