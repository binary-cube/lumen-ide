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

/// Painted, its buttons answer: Close closes; Update hands the choice to the host.
#[test]
fn its_buttons_close_or_choose() {
    let _serial = state::serial();
    state::set(Phase::Available(crate::tests::found()), None);
    let mut view = NotesView::default();
    view.set_data(&Notes::current().unwrap());
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
    paint(&mut view, &mut text);
    assert!(view.laid.is_some(), "laid out once");
    let at =
        |view: &NotesView, hit: Hit| view.hits.iter().find(|(_, h)| *h == hit).unwrap().0.center();
    let go = at(&view, Hit::Go);
    assert!(matches!(view.click(go), Some(ViewerAction::WithHost(_))));
    assert!(view.closed());
    let mut other = NotesView::default();
    other.set_data(&Notes::current().unwrap());
    paint(&mut other, &mut text);
    let close = at(&other, Hit::Close);
    assert!(matches!(other.click(close), Some(ViewerAction::Close)));
}
