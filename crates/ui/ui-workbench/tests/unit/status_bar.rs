use super::*;

/// A long path is cut in the middle (its file name and extension kept), never wider than its
/// widest, and all of it in its tooltip; a short one isn't cut.
#[test]
fn a_long_path_is_cut_in_the_middle_with_its_tooltip() {
    let (mut ts, palette) = (TextSystem::new(), Palette::dark());
    let mut bar = StatusBar::default();
    bar.set_bounds(Rect::new(0.0, 0.0, 1600.0, 32.0), 1.0);
    let long = format!("src/{}/very long name copy.css", "deep folder ".repeat(20));
    let left = [StatusItem::text(long.clone()).as_path()];
    bar.paint(&left, &[], &Icons::builtin(), &mut ts, &mut Scene::default(), &palette);
    let (rect, whole) = bar.tooltip_at(Point::new(20.0, 16.0)).expect("cut: a tooltip");
    assert_eq!(whole, long);
    assert!(rect.size.width <= crate::ds::PATH_MAX + 1.0, "{rect:?}");
    let short = [StatusItem::text("src/a.css").as_path()];
    bar.paint(&short, &[], &Icons::builtin(), &mut ts, &mut Scene::default(), &palette);
    assert!(bar.tooltip_at(Point::new(20.0, 16.0)).is_none(), "whole: no tooltip");
}

/// Too narrow for all of them: the spare items go first, then the normal ones, the farthest
/// from the corner first, a joined item with the one before it; the essential ones stay.
#[test]
fn what_doesn_t_fit_goes_by_priority() {
    let items = [
        StatusItem::text("indicator"),
        StatusItem::text("12:4").essential(),
        StatusItem::text("JSON"),
        StatusItem::text("−").spare(),
        StatusItem::text("100%").spare().joined(),
        StatusItem::text("+").spare().joined(),
        StatusItem::text("lock").essential(),
    ];
    let widths = [10.0; 7];
    assert_eq!(fit(&items, &widths, 70.0, 5.0), [true; 7], "all fit");
    // 60 with the `…`: the zoom (spare, joined: all three) goes.
    assert_eq!(fit(&items, &widths, 60.0, 5.0), [true, true, true, false, false, false, true]);
    // Then the normal ones, the first first.
    assert_eq!(fit(&items, &widths, 40.0, 5.0), [false, true, true, false, false, false, true]);
    // However narrow: the essential ones stay.
    assert_eq!(fit(&items, &widths, 0.0, 5.0), [false, true, false, false, false, false, true]);
}

/// On a narrow bar nothing overlaps: the right items that fit end where they begin, the
/// others are listed under `…` (its command), the left keeps its least.
#[test]
fn a_narrow_bar_puts_what_doesn_t_fit_under_more() {
    let (mut ts, palette) = (TextSystem::new(), Palette::dark());
    let mut bar = StatusBar::default();
    bar.set_bounds(Rect::new(0.0, 0.0, 420.0, 32.0), 1.0);
    let left = [StatusItem::text("src/components/very/long/path/file.json").as_path()];
    let right = [
        StatusItem::text("vscode-json + built-in engine").clickable("engine").spare(),
        StatusItem::text("JSON").clickable("language").grouped(),
        StatusItem::text("UTF-8").clickable("encoding"),
        StatusItem::text("4 spaces").clickable("indent"),
        StatusItem::text("12:40").essential(),
        StatusItem::text("lock").clickable("lock").essential(),
    ];
    bar.paint(&left, &right, &Icons::builtin(), &mut ts, &mut Scene::default(), &palette);
    let hidden: Vec<&str> = bar.hidden().iter().map(|i| i.text.as_str()).collect();
    assert!(hidden.contains(&"vscode-json + built-in engine"), "{hidden:?}");
    assert!(bar.action_rect(MORE).is_some(), "the `…` stands for them");
    assert!(bar.action_rect("lock").is_some(), "the essential stays");
    // The items drawn don't overlap.
    let mut rects: Vec<Rect> = bar.actions.iter().map(|(r, _)| *r).collect();
    rects.sort_by(|a, b| a.min_x().total_cmp(&b.min_x()));
    for pair in rects.windows(2) {
        assert!(pair[0].max_x() <= pair[1].min_x() + 0.5, "{pair:?}");
    }
    // Wide again: nothing hidden, no `…`.
    bar.set_bounds(Rect::new(0.0, 0.0, 1600.0, 32.0), 1.0);
    bar.paint(&left, &right, &Icons::builtin(), &mut ts, &mut Scene::default(), &palette);
    assert!(bar.hidden().is_empty() && bar.action_rect(MORE).is_none());
}

