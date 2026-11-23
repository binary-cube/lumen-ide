use super::*;

fn picker(chosen: Option<usize>) -> Picker {
    let names = ["root", "daemon", "bin", "www-data", "backup", "deploy", "northwind", "nobody"];
    let mut p = Picker::default();
    let items = names.iter().enumerate().map(|(i, n)| PickerItem::person(*n, format!("uid {i}")));
    p.open(items.collect(), chosen);
    p
}

/// Opened on the chosen one: it's under the keys.
#[test]
fn it_opens_on_the_chosen_item() {
    let p = picker(Some(3));
    assert!(p.is_open());
    assert_eq!(p.highlighted(), Some(3));
    assert_eq!(picker(None).highlighted(), Some(0));
}

/// Many items: a search, by name or detail; nothing found leaves nothing to choose.
#[test]
fn typing_filters_by_name_and_detail() {
    let mut p = picker(None);
    assert!(p.searches());
    assert!(p.text_input("de"));
    assert_eq!(p.shown(), [5]);
    p.key(ViewerKey::Backspace, KeyReach::default());
    p.key(ViewerKey::Backspace, KeyReach::default());
    assert!(p.text_input("uid 7"));
    assert_eq!(p.shown(), [7]);
    assert!(p.text_input("x"));
    assert!(p.shown().is_empty());
    assert_eq!(p.key(ViewerKey::Enter, KeyReach::default()), None);
    let mut few = Picker::default();
    few.open(vec![PickerItem::person("a", ""), PickerItem::person("b", "")], None);
    assert!(!few.searches() && !few.text_input("a"));
}

/// ↑ ↓ go around the ends, ↵ chooses (it closes), Esc closes.
#[test]
fn the_keys_move_choose_and_close() {
    let mut p = picker(Some(0));
    let reach = KeyReach::default();
    p.key(ViewerKey::Up, reach);
    assert_eq!(p.highlighted(), Some(7));
    p.key(ViewerKey::Down, reach);
    p.key(ViewerKey::Down, reach);
    assert_eq!(p.key(ViewerKey::Enter, reach), Some(PickerEvent::Chosen(1)));
    assert!(!p.is_open());
    let mut p = picker(None);
    assert_eq!(p.key(ViewerKey::Escape, reach), Some(PickerEvent::Closed));
    assert_eq!(p.key(ViewerKey::Escape, reach), None, "closed: nothing more");
}

/// A click outside it closes it.
#[test]
fn a_click_outside_closes_it() {
    let mut p = picker(None);
    assert_eq!(p.press(Point::new(5.0, 5.0)), Some(PickerEvent::Closed));
}

/// ⌘A selects what's typed (copied, cut: the list follows); closed, it isn't its own.
#[test]
fn the_search_takes_the_edit_commands() {
    let mut p = picker(None);
    p.text_input("www");
    assert!(p.command("select_all"));
    assert_eq!(p.selected_text().as_deref(), Some("www"));
    assert!(p.command("cut"));
    assert_eq!(p.shown().len(), 8, "the search emptied: every item back");
    p.close();
    assert!(!p.command("select_all"));
}

/// Off the list, no cursor of its own; closed, nothing.
#[test]
fn the_cursor_is_its_own_only_over_it() {
    let mut p = picker(None);
    assert_eq!(p.cursor_at(Point::new(5.0, 5.0)), None, "not painted yet: not over it");
    p.close();
    assert_eq!(p.cursor_at(Point::new(5.0, 5.0)), None);
}

