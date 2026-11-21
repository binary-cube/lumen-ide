use super::*;

fn item(id: &str, shortcut: Option<&str>) -> MenuEntry {
    MenuEntry::Item {
        id: id.into(),
        label: id.to_uppercase(),
        shortcut: shortcut.map(Into::into),
        icon: Some("copy"),
        on: None,
        look: Default::default(),
    }
}

fn menus(count: usize) -> Vec<DrawerMenu> {
    (0..count)
        .map(|m| DrawerMenu {
            title: format!("Menu {m}"),
            icon: Some("folder"),
            entries: vec![
                item(&format!("m{m}.a"), Some("Ctrl+A")),
                MenuEntry::Separator,
                item(&format!("m{m}.b"), None),
            ],
        })
        .collect()
}

fn paint(drawer: &mut MenuDrawer) {
    drawer.settle();
    frame(drawer);
}

/// Painted as it is (its animations not settled first).
fn frame(drawer: &mut MenuDrawer) {
    thread_local! {
        // One text system a test (its fonts loaded once, not at every frame).
        static TEXT: std::cell::RefCell<TextSystem> = std::cell::RefCell::new(TextSystem::new());
    }
    TEXT.with_borrow_mut(|text| {
        drawer.paint(&Icons::builtin(), text, &mut Scene::default(), &Palette::dark());
    });
}

fn center(r: Rect) -> Point {
    Point::new(r.min_x() + r.size.width / 2.0, r.min_y() + r.size.height / 2.0)
}

fn drawer(count: usize, height: f32) -> MenuDrawer {
    let mut drawer = MenuDrawer::new(menus(count));
    drawer.set_area(Rect::new(0.0, 40.0, 1000.0, height), 1.0);
    paint(&mut drawer);
    drawer
}

#[test]
fn a_category_opens_its_submenu_beside_and_an_item_runs() {
    let mut drawer = drawer(4, 600.0);
    assert_eq!(drawer.menu_rows.len(), 4);
    assert_eq!(drawer.items_panel, Rect::default(), "no submenu before one is chosen");
    // Stuck to the left edge, under the top bar, the whole height.
    assert_eq!((drawer.panel.min_x(), drawer.panel.min_y()), (0.0, 40.0));
    assert_eq!(drawer.panel.size.width, MENUS_W);
    let second = drawer.menu_rows[1].0;
    assert_eq!(drawer.hit(center(second)), DrawerAction::Inside);
    paint(&mut drawer);
    assert_eq!(drawer.nav().open_menu(), Some(1));
    assert_eq!(drawer.panel.size.width, MENUS_W + ITEMS_W, "widened by the submenu");
    assert_eq!(drawer.items_panel.min_x(), MENUS_W);
    // Its rows: the separator isn't one.
    assert_eq!(drawer.item_rows.len(), 2);
    let b = drawer.item_rows[1].0;
    assert!(drawer.over_clickable(center(b)));
    assert_eq!(drawer.hit(center(b)), DrawerAction::Run("m1.b".into()));
    // Outside the panels (the dimmed window): it closes.
    assert_eq!(drawer.hit(Point::new(900.0, 300.0)), DrawerAction::Close);
}

#[test]
fn hovering_shows_a_category_and_waits_before_switching() {
    let mut drawer = drawer(4, 600.0);
    let first = drawer.menu_rows[0].0;
    assert!(drawer.hover(center(first)));
    assert_eq!(drawer.nav().open_menu(), Some(0), "nothing shown yet: at once");
    paint(&mut drawer);
    // Straight down onto the next one: shown after a short rest, not at once.
    let second = drawer.menu_rows[1].0;
    drawer.hover(Point::new(center(first).x, center(second).y));
    assert_eq!(drawer.nav().open_menu(), Some(0));
    let due = drawer.deadline().expect("a switch waiting");
    assert!(due > Instant::now());
    std::thread::sleep(due.saturating_duration_since(Instant::now()));
    paint(&mut drawer);
    assert_eq!(drawer.nav().open_menu(), Some(1));
    assert_eq!(drawer.deadline(), None);
}

