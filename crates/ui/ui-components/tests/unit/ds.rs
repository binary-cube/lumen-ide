use super::*;

#[test]
fn long_text_wraps_between_words_on_two_lines_at_most() {
    let mut text = TextSystem::new();
    let font = Font::new(Palette::dark().ui_font, 13.0);
    let width = |text: &mut TextSystem, s: &str| text.shape_line(s, &font).width;
    let value = "com.apple.NSEventThread · notify-rs fsevents loop · index-watch · git-status";
    let max = width(&mut text, "com.apple.NSEventThread · notify");
    let lines = wrap(&mut text, value, &font, max, 2);
    assert_eq!(lines.len(), 2);
    assert!(lines.iter().all(|l| width(&mut text, l) <= max + 0.5), "{lines:?}");
    assert!(lines[1].ends_with('…'), "the rest cut short: {lines:?}");
    // Short: one line, untouched.
    assert_eq!(wrap(&mut text, "Interface", &font, max, 2), vec!["Interface".to_owned()]);
    // Fits on two: no ellipsis.
    let room = width(&mut text, "Previews (images,");
    let two = wrap(&mut text, "Previews (images, documents)", &font, room, 2);
    assert_eq!(two, vec!["Previews (images,".to_owned(), "documents)".to_owned()]);
}

/// A long text cut with "…" as it's drawn (nothing allocated), inside its room; a short
/// one whole.
#[test]
fn a_text_drawn_cut_stays_in_its_room() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let font = ctx.ui_font(0.0);
    let mut scene = render_api::Scene::default();
    let layer = scene.push_layer(None);
    let (w, cut) =
        draw_ellipsized(layer, &mut text, "short", &font, (10.0, 20.0), 400.0, palette.text);
    assert!(!cut && w > 0.0);
    let whole = layer.glyphs.len();
    layer.glyphs.clear();
    let long = "A rather long name of a part that will never fit in the room it has";
    for max in [40.0, 80.0, 133.0, 200.0] {
        layer.glyphs.clear();
        let (w, cut) =
            draw_ellipsized(layer, &mut text, long, &font, (10.0, 20.0), max, palette.text);
        assert!(cut && w <= max + 0.5, "{w} in {max}");
        assert!(layer.glyphs.iter().all(|g| g.origin.x <= 10.0 + max));
    }
    assert_eq!(whole, 5);
}

/// The baselines a layer's glyphs sit on, distinct, top to bottom.
fn baselines(layer: &render_api::Layer) -> Vec<f32> {
    let mut ys: Vec<f32> = layer.glyphs.iter().map(|g| g.origin.y.round()).collect();
    ys.sort_by(f32::total_cmp);
    ys.dedup();
    ys
}

/// A tooltip of two lines (the profile tree's step: its words, then its kind) shows them
/// one under the other — never drawn over each other — in a pill sized from them.
#[test]
fn a_tooltips_lines_are_stacked_in_a_pill_sized_for_them() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let bounds = Rect::new(0.0, 0.0, 1200.0, 800.0);
    let anchor = Rect::new(100.0, 100.0, 200.0, 24.0);
    let mut scene = render_api::Scene::default();
    let one = scene.push_layer(None);
    let single = tooltip(one, &mut text, &ctx, anchor, "Sequential scan", None, bounds);
    assert_eq!(baselines(one).len(), 1);
    let font = ctx.ui_font(-1.0);
    let wide = text.shape_line("Sequential scan", &font).width;
    assert!((single.size.width - (wide + 20.0).round()).abs() <= 1.0, "{single:?}");

    let label = "SEARCH t2 USING INTEGER PRIMARY KEY (rowid=?) LEFT-JOIN\nRow lookup";
    let two = scene.push_layer(None);
    let rect = tooltip(two, &mut text, &ctx, anchor, label, None, bounds);
    let ys = baselines(two);
    assert_eq!(ys.len(), 2, "two lines, each on its own baseline: {ys:?}");
    assert!(ys[1] - ys[0] >= 14.0, "not on top of each other: {ys:?}");
    assert!(rect.size.height > single.size.height);
    assert!(ys.iter().all(|y| *y > rect.min_y() && *y < rect.max_y()), "{ys:?} in {rect:?}");
    // As wide as the widest line, not as the lines put end to end.
    let first = text.shape_line("SEARCH t2 USING INTEGER PRIMARY KEY (rowid=?) LEFT-JOIN", &font);
    assert!(rect.size.width <= (first.width + 20.0).round() + 1.0, "{rect:?}");
    assert!(two.glyphs.iter().all(|g| g.origin.x >= rect.min_x() && g.origin.x < rect.max_x()));
}