/// A table's columns: the fixed ones their width, the first the rest — the same for every
/// row (nothing moves while scrolling).
#[test]
fn a_table_s_columns_are_fixed() {
    let palette = crate::palette::Palette::dark();
    let ctx = Ctx::new(&palette, 2.0);
    let mut text = TextSystem::new();
    let mut table = TableRows::new(vec![
        PickerColumn { title: "Account".into(), width: 0.0, right: false },
        PickerColumn { title: "UID".into(), width: 64.0, right: true },
    ]);
    table.prepare(&mut text, &ctx, &[], 600.0);
    assert_eq!(table.spans[1].1, 128.0, "64 logical at 2×");
    assert_eq!(table.spans[0], (0.0, 600.0 - 32.0 - 128.0 - 32.0));
    assert_eq!(table.header_height(), TABLE_HEADER);
    // The default rows: names only, the whole row theirs.
    let mut items = ItemRows::default();
    items.prepare(&mut text, &ctx, &[PickerItem::person("root", "")], 300.0);
    assert!(items.detail_at > 300.0);
}

/// Opened from a small button, it's as wide as its rows and its keys need.
#[test]
fn it_is_never_narrower_than_its_rows_need() {
    let palette = crate::palette::Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let mut p = picker(None);
    let width = p.natural_width(&mut text, &ctx);
    assert!(width >= 260.0, "a search: at least its field's");
    assert_eq!(p.natural_width(&mut text, &ctx), width, "measured once");
}

/// Suggestions under a text field: no search of its own (the field types, however many
/// there are), nothing under the keys until ↓ (↑ the last), then ↵ takes it.
#[test]
fn suggestions_leave_the_typing_to_their_field_and_light_nothing_at_first() {
    let mut p = Picker::default();
    let types = ["INTEGER", "TEXT", "REAL", "NUMERIC", "BLOB", "BOOLEAN", "DATE", "DATETIME"];
    let items = || types.iter().map(|t| PickerItem { label: (*t).into(), ..PickerItem::default() });
    p.open_suggestions(items().collect(), None, false);
    assert!(p.is_open() && !p.searches(), "the field filters it");
    assert!(!p.text_input("x"), "the typing is the field's");
    assert_eq!(p.highlighted(), None, "↵ is the form's until an arrow");
    assert_eq!(p.key(ViewerKey::Enter, KeyReach::default()), None);
    p.key(ViewerKey::Down, KeyReach::default());
    assert_eq!(p.highlighted(), Some(0));
    p.key(ViewerKey::Down, KeyReach::default());
    assert_eq!(p.key(ViewerKey::Enter, KeyReach::default()), Some(PickerEvent::Chosen(1)));
    p.open_suggestions(items().collect(), None, false);
    p.key(ViewerKey::Up, KeyReach::default());
    assert_eq!(p.highlighted(), Some(types.len() - 1), "↑ first: the last one");
}

/// Something typed: the first suggestion lit, ↵ takes it; opened on the field's value (a
/// click on a filled field): that one ticked and lit.
#[test]
fn suggestions_light_the_first_when_typed_and_the_value_when_clicked() {
    let mut p = Picker::default();
    let item = |t: &str| PickerItem { label: t.into(), ..PickerItem::default() };
    p.open_suggestions(vec![item("INTEGER")], None, true);
    assert_eq!(p.key(ViewerKey::Enter, KeyReach::default()), Some(PickerEvent::Chosen(0)));
    p.open_suggestions(vec![item("INTEGER"), item("TEXT"), item("REAL")], Some(1), false);
    assert_eq!(p.highlighted(), Some(1));
    p.open_suggestions(Vec::new(), None, true);
    assert_eq!(p.highlighted(), None, "nothing to light");
}

/// More rows than fit: its scrollbar's thumb pressed and dragged down scrolls them (as the
/// wheel does), and the press chooses nothing.
#[test]
fn its_scrollbar_thumb_is_dragged() {
    let palette = crate::palette::Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let icons = icons::Icons::builtin();
    let mut scene = Scene::default();
    let mut p = Picker::default();
    p.open((0..40).map(|i| PickerItem::person(format!("row {i}"), "")).collect(), None);
    let bounds = Rect::new(0.0, 0.0, 800.0, 900.0);
    let anchor = Rect::new(10.0, 10.0, 300.0, 30.0);
    p.paint(&mut scene, &mut text, &icons, &ctx, (anchor, bounds), "");
    let edge = p.list.max_x() - 4.0;
    let thumb = Point::new(edge, p.list.min_y() + 6.0);
    p.hover(thumb);
    assert_eq!(p.press(thumb), None, "the bar takes the press: nothing chosen");
    assert!(p.is_open());
    assert!(p.drag(Point::new(edge, p.list.min_y() + 120.0)));
    assert!(p.scroll > 0.0, "dragged down: the rows follow");
    assert!(p.release());
}

