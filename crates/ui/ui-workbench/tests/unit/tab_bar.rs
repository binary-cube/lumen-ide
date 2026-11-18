use super::*;
use text_layout::TextSystem;

fn paint(bar: &mut TabBar, count: usize, active: usize) {
    let titles: Vec<String> = (0..count).map(|i| format!("fisier_{i}.php")).collect();
    let tabs: Vec<TabTitle<'_>> = titles
        .iter()
        .map(|t| TabTitle {
            title: t,
            dirty: false,
            pinned: false,
            icon: None,
            accent: None,
            badge: None,
            locked: false,
            outside: false,
        })
        .collect();
    let mut text = TextSystem::new();
    let mut scene = Scene::default();
    let painter = Painter {
        text: &mut text,
        scene: &mut scene,
        palette: &Palette::dark(),
        icons: &Icons::builtin(),
    };
    bar.paint(&tabs, active, Navigation::default(), painter);
}

#[test]
fn hit_distinguishes_close_button_and_navigation() {
    let mut bar = TabBar::default();
    bar.set_bounds(Rect::new(0.0, 0.0, 800.0, 42.0), 1.0);
    paint(&mut bar, 2, 0);

    let (first, close) = bar.rects[0];
    assert_eq!(bar.hit(Point::new(first.min_x() + 2.0, 20.0)), Some(TabHit::Activate(0)));
    assert_eq!(
        bar.hit(Point::new(close.min_x() + 4.0, close.min_y() + 4.0)),
        Some(TabHit::Close(0))
    );
    let back = bar.back;
    assert_eq!(bar.hit(Point::new(back.min_x() + 4.0, back.min_y() + 4.0)), Some(TabHit::Back));
    assert_eq!(bar.hit(Point::new(799.0, 20.0)), None);
}

#[test]
fn overflowing_tabs_scroll_and_reveal_active() {
    let mut bar = TabBar::default();
    bar.set_bounds(Rect::new(0.0, 0.0, 500.0, 42.0), 1.0);
    paint(&mut bar, 12, 0);
    assert!(bar.scroll_by(200.0));
    assert!(!bar.scroll_by(-1000.0) || bar.scroll_x == 0.0);
    bar.reveal_active();
    paint(&mut bar, 12, 11);
    let (last, _) = bar.rects[11];
    assert!(last.max_x() <= bar.strip.max_x() + 0.5, "the last tab must be visible");
}

#[test]
fn overflow_indicators_appear_only_where_tabs_are_hidden() {
    let mut bar = TabBar::default();
    bar.set_bounds(Rect::new(0.0, 0.0, 500.0, 42.0), 1.0);
    paint(&mut bar, 12, 0);
    assert_eq!(bar.more_left, Rect::default(), "at first nothing is hidden on the left");
    let right = bar.more_right;
    assert_eq!(
        bar.hit(Point::new(right.min_x() + 5.0, right.min_y() + 5.0)),
        Some(TabHit::MoreRight)
    );
    assert!(bar.page(true));
    paint(&mut bar, 12, 0);
    assert_ne!(bar.more_left, Rect::default(), "once scrolled, the left one shows too");
}

#[test]
fn a_tab_with_a_color_carries_it_as_a_dot_on_its_icon() {
    let accent = Rgba::rgb(0xF2555A);
    let icons = Icons::builtin();
    let icon = icons.id("table").map(|id| (id, Rgba::rgb(0x888888)));
    let paint_one = |color: Option<Rgba>| {
        let mut bar = TabBar::default();
        bar.set_bounds(Rect::new(0.0, 0.0, 800.0, 42.0), 1.0);
        let tabs = [TabTitle {
            title: "orders",
            dirty: false,
            pinned: false,
            icon,
            accent: color,
            badge: None,
            locked: false,
            outside: false,
        }];
        let mut text = TextSystem::new();
        let mut scene = Scene::default();
        let painter = Painter {
            text: &mut text,
            scene: &mut scene,
            palette: &Palette::dark(),
            icons: &icons,
        };
        bar.paint(&tabs, 0, Navigation::default(), painter);
        let dots =
            scene.layers().iter().flat_map(|l| &l.quads).filter(|q| q.color == accent).count();
        (dots, bar.rects[0].0)
    };
    let (with, rect) = paint_one(Some(accent));
    let (without, plain) = paint_one(None);
    assert_eq!((with, without), (1, 0));
    assert_eq!(rect, plain, "the tab keeps its width");
}

#[test]
fn a_tab_s_badge_follows_its_title_and_widens_it() {
    let tint = Rgba::rgb(0xE5A50A);
    let paint_one = |badge: Option<(&'static str, Rgba)>| {
        let mut bar = TabBar::default();
        bar.set_bounds(Rect::new(0.0, 0.0, 800.0, 42.0), 1.0);
        let tabs = [TabTitle {
            title: "console [shop]",
            dirty: false,
            pinned: false,
            icon: None,
            accent: None,
            badge,
            locked: false,
            outside: false,
        }];
        let mut text = TextSystem::new();
        let mut scene = Scene::default();
        let painter = Painter {
            text: &mut text,
            scene: &mut scene,
            palette: &Palette::dark(),
            icons: &Icons::builtin(),
        };
        bar.paint(&tabs, 0, Navigation::default(), painter);
        let pills = scene
            .layers()
            .iter()
            .flat_map(|l| &l.quads)
            .filter(|q| q.color == tint.with_alpha(0.16))
            .count();
        (pills, bar.rects[0].0)
    };
    let (pills, marked) = paint_one(Some(("TX", tint)));
    let (none, plain) = paint_one(None);
    assert_eq!((pills, none), (1, 0));
    assert!(marked.size.width > plain.size.width, "room for it");
    assert_eq!(marked.min_x(), plain.min_x());
}
