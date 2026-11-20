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
    assert_eq!(menu.len(), 5, "Copy, Copy All, a line, the message, the line");
    assert!(view.command(COPY_LINE));
    let copied = |view: &mut LogFileView| match view.take_action() {
        Some(ViewerAction::Request(ui_components::AppRequest::CopyText(text))) => text,
        other => panic!("{other:?}"),
    };
    assert_eq!(copied(&mut view), line(1, &long).trim_end());
    assert!(view.command(COPY_MESSAGE));
    assert_eq!(copied(&mut view), format!("{long} 1"));
    assert!(!view.command(ui_components::read_text::COPY), "nothing selected: nothing copied");
    // The details are a part over the grid: × lit under the mouse with a hand; the wheel
    // over them is theirs; a press in their text (⇧ held) leaves the keys with the grid.
    let close = view.details_close;
    view.hover(close.center());
    assert!(view.close_hover);
    assert_eq!(view.cursor_at(close.center()), Some(ViewerCursor::Pointer));
    let inside = Point::new(view.details_area.min_x() + 20.0, view.details_area.max_y() - 10.0);
    view.hover(inside);
    assert!(!view.close_hover, "left ×");
    assert!(view.scroll(0.0, 30.0), "the wheel over them is theirs");
    view.pointer_down(inside, 1, PressMods { shift: true, ..PressMods::default() });
    view.release();
    assert_eq!(view.controls.focused(), Some(GRID), "the grid keeps the keys");
    assert!(view.details.is_some(), "a press in their text doesn't close them");
    // The bar's buttons: lit under the mouse, a hand; pressed, Pause pauses.
    let (pause, button) = *view.buttons.last().expect("Pause");
    assert_eq!(button, Button::Pause);
    view.hover(pause.center());
    assert_eq!(
        (view.hover, view.cursor_at(pause.center())),
        (Some(button), Some(ViewerCursor::Pointer))
    );
    view.pointer_down(pause.center(), 1, PressMods::default());
    assert!(view.grid.as_ref().is_some_and(|g| g.source().paused()), "paused");
    // ×: closed, until another row is chosen.
    view.pointer_down(close.center(), 1, PressMods::default());
    assert!(view.details.is_none());
    view.poll();
    assert!(view.details.is_none(), "stays closed on the same row");
    view.command("select:1,4");
    view.poll();
    assert_eq!(view.details.as_ref().map(|(r, _)| *r), Some(1));
    view.edit_key(ViewerKey::Escape);
    assert!(view.details.is_none(), "Esc closes them");
    // The grid has the keys: its cell's text is what's copied; a character typed is its own
    // (an editing begun takes the typing).
    assert_eq!(view.controls.focused(), Some(GRID));
    view.command("select:1,4");
    let cells = view.grid.as_ref().and_then(|g| g.selected_text());
    assert!(cells.is_some(), "a cell chosen");
    assert_eq!(view.selected_text(), cells);
    let typed = view.key('x');
    assert_eq!(view.wants_text(), typed, "typing only into what it began");
    let _ = std::fs::remove_dir_all(dir);
}

/// More lines than the grid shows: its scrollbar's thumb pressed and dragged down moves it,
/// through the view's own press, drag and release.
#[test]
fn the_grid_s_thumb_is_dragged() {
    let dir = std::env::temp_dir().join(format!("log-thumb-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("app-2026-11-20.jsonl");
    let lines: String = (0..400)
        .map(|n| {
            format!(
                r#"{{"ts":"2026-11-20T08:00:00.000Z","level":"INFO","target":"t","thread":"main","run":"r","msg":"line {n}"}}"#
            ) + "\n"
        })
        .collect();
    std::fs::write(&path, lines).unwrap();
    let icons = icons::Icons::builtin();
    let palette = ui_components::Palette::dark();
    let mut text = text_layout::TextSystem::new();
    let bounds = Rect::new(0.0, 0.0, 1200.0, 500.0);
    let paint = |view: &mut LogFileView, text: &mut text_layout::TextSystem| {
        let mut scene = render_api::Scene::default();
        view.paint(ViewerPaint {
            bounds,
            scale: 1.0,
            text,
            scene: &mut scene,
            palette: &palette,
            icons: &icons,
            focused: true,
            waker: None,
        });
    };
    let opened = |text: &mut text_layout::TextSystem| {
        let mut view = LogFileView::new(&path);
        let since = Instant::now();
        paint(&mut view, text);
        while view.grid.as_ref().is_none_or(|g| g.source().counts() != (400, 400))
            && since.elapsed() < Duration::from_secs(5)
        {
            view.poll();
            paint(&mut view, text);
            std::thread::sleep(Duration::from_millis(20));
        }
        paint(&mut view, text);
        view
    };
    let mut view = opened(&mut text);
    let area = view.grid_area;
    // Along the grid's right edge, under its header (the thumb at rest at the top).
    let at = |x: f32, y: f32| Point::new(x, y);
    let dragged = (0..24).any(|dx| {
        let x = area.max_x() - 1.0 - dx as f32;
        (0..40).any(|dy| {
            let y = area.min_y() + 6.0 * dy as f32;
            view.hover(at(x, y));
            // Not a column's edge (its drag resizes the column).
            let edge = view.cursor_at(at(x, y)) == Some(ViewerCursor::ResizeHorizontal);
            view.pointer_down(at(x, y), 1, PressMods::default());
            let moved = view.drag(at(x, y + 150.0));
            view.release();
            // On the bar, no row was chosen (a row's press would select cells instead).
            moved && !edge && view.chosen().is_none()
        })
    });
    assert!(dragged, "the thumb dragged moves the grid");
    drop(view);
    let _ = std::fs::remove_dir_all(dir);
}