/// Its search is the app's search field: typed into, ⌘A selects it, × (a hand over it)
/// empties it and every row comes back; its one field is the one with the caret.
#[test]
fn its_search_is_the_shared_field_with_its_clear_button() {
    let palette = crate::palette::Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let icons = icons::Icons::builtin();
    let mut p = picker(None);
    assert!(p.text_input("www"));
    assert_eq!(p.shown().len(), 1);
    assert!(p.command("select_all"));
    assert_eq!(p.selected_text().as_deref(), Some("www"));
    let (anchor, bounds) = (Rect::new(10.0, 10.0, 300.0, 30.0), Rect::new(0.0, 0.0, 800.0, 900.0));
    p.paint(&mut Scene::default(), &mut text, &icons, &ctx, (anchor, bounds), "");
    assert!(p.query.shows_caret(), "the list open: its field has the caret");
    assert_eq!(p.query.area(), p.search);
    let clear = p.query.clear_area().expect("× shown with text");
    let at = Point::new(clear.min_x() + 2.0, clear.min_y() + 2.0);
    assert_eq!(p.cursor_at(at), Some(crate::ViewerCursor::Pointer));
    assert_eq!(p.press(at), None);
    assert_eq!(p.query.text(), "");
    assert_eq!(p.shown().len(), 8, "emptied: every item back");
    assert!(p.is_open());
}

