use super::*;

#[test]
fn the_visible_rows_start_at_the_scrolled_one_and_stop_at_the_last() {
    let list = TreeList::new(28.0, 10);
    // 100 px tall: 4 rows fit (the 4th cut), one more for the bottom edge.
    assert_eq!(list.visible(0.0, 100.0), 0..5);
    assert_eq!(list.visible(30.0, 100.0), 1..6);
    assert_eq!(list.visible(250.0, 100.0), 8..10);
    // Scrolled past the end: nothing.
    assert_eq!(list.visible(1000.0, 100.0), 10..10);
}

#[test]
fn the_row_under_the_pointer_is_found_by_its_slot() {
    let list = TreeList::new(28.0, 3);
    assert_eq!(list.row_at(-0.5), None);
    assert_eq!(list.row_at(0.0), Some(0));
    assert_eq!(list.row_at(27.9), Some(0));
    assert_eq!(list.row_at(28.0), Some(1));
    assert_eq!(list.row_at(84.0), None, "below the last row");
    assert_eq!(list.slot_at(84.0), Some(3), "a slot past the rows is still counted");
}

#[test]
fn guides_are_one_line_per_level_on_whole_pixels() {
    let mut layer = Layer::default();
    let color = Rgba::WHITE;
    guides(&mut layer, 1..3, |level| 10.0 + level as f32 * 16.0 + 4.8, (5.0, 28.0), 1.0, color);
    let xs: Vec<f32> = layer.quads.iter().map(|q| q.bounds.min_x()).collect();
    assert_eq!(xs, vec![31.0, 47.0]);
    assert!(layer.quads.iter().all(|q| q.bounds.min_y() == 5.0 && q.bounds.size.height == 28.0));
}

#[test]
fn the_chevron_points_down_when_open() {
    assert_eq!(chevron_icon(true), "chevron-down");
    assert_eq!(chevron_icon(false), "chevron-right");
}

/// A folding node tells a screen reader it's a tree's item, open or folded, named by the
/// text on its row.
#[test]
fn a_folding_node_is_a_tree_item_named_by_its_row() {
    use ui_kit::access::Role;
    let icons = Icons::builtin();
    let mut layer = Layer::default();
    let mut text = text_layout::TextSystem::new();
    let font = text_layout::Font::new(text_layout::FontFamily::SansSerif, 13.0);
    let recording = ui_kit::access::begin();
    chevron(&mut layer, &icons, Rect::new(0.0, 0.0, 16.0, 16.0), true, Rgba::WHITE);
    let label = text.shape_line("src", &font);
    ui_kit::draw_text(&mut layer, &label, 20.0, 12.0, Rgba::WHITE);
    let frame = recording.take();
    let item = frame.nodes().iter().find(|n| n.role == Role::TreeItem).expect("the item");
    assert_eq!((&*item.name, item.state.expanded), ("src", Some(true)));
}

/// A row told whole is one item: its name, its depth, open or not, chosen; its name's text
/// (even cut short) isn't read again, its note is, under it; its chevron adds nothing.
#[test]
fn a_row_told_whole_is_one_item_with_its_level() {
    use ui_kit::access::Role;
    let icons = Icons::builtin();
    let mut layer = Layer::default();
    let mut text = text_layout::TextSystem::new();
    let font = text_layout::Font::new(text_layout::FontFamily::SansSerif, 13.0);
    let recording = ui_kit::access::begin();
    let row = Item {
        name: "controllers",
        bounds: Rect::new(0.0, 0.0, 300.0, 22.0),
        level: 2,
        expanded: Some(false),
        selected: true,
    };
    let told = item(row);
    chevron(&mut layer, &icons, Rect::new(30.0, 4.0, 14.0, 14.0), false, Rgba::WHITE);
    let name = text.shape_line("controll…", &font);
    ui_kit::draw_text(&mut layer, &name, 50.0, 15.0, Rgba::WHITE);
    let note = text.shape_line("3 files", &font);
    ui_kit::draw_text(&mut layer, &note, 200.0, 15.0, Rgba::WHITE);
    drop(told);
    let frame = recording.take();
    let items: Vec<_> = frame.nodes().iter().filter(|n| n.role == Role::TreeItem).collect();
    assert_eq!(items.len(), 1, "the chevron adds none");
    let it = items[0];
    assert_eq!(
        (&*it.name, it.place.level, it.state.expanded),
        ("controllers", Some(2), Some(false))
    );
    assert!(it.state.selected);
    let under: Vec<_> =
        frame.nodes().iter().filter(|n| n.parent == it.id).map(|n| n.name.to_string()).collect();
    assert_eq!(under, ["3 files"]);
}