#[test]
fn the_filter_lists_results_with_their_paths_and_runs_them() {
    let mut drawer = drawer(3, 600.0);
    drawer.type_text("m2.");
    paint(&mut drawer);
    assert!(drawer.has_query());
    assert_eq!(drawer.result_rows.len(), 2);
    assert_eq!(drawer.items_panel, Rect::default(), "no submenu while filtering");
    assert_eq!(drawer.key(NavKey::Down), NavOutcome::Used);
    assert_eq!(drawer.key(NavKey::Enter), NavOutcome::Run("m2.b".into()));
    let first = drawer.result_rows[0].0;
    assert_eq!(drawer.hit(center(first)), DrawerAction::Run("m2.a".into()));
    drawer.clear_query();
    paint(&mut drawer);
    assert_eq!(drawer.menu_rows.len(), 3);
}

#[test]
fn the_footer_tells_the_keys_that_work_now() {
    let mut drawer = drawer(2, 600.0);
    let keys = |d: &MenuDrawer| -> Vec<Vec<&'static str>> {
        d.footer_hints().into_iter().map(|(keys, _)| keys).collect()
    };
    assert_eq!(keys(&drawer), [vec!["↑", "↓"], vec!["→"], vec!["esc"]]);
    drawer.key(NavKey::Down);
    drawer.key(NavKey::Right);
    assert_eq!(keys(&drawer), [vec!["↑", "↓"], vec!["↵"], vec!["←"], vec!["esc"]]);
    drawer.type_text("a");
    assert_eq!(keys(&drawer), [vec!["↑", "↓"], vec!["↵"], vec!["esc"]]);
}

#[test]
fn a_short_window_scrolls_the_categories_and_only_those_shown_are_clickable() {
    // Twenty categories in a short room.
    let mut drawer = drawer(20, 400.0);
    let room = drawer.menus_room;
    let shown: Vec<usize> = drawer.menu_rows.iter().map(|(_, i)| *i).collect();
    assert!(shown.len() < 20, "only the rows in view are laid out");
    // The keyboard's row kept in view: up from the first, around to the last.
    drawer.key(NavKey::Up);
    paint(&mut drawer);
    let last = drawer.menu_rows.iter().find(|(_, i)| *i == 19).map(|(r, _)| *r);
    let last = last.expect("the last category scrolled into view");
    assert!(last.max_y() <= room.max_y() + 0.5);
    // A row scrolled out of the room isn't hit.
    let above = Point::new(center(last).x, room.min_y() - 4.0);
    assert_eq!(drawer.row_at(above), None);
    // The wheel scrolls it back up (never past the top).
    assert!(drawer.scroll_by(-10_000.0));
    paint(&mut drawer);
    assert!(drawer.menu_rows.iter().any(|(_, i)| *i == 0));
    assert!(!drawer.scroll_by(-10.0));
}

#[test]
fn it_slides_out_and_is_gone_after() {
    let mut drawer = drawer(2, 600.0);
    assert!(drawer.is_open() && !drawer.is_gone());
    drawer.close();
    assert!(!drawer.is_open());
    assert_eq!(drawer.hit(Point::new(10.0, 100.0)), DrawerAction::Close);
    paint(&mut drawer);
    assert!(drawer.is_gone());
    assert_eq!(drawer.panel, Rect::default());
}

