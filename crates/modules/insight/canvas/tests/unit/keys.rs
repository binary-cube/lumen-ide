use super::*;
use kernel::{Point, Rect};
use ui_components::ViewerCursor;
use ui_components::control::Press;

#[test]
fn the_search_takes_its_keys_and_asks_for_the_chosen_result() {
    let (mut open, mut chosen) = (Some(search::Search::default()), None);
    let mut part = SearchPart { search: &mut open, catalog: &[], chosen: &mut chosen };
    let reach = KeyReach::default();
    assert!(part.focusable() && part.keeps_tab());
    assert!(part.text("ab"));
    assert!(part.command("select_all"));
    assert_eq!(part.selected_text().as_deref(), Some("ab"), "⌘A: all of it");
    assert!(part.key(ViewerKey::Left, reach), "← drops the selection");
    assert_eq!(part.selected_text(), None);
    assert!(part.key(ViewerKey::Backspace, reach));
    assert!(part.key(ViewerKey::Tab, reach), "Tab moves among the results");
    assert!(part.key(ViewerKey::Delete, reach), "Delete: the field's");
    assert!(part.key(ViewerKey::Enter, reach));
    assert_eq!(*part.chosen, Some(None), "no result to jump to");
    assert!(part.key(ViewerKey::Escape, reach));
    assert!(open.is_none() && chosen == Some(None));
}

/// The search's field as the app's: ⌘X cuts, ⌘Z brings it back, ⌥⌫ a word, × empties it.
#[test]
fn the_search_edits_and_empties_as_the_app_s_field() {
    let (mut open, mut chosen) = (Some(search::Search::default()), None);
    let mut part = SearchPart { search: &mut open, catalog: &[], chosen: &mut chosen };
    assert!(part.text("user repo"));
    assert!(part.command("select_all") && part.command("cut"));
    assert_eq!(part.search.as_ref().map(search::Search::query), Some(""));
    assert!(part.command("undo"));
    assert_eq!(part.search.as_ref().map(search::Search::query), Some("user repo"), "⌘Z");
    assert!(part.key(ViewerKey::End, KeyReach::default()));
    assert!(part.key(ViewerKey::Backspace, KeyReach { word: true, extend: false }));
    assert_eq!(part.search.as_ref().map(search::Search::query), Some("user "));
    // Drawn, its × empties it.
    paint_field(part.search.as_mut().map(|s| &mut s.field));
    let clear = clear_point(part.search.as_ref().map(|s| &s.field));
    assert_eq!(part.cursor(clear), Some(ViewerCursor::Pointer));
    assert_eq!(part.press(clear, 1), Press::Took);
    assert_eq!(part.search.as_ref().map(search::Search::query), Some(""));
}

/// Paints the search's field at a known place.
fn paint_field(field: Option<&mut ui_components::search_field::SearchField>) {
    let Some(field) = field else { return };
    let palette = ui_components::palette::Palette::dark();
    let ctx = ui_components::ds::Ctx::new(&palette, 1.0);
    let mut layer = render_api::Layer::default();
    let mut text = text_layout::TextSystem::new();
    let icons = icons::Icons::builtin();
    field.paint(&mut layer, &mut text, &icons, &ctx, (Rect::new(0.0, 0.0, 300.0, 40.0), true), "");
}

/// A point on the field's ×: its right end, where × is drawn while it has text.
fn clear_point(field: Option<&ui_components::search_field::SearchField>) -> Point {
    let area = field.map(|f| f.area()).unwrap_or_default();
    // × is centered this far from the field's right edge (the shared field's room for it).
    Point::new(area.max_x() - 17.0, area.min_y() + area.size.height / 2.0)
}

#[test]
fn closed_nothing_takes_the_keys() {
    let (mut closed, mut chosen) = (None, None);
    let mut search = SearchPart { search: &mut closed, catalog: &[], chosen: &mut chosen };
    let (mut editing, mut relaid, mut finished) = (None, false, false);
    let mut code = EditPart { editing: &mut editing, relaid: &mut relaid, finished: &mut finished };
    let reach = KeyReach::default();
    assert!(!search.focusable() && !code.focusable());
    assert!(!search.key(ViewerKey::Enter, reach) && !search.text("x"));
    assert!(!code.key(ViewerKey::Escape, reach) && !code.text("x"));
    assert!(!relaid && !finished);
}
