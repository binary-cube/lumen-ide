use super::*;
use crate::control::Controls;
use crate::palette::Palette;

fn painted(field: &mut SearchField, focused: bool) {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut layer = Layer::default();
    let mut text = TextSystem::new();
    let icons = icons::Icons::builtin();
    field.paint(
        &mut layer,
        &mut text,
        &icons,
        &ctx,
        (Rect::new(0.0, 0.0, 300.0, 34.0), focused),
        "Search",
    );
}

/// Typed into, ⌘A selects all of it, × empties it (said once), and the caret's field is the
/// one the view's focus says: two search fields, one focus.
#[test]
fn a_search_field_types_selects_and_empties_with_one_focus() {
    let (mut a, mut b) = (SearchField::new(), SearchField::new());
    painted(&mut a, true);
    painted(&mut b, false);
    let mut c = Controls::default();
    {
        let mut parts: [&mut dyn Control; 2] = [&mut a, &mut b];
        c.focus(&mut parts, Some(1));
        assert_eq!(c.text(&mut parts, "users"), Some(1), "the focused one takes the text");
        assert_eq!(c.command(&mut parts, "select_all"), Some(1));
        assert_eq!(c.selected_text(&parts).as_deref(), Some("users"));
    }
    assert_eq!(a.text(), "", "the other untouched");
    painted(&mut b, true);
    let clear = b.clear.expect("× shown with text");
    let at = Point::new(clear.min_x() + 2.0, clear.min_y() + 2.0);
    assert_eq!(Control::cursor(&b, at), Some(ViewerCursor::Pointer));
    assert_eq!(Control::press(&mut b, at, 1), Press::Took);
    assert_eq!(b.text(), "");
    assert!(b.take_cleared());
    assert!(!b.take_cleared(), "said once");
}

/// Its text changed is told once (typed, ×), a key that changes nothing isn't; the room a
/// view keeps on the right is given back with × left of it; forgotten, it can't be hit.
#[test]
fn edits_are_told_and_the_view_keeps_room_on_the_right() {
    let mut field = SearchField::new();
    painted(&mut field, true);
    assert!(Control::text(&mut field, "ab"));
    assert!(field.take_edited());
    assert!(!field.take_edited(), "told once");
    assert!(Control::key(&mut field, ViewerKey::Left, KeyReach::default()));
    assert!(!field.take_edited(), "a move changes no text");
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let (mut layer, mut text, icons) =
        (Layer::default(), TextSystem::new(), icons::Icons::builtin());
    let room = field.paint_with(
        &mut layer,
        &mut text,
        &icons,
        &ctx,
        (Rect::new(0.0, 0.0, 300.0, 34.0), true),
        "Search",
        SearchExtras { trailing: 60.0, invalid: true },
    );
    assert_eq!(room.size.width, 60.0);
    assert!(field.clear_area().is_some_and(|c| c.max_x() <= room.min_x()), "× left of it");
    field.forget();
    assert!(!Control::hit(&field, Point::new(10.0, 10.0)), "forgotten: not hit");
}

/// Its owner setting its text is no edit: nothing to search again for.
#[test]
fn set_text_by_the_owner_is_not_an_edit() {
    let mut field = SearchField::new();
    field.set_text("query");
    assert_eq!(field.text(), "query");
    assert!(!field.take_edited());
}

/// A very long text pasted stays inside the field: no glyph past its × (nor its left edge),
/// the caret at the text's end, within the field, never over a glyph.
#[test]
fn a_very_long_paste_stays_inside_the_field() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut field = SearchField::new();
    painted(&mut field, true);
    let long = "klgaksld fgkldfgkl adkflg kaldfgkl adklgb ".repeat(40);
    assert!(Control::text(&mut field, &long));
    let mut layer = Layer::default();
    let mut text = TextSystem::new();
    let icons = icons::Icons::builtin();
    let rect = Rect::new(0.0, 0.0, 300.0, 34.0);
    field.paint(&mut layer, &mut text, &icons, &ctx, (rect, true), "Search");
    let clear = field.clear_area().expect("× with text");
    let font = ctx.ui_font(-0.5);
    let glyphs: Vec<_> = layer.glyphs.iter().filter(|g| g.origin.y > rect.min_y()).collect();
    let text_glyphs: Vec<_> = glyphs
        .iter()
        .filter(|g| g.origin.x > rect.min_x() + 20.0 && g.origin.x < clear.min_x())
        .collect();
    assert!(!text_glyphs.is_empty(), "the text shows");
    let widest = text.shape_line("W", &font).width;
    for g in &text_glyphs {
        assert!(g.origin.x + widest * 0.5 <= clear.min_x(), "a glyph runs into ×: {:?}", g.origin);
    }
    let caret = layer
        .quads
        .iter()
        .rev()
        .find(|q| q.bounds.size.width <= 2.0 && q.bounds.size.height > 10.0)
        .map(|q| q.bounds);
    if let Some(caret) = caret {
        assert!(caret.max_x() <= clear.min_x(), "the caret before ×: {caret:?}");
        let last = text_glyphs.iter().map(|g| g.origin.x).fold(f32::MIN, f32::max);
        assert!(last < caret.min_x(), "the caret after the last glyph: {last} vs {caret:?}");
    }
}
