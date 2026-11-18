//! The modal frame's size: its size's at most, or as tall as the content it's asked to fit.

use super::*;

/// The content area of a modal painted in a big window, asked to fit `fit`.
fn content_with(fit: Option<f32>, hints: &[KeyHint<'_>]) -> Rect {
    content_sized(fit, None, "Title", hints)
}

/// The same, asked to be as narrow as `fit_width`, with `title`.
fn content_sized(
    fit: Option<f32>,
    fit_width: Option<f32>,
    title: &str,
    hints: &[KeyHint<'_>],
) -> Rect {
    let mut modal = Modal::default();
    modal.set_viewport(Rect::new(0.0, 0.0, 1600.0, 1200.0), 1.0);
    modal.open();
    let spec = ModalSpec {
        title,
        subtitle: Some("Subtitle"),
        icon: None,
        size: ModalSize::Large,
        sections: &[],
        nav_header: 0.0,
        header_field: 0.0,
        active: None,
        hints,
        buttons: &[],
        status: None,
        fit,
        fit_width,
        zoom: None,
    };
    let icons = Icons::builtin();
    let palette = Palette::dark();
    let mut text = TextSystem::new();
    let mut scene = Scene::default();
    modal.paint(&spec, &icons, &mut text, &mut scene, &palette).expect("open").0
}

#[test]
fn a_modal_fits_the_content_it_asks_for_within_its_size() {
    let hints: &[KeyHint<'_>] = &[(&["esc"], "close")];
    assert_eq!(content_with(Some(240.0), hints).size.height, 240.0);
    assert_eq!(content_with(Some(240.0), &[]).size.height, 240.0, "without a footer too");
    let full = content_with(None, hints).size.height;
    assert!(full > 240.0, "its size's height: {full}");
    assert_eq!(content_with(Some(5000.0), hints).size.height, full, "never taller");
}

/// A short content (a question) asks for a narrow window: as wide as that, or as its title
/// needs, never wider than its size.
#[test]
fn a_question_is_as_wide_as_its_title_needs_within_its_size() {
    let short = content_sized(Some(100.0), Some(440.0), "Stop?", &[]);
    assert_eq!(short.size.width, 440.0);
    let long =
        "Stop the program that formats and checks this project's files, with all it started?";
    let wide = content_sized(Some(100.0), Some(440.0), long, &[]);
    assert!(wide.size.width > 440.0 && wide.size.width <= 878.0, "{}", wide.size.width);
    let huge = content_sized(Some(100.0), Some(440.0), &long.repeat(4), &[]);
    // Inside the panel's border (a line each side): the panel as wide as its size.
    assert_eq!(huge.size.width, 878.0, "never wider than its size");
    assert_eq!(content_with(None, &[]).size.width, 878.0, "no width asked: the size's");
}

/// A navigation taller than its room keeps its place on the left and scrolls (its bar in
/// a lane of its own): only the rows that show are clickable, the wheel over it moves it,
/// never past its end; one that fits doesn't scroll.
#[test]
fn a_navigation_taller_than_its_room_scrolls() {
    const IDS: [&str; 12] = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l"];
    let sections: Vec<ModalSection<'_>> = IDS
        .iter()
        .enumerate()
        .map(|(i, id)| ModalSection {
            id,
            label: id,
            icon: "gear",
            group: if i < 6 { "One" } else { "Two" },
            group_icon: "gear",
        })
        .collect();
    let paint = |modal: &mut Modal, height: f32| {
        modal.set_viewport(Rect::new(0.0, 0.0, 1400.0, height), 1.0);
        let spec = ModalSpec {
            title: "Settings",
            subtitle: None,
            icon: None,
            size: ModalSize::XLarge,
            sections: &sections,
            nav_header: 0.0,
            header_field: 0.0,
            active: Some("a"),
            hints: &[],
            buttons: &[],
            status: None,
            fit: None,
            fit_width: None,
            zoom: None,
        };
        let icons = Icons::builtin();
        let mut text = TextSystem::new();
        let mut scene = Scene::default();
        modal.paint(&spec, &icons, &mut text, &mut scene, &Palette::dark()).expect("open");
    };
    let mut modal = Modal::default();
    modal.open();
    paint(&mut modal, 360.0);
    let nav = modal.nav();
    assert!(nav.size.width > 0.0 && nav.size.width < 300.0, "still the side list: {nav:?}");
    let inside = Point::new(nav.min_x() + 40.0, nav.min_y() + 20.0);
    assert!(modal.scroll_nav(inside, 80.0));
    paint(&mut modal, 360.0);
    assert!(modal.nav_scroll() > 0.0);
    // Never past its end.
    assert!(modal.scroll_nav(inside, 10_000.0));
    paint(&mut modal, 360.0);
    let end = modal.nav_scroll();
    assert!(end < 1_000.0, "{end}");
    // The last row shows at the bottom, the first is gone: clicking where it was does nothing.
    let bottom = Point::new(nav.min_x() + 40.0, nav.max_y() - 20.0);
    assert_eq!(modal.hit(bottom), Some(ModalHit::Section("l")));
    assert_ne!(
        modal.hit(Point::new(nav.min_x() + 40.0, nav.min_y() + 2.0)),
        Some(ModalHit::Section("a"))
    );
    // Outside it the wheel isn't its; tall enough, nothing scrolls.
    assert!(!modal.scroll_nav(Point::new(nav.max_x() + 50.0, nav.min_y() + 20.0), 10.0));
    let mut tall = Modal::default();
    tall.open();
    paint(&mut tall, 1400.0);
    assert!(
        !tall.scroll_nav(Point::new(tall.nav().min_x() + 40.0, tall.nav().min_y() + 20.0), 50.0)
    );
}
