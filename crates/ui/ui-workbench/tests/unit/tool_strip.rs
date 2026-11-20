use super::*;

#[test]
fn top_and_bottom_groups_are_hit_tested() {
    let icons = Icons::builtin();
    let folder = icons.id("folder").unwrap();
    let mut strip = ToolStrip::default();
    strip.set_bounds(Rect::new(0.0, 0.0, 44.0, 600.0), 1.0);
    let buttons = [
        ToolButton {
            id: "project",
            icon: folder,
            active: true,
            bottom: false,
            badge: None,
            group: "",
            tone: None,
        },
        ToolButton {
            id: "terminal",
            icon: folder,
            active: false,
            bottom: true,
            badge: Some(crate::ds::BadgeKind::Count),
            group: "",
            tone: None,
        },
    ];
    strip.paint(&buttons, &icons, &Palette::dark(), &mut Scene::default());
    assert_eq!(strip.hit(Point::new(22.0, 20.0)), Some("project"));
    assert_eq!(strip.hit(Point::new(22.0, 580.0)), Some("terminal"));
    assert_eq!(strip.hit(Point::new(22.0, 300.0)), None);
}

/// A dot at the icon's top right corner (something to see; its count in the tooltip): on
/// whole pixels, its color by meaning, a ring in the strip's background around it, inside
/// its button and over no more than the icon's corner.
#[test]
fn a_dot_sits_on_the_icons_corner() {
    use crate::ds::{BadgeKind, DOT_BADGE};
    let icons = Icons::builtin();
    let folder = icons.id("folder").unwrap();
    let palette = Palette::dark();
    let mut strip = ToolStrip::default();
    strip.set_bounds(Rect::new(0.0, 0.0, 44.0, 600.0), 1.0);
    for (kind, color) in [
        (BadgeKind::Count, palette.accent),
        (BadgeKind::Error, ui_components::severity::Level::Critical.color(&palette)),
    ] {
        let button = ToolButton {
            id: "remote",
            icon: folder,
            active: false,
            bottom: false,
            badge: Some(kind),
            group: "",
            tone: None,
        };
        let mut scene = Scene::default();
        strip.paint(&[button], &icons, &palette, &mut scene);
        let layer = &scene.layers()[0];
        let dot = layer.quads.iter().find(|q| q.color == color).expect("the dot").bounds;
        assert_eq!((dot.size.width, dot.size.height), (DOT_BADGE, DOT_BADGE), "a circle");
        assert_eq!((dot.min_x().fract(), dot.min_y().fract()), (0.0, 0.0), "whole pixels");
        // The icon: 18 px centered in the 32 px button at x 6..38, y 8..40: icon 13..31.
        let icon = Rect::new(13.0, 15.0, 18.0, 18.0);
        assert!(dot.max_y() <= icon.min_y() + icon.size.height / 2.0, "{dot:?}");
        assert!(dot.min_x() >= icon.min_x() + icon.size.width / 2.0, "{dot:?}");
        assert!(layer.quads.iter().any(|q| q.color == palette.window_background), "its ring");
        // Inside its button (6..38, 8..40), ring and all.
        assert!(dot.min_y() - 2.0 >= 8.0 && dot.max_x() + 2.0 <= 38.0, "{dot:?}");
    }
    // Nothing to see: no dot.
    let button = ToolButton {
        id: "remote",
        icon: folder,
        active: false,
        bottom: false,
        badge: None,
        group: "",
        tone: None,
    };
    let mut scene = Scene::default();
    strip.paint(&[button], &icons, &palette, &mut scene);
    assert!(!scene.layers()[0].quads.iter().any(|q| q.color == palette.accent));
}

/// On a lit button, the dot's ring is the button's own color (not the strip's): it reads
/// cut out of the button, not stuck on it.
#[test]
fn a_lit_button_s_dot_is_ringed_in_its_color() {
    use crate::ds::BadgeKind;
    let icons = Icons::builtin();
    let palette = Palette::dark();
    let mut strip = ToolStrip::default();
    strip.set_bounds(Rect::new(0.0, 0.0, 44.0, 600.0), 1.0);
    let button = ToolButton {
        id: "git",
        icon: icons.id("folder").unwrap(),
        active: true,
        bottom: false,
        badge: Some(BadgeKind::Count),
        group: "",
        tone: None,
    };
    let mut scene = Scene::default();
    strip.paint(&[button], &icons, &palette, &mut scene);
    let quads = &scene.layers()[0].quads;
    let lit = quads[0].color;
    assert!(quads.iter().skip(1).any(|q| q.color == lit), "the ring: the button's color");
    assert!(!quads.iter().any(|q| q.color == palette.window_background), "not the strip's");
}

/// Neighbors of different groups: a short line between them, with room around it (the next
/// button lower by it); none between those of a group — at the top and at the bottom.
#[test]
fn a_line_parts_the_groups() {
    let icons = Icons::builtin();
    let folder = icons.id("folder").unwrap();
    let palette = Palette::dark();
    let button = |id, bottom, group| ToolButton {
        id,
        icon: folder,
        active: false,
        bottom,
        badge: None,
        group,
        tone: None,
    };
    let lines = |scene: &Scene| {
        scene.layers()[0]
            .quads
            .iter()
            .filter(|q| q.bounds.size.height == 1.0 && q.bounds.size.width < 10.0)
            .map(|q| q.bounds.min_y() as i32)
            .collect::<std::collections::BTreeSet<_>>()
    };
    let mut strip = ToolStrip::default();
    strip.set_bounds(Rect::new(0.0, 0.0, 44.0, 600.0), 1.0);
    // One group: no line, the buttons 38 apart (32 and a 6 gap).
    let mut scene = Scene::default();
    let same = [button("a", false, "code"), button("b", false, "code")];
    strip.paint(&same, &icons, &palette, &mut scene);
    assert!(lines(&scene).is_empty());
    let b_at = strip.hit_rect(Point::new(22.0, 50.0)).map(|(r, _)| r.min_y());
    assert_eq!(b_at, Some(46.0));
    // Two groups at the top, two at the bottom: a line in each place, the next button 10 lower.
    let mut scene = Scene::default();
    let parted = [
        button("a", false, "code"),
        button("b", false, "services"),
        button("x", true, "modes"),
        button("y", true, ""),
    ];
    strip.paint(&parted, &icons, &palette, &mut scene);
    let found = lines(&scene);
    assert_eq!(found.len(), 2, "{found:?}");
    let b = strip.hit_rect(Point::new(22.0, 60.0)).expect("b").0;
    assert_eq!(b.min_y(), 56.0, "10 lower");
    let line = *found.iter().next().unwrap();
    assert!(line > 40 && (line as f32) < b.min_y(), "between a and b: {line}");
    assert_eq!(strip.hit(Point::new(22.0, 580.0)), Some("y"));
    // A strip too short for the lines' room: no lines, the buttons as without groups.
    strip.set_bounds(Rect::new(0.0, 0.0, 44.0, 160.0), 1.0);
    let mut scene = Scene::default();
    strip.paint(&parted, &icons, &palette, &mut scene);
    assert!(lines(&scene).is_empty());
    let b_at = strip.hit_rect(Point::new(22.0, 50.0)).map(|(r, _)| r.min_y());
    assert_eq!(b_at, Some(46.0));
}
