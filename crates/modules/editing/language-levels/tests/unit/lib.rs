use super::*;

fn choices(file: bool) -> LevelChoices {
    LevelChoices {
        anchor: kernel::Rect::new(900.0, 700.0, 80.0, 20.0),
        name: "PHP".into(),
        levels: vec!["8.2".into(), "8.3".into(), "8.4".into()],
        project: "8.3".into(),
        auto: true,
        detected: Some("8.3".into()),
        source: Some("composer.json".into()),
        file_name: file.then(|| "index.php".into()),
        file: None,
    }
}

fn opened(file: bool) -> PickerView {
    let mut view = PickerView::default();
    view.set_data(&choices(file));
    view
}

#[test]
fn the_provider_opens_its_popover_only() {
    let provider = PickerProvider;
    let path = format!("modal.{LEVEL_PICKER_EXTENSION}");
    assert_eq!(provider.kind(Path::new(&path)), Some(ViewerKind::Replace));
    assert_eq!(provider.kind(Path::new("modal.view-color-picker")), None);
}

#[test]
fn esc_clears_the_filter_first_then_closes() {
    let mut view = opened(false);
    assert!(view.text_input("8.4"));
    assert!(view.edit_key(ViewerKey::Escape));
    assert!(!view.closed(), "the filter went first");
    assert!(view.picker.query.text().is_empty());
    assert!(view.edit_key(ViewerKey::Escape));
    assert!(view.closed());
    assert_eq!(view.take_action(), Some(ViewerAction::Close), "nothing chosen: it closes");
}

#[test]
fn enter_sends_the_choice_to_the_app() {
    let mut view = opened(true);
    // Nothing painted yet: no pills, nothing to choose.
    assert!(view.edit_key(ViewerKey::Enter));
    assert!(!view.closed());
    let mut text = text_layout::TextSystem::new();
    let mut scene = render_api::Scene::default();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    view.paint(ViewerPaint {
        bounds: kernel::Rect::new(0.0, 0.0, 1200.0, 800.0),
        scale: 1.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
    assert!(view.edit_key(ViewerKey::Enter));
    assert!(view.closed());
    // The project's scope (the file has no version of its own), its first pill: Auto.
    assert_eq!(
        view.take_action(),
        Some(ViewerAction::Request(AppRequest::SetLevel(LevelPick::Project(None))))
    );
}

#[test]
fn a_click_outside_closes_it() {
    let mut view = opened(false);
    assert_eq!(view.click(Point::new(5.0, 5.0)), Some(ViewerAction::Close));
    assert!(view.closed());
}
