use super::*;

fn paint(view: &mut ConsentView) {
    let icons = icons::Icons::builtin();
    let palette = ui_components::Palette::dark();
    let mut text = text_layout::TextSystem::new();
    let mut scene = render_api::Scene::default();
    view.paint(ViewerPaint {
        bounds: Rect::new(0.0, 0.0, 560.0, 470.0),
        scale: 1.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
}

/// Its length chosen in our dropdown (the field, then a row of its list), 5 minutes by
/// default; its footer has the buttons; Esc closes it with nothing recorded.
#[test]
fn the_consent_window_chooses_a_length_and_esc_leaves() {
    let mut view = ConsentView::new();
    paint(&mut view);
    assert_eq!(view.length, DEFAULT_LENGTH);
    assert!(view.hits.iter().any(|(_, h)| *h == Hit::Start));
    assert!(view.hits.iter().any(|(_, h)| *h == Hit::Cancel));
    // Pressed as the modal window presses (and a tab: both reach `click`).
    view.press(view.field.center(), 1);
    assert!(view.picker.is_open());
    view.edit_key_with(ViewerKey::Down, KeyReach::default());
    view.edit_key_with(ViewerKey::Enter, KeyReach::default());
    assert!(!view.picker.is_open());
    assert_eq!(view.length, DEFAULT_LENGTH + 1);
    assert!(view.edit_key_with(ViewerKey::Escape, KeyReach::default()));
    assert!(matches!(view.take_action(), Some(ViewerAction::Close)));
    // Its Cancel button, clicked: closed too, nothing recorded.
    paint(&mut view);
    let cancel = view.hits.iter().find(|(_, h)| *h == Hit::Cancel).map(|(r, _)| *r).unwrap();
    assert!(matches!(view.press(cancel.center(), 1), Some(ViewerAction::Close)));
    assert!(!crate::recording::recording(), "nothing recorded");
}