/// A meter's bars glide to new heights (frames only while they move), then stand: the same
/// values again ask for nothing.
#[test]
fn a_meter_glides_only_when_its_values_change() {
    let mut bar = StatusBar::default();
    let t0 = std::time::Instant::now();
    assert_eq!(bar.meter_at(&[0.2, 0.5], t0), [0.2, 0.5], "first drawn: as it is");
    assert_eq!(bar.meter_at(&[0.2, 0.5], t0), [0.2, 0.5]);
    let half = bar.meter_at(&[0.8, 0.5], t0);
    assert_eq!(half, [0.2, 0.5], "a change starts from where it was");
    let mid = bar.meter_at(&[0.8, 0.5], t0 + METER_GLIDE / 2);
    assert!(mid[0] > 0.2 && mid[0] < 0.8, "{mid:?}");
    assert!(bar.meter_since.is_some(), "frames while it moves");
    let end = bar.meter_at(&[0.8, 0.5], t0 + METER_GLIDE * 2);
    assert_eq!(end, [0.8, 0.5]);
    assert!(bar.meter_since.is_none() && bar.next_frame().is_none(), "at rest: no frames");
}

/// In a window not in front the bars jump to their heights: no glide, no frames asked (a
/// dozen whole frames at every sample, in every window, before).
#[test]
fn a_meter_in_a_window_behind_jumps() {
    let mut bar = StatusBar::default();
    bar.set_still(true);
    let t0 = std::time::Instant::now();
    bar.meter_at(&[0.2, 0.5], t0);
    assert_eq!(bar.meter_at(&[0.8, 0.5], t0), [0.8, 0.5], "at once");
    assert!(bar.next_frame().is_none(), "no frames for it");
    // In front again: it glides.
    bar.set_still(false);
    assert_eq!(bar.meter_at(&[0.1, 0.5], t0), [0.8, 0.5]);
    assert!(bar.next_frame().is_some() || bar.meter_since.is_some());
}

/// A clickable item is the shared button: under the mouse its back in the tabs' hover,
/// rounded; its text on the same baseline as a plain item's.
#[test]
fn a_clickable_item_is_a_button_on_the_bar_s_baseline() {
    let (mut ts, palette) = (TextSystem::new(), Palette::dark());
    let mut bar = StatusBar::default();
    bar.set_bounds(Rect::new(0.0, 0.0, 800.0, 32.0), 1.0);
    let right = [StatusItem::text("UTF-8").clickable("encoding"), StatusItem::text("12:40")];
    let paint = |bar: &mut StatusBar, ts: &mut TextSystem| {
        let mut scene = Scene::default();
        bar.paint(&[], &right, &Icons::builtin(), ts, &mut scene, &palette);
        scene
    };
    let rest = paint(&mut bar, &mut ts);
    let backs = |scene: &Scene| {
        scene
            .layers()
            .iter()
            .flat_map(|l| &l.quads)
            .filter(|q| q.color == palette.tab_hover)
            .count()
    };
    assert_eq!(backs(&rest), 0, "at rest: no back");
    let pill = bar.action_rect("encoding").expect("its hit");
    assert!(bar.hover(pill.center()));
    let lit = paint(&mut bar, &mut ts);
    let back = lit.layers().iter().flat_map(|l| &l.quads).find(|q| q.color == palette.tab_hover);
    assert_eq!(back.map(|q| (q.bounds, q.corner_radius)), Some((pill, ITEM_RADIUS)));
    let mut baselines: Vec<f32> =
        lit.layers().iter().flat_map(|l| &l.glyphs).map(|g| g.origin.y.round()).collect();
    baselines.dedup();
    assert_eq!(baselines.len(), 1, "one baseline: {baselines:?}");
}