/// A line too long for the pill wraps between words; the pill stays within its bounds.
#[test]
fn a_long_tooltip_wraps_within_its_bounds() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 2.0);
    let mut text = TextSystem::new();
    let bounds = Rect::new(0.0, 0.0, 2400.0, 1400.0);
    let anchor = Rect::new(600.0, 600.0, 100.0, 40.0);
    let long = "word ".repeat(120);
    let mut scene = render_api::Scene::default();
    let layer = scene.push_layer(None);
    let rect = tooltip(layer, &mut text, &ctx, anchor, &long, Some("⌘K"), bounds);
    assert!(baselines(layer).len() > 1);
    assert!(rect.size.width <= ctx.px(TOOLTIP_MAX_W) + ctx.px(60.0), "{rect:?}");
    assert!(bounds.contains(rect.origin) && rect.max_x() <= bounds.max_x());
    // Empty lines are dropped; a lone line keeps the one-line height.
    let layer = scene.push_layer(None);
    let one = tooltip(layer, &mut text, &ctx, anchor, "Close\n\n", None, bounds);
    assert_eq!(one.size.height, ctx.px(28.0));
}

/// A section's header: its icon, its title in capitals, and a thin line from after the
/// title to the end of its width; the content starts below it.
#[test]
fn a_section_header_draws_its_icon_title_and_line_to_the_end() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 2.0);
    let icons = icons::Icons::builtin();
    let mut text = TextSystem::new();
    let mut layer = Layer::default();
    let below = section_header(
        &mut LayerPainter::new(&mut layer, &mut text, &icons, ctx),
        (40.0, 100.0, 600.0),
        "palette",
        "Interface",
    );
    assert!(below > 100.0 + ctx.px(SECTION_HEADER));
    assert!(layer.glyphs.len() > "INTERFACE".len(), "the icon and the title");
    let line = layer.quads.last().unwrap().bounds;
    assert_eq!(line.max_x(), 640.0, "to the end of its width");
    assert!(line.min_x() > 40.0 + ctx.px(14.0) + 9.0 * ctx.px(5.0), "after the title");
    assert_eq!(line.size.height, ctx.line());
    assert!(line.min_y() > 100.0 && line.max_y() < 100.0 + ctx.px(SECTION_HEADER));
    // Narrow: no line; an unknown icon: only the title.
    let mut narrow = Layer::default();
    section_header(
        &mut LayerPainter::new(&mut narrow, &mut text, &icons, ctx),
        (0.0, 0.0, 30.0),
        "nope",
        "Interface",
    );
    assert!(narrow.quads.is_empty());
}

#[test]
fn a_faded_divider_is_strongest_in_the_middle_and_covers_its_span_once() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut scene = render_api::Scene::default();
    let layer = scene.push_layer(None);
    divider_vertical_faded(layer, &ctx, 10.0, 0.0, 240.0);
    let quads = &scene.layers()[0].quads;
    // Whole pixels, end to end, no overlap.
    assert_eq!(quads.first().map(|q| q.bounds.min_y()), Some(0.0));
    assert_eq!(quads.last().map(|q| q.bounds.max_y()), Some(240.0));
    assert!(quads.windows(2).all(|w| w[0].bounds.max_y() == w[1].bounds.min_y()));
    let alphas: Vec<f32> = quads.iter().map(|q| q.color.a).collect();
    let middle = alphas[alphas.len() / 2];
    assert!(alphas[0] < middle * 0.1 && alphas[alphas.len() - 1] < middle * 0.1, "{alphas:?}");
    assert!(middle > palette.border.a * 0.9);
}