/// Painted as the app does it: no transitions skipped.
#[test]
fn moving_over_the_submenu_keeps_its_category_open_and_its_width() {
    let mut drawer = drawer(4, 600.0);
    let code = drawer.menu_rows[2].0;
    drawer.hover(center(code));
    // The submenu widens out, frame by frame, while the mouse goes right across it.
    let mut x = center(code).x;
    let y = center(code).y;
    let mut widths = Vec::new();
    for step in 0..60 {
        frame(&mut drawer);
        std::thread::sleep(Duration::from_millis(5));
        x += 12.0;
        // Down and up over its rows, the gaps and the separator too.
        let wobble = if x > MENUS_W { ((step % 7) as f32 - 3.0) * 9.0 } else { 0.0 };
        let p = Point::new(x, y + wobble);
        drawer.hover(p);
        assert_eq!(drawer.nav().open_menu(), Some(2), "step {step}: {p:?}");
        if drawer.items_panel.contains(p) {
            assert_eq!(drawer.nav().focus(), NavFocus::Items, "step {step}: in the submenu");
        }
        widths.push(drawer.panel.size.width);
    }
    // It only ever grew, up to its full width.
    assert!(widths.windows(2).all(|w| w[1] >= w[0]), "{widths:?}");
    assert_eq!(widths.last().copied(), Some(MENUS_W + ITEMS_W));
}

/// One category: a row, a row with a submenu (Folding: two rows), a row.
fn nested(width: f32) -> MenuDrawer {
    let menus = vec![DrawerMenu {
        title: "Code".into(),
        icon: Some("braces"),
        entries: vec![
            item("code.a", None),
            MenuEntry::Submenu {
                label: "Folding".into(),
                icon: Some("chevrons-collapse"),
                entries: vec![item("fold.a", Some("Ctrl+A")), item("fold.b", None)],
            },
            item("code.b", None),
        ],
    }];
    let mut drawer = MenuDrawer::new(menus);
    drawer.set_area(Rect::new(0.0, 0.0, width, 600.0), 1.0);
    paint(&mut drawer);
    let code = drawer.menu_rows[0].0;
    drawer.hit(center(code));
    paint(&mut drawer);
    drawer
}

#[test]
fn a_rows_submenu_opens_as_a_third_panel_beside_its_row() {
    let mut drawer = nested(1200.0);
    assert_eq!(drawer.sub_panel, Rect::default(), "nothing before it's asked for");
    let folding = drawer.item_rect(1).expect("its row");
    assert!(drawer.over_clickable(center(folding)));
    // A click on the row shows it, nothing runs.
    assert_eq!(drawer.hit(center(folding)), DrawerAction::Inside);
    paint(&mut drawer);
    assert_eq!(drawer.nav().open_sub(), Some(1));
    assert_eq!(drawer.panel.size.width, MENUS_W + ITEMS_W + SUB_W, "widened once more");
    assert_eq!(drawer.sub_panel.min_x(), MENUS_W + ITEMS_W);
    // Its first row level with the row that opened it.
    let first = drawer.sub_item_rect(0).expect("its first row");
    assert_eq!(first.min_y(), folding.min_y());
    assert_eq!(drawer.hit(center(first)), DrawerAction::Run("fold.a".into()));
    // The keyboard: ← back to its row, the panel folding away.
    drawer.hover(center(first));
    assert_eq!(drawer.nav().focus(), NavFocus::Sub);
    let keys = drawer.footer_hints().into_iter().map(|(keys, _)| keys).collect::<Vec<_>>();
    assert_eq!(keys, [vec!["↑", "↓"], vec!["↵"], vec!["←"], vec!["esc"]]);
    drawer.key(NavKey::Left);
    paint(&mut drawer);
    assert_eq!(drawer.nav().open_sub(), None);
    assert_eq!(drawer.panel.size.width, MENUS_W + ITEMS_W);
}

#[test]
fn resting_on_a_rows_submenu_shows_it_and_another_row_closes_it_after_a_while() {
    let mut drawer = nested(1200.0);
    let folding = drawer.item_rect(1).unwrap();
    drawer.hover(center(folding));
    assert_eq!(drawer.nav().open_sub(), Some(1), "none shown: at once");
    paint(&mut drawer);
    // Down onto the next row: closed after a short rest, not at once.
    let next = drawer.item_rect(2).unwrap();
    drawer.hover(Point::new(center(folding).x, center(next).y));
    assert_eq!(drawer.nav().open_sub(), Some(1));
    let due = drawer.deadline().expect("a switch waiting");
    std::thread::sleep(due.saturating_duration_since(Instant::now()));
    paint(&mut drawer);
    assert_eq!(drawer.nav().open_sub(), None);
}

