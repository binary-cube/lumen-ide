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
