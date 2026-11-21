//! The diagram's filter as a control: the app's search field (typing, ⌘A, × emptying it,
//! said back as an edit), Esc empties it then gives the keys back, ↵ gives them back.

use super::*;
use ui_components::control::Controls;
use ui_components::ds::Ctx;

/// `field` drawn as the view draws it (`focused`: its caret).
fn painted(field: &mut SearchField, focused: bool) {
    let palette = ui_components::Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut layer = render_api::Layer::default();
    let mut text = text_layout::TextSystem::new();
    let icons = icons::Icons::builtin();
    let area = Rect::new(0.0, 0.0, 200.0, 30.0);
    field.paint(&mut layer, &mut text, &icons, &ctx, (area, focused), "Filter");
}

#[test]
fn its_keys_go_through_the_controls_and_esc_leaves_it() {
    let mut search = SearchField::new();
    painted(&mut search, false);
    let (mut edited, mut left) = (false, None);
    let mut controls = Controls::default();
    {
        let mut f =
            FilterField { field: &mut search, shown: true, edited: &mut edited, left: &mut left };
        let mut parts: [&mut dyn Control; 1] = [&mut f];
        assert_eq!(controls.press(&mut parts, Point::new(20.0, 15.0), 1), Some((0, Press::Held)));
        assert_eq!(controls.text(&mut parts, "ord"), Some(0));
        assert_eq!(controls.command(&mut parts, "select_all"), Some(0));
        assert_eq!(controls.selected_text(&parts).as_deref(), Some("ord"), "⌘A");
        assert_eq!(controls.key(&mut parts, ViewerKey::Escape, KeyReach::default()), Some(0));
        assert_eq!(controls.key(&mut parts, ViewerKey::Escape, KeyReach::default()), Some(0));
        assert_eq!(controls.key(&mut parts, ViewerKey::Tab, KeyReach::default()), None);
    }
    assert_eq!(search.text(), "");
    assert!(edited);
    assert_eq!(left, Some(false), "the second Esc gives the keys back");
}

/// × empties it, said back as an edit (the tables shown again); not drawn, it's never hit.
#[test]
fn its_clear_empties_it_as_an_edit() {
    let mut search = SearchField::new();
    search.set_text("ord");
    painted(&mut search, true);
    let x = Point::new(200.0 - 17.0, 15.0);
    let (mut edited, mut left) = (false, None);
    let mut f =
        FilterField { field: &mut search, shown: true, edited: &mut edited, left: &mut left };
    assert_eq!(f.cursor(x), Some(ViewerCursor::Pointer), "a hand on ×");
    assert_eq!(f.press(x, 1), Press::Took);
    f.shown = false;
    assert!(!f.hit(Point::new(20.0, 15.0)), "not drawn: not hit");
    assert_eq!(search.text(), "");
    assert!(edited);
}

/// Only a change of its text is an edit: a caret moved or its text selected isn't (the
/// field's own `take_edited`).
#[test]
fn a_caret_moved_is_no_edit() {
    let mut search = SearchField::new();
    search.set_text("ord");
    let (mut edited, mut left) = (false, None);
    let mut f =
        FilterField { field: &mut search, shown: true, edited: &mut edited, left: &mut left };
    assert!(f.key(ViewerKey::Left, KeyReach::default()));
    assert!(f.command("select_all"));
    assert!(!*f.edited, "nothing changed");
    assert!(f.text("x"));
    assert!(*f.edited);
}