#[test]
fn in_a_narrow_window_the_third_panel_covers_the_second_with_a_way_back() {
    let mut drawer = nested(800.0);
    let folding = drawer.item_rect(1).unwrap();
    drawer.hit(center(folding));
    paint(&mut drawer);
    assert!(drawer.narrow);
    assert_eq!(drawer.panel.size.width, MENUS_W + ITEMS_W, "no wider: over the submenu");
    assert_eq!(drawer.sub_panel, drawer.items_panel);
    // Its rows are under the header, the submenu's not hit through it.
    let back = drawer.sub_back;
    assert!(back.size.width > 0.0 && drawer.over_clickable(center(back)));
    let first = drawer.sub_item_rect(0).unwrap();
    assert!(first.min_y() > back.max_y());
    assert_eq!(drawer.hit(center(first)), DrawerAction::Run("fold.a".into()));
    // The header goes back.
    assert_eq!(drawer.hit(center(back)), DrawerAction::Inside);
    paint(&mut drawer);
    assert_eq!(drawer.nav().open_sub(), None);
    assert_eq!(drawer.sub_panel, Rect::default());
    assert_eq!(drawer.nav().selected_item(), Some(1), "back on its row");
}

/// A row that doesn't apply now: greyed out, found and selectable, never run — by a click,
/// ↵ or the filter.
#[test]
fn a_disabled_row_is_shown_but_refused() {
    let mut drawer = drawer(2, 600.0);
    drawer.set_disabled(vec![("m0.a".into(), Some("Open a project first".into()))]);
    drawer.key(NavKey::Down);
    paint(&mut drawer);
    let a = drawer.item_rect(0).expect("still drawn");
    assert!(!drawer.over_clickable(center(a)));
    assert_eq!(drawer.hit(center(a)), DrawerAction::Inside, "a click runs nothing");
    drawer.key(NavKey::Right);
    assert_eq!(drawer.nav().selected_item(), Some(0), "the keyboard still reaches it");
    assert_eq!(drawer.key(NavKey::Enter), NavOutcome::Used, "↵ runs nothing");
    paint(&mut drawer);
    // The other row runs.
    let b = drawer.item_rect(2).unwrap();
    assert_eq!(drawer.hit(center(b)), DrawerAction::Run("m0.b".into()));
    // Filtered: found, still refused.
    drawer.type_text("m0.a");
    paint(&mut drawer);
    assert_eq!(drawer.result_rows.len(), 1);
    assert_eq!(drawer.key(NavKey::Enter), NavOutcome::Used);
    let row = drawer.result_rows[0].0;
    assert_eq!(drawer.hit(center(row)), DrawerAction::Inside);
}