/// A caption's line: whole pixels end to end, strongest at its start, fading out to the
/// right.
#[test]
fn the_faded_right_divider_fades_out_to_the_right() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut layer = Layer::default();
    divider_faded_right(&mut layer, &ctx, 20.0, 220.0, 7.4, palette.accent);
    let quads = &layer.quads;
    assert_eq!(quads.first().map(|q| q.bounds.min_x()), Some(20.0));
    assert_eq!(quads.last().map(|q| q.bounds.max_x()), Some(220.0));
    assert!(quads.windows(2).all(|w| w[0].bounds.max_x() == w[1].bounds.min_x()));
    assert!(quads.iter().all(|q| q.bounds.min_y() == 7.0 && q.bounds.size.height == 1.0));
    let alphas: Vec<f32> = quads.iter().map(|q| q.color.a).collect();
    assert!(alphas.windows(2).all(|w| w[0] > w[1]), "{alphas:?}");
    assert!(alphas[0] > palette.accent.a * 0.9 && alphas[alphas.len() - 1] < 0.01);
    // Nothing to draw: nothing drawn.
    let mut empty = Layer::default();
    divider_faded_right(&mut empty, &ctx, 50.0, 50.0, 0.0, palette.border);
    assert!(empty.quads.is_empty());
}

/// The backdrop: the palette's color over the whole area, fading with its animation; the
/// drawer's leaves a hole clear (the window's own buttons).
#[test]
fn the_backdrop_follows_the_palettes_style() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0).with_opacity(0.5);
    let area = Rect::new(0.0, 0.0, 400.0, 300.0);
    let mut layer = Layer::default();
    backdrop(&mut layer, &ctx, area);
    assert_eq!(layer.quads.len(), 1);
    assert_eq!(layer.quads[0].bounds, area);
    assert!((layer.quads[0].color.a - 0.08).abs() < 1e-6);
    let mut layer = Layer::default();
    let hole = Rect::new(300.0, 0.0, 100.0, 40.0);
    backdrop_faded(&mut layer, &ctx, area, BackdropRole::Drawer, 1.0, Some(hole));
    assert!(layer.quads.len() >= 2);
    let covered: f32 = layer.quads.iter().map(|q| q.bounds.size.width * q.bounds.size.height).sum();
    assert_eq!(covered, 400.0 * 300.0 - 100.0 * 40.0);
    let strong = palette.backdrop.opacity(BackdropRole::Drawer);
    assert!(layer.quads.iter().all(|q| q.color == Rgba::BLACK.with_alpha(strong)));
}

/// A view header's button: compact (lower than a dialog's), bordered, its icon then its
/// label; the main one tinted in the accent (not filled), the others neutral.
#[test]
fn a_header_s_button_is_compact_with_its_icon_and_the_accent_tinted() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let icons = icons::Icons::builtin();
    let mut text = TextSystem::new();
    let w = toolbar_button_width(&mut text, &ctx, "Upload all");
    assert!(w > text.shape_line("Upload all", &toolbar_button_font(&ctx)).width + 14.0);
    let rect = Rect::new(10.0, 10.0, w, TOOLBAR_BUTTON_H);
    assert!(rect.size.height < BUTTON_H, "lower than a dialog's");
    let paint = |accent: bool| {
        let mut layer = Layer::default();
        let spec = ToolbarButton { label: "Upload all", icon: "upload", accent, hovered: false };
        toolbar_button(&mut layer, &mut TextSystem::new(), &icons, &ctx, rect, spec);
        layer
    };
    let main = paint(true);
    assert_eq!(main.quads[0].bounds, rect);
    assert_eq!(main.quads[0].color, palette.accent.with_alpha(0.16), "tinted, not filled");
    assert!(main.images.len() + main.glyphs.len() > "Upload all".len() - 1, "icon and label");
    let other = paint(false);
    assert_eq!(other.quads[0].color.a, 0.0);
    assert_eq!(other.quads[0].border_color, palette.card_border);
}

/// A long file name keeps its extension and its stem's end; one that fits stays whole.
#[test]
fn a_file_name_keeps_its_extension() {
    let mut text = TextSystem::new();
    let palette = crate::palette::Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let font = ctx.ui_font(0.0);
    let long = "asdfasdf asdf asdfasd fasdf asdfasdfasdfasdf fsad.php";
    let width = text.shape_line(long, &font).width;
    assert_eq!(ellipsize_file(&mut text, "index.php", &font, width), "index.php");
    let cut = ellipsize_file(&mut text, long, &font, width * 0.5);
    assert!(cut.ends_with("fsad.php") && cut.contains('…'), "{cut}");
    assert!(cut.starts_with("asdf"), "{cut}");
    assert!(text.shape_line(&cut, &font).width <= width * 0.5);
    // No extension, a dotfile: cut in the middle.
    let plain = ellipsize_file(&mut text, &"x".repeat(80), &font, 60.0);
    assert!(plain.contains('…'));
    assert!(ellipsize_file(&mut text, ".env", &font, width) == ".env");
}

