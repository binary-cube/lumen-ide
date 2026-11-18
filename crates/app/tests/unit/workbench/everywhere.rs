//! A view over every window: opened in one, the others follow; closed in any, they all
//! close; a window with another view over it keeps that one.

use super::*;

fn window() -> (Workbench, TextSystem) {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1000, 700), 1.0);
    let mut text = TextSystem::new();
    wb.paint(&mut text, &mut render_api::Scene::default());
    (wb, text)
}

#[test]
fn a_view_over_every_window_opens_and_closes_in_all() {
    let (mut one, mut text) = window();
    let (mut two, _) = window();
    assert_eq!(one.everywhere_changed(), None, "nothing yet");
    assert!(one.open_everywhere(relax::EXTENSION, &()));
    let wanted = one.everywhere_changed().expect("opened here");
    assert_eq!(wanted.as_deref(), Some(relax::EXTENSION));
    assert_eq!(one.everywhere_changed(), None, "told once");
    // The other window follows: the same view over it.
    assert!(two.follow_everywhere(wanted.as_deref()));
    assert_eq!(two.active_overlay(), Some(Overlay::ModalView));
    assert!(!two.follow_everywhere(wanted.as_deref()), "followed once");
    // Closed in the second (Esc): every window told, the first one closes it too.
    two.edit_key(&mut text, EditKey::Escape);
    assert_eq!(two.active_overlay(), None);
    assert_eq!(two.everywhere_changed(), Some(None));
    assert!(one.follow_everywhere(None));
    assert_eq!(one.active_overlay(), None);
    assert_eq!(one.everywhere_changed(), None, "closed by the app: nothing to tell");
}

#[test]
fn a_window_with_another_view_over_it_keeps_that_one() {
    let (mut one, _) = window();
    let pick = ui_workbench::ColorPick {
        anchor: Rect::new(30.0, 20.0, 12.0, 12.0),
        color: kernel::Rgba::rgb(0x3b82f6),
        format: code_intelligence::ColorFormat::Hex,
        range: 0..0,
    };
    assert!(one.open_bare_view(ui_workbench::PICKER_EXTENSION, &pick, true));
    assert!(!one.follow_everywhere(Some(relax::EXTENSION)), "the picker stays");
    assert!(!one.follow_everywhere(None), "and isn't closed for the other");
    assert_eq!(one.active_overlay(), Some(Overlay::ModalView));
}

/// On the whole screen: shown by the window that started it; another only knows it's shown
/// elsewhere (nothing over it, nothing drawn), and a key or a click there ends it everywhere.
#[test]
fn a_full_screen_view_is_shown_once_and_ended_from_anywhere() {
    let (mut one, _) = window();
    let (mut two, _) = window();
    assert!(one.open_full_everywhere(relax::EXTENSION));
    assert!(one.everywhere_full(), "this window takes its screen");
    assert_eq!(one.everywhere_changed(), Some(Some(relax::EXTENSION.to_owned())));
    // The other: nothing over it.
    assert!(!two.follow_elsewhere(relax::EXTENSION));
    assert_eq!(two.active_overlay(), None);
    assert!(!two.everywhere_full());
    assert_eq!(two.everywhere_changed(), None, "nothing closed here");
    // A key there: ended for all.
    assert!(two.everywhere_input());
    assert_eq!(two.everywhere_changed(), Some(None));
    assert!(!two.everywhere_input(), "once");
    assert!(one.follow_everywhere(None));
    assert_eq!(one.active_overlay(), None);
}

/// Given way to one started first on the same screen: its own closes quietly (nothing told
/// to the others, which would close the one that stays).
#[test]
fn a_full_screen_view_given_way_closes_quietly() {
    let (mut one, _) = window();
    assert!(one.open_full_everywhere(relax::EXTENSION));
    let _ = one.everywhere_changed();
    assert!(one.follow_elsewhere(relax::EXTENSION), "its own closed");
    assert_eq!(one.active_overlay(), None);
    assert_eq!(one.everywhere_changed(), None, "nothing told");
    // Then ended where it stayed: nothing left here either.
    assert!(!one.follow_everywhere(None));
    assert_eq!(one.everywhere_changed(), None);
}