/// The drawer's panel as the palette's floating glass says (55k): Glass over a glass
/// backdrop — slightly translucent, no blur of its own (the backdrop's is under it); Glass
/// over a simple backdrop — its own blur once it has slid in (opaque while it slides);
/// Simple — opaque, as always.
#[test]
fn its_panel_is_frosted_as_the_palette_says() {
    use ui_components::backdrop::SurfaceStyle;
    use ui_components::backdrop::{BackdropKind, BackdropSpec, BackdropStyle, SurfaceSpec};
    let glass = BackdropSpec { style: Some(BackdropKind::Glass), ..BackdropSpec::default() };
    let palette = |backdrop: BackdropSpec, floating: SurfaceSpec| {
        let base = Palette::dark();
        Palette {
            backdrop: BackdropStyle::resolve(backdrop, base.window_background),
            floating: SurfaceStyle::resolve(floating),
            ..base
        }
    };
    let painted = |drawer: &mut MenuDrawer, palette: &Palette| {
        let mut scene = Scene::default();
        drawer.paint(&Icons::builtin(), &mut TextSystem::new(), &mut scene, palette);
        // The backdrop's layer, then the panel's.
        let layer = scene.layers()[1].clone();
        assert_eq!(layer.quads[0].bounds.size.height, 600.0, "the panel's surface");
        (layer.quads[0].color.a, layer.blurs.len())
    };
    let mut d = drawer(4, 600.0);
    d.settle();
    let (alpha, blurs) = painted(&mut d, &palette(glass, glass));
    assert!((alpha - theming::backdrop::PANEL_OPACITY).abs() < 1e-3, "{alpha}");
    assert_eq!(blurs, 0, "the backdrop's blur is under it");
    d.settle();
    let (alpha, blurs) = painted(&mut d, &palette(BackdropSpec::default(), glass));
    assert!(alpha < 1.0);
    assert_eq!(blurs, 1, "its own, once settled");
    d.settle();
    let (alpha, blurs) = painted(&mut d, &palette(glass, SurfaceSpec::default()));
    assert_eq!((alpha, blurs), (1.0, 0), "Simple: opaque");
    // While it slides in over a simple backdrop: opaque, nothing blurred per frame.
    let mut sliding = MenuDrawer::new(menus(4));
    sliding.set_area(Rect::new(0.0, 40.0, 1000.0, 600.0), 1.0);
    let (alpha, blurs) = painted(&mut sliding, &palette(BackdropSpec::default(), glass));
    assert_eq!((alpha, blurs), (1.0, 0));
}

/// The categories' scrollbar: its thumb pressed holds the mouse (nothing under it chosen),
/// dragged down the list follows, let go nothing follows.
#[test]
fn the_categories_scroll_by_their_thumb() {
    let mut drawer = drawer(20, 400.0);
    let room = drawer.menus_room;
    let lane = Point::new(room.max_x() - 3.0, room.min_y() + 6.0);
    drawer.hover(lane);
    paint(&mut drawer);
    assert_eq!(drawer.hit(lane), DrawerAction::Drag, "the thumb takes the press");
    assert!(drawer.scrollbar_held());
    assert!(drawer.drag_scrollbar(Point::new(lane.x, lane.y + 120.0)), "the list follows");
    assert!(drawer.release());
    assert!(!drawer.scrollbar_held());
    assert!(!drawer.drag_scrollbar(Point::new(lane.x, lane.y)), "let go: nothing follows");
}

/// Its filter is the app's search field among its parts: typed into, ⌘A selects it, a
/// press and a drag select in it, its caret while the drawer is open, × (a hand over it)
/// empties it and every menu comes back.
#[test]
fn its_filter_is_the_shared_field_with_its_clear_button() {
    let mut drawer = drawer(3, 600.0);
    drawer.type_text("m2.");
    ui_components::control::Control::command(&mut drawer.query, "select_all");
    assert_eq!(drawer.query.input.selected_text(), "m2.");
    paint(&mut drawer);
    assert!(drawer.query.shows_caret(), "open: its one field has the caret");
    let field = drawer.query.area();
    let start = Point::new(field.min_x() + 40.0, field.min_y() + field.size.height / 2.0);
    assert_eq!(drawer.hit_with(start, 1), DrawerAction::Drag, "a press in it holds the mouse");
    assert!(drawer.scrollbar_held());
    assert!(drawer.drag_scrollbar(Point::new(field.min_x() + 2.0, start.y)));
    assert!(drawer.release());
    let clear = drawer.query.clear_area().expect("× shown with text");
    let on = Point::new(clear.min_x() + 2.0, clear.min_y() + 2.0);
    assert!(drawer.over_clickable(on));
    assert_eq!(drawer.hit(on), DrawerAction::Inside);
    assert!(!drawer.has_query(), "× emptied it");
    paint(&mut drawer);
    assert_eq!(drawer.menu_rows.len(), 3, "every menu back");
}
