use super::*;
use std::time::{Duration, Instant};

/// A real file, its own reader: the rows come without the UI thread touching it, a line
/// appended shows up live, Pause holds what's shown, Resume shows what came.
#[test]
fn a_log_file_is_shown_live_and_paused() {
    let dir = std::env::temp_dir().join(format!("log-view-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("app-2026-11-20.jsonl");
    let line = |n: u32| {
        format!(
            r#"{{"ts":"2026-11-20T08:00:00.000Z","level":"INFO","target":"t","thread":"main","run":"r","msg":"line {n}"}}"#
        ) + "\n"
    };
    std::fs::write(&path, line(0) + &line(1)).unwrap();
    let mut view = LogFileView::new(&path);
    let icons = icons::Icons::builtin();
    let palette = ui_components::Palette::dark();
    let mut text = text_layout::TextSystem::new();
    let paint = |view: &mut LogFileView, text: &mut text_layout::TextSystem| {
        let mut scene = render_api::Scene::default();
        let wake: std::sync::Arc<dyn Fn() + Send + Sync> = std::sync::Arc::new(|| {});
        view.paint(ViewerPaint {
            bounds: Rect::new(0.0, 0.0, 1200.0, 700.0),
            scale: 1.0,
            text,
            scene: &mut scene,
            palette: &palette,
            icons: &icons,
            focused: true,
            waker: Some(wake),
        });
    };
    let wait = |view: &mut LogFileView,
                text: &mut text_layout::TextSystem,
                until: &dyn Fn(&LogFileView) -> bool| {
        let since = Instant::now();
        while !until(view) && since.elapsed() < Duration::from_secs(5) {
            view.poll();
            paint(view, text);
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    let counts = |v: &LogFileView| v.grid.as_ref().map(|g| g.source().counts());
    paint(&mut view, &mut text);
    wait(&mut view, &mut text, &|v| counts(v) == Some((2, 2)));
    assert_eq!(counts(&view), Some((2, 2)));
    // Live: a line appended shows.
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, line(2).as_bytes()))
        .unwrap();
    wait(&mut view, &mut text, &|v| counts(v) == Some((3, 3)));
    assert_eq!(counts(&view), Some((3, 3)));
    // Paused: what's shown stays while the reader goes on.
    view.press_button(Button::Pause);
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, line(3).as_bytes()))
        .unwrap();
    wait(&mut view, &mut text, &|v| counts(v) == Some((3, 4)));
    assert_eq!(counts(&view), Some((3, 4)));
    view.press_button(Button::Pause);
    assert_eq!(counts(&view), Some((4, 4)), "resumed: what came is shown");
    drop(view);
    let _ = std::fs::remove_dir_all(dir);
}

/// A row clicked: its details under the grid (its message whole); × closes them, another
/// row chosen opens them again; Esc closes them too.
#[test]
fn a_row_chosen_shows_its_details() {
    let dir = std::env::temp_dir().join(format!("log-details-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("app-2026-11-20.jsonl");
    let long = "word ".repeat(200);
    let line = |n: u32, msg: &str| {
        format!(
            r#"{{"ts":"2026-11-20T08:00:00.000Z","level":"WARN","target":"t","thread":"main","run":"r","msg":"{msg} {n}"}}"#
        ) + "\n"
    };
    std::fs::write(&path, line(0, "short") + &line(1, &long)).unwrap();
    let mut view = LogFileView::new(&path);
    let icons = icons::Icons::builtin();
    let palette = ui_components::Palette::dark();
    let mut text = text_layout::TextSystem::new();
    let paint = |view: &mut LogFileView, text: &mut text_layout::TextSystem| {
        let mut scene = render_api::Scene::default();
        view.paint(ViewerPaint {
            bounds: Rect::new(0.0, 0.0, 1200.0, 800.0),
            scale: 1.0,
            text,
            scene: &mut scene,
            palette: &palette,
            icons: &icons,
            focused: true,
            waker: None,
        });
    };
    let since = Instant::now();
    paint(&mut view, &mut text);
    while view.grid.as_ref().is_none_or(|g| g.source().entry_at(0).is_none())
        && since.elapsed() < Duration::from_secs(5)
    {
        view.poll();
        paint(&mut view, &mut text);
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(view.details.is_none(), "nothing chosen yet");
    // The newest row chosen (the long one): its message whole in the details.
    view.command("select:0,4");
    view.poll();
    let (row, read) = view.details.as_ref().expect("its details");
    assert_eq!(*row, 0);
    assert_eq!(read.blocks()[0].text, format!("{long} 1"));
    paint(&mut view, &mut text);
    assert!(
        view.details_area.size.height > 0.0 && view.grid_area.max_y() <= view.details_area.min_y()
    );
    // A right-click on them: Copy (greyed out with nothing selected), the message, the line
    // as written.
    let menu = view.context_menu(view.details_area.center()).expect("the details' menu");
    assert_eq!(menu.len(), 3);
    assert!(view.command(COPY_LINE));
    let copied = |view: &mut LogFileView| match view.take_action() {
        Some(ViewerAction::Request(ui_components::AppRequest::CopyText(text))) => text,
        other => panic!("{other:?}"),
    };
    assert_eq!(copied(&mut view), line(1, &long).trim_end());
    assert!(view.command(COPY_MESSAGE));
    assert_eq!(copied(&mut view), format!("{long} 1"));
    assert!(!view.command(COPY_SELECTION), "nothing selected: nothing copied");
    // ×: closed, until another row is chosen.
    let close = view.details_close;
    view.pointer_down(close.center(), 1, PressMods::default());
    assert!(view.details.is_none());
    view.poll();
    assert!(view.details.is_none(), "stays closed on the same row");
    view.command("select:1,4");
    view.poll();
    assert_eq!(view.details.as_ref().map(|(r, _)| *r), Some(1));
    view.edit_key(ViewerKey::Escape);
    assert!(view.details.is_none(), "Esc closes them");
    let _ = std::fs::remove_dir_all(dir);
}
