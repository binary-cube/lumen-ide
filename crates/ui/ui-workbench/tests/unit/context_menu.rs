use super::*;

fn item(id: &str) -> MenuEntry {
    MenuEntry::Item {
        id: id.into(),
        label: id.to_uppercase(),
        shortcut: None,
        icon: None,
        on: None,
        look: Default::default(),
    }
}

#[test]
fn runs_items_skips_separators_and_stays_in_the_window() {
    let mut menu = ContextMenu::default();
    menu.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0), 1.0);
    // Opened near the bottom-right corner: moved up and left to fit.
    menu.open(vec![item("a"), MenuEntry::Separator, item("b")], Point::new(790.0, 590.0));
    menu.paint(&Icons::builtin(), &mut TextSystem::new(), &mut Scene::default(), &Palette::dark());
    assert!(menu.panel.max_x() <= 800.0 && menu.panel.max_y() <= 600.0);
    let second = menu.rows[1].0;
    let at = Point::new(second.min_x() + 4.0, second.min_y() + 4.0);
    assert_eq!(menu.hit(at), Some(ContextAction::Run("b".into())));
    assert_eq!(menu.hit(Point::new(1.0, 1.0)), Some(ContextAction::Close));
    menu.step(1);
    menu.step(1);
    assert_eq!(menu.confirm().as_deref(), Some("b"), "the separator is skipped");
    menu.step(1);
    assert_eq!(menu.confirm().as_deref(), Some("a"));
}

fn paint(menu: &mut ContextMenu) {
    menu.paint(&Icons::builtin(), &mut TextSystem::new(), &mut Scene::default(), &Palette::dark());
}

fn center(r: Rect) -> Point {
    Point::new(r.min_x() + r.size.width / 2.0, r.min_y() + r.size.height / 2.0)
}

fn with_submenu() -> Vec<MenuEntry> {
    vec![
        item("a"),
        MenuEntry::Submenu {
            label: "Git".into(),
            icon: None,
            entries: vec![item("pull"), MenuEntry::Separator, item("push")],
        },
    ]
}

#[test]
fn a_submenu_opens_on_hover_and_runs_its_items() {
    let mut menu = ContextMenu::default();
    menu.set_viewport(Rect::new(0.0, 0.0, 1000.0, 700.0), 1.0);
    menu.open(with_submenu(), Point::new(100.0, 100.0));
    paint(&mut menu);
    let git = menu.rows[1].0;
    assert!(menu.hover(center(git)));
    paint(&mut menu);
    let (_, child) = menu.child.as_ref().expect("the submenu opened");
    assert!(child.panel.min_x() >= menu.panel.max_x() - 5.0, "beside its parent");
    let push = child.rows[1].0;
    assert_eq!(menu.hit(center(push)), Some(ContextAction::Run("push".into())));
    // Back on another row of the parent: the submenu closes.
    menu.hover(center(menu.rows[0].0));
    assert!(menu.child.is_none());
}

#[test]
fn the_keyboard_enters_and_leaves_a_submenu() {
    let mut menu = ContextMenu::default();
    menu.set_viewport(Rect::new(0.0, 0.0, 1000.0, 700.0), 1.0);
    menu.open(with_submenu(), Point::new(100.0, 100.0));
    paint(&mut menu);
    menu.step(1);
    menu.step(1);
    // Enter on a submenu opens it, its first row selected.
    assert_eq!(menu.confirm(), None);
    assert_eq!(menu.confirm().as_deref(), Some("pull"));
    menu.step(1);
    assert_eq!(menu.confirm().as_deref(), Some("push"), "the separator is skipped");
    assert!(menu.enter_submenu(false));
    assert!(menu.child.is_none());
}

#[test]
fn a_submenu_without_room_on_the_right_opens_on_the_left() {
    let mut menu = ContextMenu::default();
    menu.set_viewport(Rect::new(0.0, 0.0, 600.0, 700.0), 1.0);
    menu.open(with_submenu(), Point::new(400.0, 100.0));
    paint(&mut menu);
    menu.hover(center(menu.rows[1].0));
    paint(&mut menu);
    let (_, child) = menu.child.as_ref().unwrap();
    assert!(child.panel.max_x() <= menu.panel.min_x() + 5.0);
}

