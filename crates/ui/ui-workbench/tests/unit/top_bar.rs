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
    let actions =
        [("search", icons.id_or_file("search")), ("settings", icons.id_or_file("settings"))];
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
