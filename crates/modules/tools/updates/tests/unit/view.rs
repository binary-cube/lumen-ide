use super::*;
use text_layout::TextSystem;

/// The notes of the version found: only the part written for the app, as lines; the title
/// and the subtitle say the versions.
#[test]
fn the_notes_are_the_part_written_for_the_app() {
    let _serial = state::serial();
    assert_eq!(Notes::current(), None, "nothing found");
    state::set(Phase::Available(crate::tests::found()), None);
    let notes = Notes::current().unwrap();
    let texts: Vec<&str> = notes.lines.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(texts, ["New", "", "tabs"], "the download section left out");
    assert_eq!(notes.date, "2026-12-01");
    assert!(notes.title().contains("2.0.0"));
    let view = NotesView::default();
    assert_eq!(view.modal_width(), Some(WIDTH), "narrower than a large modal");
    assert!(!view.modal_title_widens(), "a long title cut, the window as wide");
}

/// Painted, the window's footer does the choosing: ready, the main button restarts (the
/// host's); while it downloads, Skip can't stop it and the footer says how far; Close closes.
#[test]
fn its_buttons_close_or_choose() {
    let _serial = state::serial();
    let found = crate::tests::found();
    let mut text = TextSystem::new();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    let paint = |view: &mut NotesView, text: &mut TextSystem| {
        let mut scene = render_api::Scene::default();
        view.paint(ViewerPaint {
            bounds: Rect::new(0.0, 0.0, 640.0, 520.0),
            scale: 1.0,
            text,
            scene: &mut scene,
            palette: &palette,
            icons: &icons,
            focused: true,
            waker: None,
        });
    };
    let opened = |text: &mut TextSystem| {
        let mut view = NotesView::default();
        view.set_data(&Notes::current().unwrap());
        paint(&mut view, text);
        view
    };
    // Downloading: the main button and Skip off, the progress in the footer.
    state::set(
        Phase::Downloading { available: std::sync::Arc::clone(&found), received: 25, total: 100 },
        None,
    );
    let mut view = opened(&mut text);
    assert!(view.read.content_height() > 0.0, "laid out");
    let buttons = view.modal_buttons();
    assert_eq!(buttons.iter().map(|b| b.id).collect::<Vec<_>>(), ["go", "skip", "close"]);
    assert!(!buttons[0].enabled && !buttons[1].enabled && buttons[2].enabled);
    assert_eq!(view.modal_status(), None, "its progress under the notes, not twice");
    assert!(state::is_notes_open(), "its notifications wait");
    assert!(view.is_loading(), "frames while it moves");
    assert!(view.command("go") && view.take_action().is_none(), "nothing to press");
    // Ready: the restart, handed to the host.
    let staged = self_update::stage::Staged {
        dir: std::path::PathBuf::from("/updates/2.0.0"),
        version: "2.0.0".into(),
        app: std::path::PathBuf::from("/updates/2.0.0/app"),
    };
    state::set(Phase::Ready { available: found, staged }, None);
    let mut view = opened(&mut text);
    assert_eq!(view.modal_status(), None);
    assert!(view.command("go"));
    assert!(matches!(view.take_action(), Some(ViewerAction::WithHost(_))));
    assert!(view.closed());
    let mut other = opened(&mut text);
    assert!(other.command("close"));
    assert!(matches!(other.take_action(), Some(ViewerAction::Close)));
}

/// Notes longer than the window scroll: by the wheel, and by the bar's thumb dragged
/// (pressed at the top of its track, then moved down).
#[test]
fn long_notes_scroll_by_the_wheel_and_the_thumb() {
    let mut text = TextSystem::new();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    let bounds = Rect::new(0.0, 0.0, 640.0, 520.0);
    let long = || {
        let mut view = NotesView::default();
        let lines =
            (0..80).map(|i| Block::new(BlockKind::Point { depth: 0 }, format!("Change {i}")));
        view.read.set_blocks(lines.collect());
        view
    };
    let paint = |view: &mut NotesView, text: &mut TextSystem| {
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
    let mut view = long();
    paint(&mut view, &mut text);
    assert!(view.scroll(0.0, 120.0), "the wheel moves it");
    assert!(view.read.scroll() > 0.0);
    assert!(view.scroll(0.0, -10_000.0) && view.read.scroll() == 0.0, "back to its top");
    // The thumb: somewhere along the right edge, at the top of the notes.
    let dragged = (0..40).rev().map(|dx| bounds.max_x() - 1.0 - dx as f32).any(|x| {
        let mut view = long();
        paint(&mut view, &mut text);
        view.hover(Point::new(x, 60.0));
        Viewer::press(&mut view, Point::new(x, 60.0), 1);
        let moved = view.drag(Point::new(x, 260.0)) && view.read.scroll() > 0.0;
        view.release();
        moved && !view.read.has_selection()
    });
    assert!(dragged, "the thumb dragged scrolls the notes");
}