fn sub(label: &str, entries: Vec<MenuEntry>) -> MenuEntry {
    MenuEntry::Submenu { label: label.into(), icon: None, entries }
}

#[test]
fn separators_never_leave_an_empty_section() {
    use MenuEntry::Separator as S;
    let tidy = tidy_menu(vec![
        S,
        S,
        item("a"),
        S,
        S,
        S,
        item("b"),
        sub("empty", vec![S, S]),
        S,
        sub("c", vec![S, item("c1"), S, S, item("c2"), S]),
        S,
        S,
    ]);
    assert_eq!(
        tidy,
        vec![item("a"), S, item("b"), S, sub("c", vec![item("c1"), S, item("c2")]),],
        "leading, trailing and repeated separators dropped, in submenus too; an empty submenu dropped"
    );
    assert!(tidy_menu(vec![S, sub("x", vec![S]), S]).is_empty());
    let kept = vec![item("a"), S, item("b")];
    assert_eq!(tidy_menu(kept.clone()), kept, "a tidy menu is unchanged");
}

#[test]
fn an_opened_menu_draws_no_empty_section() {
    use MenuEntry::Separator as S;
    let mut tidy = ContextMenu::default();
    tidy.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0), 1.0);
    tidy.open(vec![item("a"), S, item("b")], Point::new(10.0, 10.0));
    paint(&mut tidy);
    let mut messy = ContextMenu::default();
    messy.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0), 1.0);
    messy.open(vec![S, item("a"), S, S, item("b"), S], Point::new(10.0, 10.0));
    paint(&mut messy);
    assert_eq!(messy.panel, tidy.panel, "the same height: no gap");
    assert_eq!(messy.entries, tidy.entries);
    // A submenu with repeated separators opens tidied too.
    let mut menu = ContextMenu::default();
    menu.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0), 1.0);
    menu.open(vec![sub("s", vec![S, item("x"), S, S, item("y"), S])], Point::new(10.0, 10.0));
    paint(&mut menu);
    let row = menu.rows[0].0;
    menu.hover(center(row));
    let child = &menu.child.as_ref().expect("the submenu opened").1;
    assert_eq!(child.entries, vec![item("x"), S, item("y")]);
    // Nothing but separators: nothing opens.
    let mut empty = ContextMenu::default();
    empty.open(vec![S, S], Point::new(10.0, 10.0));
    assert!(!empty.is_open());
}

/// A setting's row ends with a small switch in place of its shortcut; lit only when on.
#[test]
fn a_setting_s_row_shows_how_it_stands() {
    let row = |on| MenuEntry::Item {
        id: "a".into(),
        label: "A".into(),
        shortcut: Some("⌘A".into()),
        icon: None,
        on,
        look: Default::default(),
    };
    let quads = |on: Option<bool>| {
        let mut menu = ContextMenu::default();
        menu.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0), 1.0);
        menu.open(vec![row(on)], Point::new(10.0, 10.0));
        let mut scene = Scene::default();
        let palette = Palette::dark();
        menu.paint(&Icons::builtin(), &mut TextSystem::new(), &mut scene, &palette);
        let accent = scene.layers().iter().flat_map(|l| &l.quads).any(|q| {
            q.color.with_alpha(1.0) == palette.accent.with_alpha(1.0) && q.bounds.size.width > 20.0
        });
        (scene.quad_count(), accent)
    };
    let (plain, _) = quads(None);
    let (off, off_lit) = quads(Some(false));
    let (on, on_lit) = quads(Some(true));
    assert_eq!((off, on), (plain + 2, plain + 2), "its track and its knob");
    assert!(on_lit && !off_lit);
}