/// Its rows tell a screen reader their names, the chosen one selected; a row's name isn't
/// read again, its detail is, under it.
#[test]
fn its_rows_tell_their_names_and_the_chosen_one() {
    use ui_kit::access::Role;
    let palette = crate::palette::Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let icons = icons::Icons::builtin();
    let mut p = picker(Some(2));
    let (anchor, bounds) = (Rect::new(10.0, 10.0, 300.0, 30.0), Rect::new(0.0, 0.0, 800.0, 900.0));
    let recording = ui_kit::access::begin();
    p.paint(&mut Scene::default(), &mut text, &icons, &ctx, (anchor, bounds), "");
    let frame = recording.take();
    let items: Vec<_> = frame.nodes().iter().filter(|n| n.role == Role::ListItem).collect();
    assert_eq!(items.len(), 8);
    assert_eq!(&*items[2].name, "bin");
    assert!(items[2].state.selected && !items[0].state.selected);
    // A row's name isn't read again; its detail is, under it.
    let under = |row: &ui_kit::access::Node| {
        frame
            .nodes()
            .iter()
            .filter(|n| n.parent == row.id)
            .map(|n| n.name.to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(under(items[2]), ["uid 2"]);
    // Under a list that knows how many rows it has; each row its place in it.
    let list = frame.nodes().iter().find(|n| n.role == Role::List).expect("the list");
    assert_eq!(list.place.count, Some(8));
    assert!(items.iter().all(|i| i.parent == list.id));
    assert_eq!(items[2].place.index, Some(2));
}

/// The same frame drawn again (warmed up, its scene kept): nothing allocated, at both
/// scales.
#[test]
fn an_identical_repaint_allocates_nothing() {
    let palette = crate::palette::Palette::dark();
    let mut text = TextSystem::new();
    let icons = icons::Icons::builtin();
    let mut scene = Scene::default();
    for scale in [1.0, 2.0] {
        let ctx = Ctx::new(&palette, scale);
        let mut p = picker(Some(2));
        let bounds = Rect::new(0.0, 0.0, 800.0 * scale, 900.0 * scale);
        let anchor = Rect::new(10.0, 10.0, 300.0 * scale, 30.0 * scale);
        let mut frame = |p: &mut Picker| {
            scene.reset(palette.editor_background);
            p.paint(&mut scene, &mut text, &icons, &ctx, (anchor, bounds), "Search");
        };
        for _ in 0..3 {
            frame(&mut p);
        }
        let n = crate::allocations::during(|| frame(&mut p));
        assert_eq!(n, 0, "at {scale}: {n} allocations");
    }
}

/// Paths: the name first, the characters typed lit in the accent, the folder after it
/// (cut in the middle when narrow, its end kept); the whole path in the tooltip; a repaint
/// allocates nothing.
#[test]
fn path_rows_show_the_name_first_and_the_path_in_the_tooltip() {
    let palette = crate::palette::Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let icons = icons::Icons::builtin();
    let mut scene = Scene::default();
    let path = "qq/vendor/twig/twig/src/Sandbox";
    let item = PickerItem {
        label: path.into(),
        icon: Some("folder"),
        marks: vec![24, 25],
        ..PickerItem::default()
    };
    let mut p = Picker::default();
    p.open_suggestions_with(vec![item], (None, true), Box::new(PathRows));
    let (anchor, bounds) = (Rect::new(10.0, 10.0, 260.0, 30.0), Rect::new(0.0, 0.0, 800.0, 600.0));
    p.paint(&mut scene, &mut text, &icons, &ctx, (anchor, bounds), "");
    let glyphs: Vec<_> = scene.layers().iter().flat_map(|l| l.glyphs.iter()).collect();
    let lit = glyphs.iter().filter(|g| g.color == palette.accent).count();
    assert_eq!(lit, 2, "\"Sa\" of Sandbox lit");
    let (row, _) = p.rows[0];
    let on_row = |c: kernel::Rgba| {
        glyphs.iter().filter(move |g| g.color == c && row.contains(g.origin)).map(|g| g.origin.x)
    };
    let lit_x = on_row(palette.accent).fold(f32::MIN, f32::max);
    // The muted ones after the name are its folder (the icon, muted too, is before it).
    let folder_after = on_row(palette.text_muted).filter(|x| *x > lit_x).count();
    assert!(folder_after > 3, "the folder after the name");
    let tip = p.tooltip_at(row.center()).expect("its tooltip");
    assert_eq!(tip.label, path);
    let n = crate::allocations::during(|| {
        scene.reset(palette.editor_background);
        p.paint(&mut scene, &mut text, &icons, &ctx, (anchor, bounds), "");
    });
    assert_eq!(n, 0, "{n} allocations");
    // As wide as its field, however long its rows (they shorten): it never grows as you type.
    assert_eq!(p.area.size.width, anchor.size.width);
    let long = PickerItem {
        label: format!("{}/AVeryLongFolderNameThatWouldWidenTheList", path.repeat(3)),
        icon: Some("folder"),
        ..PickerItem::default()
    };
    p.open_suggestions_with(vec![long], (None, true), Box::new(PathRows));
    p.paint(&mut scene, &mut text, &icons, &ctx, (anchor, bounds), "");
    assert_eq!(p.area.size.width, anchor.size.width);
}

/// A path cut in the middle: its start and its end kept, "…" between, inside the room.
#[test]
fn a_path_is_cut_in_the_middle() {
    let palette = crate::palette::Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let font = ctx.ui_font(-1.5);
    let value = "qq/vendor/symfony/form/Extension/Validator/Constraints";
    let whole = text.shape_line(value, &font).width;
    let mut layer = Layer::default();
    let w = ds::draw_ellipsized_middle(
        &mut layer,
        &mut text,
        value,
        &font,
        (0.0, 20.0),
        whole / 2.0,
        palette.text,
    );
    assert!(w <= whole / 2.0 + 0.5 && w > whole / 3.0, "{w} in {}", whole / 2.0);
    let last = layer.glyphs.last().unwrap().origin.x;
    assert!(last > w * 0.8, "its end kept at the end");
}
