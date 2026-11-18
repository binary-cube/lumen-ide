use super::*;

fn care(drift: bool) -> ScreenCare {
    ScreenCare { dim_after: Some(Duration::from_secs(600)), level: 0.7, drift }
}

/// The veil comes gently to its level; any input closes it.
#[test]
fn it_dims_gently_and_any_input_closes_it() {
    let start = Instant::now();
    let mut view = ScreenView::at(care(true), start);
    assert_eq!(view.dim(start), 0.0);
    assert!((view.dim(start + Duration::from_secs(10)) - 0.7).abs() < 1e-6);
    assert_eq!(view.take_action(), None);
    assert!(view.hover(Point::new(10.0, 10.0)), "a move of the mouse");
    assert_eq!(view.take_action(), Some(ViewerAction::Close));
    let mut view = ScreenView::at(care(false), start);
    assert!(view.edit_key(ViewerKey::Escape));
    assert_eq!(view.take_action(), Some(ViewerAction::Close));
    let mut view = ScreenView::at(care(false), start);
    assert!(view.text_input("a"));
    assert_eq!(view.take_action(), Some(ViewerAction::Close));
    assert_eq!(
        ScreenView::at(care(false), start).click(Point::default()),
        Some(ViewerAction::Close)
    );
}