/// A setting's row runs and leaves the menu to show how it stands now; a plain row closes it.
#[test]
fn a_setting_s_row_keeps_the_menu_open_and_follows_its_state() {
    let setting = MenuEntry::Item {
        id: "s".into(),
        label: "S".into(),
        shortcut: None,
        icon: None,
        on: Some(false),
        look: Default::default(),
    };
    let mut menu = ContextMenu::default();
    menu.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0), 1.0);
    menu.open(vec![setting, item("a")], Point::new(10.0, 10.0));
    menu.paint(&Icons::builtin(), &mut TextSystem::new(), &mut Scene::default(), &Palette::dark());
    let at = |r: Rect| Point::new(r.min_x() + 4.0, r.min_y() + 4.0);
    assert_eq!(menu.hit(at(menu.rows[0].0)), Some(ContextAction::Switch("s".into())));
    assert_eq!(menu.hit(at(menu.rows[1].0)), Some(ContextAction::Run("a".into())));
    menu.refresh_switches(&|id| (id == "s").then_some(true));
    assert!(matches!(menu.entries[0], MenuEntry::Item { on: Some(true), .. }));
    assert!(matches!(menu.entries[1], MenuEntry::Item { on: None, .. }), "a plain row stays so");
}

#[test]
fn a_right_side_buttons_menu_stands_to_its_left_however_wide() {
    let mut menu = ContextMenu::default();
    menu.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0), 1.0);
    let button = Rect::new(770.0, 560.0, 24.0, 24.0);
    let wide = MenuEntry::Item {
        id: "wide".into(),
        label: "Full-Screen Scene Instead of Dimming The Display".into(),
        shortcut: None,
        icon: None,
        on: None,
        look: Default::default(),
    };
    menu.open_left_of(vec![wide, item("a"), item("b")], button, 6.0);
    paint(&mut menu);
    assert_eq!(menu.panel.max_x(), 764.0, "beside the button, not over it");
    assert_eq!(menu.panel.max_y(), button.max_y(), "ends with the button: no room below");
}

#[test]
fn a_very_long_name_doesnt_widen_the_menu_past_its_limit() {
    let mut menu = ContextMenu::default();
    menu.set_viewport(Rect::new(0.0, 0.0, 2000.0, 600.0), 1.0);
    let long = MenuEntry::Item {
        id: "long".into(),
        label: format!("{}.php", "asdf ".repeat(100)),
        shortcut: None,
        icon: None,
        on: None,
        look: Default::default(),
    };
    menu.open(vec![long], Point::new(10.0, 10.0));
    paint(&mut menu);
    assert!(menu.panel.size.width < 520.0, "{}", menu.panel.size.width);
}

/// A greyed-out row is shown but never run, lit or stopped on by the keys; a checked one
/// and one with a color dot run as any other.
#[test]
fn a_disabled_row_is_shown_but_never_run() {
    let mut menu = ContextMenu::default();
    menu.set_viewport(Rect::new(0.0, 0.0, 800.0, 600.0), 1.0);
    let greyed = |id: &str| match item(id) {
        MenuEntry::Item { id, label, shortcut, icon, on, .. } => MenuEntry::Item {
            id,
            label,
            shortcut,
            icon,
            on,
            look: RowLook { disabled: true, ..RowLook::default() },
        },
        other => other,
    };
    let checked = match item("c") {
        MenuEntry::Item { id, label, shortcut, icon, on, .. } => MenuEntry::Item {
            id,
            label,
            shortcut,
            icon,
            on,
            look: RowLook { checked: true, swatch: Some(0xFF0000), ..RowLook::default() },
        },
        other => other,
    };
    menu.open(vec![item("a"), greyed("b"), checked], Point::new(10.0, 10.0));
    paint(&mut menu);
    assert_eq!(menu.rows.len(), 3, "the greyed-out row is shown");
    let b = center(menu.rows[1].0);
    assert_eq!(menu.hit(b), Some(ContextAction::Inside), "not run");
    assert!(!menu.hover(b), "not lit");
    assert_eq!(menu.hit(center(menu.rows[2].0)), Some(ContextAction::Run("c".into())));
    menu.step(1);
    menu.step(1);
    assert_eq!(menu.confirm().as_deref(), Some("c"), "the keys skip it");
}
