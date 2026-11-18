use super::*;
use code_intelligence::ColorFormat;
use kernel::{Rect, Rgba};
use ui_components::AppRequest;

fn opened() -> PickerView {
    let mut view = PickerView::default();
    view.set_data(&ColorPick {
        anchor: Rect::new(100.0, 100.0, 12.0, 12.0),
        color: Rgba::rgb(0x3b82f6),
        format: ColorFormat::Hex,
        range: 7..14,
    });
    view
}

#[test]
fn enter_writes_the_color_back_in_its_form_and_closes() {
    let mut view = opened();
    assert!(view.edit_key(ViewerKey::Enter));
    let action = view.take_action();
    assert_eq!(
        action,
        Some(ViewerAction::Request(AppRequest::ReplaceInActive {
            range: 7..14,
            text: "#3b82f6".into()
        }))
    );
    assert!(view.closed(), "done: the app closes it");
}

#[test]
fn esc_gives_up_and_the_eyedropper_asks_the_screen() {
    let mut view = opened();
    assert!(view.edit_key(ViewerKey::Escape));
    assert_eq!(view.take_action(), Some(ViewerAction::Close));
    let mut view = opened();
    view.act(PickerAction::Eyedropper);
    assert_eq!(view.take_action(), Some(ViewerAction::Request(AppRequest::SampleScreen)));
    assert!(!view.closed(), "it stays open meanwhile");
    // The sampled color comes back as a command.
    assert!(view.command("screen-color:#ff0000"));
    assert!(!view.command("screen-color:nope"));
}
