use super::*;

#[test]
fn initials_from_project_names() {
    assert_eq!(project_initials("docker.acme.test"), "DA");
    assert_eq!(project_initials("seo-analyzer"), "SA");
    assert_eq!(project_initials("local"), "LO");
    assert_eq!(project_initials("..."), "?");
}

#[test]
fn hits_buttons_and_empty_area() {
    let mut bar = TopBar::default();
    bar.set_bounds(Rect::new(0.0, 0.0, 1000.0, 40.0), 1.0);
    let icons = Icons::builtin();
    let actions = [action(&icons, "search", ""), action(&icons, "settings", "")];
    let model = TopBarModel {
        project: Some("demo"),
        branch: Some("main"),
        branch_button: false,
        badge: None,
        left_inset: 80.0,
        controls: None,
        full_screen: false,
        maximize_hovered: false,
        actions: &actions,
        menu: None,
    };
    bar.paint(
        model,
        false,
        &icons,
        &mut TextSystem::new(),
        &mut Scene::default(),
        &Palette::dark(),
    );
    let a = bar.project_anchor();
    assert_eq!(bar.hit(Point::new(a.min_x() + 5.0, 20.0)), Some(TopBarHit::Projects));
    // The registered buttons, left to right, the last one nearest the edge.
    assert_eq!(bar.hit(Point::new(980.0, 20.0)), Some(TopBarHit::Action("settings")));
    assert_eq!(bar.hit(Point::new(948.0, 20.0)), Some(TopBarHit::Action("search")));
    assert_eq!(bar.hit(Point::new(600.0, 20.0)), Some(TopBarHit::Empty));
    assert_eq!(bar.hit(Point::new(600.0, 60.0)), None);
}

#[test]
fn the_menu_button_comes_first_and_the_project_after_it() {
    let icons = Icons::builtin();
    let paint = |menu: Option<bool>| {
        let mut bar = TopBar::default();
        bar.set_bounds(Rect::new(0.0, 0.0, 1000.0, 40.0), 1.0);
        let model = TopBarModel {
            project: Some("demo"),
            branch: None,
            branch_button: false,
            badge: None,
            left_inset: 0.0,
            controls: Some(false),
            full_screen: false,
            maximize_hovered: false,
            actions: &[],
            menu,
        };
        bar.paint(
            model,
            false,
            &icons,
            &mut TextSystem::new(),
            &mut Scene::default(),
            &Palette::dark(),
        );
        bar
    };
    let without = paint(None);
    assert_eq!(without.menu_button(), None);
    let with = paint(Some(false));
    let menu = with.menu_button().expect("the menu button");
    assert_eq!(menu.min_x(), 10.0, "first on the left");
    assert_eq!(with.hit(Point::new(menu.min_x() + 4.0, 20.0)), Some(TopBarHit::Menu));
    assert_eq!(with.hit_rect(Point::new(menu.min_x() + 4.0, 20.0)).map(|(r, _)| r), Some(menu));
    // The project's button moved right past it; nothing else changed.
    assert!(with.project_anchor().min_x() >= menu.max_x());
    assert_eq!(
        with.project_anchor().min_x() - without.project_anchor().min_x(),
        menu.max_x() + 4.0 - menu.min_x()
    );
}

/// A registered button of `group`, a plain one.
fn action(icons: &Icons, id: &'static str, group: &'static str) -> TopBarAction {
    TopBarAction { id, icon: icons.id_or_file(id), group, on: None }
}

/// The bar painted 1000 wide with `actions` on its right (no window buttons).
fn bar_with(actions: &[TopBarAction]) -> TopBar {
    let icons = Icons::builtin();
    let mut bar = TopBar::default();
    bar.set_bounds(Rect::new(0.0, 0.0, 1000.0, 40.0), 1.0);
    let model = TopBarModel {
        project: Some("demo"),
        branch: None,
        branch_button: false,
        badge: None,
        left_inset: 0.0,
        controls: None,
        full_screen: false,
        maximize_hovered: false,
        actions,
        menu: None,
    };
    bar.paint(
        model,
        false,
        &icons,
        &mut TextSystem::new(),
        &mut Scene::default(),
        &Palette::dark(),
    );
    bar
}

/// A button of another group after the others: a short line between them, the button past
/// it; none between two of the same group.
#[test]
fn a_line_parts_buttons_of_different_groups() {
    let icons = Icons::builtin();
    let pin = TopBarAction { on: Some(true), ..action(&icons, "pin", "window") };
    let bar = bar_with(&[action(&icons, "search", ""), action(&icons, "settings", ""), pin]);
    let at = |x: f32| bar.hit(Point::new(x, 20.0));
    assert_eq!(at(980.0), Some(TopBarHit::Action("pin")), "the last one at the edge");
    let lines = bar.separators();
    assert_eq!(lines.len(), 1, "one line: between settings and the pin");
    let line = lines[0];
    // Where each one starts, from the left.
    let left = |id| (0..1000).map(|x| x as f32).find(|x| at(*x) == Some(TopBarHit::Action(id)));
    let [Some(search), Some(settings), Some(pin)] = ["search", "settings", "pin"].map(left) else {
        panic!("every button drawn");
    };
    // Left to right: search, settings, the line, the pin.
    assert!(search < settings && settings < line.min_x() && line.max_x() < pin);
    assert!(at(line.min_x()) == Some(TopBarHit::Empty), "the line isn't a button");
    assert!(line.size.height <= 16.0 && line.min_y() > 0.0, "short, centered");
    // Without another group: no line, the buttons as before.
    let plain = bar_with(&[action(&icons, "search", ""), action(&icons, "settings", "")]);
    assert!(plain.separators().is_empty());
}

/// The window's button pressed is kept until it's let go (its release acts).
#[test]
fn an_armed_window_button_is_remembered_until_let_go() {
    let mut bar = TopBar::default();
    assert_eq!(bar.armed(), None);
    bar.arm(Some(TopBarHit::Close));
    assert_eq!(bar.armed(), Some(TopBarHit::Close));
    bar.arm(None);
    assert_eq!(bar.armed(), None);
}
