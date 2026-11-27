use super::*;

#[test]
fn the_start_goes_from_the_left_and_the_end_ends_at_the_right_in_order() {
    let items = [(30.0, false), (20.0, true), (40.0, false), (10.0, true)];
    let xs = row_places((100.0, 300.0), 5.0, &items);
    assert_eq!(xs, vec![100.0, 265.0, 135.0, 290.0]);
}

/// Too narrow for all of them: the end ones follow the start ones (cut at the row's edge),
/// never over them.
#[test]
fn too_narrow_the_end_follows_the_start_without_overlapping() {
    let items = [(30.0, false), (20.0, true), (40.0, false), (10.0, true)];
    let xs = row_places((100.0, 150.0), 5.0, &items);
    assert_eq!(xs, vec![100.0, 180.0, 135.0, 205.0]);
}

#[test]
fn a_row_s_width_counts_the_gaps_between_its_items() {
    assert_eq!(row_width(&[30.0, 20.0, 40.0], 5.0), 100.0);
    assert_eq!(row_width(&[30.0], 5.0), 30.0);
    assert_eq!(row_width(&[], 5.0), 0.0);
}

/// Labeled while they fit, by their icons when they don't: never one over another, each
/// label then a tooltip; a filled one alone by its icon is round.
#[test]
fn buttons_that_dont_fit_with_their_labels_show_their_icons_without_overlapping() {
    let palette = crate::palette::Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let icons = icons::Icons::builtin();
    let mut scene = render_api::Scene::default();
    let layer = scene.push_layer(None);
    let buttons = [
        RowButton::start(ButtonSpec::new(ButtonKind::Toolbar).label("Add a row").icon("plus")),
        RowButton::start(ButtonSpec::new(ButtonKind::Toolbar).label("Delete All").icon("trash")),
        RowButton::end(ButtonSpec::new(ButtonKind::Primary).label("Send").icon("play")),
    ];
    let wide = Rect::new(0.0, 0.0, 1000.0, 30.0);
    let placed = button_row(layer, &mut text, &icons, &ctx, wide, &buttons);
    assert!(placed.iter().all(|p| p.tip.is_none()), "room enough: labeled");
    let narrow = Rect::new(0.0, 0.0, 140.0, 30.0);
    let recording = ui_kit::access::begin();
    let placed = button_row(layer, &mut text, &icons, &ctx, narrow, &buttons);
    let frame = recording.take();
    let named: Vec<&str> = frame
        .nodes()
        .iter()
        .filter(|n| n.role == ui_kit::access::Role::Button)
        .map(|n| &*n.name)
        .collect();
    assert_eq!(named, ["Add a row", "Delete All", "Send"], "by their icons, still named");
    assert_eq!(
        placed.iter().map(|p| p.tip).collect::<Vec<_>>(),
        [Some("Add a row"), Some("Delete All"), Some("Send")]
    );
    assert!(placed.iter().all(|p| p.rect.size.width == 30.0), "squares: {placed:?}");
    let mut spans: Vec<(f32, f32)> =
        placed.iter().map(|p| (p.rect.min_x(), p.rect.max_x())).collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert!(spans.windows(2).all(|w| w[0].1 <= w[1].0), "none over another: {spans:?}");
    assert!(spans.iter().all(|s| s.0 >= 0.0 && s.1 <= 140.0), "within the row: {spans:?}");
}