/// A long path keeps its file's name and cuts its folders first.
#[test]
fn a_path_keeps_its_file_s_name() {
    let mut text = TextSystem::new();
    let palette = crate::palette::Palette::dark();
    let font = Ctx::new(&palette, 1.0).ui_font(0.0);
    let path = "app/Http/Controllers/Admin/Reports/MonthlyReportController.php";
    let name = text.shape_line("…/MonthlyReportController.php", &font).width;
    let cut = ellipsize_path(&mut text, path, &font, name + 40.0);
    assert!(cut.starts_with("app") && cut.ends_with("…/MonthlyReportController.php"), "{cut}");
    // Not even the name fits: the name alone, its extension kept.
    let tight = ellipsize_path(&mut text, path, &font, name * 0.6);
    assert!(tight.ends_with(".php") && !tight.contains('/'), "{tight}");
}

#[test]
fn a_pane_goes_in_its_corner_at_its_margin() {
    let area = Rect::new(10.0, 20.0, 400.0, 300.0);
    let at = |corner| in_corner(area, (100.0, 40.0), corner, 8.0);
    assert_eq!(at(Corner::TopLeft), Point::new(18.0, 28.0));
    assert_eq!(at(Corner::TopRight), Point::new(302.0, 28.0));
    assert_eq!(at(Corner::BottomLeft), Point::new(18.0, 272.0));
    assert_eq!(at(Corner::BottomRight), Point::new(302.0, 272.0));
}

/// Every cut fits its width, keeps the end it's for and never splits a character (text with
/// diacritics and an emoji), whatever the width.
#[test]
fn the_cuts_fit_and_keep_their_ends_at_any_width() {
    let mut text = TextSystem::new();
    let palette = crate::palette::Palette::dark();
    let font = Ctx::new(&palette, 1.0).ui_font(0.0);
    let value = "Ștefan și Ăla 🚀 au mâncat împreună salată de vinete";
    let file = "Raport_lunar_șantier_🚀_final_v2.php";
    let full = text.shape_line(value, &font).width;
    for step in 1..=20 {
        let max = full * step as f32 / 20.0 + 10.0;
        for cut in [
            ellipsize(&mut text, value, &font, max),
            ellipsize_start(&mut text, value, &font, max),
            ellipsize_middle(&mut text, value, &font, max),
            ellipsize_file(&mut text, file, &font, max),
            ellipsize_path(&mut text, &format!("app/Șantier/{file}"), &font, max),
        ] {
            let width = text.shape_line(&cut, &font).width;
            let whole = cut == value || cut.ends_with(file);
            assert!(width <= max || cut.chars().count() <= 6 || whole, "{cut:?} {width} > {max}");
        }
        let end = ellipsize(&mut text, value, &font, max);
        assert!(
            end == value || (end.ends_with('…') && value.starts_with(end.trim_end_matches('…')))
        );
        let start = ellipsize_start(&mut text, value, &font, max);
        assert!(
            start == value || (start.starts_with('…') && value.ends_with(&start['…'.len_utf8()..]))
        );
    }
}

/// Between a strip's groups: whole pixels end to end, strongest at its middle, fading out
/// toward both ends.
#[test]
fn the_horizontal_faded_divider_fades_to_both_ends() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut layer = Layer::default();
    divider_horizontal_faded(&mut layer, &ctx, 10.0, 34.0, 50.4);
    let quads = &layer.quads;
    assert_eq!(quads.first().map(|q| q.bounds.min_x()), Some(10.0));
    assert_eq!(quads.last().map(|q| q.bounds.max_x()), Some(34.0));
    assert!(quads.windows(2).all(|w| w[0].bounds.max_x() == w[1].bounds.min_x()));
    assert!(quads.iter().all(|q| q.bounds.min_y() == 50.0 && q.bounds.size.height == 1.0));
    let alphas: Vec<f32> = quads.iter().map(|q| q.color.a).collect();
    let middle = alphas[alphas.len() / 2];
    assert!(alphas[0] < middle * 0.1 && alphas[alphas.len() - 1] < middle * 0.1, "{alphas:?}");
    let mut empty = Layer::default();
    divider_horizontal_faded(&mut empty, &ctx, 20.0, 20.0, 0.0);
    assert!(empty.quads.is_empty());
}
