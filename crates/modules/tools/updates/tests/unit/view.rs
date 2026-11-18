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
