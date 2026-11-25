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

/// Wrapped as it was broken word by word: runs of spaces and new lines one space, a word
/// longer than a line alone on its line and cut, nothing for an empty text; one line asked:
/// all of it on it, cut.
#[test]
fn a_wrap_breaks_between_words_whatever_the_spaces() {
    let mut text = TextSystem::new();
    let font = Font::new(Palette::dark().ui_font, 13.0);
    let max = text.shape_line("alpha beta", &font).width;
    let max = max.max(text.shape_line("gamma delta", &font).width);
    let lines = wrap(&mut text, "  alpha \n beta\tgamma  delta ", &font, max, 9);
    assert_eq!(lines, ["alpha beta", "gamma delta"]);
    let long = "a".repeat(80);
    let lines = wrap(&mut text, &format!("{long} beta"), &font, max, 9);
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].ends_with('…') && lines[1] == "beta", "{lines:?}");
    assert!(wrap(&mut text, " \n ", &font, max, 3).is_empty());
    let one = wrap(&mut text, "alpha beta gamma", &font, max, 1);
    assert_eq!(one.len(), 1);
    assert!(one[0].starts_with("alpha") && one[0].ends_with('…'), "{one:?}");
}

/// The reference [`wrap`] is held to: every line shaped anew as each word is added, every
/// line then cut to fit (as it was first written, before its words were shaped once).
fn wrap_word_by_word(
    text: &mut TextSystem,
    value: &str,
    font: &Font,
    max: f32,
    lines: usize,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut line = String::new();
    let mut words = value.split_whitespace().peekable();
    while let Some(word) = words.next() {
        let next = if line.is_empty() { word.to_owned() } else { format!("{line} {word}") };
        if line.is_empty() || text.shape_line(&next, font).width <= max {
            line = next;
            continue;
        }
        if out.len() + 1 == lines {
            let rest: Vec<&str> = std::iter::once(word).chain(words).collect();
            line = format!("{line} {}", rest.join(" "));
            break;
        }
        out.push(std::mem::replace(&mut line, word.to_owned()));
    }
    if !line.is_empty() {
        out.push(line);
    }
    out.truncate(lines.max(1));
    out.into_iter().map(|l| ellipsize(text, &l, font, max)).collect()
}

/// Broken as word by word, over texts of short and long words, narrow and wide lines, few
/// and many of them: the same lines, the same cuts.
#[test]
fn a_wrap_breaks_as_word_by_word_does() {
    let mut text = TextSystem::new();
    let font = Font::new(Palette::dark().ui_font, 13.0);
    let texts = [
        "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu",
        "com.apple.NSEventThread · notify-rs fsevents loop · index-watch · git-status",
        "Previews (images, documents) are made only while they're shown, then let go.",
        "a b c d e f g h i j k l m n o p q r s t u v w x y z",
        "Supercalifragilisticexpialidocious is a word longer than most lines here",
        "  spaces   and\ttabs\n and new lines   between   words  ",
        "WWWW iiii WWWW iiii WWWW iiii mmmm llll",
        "Înțelege diacriticele: ăâîșț și ĂÂÎȘȚ, la fel de bine ca restul textului",
        "one",
    ];
    let mut compared = 0;
    for value in texts {
        for max in [20.0, 45.0, 60.0, 80.0, 111.0, 150.0, 200.0, 333.0, 600.0] {
            for lines in [1, 2, 3, 5, 40] {
                let wanted = wrap_word_by_word(&mut text, value, &font, max, lines);
                let got = wrap(&mut text, value, &font, max, lines);
                assert_eq!(got, wanted, "{value:?} in {max} on {lines} lines");
                compared += 1;
            }
        }
    }
    assert_eq!(compared, 9 * 9 * 5);
}

/// A section's description over several lines: broken between words in its width, the last
/// cut with "…" when there's more, each on a line of its own below the title; the y it
/// returns below the last.
#[test]
fn a_section_s_description_goes_over_its_lines() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let mut scene = render_api::Scene::default();
    let layer = scene.push_layer(None);
    let width = 220.0;
    let detail = "What the editor shows while it works on a project, and how much of it, \
                  told once in a description long enough to need more than three lines.";
    let lines = section_detail_lines(&mut text, &ctx, detail, width, 3);
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(lines[2].ends_with('…'), "the rest cut: {lines:?}");
    assert!(!lines[0].ends_with('…') && !lines[1].ends_with('…'), "{lines:?}");
    let section = SectionTitle { title: "Display", detail: None, lines: &lines };
    let bottom = section_title_lines(layer, &mut text, &ctx, (10.0, 0.0, width), section);
    assert_eq!(baselines(layer).len(), 4, "the title and three lines");
    assert!(layer.glyphs.iter().all(|g| g.origin.x <= 10.0 + width), "drawn past its width");
    // Taller than one line by two lines and their leading; one line as section_title_in.
    layer.glyphs.clear();
    let one = section_title_in(layer, &mut text, &ctx, (10.0, 0.0, width), "Display", Some(detail));
    assert_eq!(baselines(layer).len(), 2);
    let small = text.metrics(&ctx.ui_font(-1.0));
    let line = small.ascent + small.descent + ctx.px(2.0);
    assert!((bottom - one - 2.0 * line).abs() < 0.5, "{bottom} {one}");
    layer.glyphs.clear();
    let none = section_title_in(layer, &mut text, &ctx, (10.0, 0.0, width), "Display", None);
    assert!(none < one);
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

/// A section title made from what was typed (a search's "Results for …") stays in its
/// width, cut with "…"; a short one is drawn whole, as without a width.
#[test]
fn a_long_section_title_stays_in_its_width() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let mut scene = render_api::Scene::default();
    let layer = scene.push_layer(None);
    let long =
        "Results for http://10.0.13.174:8090/download/a-very-long-pasted-address.apk".repeat(4);
    let width = 300.0;
    section_title_in(layer, &mut text, &ctx, (10.0, 0.0, width), &long, Some(&long));
    assert!(!layer.glyphs.is_empty());
    assert!(layer.glyphs.iter().all(|g| g.origin.x <= 10.0 + width), "drawn past its width");
    layer.glyphs.clear();
    let bottom = section_title_in(layer, &mut text, &ctx, (10.0, 0.0, width), "Keymap", None);
    let whole = layer.glyphs.len();
    layer.glyphs.clear();
    assert_eq!(section_title(layer, &mut text, &ctx, 10.0, 0.0, "Keymap", None), bottom);
    assert_eq!((whole, layer.glyphs.len()), (6, 6));
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

#[test]
fn every_change_has_its_badge_sign_and_the_theme_s_color() {
    use crate::changed_files::ChangeState;
    let p = Palette::dark();
    let icons = icons::Icons::builtin();
    for state in [
        ChangeState::Modified,
        ChangeState::Added,
        ChangeState::Deleted,
        ChangeState::Renamed,
        ChangeState::Untracked,
        ChangeState::Conflicted,
        ChangeState::Failed,
    ] {
        let (_, icon) = change_badge_look(state, &p);
        assert!(icons.id(icon).is_some(), "{icon} is in the pack");
    }
    assert_eq!(change_badge_look(ChangeState::Untracked, &p).0, p.vcs_badges.untracked);
    assert_eq!(change_badge_look(ChangeState::Modified, &p).0, p.vcs_badges.modified);
}

#[test]
fn a_badge_s_sign_reads_on_its_color() {
    // Dark on a light amber, white on a dark purple.
    assert_ne!(sign_on(kernel::Rgba::rgb(0xedbd4f)), kernel::Rgba::WHITE);
    assert_eq!(sign_on(kernel::Rgba::rgb(0x5d4b8c)), kernel::Rgba::WHITE);
    assert_eq!(sign_on(Palette::dark().vcs_badges.deleted), kernel::Rgba::WHITE);
}

/// The one button: a ghost icon toggle is lit only when on or under the mouse; a soft one
/// always has its back; the dialogs' kinds draw as they always did; a label's width counts
/// its icon.
#[test]
fn the_one_button_draws_every_kind() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let icons = icons::Icons::builtin();
    let r = square_button(&ctx, (0.0, 20.0), button_size::LARGE);
    let draw = |spec: ButtonSpec<'_>, text: &mut TextSystem| {
        let mut layer = Layer::default();
        button(&mut layer, text, &icons, &ctx, r, spec);
        layer.quads.len()
    };
    let ghost = ButtonSpec::new(ButtonKind::Ghost).icon("search");
    assert_eq!(draw(ghost, &mut text), 0, "at rest: no back");
    assert_eq!(draw(ghost.hovered(true), &mut text), 1, "lit under the mouse");
    assert_eq!(draw(ghost.on(true), &mut text), 1, "on: its back in the accent");
    assert_eq!(draw(ghost.hovered(true).enabled(false), &mut text), 0, "disabled: never lit");
    assert_eq!(draw(ButtonSpec::new(ButtonKind::Soft).label("Add"), &mut text), 1);
    assert_eq!(draw(ButtonSpec::new(ButtonKind::Primary).label("Run"), &mut text), 1);
    let plain =
        button_width(&mut text, &ctx, &ButtonSpec::new(ButtonKind::Ghost).label("Add"), 26.0);
    let with_icon = button_width(
        &mut text,
        &ctx,
        &ButtonSpec::new(ButtonKind::Ghost).label("Add").icon("plus"),
        26.0,
    );
    assert!(with_icon > plain, "the icon's room counted");
    assert_eq!(button_width(&mut text, &ctx, &ghost, 28.0), 28.0, "an icon alone: square");
}

/// A quiet icon (`rest_tone`) is muted at rest, in the text color under the mouse and in the
/// accent while on.
#[test]
fn a_quiet_button_is_muted_only_at_rest() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let icons = icons::Icons::builtin();
    let r = square_button(&ctx, (0.0, 20.0), button_size::MEDIUM);
    let color = |spec: ButtonSpec<'_>, text: &mut TextSystem| {
        let mut layer = Layer::default();
        button(&mut layer, text, &icons, &ctx, r, spec);
        layer.glyphs.first().map(|g| g.color)
    };
    let quiet = ButtonSpec::new(ButtonKind::Ghost).icon("search").rest_tone(Tone::Neutral);
    assert_eq!(color(quiet, &mut text), Some(palette.text_muted));
    assert_eq!(color(quiet.hovered(true), &mut text), Some(palette.text));
    assert_eq!(color(quiet.on(true), &mut text), Some(palette.accent));
}

/// A footer's plain button is the shared secondary one: lit under the mouse, never while
/// disabled.
#[test]
fn a_footer_plain_button_is_the_secondary_one() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let fill = |enabled: bool, text: &mut TextSystem| {
        let mut layer = Layer::default();
        let b = FooterButton { label: "Cancel", enabled, hovered: true, ..Default::default() };
        footer_buttons(&mut layer, text, &ctx, 400.0, 20.0, &[b]);
        layer.quads.first().map(|q| q.color.a)
    };
    assert!(fill(true, &mut text).is_some_and(|a| a > 0.0), "lit");
    assert_eq!(fill(false, &mut text), Some(0.0), "disabled: not lit");
}

/// A menu button has its chevron after its content, counted in its width, in every kind.
#[test]
fn a_menu_button_has_its_chevron() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let icons = icons::Icons::builtin();
    for kind in [ButtonKind::Ghost, ButtonKind::Soft, ButtonKind::Secondary, ButtonKind::Toolbar] {
        let plain = ButtonSpec::new(kind).label("Export").icon("download");
        let menu = plain.menu(true);
        let h = button_size::MEDIUM;
        let (w, w_menu) =
            (button_width(&mut text, &ctx, &plain, h), button_width(&mut text, &ctx, &menu, h));
        assert!(w_menu > w, "{kind:?}: the chevron's room counted");
        let icon_count = |spec: ButtonSpec<'_>, text: &mut TextSystem| {
            let mut layer = Layer::default();
            let r = Rect::new(0.0, 0.0, w_menu, h);
            button(&mut layer, text, &icons, &ctx, r, spec);
            let glyphs = layer.glyphs.iter().filter(|g| icons::is_icon(g.key)).count();
            let right = layer.glyphs.iter().map(|g| g.origin.x).fold(0.0_f32, f32::max);
            (glyphs, right)
        };
        let (n, _) = icon_count(plain, &mut text);
        let (n_menu, right) = icon_count(menu.on(true), &mut text);
        assert_eq!(n_menu, n + 1, "{kind:?}: one more icon, the chevron");
        assert!(right < w_menu, "{kind:?}: inside the button");
    }
}

/// A segmented control filling a width: equal segments across it; a disabled one isn't
/// handed out (nor lit); from the left it starts where it's told.
#[test]
fn a_styled_segmented_fills_starts_left_and_disables() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let icons = icons::Icons::builtin();
    let labels = ["One", "A much longer label than fits", "Three"];
    let spec =
        SegmentedSpec { right: 0.0, mid: 20.0, labels: &labels, selected: 0, hovered: Some(2) };
    let look = SegmentedLook {
        height: 28.0,
        place: SegmentPlace::Fill { x: 10.0, width: 306.0 },
        disabled: &[2],
        icons: &["", "", "x"],
    };
    let mut layer = Layer::default();
    let mut out = Vec::new();
    segmented_styled(&mut layer, &mut text, &icons, &ctx, (spec, look), |i, r| out.push((i, r)));
    assert_eq!(out.iter().map(|(i, _)| *i).collect::<Vec<_>>(), vec![0, 1], "the disabled one out");
    assert_eq!(out[0].1.size.width, out[1].1.size.width, "equal");
    assert_eq!(out[0].1.size.height, 28.0 - 6.0, "its height");
    assert!(layer.glyphs.iter().any(|g| icons::is_icon(g.key)), "the icon drawn");
    assert_eq!(layer.quads.len(), 2, "the track and the selected one: the disabled one not lit");
    let look = SegmentedLook { place: SegmentPlace::Left(50.0), ..Default::default() };
    let mut first = None;
    segmented_styled(&mut Layer::default(), &mut text, &icons, &ctx, (spec, look), |i, r| {
        if i == 0 {
            first = Some(r);
        }
    });
    assert_eq!(first.map(|r| r.min_x()), Some(53.0), "from the left, inside the track");
}

/// A styled dropdown shows its dot and its muted prefix before the value, and is as wide as
/// they need.
#[test]
fn a_styled_dropdown_has_its_prefix_and_dot() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let icons = icons::Icons::builtin();
    let spec = DropdownSpec {
        value: "Auto",
        open: false,
        hovered: false,
        enabled: true,
        note: None,
        placeholder: None,
    };
    let look = DropdownLook {
        prefix: Some("Tx:"),
        dot: Some(palette.accent),
        compact: true,
        ..DropdownLook::default()
    };
    let plain = dropdown_width(&mut text, &ctx, (spec, DropdownLook::default()));
    let wide = dropdown_width(&mut text, &ctx, (spec, look));
    assert!(wide > plain, "{wide} > {plain}");
    let mut layer = Layer::default();
    dropdown_styled(
        &mut layer,
        &mut text,
        &icons,
        &ctx,
        Rect::new(0.0, 0.0, wide, 24.0),
        (spec, look),
    );
    assert!(layer.quads.iter().any(|q| q.color == palette.accent), "the dot");
    assert!(layer.glyphs.iter().any(|g| g.color == palette.text_muted && !icons::is_icon(g.key)));
    assert!(layer.glyphs.iter().all(|g| g.origin.x < wide), "inside its width");
}

/// Draws `spec` in `r` on a fresh layer.
fn drawn(spec: ButtonSpec<'_>, r: Rect, ctx: &Ctx<'_>, text: &mut TextSystem) -> Layer {
    let icons = icons::Icons::builtin();
    let mut layer = Layer::default();
    button(&mut layer, text, &icons, ctx, r, spec);
    layer
}

/// A key cap on any kind: counted in its width, drawn after its label (its tile and key).
#[test]
fn a_button_s_key_is_counted_and_drawn() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    for kind in [ButtonKind::Primary, ButtonKind::Secondary, ButtonKind::Ghost, ButtonKind::Soft] {
        let plain = ButtonSpec::new(kind).label("Delete");
        let keyed = plain.key("↵");
        let h = 32.0;
        let (w, wk) =
            (button_width(&mut text, &ctx, &plain, h), button_width(&mut text, &ctx, &keyed, h));
        assert!(wk > w, "{kind:?}: the key's room counted");
        let r = Rect::new(0.0, 0.0, wk, h);
        let (a, b) = (drawn(plain, r, &ctx, &mut text), drawn(keyed, r, &ctx, &mut text));
        assert_eq!(b.quads.len(), a.quads.len() + 1, "{kind:?}: the key's tile");
        assert!(b.glyphs.len() > a.glyphs.len(), "{kind:?}: the key's glyph");
        assert!(b.glyphs.iter().all(|g| g.origin.x < wk), "{kind:?}: inside");
    }
}

/// A hover tone fills the back under the mouse: solid with white content (a close ×), or
/// faint keeping its content's color; nothing at rest.
#[test]
fn a_hover_tone_fills_the_back_under_the_mouse() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let r = square_button(&ctx, (0.0, 20.0), button_size::LARGE);
    let close = ButtonSpec::new(ButtonKind::Ghost).icon("x").hover_tone(Tone::Danger);
    assert!(drawn(close, r, &ctx, &mut text).quads.is_empty(), "at rest: no back");
    let lit = drawn(close.hovered(true), r, &ctx, &mut text);
    assert_eq!(lit.quads.first().map(|q| q.color), Some(palette.danger), "solid");
    assert_eq!(lit.glyphs.first().map(|g| g.color), Some(sign_on(palette.danger)));
    let link = ButtonSpec::new(ButtonKind::Ghost)
        .label("Delete")
        .tone(Tone::Danger)
        .hover_tint(Tone::Danger)
        .hovered(true);
    let faint = drawn(link, r, &ctx, &mut text);
    assert!(faint.quads.first().is_some_and(|q| q.color.a < 0.2 && q.color.r > 0.5));
    assert!(faint.glyphs.iter().all(|g| g.color == palette.danger), "its color kept");
}

/// `on` still lights under the mouse: a stronger fill than at rest.
#[test]
fn an_on_button_lights_under_the_mouse() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let r = square_button(&ctx, (0.0, 20.0), button_size::SMALL);
    let pin = ButtonSpec::new(ButtonKind::Ghost).icon("pin").on(true);
    let alpha = |spec, text: &mut TextSystem| drawn(spec, r, &ctx, text).quads[0].color.a;
    assert!(alpha(pin.hovered(true), &mut text) > alpha(pin, &mut text));
}

/// A fractional hover lights the back as far as it goes; the content turns at half.
#[test]
fn a_fractional_hover_lights_by_its_amount() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let r = square_button(&ctx, (0.0, 20.0), button_size::LARGE);
    let menu = ButtonSpec::new(ButtonKind::Ghost).icon("menu").rest_tone(Tone::Neutral);
    let alpha = |spec, text: &mut TextSystem| {
        drawn(spec, r, &ctx, text).quads.first().map_or(0.0, |q| q.color.a)
    };
    let (none, half, full) = (
        alpha(menu.hover_amount(0.0), &mut text),
        alpha(menu.hover_amount(0.25), &mut text),
        alpha(menu.hover_amount(1.0), &mut text),
    );
    assert!(none == 0.0 && half > 0.0 && full > half, "{none} {half} {full}");
    assert_eq!(full, alpha(menu.hovered(true), &mut text), "1: as hovered");
    let color = |spec, text: &mut TextSystem| drawn(spec, r, &ctx, text).glyphs[0].color;
    assert_eq!(color(menu.hover_amount(0.25), &mut text), palette.text_muted);
    assert_eq!(color(menu.hover_amount(0.75), &mut text), palette.text);
}

/// An icon's own color tints only the icon; its label keeps the kind's.
#[test]
fn an_icon_keeps_its_own_color() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let tint = kernel::Rgba::rgb(0x33aa55);
    let spec = ButtonSpec::new(ButtonKind::Ghost).icon("folder").label("src").icon_color(tint);
    let w = button_width(&mut text, &ctx, &spec, 22.0);
    let layer = drawn(spec, Rect::new(0.0, 0.0, w, 22.0), &ctx, &mut text);
    let icon = layer.glyphs.iter().find(|g| icons::is_icon(g.key)).map(|g| g.color);
    assert_eq!(icon, Some(tint));
    assert!(
        layer.glyphs.iter().filter(|g| !icons::is_icon(g.key)).all(|g| g.color == palette.text)
    );
}

/// A tinted button has a faint back in its tone at rest, stronger under the mouse, its
/// content in the tone.
#[test]
fn a_tinted_button_is_filled_in_its_tone() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let r = Rect::new(0.0, 0.0, 80.0, 26.0);
    let spec = ButtonSpec::new(ButtonKind::Ghost).label("Open").tone(Tone::Warning).tinted();
    let rest = drawn(spec, r, &ctx, &mut text).quads[0].color;
    let lit = drawn(spec.hovered(true), r, &ctx, &mut text).quads[0].color;
    assert_eq!(rest.with_alpha(1.0), palette.warning.with_alpha(1.0));
    assert!(lit.a > rest.a);
}

/// A theme's own colors replace the kind's: its icon at rest, its hover, its solid on.
#[test]
fn a_button_takes_a_theme_s_colors() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let r = square_button(&ctx, (0.0, 20.0), button_size::SPACIOUS);
    let colors = ButtonColors {
        icon: kernel::Rgba::rgb(0x112233),
        hover: kernel::Rgba::rgb(0x445566).with_alpha(0.5),
        on_back: Some(kernel::Rgba::rgb(0x778899)),
        on_icon: kernel::Rgba::WHITE,
    };
    let spec = ButtonSpec::new(ButtonKind::Ghost).icon("search").colors(colors).icon_size(18.0);
    let rest = drawn(spec, r, &ctx, &mut text);
    assert!(rest.quads.is_empty());
    assert_eq!(rest.glyphs[0].color, colors.icon);
    assert_eq!(drawn(spec.hovered(true), r, &ctx, &mut text).quads[0].color, colors.hover);
    let on = drawn(spec.on(true), r, &ctx, &mut text);
    assert_eq!(
        (on.quads[0].color, on.glyphs[0].color),
        (kernel::Rgba::rgb(0x778899), colors.on_icon)
    );
}

/// On a fixed surface its content reads on it, whatever the theme.
#[test]
fn a_button_on_a_dark_surface_is_light() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let r = square_button(&ctx, (0.0, 20.0), button_size::LARGE);
    let dark = kernel::Rgba::rgb(0x202020);
    let spec = ButtonSpec::new(ButtonKind::Ghost).icon("search").on_surface(dark);
    assert_eq!(drawn(spec, r, &ctx, &mut text).glyphs[0].color, kernel::Rgba::WHITE);
    let lit = drawn(spec.hovered(true), r, &ctx, &mut text);
    assert_eq!(lit.quads[0].color.with_alpha(1.0), kernel::Rgba::WHITE);
}

/// A raised button floats: a shadow and a surface under its content, more than a soft one.
#[test]
fn a_raised_button_floats() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let r = square_button(&ctx, (0.0, 20.0), button_size::LARGE);
    let raised = drawn(ButtonSpec::new(ButtonKind::Raised).icon("copy"), r, &ctx, &mut text);
    assert!(raised.quads.len() > 1, "shadow and surface");
    assert!(raised.quads.iter().any(|q| q.color == palette.popup_background));
}

/// A round primary is a filled circle with its icon in white.
#[test]
fn a_round_primary_is_a_filled_circle() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let r = square_button(&ctx, (0.0, 20.0), button_size::ROOMY);
    let layer =
        drawn(ButtonSpec::new(ButtonKind::Primary).icon("arrow-up").round(), r, &ctx, &mut text);
    assert_eq!(layer.quads.len(), 1);
    assert_eq!(layer.quads[0].color, palette.accent);
    assert_eq!(layer.glyphs.first().map(|g| g.color), Some(kernel::Rgba::WHITE));
}

/// A chip has its body and a × of its own, side by side inside it.
#[test]
fn a_chip_has_its_clear_part() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let icons = icons::Icons::builtin();
    let spec = chip_spec("Author: me", true, false, true);
    let w = button_width(&mut text, &ctx, &spec, 24.0);
    let plain = button_width(&mut text, &ctx, &spec.menu(false), 24.0);
    assert!(w > plain);
    let r = Rect::new(10.0, 0.0, w, 24.0);
    let mut layer = Layer::default();
    let parts = chip(&mut layer, &mut text, &icons, &ctx, r, spec);
    assert!(parts.body.max_x() <= parts.clear.min_x(), "{parts:?}");
    assert!(parts.clear.max_x() <= r.max_x() && parts.body.min_x() == r.min_x());
    let glyphs = layer.glyphs.iter().filter(|g| icons::is_icon(g.key)).count();
    assert_eq!(glyphs, 2, "the chevron and the ×");
    assert!(layer.glyphs.iter().all(|g| g.origin.x < r.max_x()));
}

/// The named sizes are the ones views use.
#[test]
fn the_button_sizes_are_named() {
    assert_eq!(
        [
            button_size::SMALL,
            button_size::COMPACT,
            button_size::MEDIUM,
            button_size::LARGE,
            button_size::ROOMY,
            button_size::SPACIOUS
        ],
        [22.0, 24.0, 26.0, 28.0, 30.0, 32.0]
    );
}

/// A check row: the chosen one tinted with its ✓, the one under the mouse lit, the rest bare.
#[test]
fn a_check_row_shows_its_choice() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let icons = icons::Icons::builtin();
    let r = Rect::new(0.0, 0.0, 160.0, 30.0);
    let row = |checked, hovered, text: &mut TextSystem| {
        let mut layer = Layer::default();
        check_row(&mut layer, text, &icons, &ctx, r, CheckRowSpec::new("Today", checked, hovered));
        layer
    };
    let on = row(true, false, &mut text);
    assert_eq!(on.quads.len(), 1);
    assert!(on.glyphs.iter().any(|g| icons::is_icon(g.key) && g.color == palette.accent));
    assert_eq!(row(false, true, &mut text).quads.len(), 1);
    let bare = row(false, false, &mut text);
    assert!(bare.quads.is_empty() && !bare.glyphs.iter().any(|g| icons::is_icon(g.key)));
}

/// A fractional busy: its spinner grows in its icon's place, the mouse's light fades out.
#[test]
fn a_fractional_busy_grows_its_spinner() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let r = square_button(&ctx, (0.0, 20.0), button_size::LARGE);
    let upload = ButtonSpec::new(ButtonKind::Ghost).icon("upload").hovered(true);
    let back = |spec, text: &mut TextSystem| {
        drawn(spec, r, &ctx, text).quads.first().map_or(0.0, |q| q.color.a)
    };
    assert!(back(upload.busy_amount(0.5), &mut text) < back(upload, &mut text));
    let at = |amount: f32, text: &mut TextSystem| drawn(upload.busy_amount(amount), r, &ctx, text);
    assert!(at(0.0, &mut text).glyphs.iter().any(|g| icons::is_icon(g.key)), "its icon");
    let full = at(1.0, &mut text);
    assert!(!full.glyphs.iter().any(|g| icons::is_icon(g.key)), "the spinner instead");
    let primary = ButtonSpec::new(ButtonKind::Primary).label("Run");
    let w = button_width(&mut text, &ctx, &primary, 34.0);
    let r = Rect::new(0.0, 0.0, w + 18.0, 34.0);
    let (idle, busy) =
        (drawn(primary, r, &ctx, &mut text), drawn(primary.busy_amount(1.0), r, &ctx, &mut text));
    assert!(busy.quads.len() > idle.quads.len(), "the primary's spinner");
}

/// A lit tone: its content in the tone only under the mouse, its back as its kind's.
#[test]
fn a_hover_content_tone_shows_only_under_the_mouse() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let r = square_button(&ctx, (0.0, 20.0), button_size::SMALL);
    let delete = ButtonSpec::new(ButtonKind::Ghost)
        .icon("x")
        .rest_tone(Tone::Neutral)
        .hover_content_tone(Tone::Danger);
    assert_eq!(drawn(delete, r, &ctx, &mut text).glyphs[0].color, palette.text_muted);
    let lit = drawn(delete.hovered(true), r, &ctx, &mut text);
    assert_eq!(lit.glyphs[0].color, palette.danger);
    assert_eq!(lit.quads[0].color, palette.text.with_alpha(0.08), "the ghost's back");
}

/// A choice may have a dot before its label, counted in its width.
#[test]
fn a_choice_may_have_its_dot() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let spec = ChoiceSpec { x: 0.0, mid: 20.0, label: "Slow", selected: false, hovered: false };
    let dot = palette.warning;
    let mut layer = Layer::default();
    let r = choice_with_dot(&mut layer, &mut text, &ctx, spec, dot);
    assert_eq!(r.size.width, choice_width_with_dot(&mut text, &ctx, "Slow"));
    assert!(r.size.width > choice_width(&mut text, &ctx, "Slow"));
    assert!(layer.quads.iter().any(|q| q.color == dot));
    let mut plain = Layer::default();
    choice(&mut plain, &mut text, &ctx, spec);
    assert_eq!(plain.quads.len(), 1, "no dot by default");
}

/// A dropdown with a leading icon: drawn, its room counted.
#[test]
fn a_dropdown_may_lead_with_an_icon() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let icons = icons::Icons::builtin();
    let spec = DropdownSpec {
        value: "local",
        open: false,
        hovered: false,
        enabled: true,
        note: None,
        placeholder: None,
    };
    let look = DropdownLook::default();
    let plain = dropdown_width(&mut text, &ctx, (spec, look));
    let wide = dropdown_width_with_icon(&mut text, &ctx, (spec, look));
    assert!(wide > plain);
    let count = |icon: Option<&str>, text: &mut TextSystem| {
        let mut layer = Layer::default();
        let r = Rect::new(0.0, 0.0, wide, 30.0);
        match icon {
            Some(icon) => dropdown_with_icon(&mut layer, text, &icons, &ctx, r, (spec, look), icon),
            None => dropdown_styled(&mut layer, text, &icons, &ctx, r, (spec, look)),
        }
        layer.glyphs.iter().filter(|g| icons::is_icon(g.key)).count()
    };
    assert_eq!(count(Some("database"), &mut text), count(None, &mut text) + 1);
}

/// A segmented control as low as a row's (22): its segments inside the track.
#[test]
fn a_low_segmented_keeps_its_segments_inside() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let icons = icons::Icons::builtin();
    let labels = ["Left", "Right"];
    let spec =
        SegmentedSpec { right: 200.0, mid: 11.0, labels: &labels, selected: 0, hovered: None };
    for height in [22.0, 24.0] {
        let look = SegmentedLook { height, ..Default::default() };
        let mut layer = Layer::default();
        let mut segs = Vec::new();
        segmented_styled(&mut layer, &mut text, &icons, &ctx, (spec, look), |_, r| segs.push(r));
        let track = layer.quads[0].bounds;
        assert_eq!(track.size.height, height);
        assert!(segs.iter().all(|s| s.min_y() >= track.min_y() && s.max_y() <= track.max_y()));
        assert!(layer.glyphs.iter().all(|g| g.origin.y <= track.max_y()), "{height}: text inside");
    }
}

/// A compact dropdown with an icon, as wide as [`dropdown_width_with_icon`] says, shows its
/// whole value at every scale (never cut to "…" by the rounding of its own width).
#[test]
fn a_dropdown_at_its_width_shows_its_whole_value() {
    let palette = Palette::dark();
    let mut text = TextSystem::new();
    let icons = icons::Icons::builtin();
    let spec = DropdownSpec {
        value: "shop",
        open: false,
        hovered: false,
        enabled: true,
        note: None,
        placeholder: None,
    };
    let look = DropdownLook { compact: true, ..DropdownLook::default() };
    for scale in [1.0, 1.25, 1.5, 2.0, 3.0] {
        let ctx = Ctx::new(&palette, scale);
        let w = dropdown_width_with_icon(&mut text, &ctx, (spec, look));
        let mut layer = Layer::default();
        let r = Rect::new(0.0, 0.0, w, 26.0 * scale);
        dropdown_with_icon(&mut layer, &mut text, &icons, &ctx, r, (spec, look), "database");
        let want: Vec<_> =
            text.shape_line("shop", &ctx.ui_font(-1.0)).glyphs.iter().map(|g| g.key).collect();
        let drawn: Vec<_> =
            layer.glyphs.iter().filter(|g| !icons::is_icon(g.key)).map(|g| g.key).collect();
        assert_eq!(drawn, want, "at scale {scale}");
    }
}

/// A tone given as a color wins over the named tone: a palette of its own (the severities')
/// keeps its colors on the shared button.
#[test]
fn a_tone_color_wins_over_the_named_tone() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let icons = icons::Icons::builtin();
    let r = square_button(&ctx, (0.0, 20.0), button_size::LARGE);
    let own = Rgba::rgb(0x1ACC4D);
    let mut layer = Layer::default();
    let spec =
        ButtonSpec::new(ButtonKind::Soft).label("2").tinted().tone(Tone::Danger).tone_color(own);
    button(&mut layer, &mut text, &icons, &ctx, r, spec);
    let back = layer.quads.first().map(|q| q.color);
    assert!(
        back.is_some_and(|c| (c.g - own.g).abs() < 0.01 && (c.r - own.r).abs() < 0.01),
        "{back:?}"
    );
}

/// A button tells a screen reader what it is as it's drawn: named by its label (an icon
/// alone: never by the icon's id), where it is, greyed out while it can't be used.
#[test]
fn a_button_tells_what_it_is_as_it_is_drawn() {
    use ui_kit::access::Role;
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let icons = icons::Icons::builtin();
    let r = square_button(&ctx, (0.0, 20.0), button_size::LARGE);
    let mut layer = Layer::default();
    let recording = ui_kit::access::begin();
    button(
        &mut layer,
        &mut text,
        &icons,
        &ctx,
        r,
        ButtonSpec::new(ButtonKind::Primary).label("Save"),
    );
    let off = ButtonSpec::new(ButtonKind::Ghost).icon("close").enabled(false);
    button(&mut layer, &mut text, &icons, &ctx, r, off);
    let frame = recording.take();
    let nodes = frame.nodes();
    assert_eq!(nodes.len(), 2);
    assert_eq!((nodes[0].role, &*nodes[0].name, nodes[0].bounds), (Role::Button, "Save", r));
    assert!(nodes[0].actions.press && !nodes[0].state.disabled);
    assert_eq!(&*nodes[1].name, "", "not the icon's id");
    assert!(nodes[1].state.disabled && !nodes[1].actions.press);
}

/// A button's key is said with it, not read after it as a text of its own.
#[test]
fn a_button_s_key_is_its_shortcut() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let icons = icons::Icons::builtin();
    let r = Rect::new(0.0, 0.0, 160.0, 32.0);
    let recording = ui_kit::access::begin();
    let spec = ButtonSpec::new(ButtonKind::Secondary).label("Cancel").key("esc");
    button(&mut Layer::default(), &mut text, &icons, &ctx, r, spec);
    let frame = recording.take();
    let names: Vec<_> = frame.nodes().iter().map(|n| n.name.to_string()).collect();
    assert_eq!(names, ["Cancel"]);
    assert_eq!(frame.nodes()[0].shortcut.as_deref(), Some("esc"));
}

/// A switch, a checkbox and a radio tell their state as they're drawn: on or off, a
/// checkbox's group partly chosen, a switch off limits greyed out.
#[test]
fn the_state_controls_tell_their_state() {
    use ui_kit::access::Role;
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let mut layer = Layer::default();
    let recording = ui_kit::access::begin();
    switch_enabled(&mut layer, &ctx, (0.0, 10.0), true, false, false);
    Checkbox::new(Check::Mixed).paint(&mut layer, &mut text, &ctx, 0.0, 40.0);
    radio(&mut layer, &ctx, 0.0, 70.0, Radio::new(true));
    let frame = recording.take();
    let n = frame.nodes();
    assert_eq!(
        n.iter().map(|n| n.role).collect::<Vec<_>>(),
        [Role::Switch, Role::CheckBox, Role::Radio]
    );
    assert_eq!(n[0].state.checked, Some(true));
    assert!(n[0].state.disabled);
    assert!(n[1].state.mixed && n[1].state.checked == Some(false));
    assert_eq!(n[2].state.checked, Some(true));
}

/// The choices tell what they offer: each segment a radio named by its label (the one in
/// force checked), a dropdown its value and whether its list is open, a check row its
/// label and whether it's the one chosen.
#[test]
fn the_choices_tell_what_they_offer() {
    use ui_kit::access::Role;
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let icons = icons::Icons::builtin();
    let mut layer = Layer::default();
    let recording = ui_kit::access::begin();
    let labels = ["Slow", "Normal", "Fast"];
    let spec =
        SegmentedSpec { right: 300.0, mid: 20.0, labels: &labels, selected: 1, hovered: None };
    segmented(&mut layer, &mut text, &ctx, spec);
    let field = DropdownSpec {
        value: "Dark",
        open: true,
        hovered: false,
        enabled: true,
        note: None,
        placeholder: None,
    };
    dropdown(&mut layer, &mut text, &icons, &ctx, Rect::new(0.0, 50.0, 200.0, 30.0), field);
    let row = CheckRowSpec::new("Last 7 days", true, false);
    check_row(&mut layer, &mut text, &icons, &ctx, Rect::new(0.0, 100.0, 200.0, 30.0), row);
    let frame = recording.take();
    let n = frame.nodes();
    let radios: Vec<_> = n.iter().filter(|n| n.role == Role::Radio).collect();
    assert_eq!(radios.iter().map(|r| &*r.name).collect::<Vec<_>>(), labels);
    assert_eq!(
        radios.iter().map(|r| r.state.checked).collect::<Vec<_>>(),
        [Some(false), Some(true), Some(false)]
    );
    let combo = n.iter().find(|n| n.role == Role::ComboBox).expect("the dropdown");
    assert_eq!((combo.value.as_deref(), combo.state.expanded), (Some("Dark"), Some(true)));
    let item = n.iter().find(|n| n.role == Role::ListItem).expect("the check row");
    assert_eq!((&*item.name, item.state.checked), ("Last 7 days", Some(true)));
    assert!(!n.iter().any(|n| n.role == Role::Label && labels.contains(&&*n.name)));
}

fn quads_of(draw: impl FnOnce(&mut Layer, &mut TextSystem, &Ctx<'_>)) -> Vec<render_api::Quad> {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 2.0);
    let mut text = TextSystem::new();
    let mut layer = Layer::default();
    draw(&mut layer, &mut text, &ctx);
    layer.quads
}

/// The badge's defaults are [`badge`]'s; its options change its height, room, corners,
/// fill and where its label sits.
#[test]
fn a_badge_is_configured_by_its_builder() {
    let palette = Palette::dark();
    let old = quads_of(|l, t, c| {
        let spec = BadgeSpec {
            label: "semantic",
            x: 100.0,
            mid: 50.0,
            align_right: true,
            tone: Tone::Accent,
        };
        badge(l, t, c, spec);
    });
    let new = quads_of(|l, t, c| {
        Badge::new("semantic").tone(Tone::Accent).paint_right(l, t, c, 100.0, 50.0);
    });
    assert_eq!(old, new, "the same badge");
    assert_eq!(new[0].bounds.size.height, 40.0, "20 at scale 2");
    assert_eq!(new[0].color, palette.accent.with_alpha(0.12));
    let count = quads_of(|l, t, c| {
        Badge::new("3")
            .color(Rgba::WHITE)
            .fill_alpha(0.14)
            .size(BadgeSize::Medium)
            .pad(5.0)
            .min_width(18.0)
            .centered()
            .radius(4.0)
            .paint(l, t, c, 10.0, 50.0);
    });
    assert_eq!(count[0].bounds, Rect::new(10.0, 32.0, 36.0, 36.0), "a round count");
    assert_eq!(count[0].color, Rgba::WHITE.with_alpha(0.14));
    assert_eq!(count[0].corner_radius, 8.0);
    assert_eq!(
        [BadgeSize::Tiny, BadgeSize::Small, BadgeSize::Medium].map(BadgeSize::height),
        [15.0, 16.0, 18.0]
    );
    let zoomed = quads_of(|l, t, c| {
        Badge::new("x").zoom(1.5).size(BadgeSize::Small).paint(l, t, c, 0.0, 50.0);
    });
    assert_eq!(zoomed[0].bounds.size.height, 48.0, "16 × 1.5 at scale 2");
    let mut text = TextSystem::new();
    let ctx = Ctx::new(&palette, 2.0);
    let b = Badge::new("semantic");
    assert_eq!(b.width(&mut text, &ctx), new[0].bounds.size.width);
}

/// A badge has no outline by default; asked, one a line wide in its color.
#[test]
fn a_badge_may_be_outlined() {
    let palette = Palette::dark();
    let plain = quads_of(|l, t, c| {
        Badge::new("x").paint(l, t, c, 0.0, 50.0);
    });
    assert_eq!(plain[0].border_width, 0.0);
    let outlined = quads_of(|l, t, c| {
        Badge::new("x").border(palette.card_border).paint(l, t, c, 0.0, 50.0);
    });
    assert_eq!(outlined[0].border_width, 2.0, "a line at scale 2");
    assert_eq!(outlined[0].border_color, palette.card_border);
    assert_eq!(outlined[0].bounds, plain[0].bounds);
}

/// A badge's label on the caller's baseline (a pill among a row's words).
#[test]
fn a_badge_label_sits_on_a_given_baseline() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let mut layer = Layer::default();
    Badge::new("tag")
        .baseline(BadgeBaseline::At(77.0))
        .bold()
        .paint(&mut layer, &mut text, &ctx, 0.0, 70.0);
    assert!(layer.glyphs.iter().all(|g| (g.origin.y - 77.0).abs() < 20.0));
}

/// A key cap's width is the one it's drawn at; it may be anchored on its left; a small one.
#[test]
fn a_keycap_is_measured_and_placed_from_either_side() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 2.0);
    let mut text = TextSystem::new();
    let mut layer = Layer::default();
    let right = keycap(&mut layer, &mut text, &ctx, "⌘K", 300.0, 50.0, false);
    let w = keycap_width(&mut text, &ctx, "⌘K");
    assert_eq!(right.size.width, w.round());
    let left = Keycap::new("⌘K").paint(&mut layer, &mut text, &ctx, 40.0, 50.0);
    assert_eq!((left.min_x(), left.size.width), (40.0, right.size.width));
    let small =
        Keycap::new("K").size(KeycapSize::Small).paint(&mut layer, &mut text, &ctx, 0.0, 50.0);
    assert_eq!((small.size.height, small.size.width), (40.0, 44.0));
    let lit = Keycap::new("esc").hovered(true).look(&ctx);
    assert_eq!(lit.background, palette.text.with_alpha(0.06));
    assert_eq!(lit.relief, None);
}

/// A loader turned by the caller's clock: its lit dot by the time given.
#[test]
fn a_loader_turns_by_the_callers_clock() {
    let at = |ms: u64| Loader::default().elapsed(std::time::Duration::from_millis(ms)).step();
    assert_eq!((at(0), at(90), at(3 * 90 + 10), at(8 * 90)), (0, 1, 3, 0));
    let quads = quads_of(|l, _, c| loader_spinner(l, c, (0.0, 20.0), &Loader::default()));
    assert_eq!(quads.len(), 8, "eight dots");
}

/// A check row's options: untinted (the hover over the chosen one), its hover's fill, the
/// row under the keys lit, its corners.
#[test]
fn a_check_row_takes_its_lists_look() {
    let palette = Palette::dark();
    let icons = icons::Icons::builtin();
    let row = |spec: CheckRowSpec<'_>| {
        quads_of(|l, t, c| check_row(l, t, &icons, c, Rect::new(0.0, 0.0, 200.0, 60.0), spec))
    };
    assert!(row(CheckRowSpec::new("All", true, false).untinted()).is_empty(), "no tint");
    let lit =
        row(CheckRowSpec::new("All", true, true).untinted().hover_fill(Rgba::BLACK).radius(6.0));
    assert_eq!((lit[0].color, lit[0].corner_radius), (Rgba::BLACK, 12.0));
    let keys = row(CheckRowSpec::new("All", false, false).active(true));
    assert_eq!(keys[0].color, palette.text.with_alpha(0.06));
    assert_eq!(CheckRowSpec::new("All", false, false).icon(13.0).icon, 13.0);
}

/// A check row's label: the chosen one's in the accent by default, in the text's color when
/// plain; ellipsized before the room kept at its right.
#[test]
fn a_check_row_label_can_be_plain_and_cut() {
    let palette = Palette::dark();
    let icons = icons::Icons::builtin();
    let glyphs = |spec: CheckRowSpec<'_>| {
        let ctx = Ctx::new(&palette, 1.0);
        let mut text = TextSystem::new();
        let mut layer = Layer::default();
        check_row(&mut layer, &mut text, &icons, &ctx, Rect::new(0.0, 0.0, 120.0, 30.0), spec);
        layer.glyphs
    };
    let label = "A rather long label that never fits its row";
    // The ✓ stays in the accent either way; the label's letters follow it or not.
    let in_text = |glyphs: &[render_api::GlyphInstance]| {
        glyphs.iter().filter(|g| g.color == palette.text).count()
    };
    let tinted = glyphs(CheckRowSpec::new(label, true, false));
    assert_eq!(in_text(&tinted), 0);
    let plain = glyphs(CheckRowSpec::new(label, true, false).plain_label());
    assert!(in_text(&plain) > 0);
    assert_eq!(plain.len(), tinted.len());
    let cut = glyphs(CheckRowSpec::new(label, true, false).trailing(20.0));
    assert!(cut.len() < tinted.len(), "ellipsized");
    assert_eq!(CheckRowSpec::new(label, false, false).trailing, None, "whole by default");
}

/// A row's highlight: the defaults are [`row_highlight`]'s; an inactive selection grey; the
/// hover by a token; its corners and alpha.
#[test]
fn a_row_highlight_is_configured_by_its_spec() {
    let p = Palette::dark();
    let r = Rect::new(0.0, 0.0, 100.0, 20.0);
    let old = quads_of(|l, _, c| row_highlight(l, c, r, true, false));
    let new = quads_of(|l, _, c| RowHighlight::new(true, false).paint(l, c, r));
    assert_eq!(old, new);
    assert_eq!(new[0].color, p.accent.with_alpha(ROW_SELECTED));
    assert_eq!(
        RowHighlight::new(true, true).inactive(true).fill(&p),
        Some(p.text.with_alpha(ROW_INACTIVE))
    );
    assert_eq!(RowHighlight::new(false, true).hover(HoverFill::Tree).fill(&p), Some(p.tree_hover));
    assert_eq!(
        RowHighlight::new(false, true).hover(HoverFill::Background).fill(&p),
        Some(p.hover_background)
    );
    assert_eq!(
        RowHighlight::new(true, false).selected_alpha(0.2).fill(&p),
        Some(p.accent.with_alpha(0.2))
    );
    assert_eq!(RowHighlight::new(false, false).fill(&p), None);
    let round =
        quads_of(|l, _, c| RowHighlight::new(false, true).radius(radius::SM).paint(l, c, r));
    assert_eq!(round[0].corner_radius, 12.0);
    assert_eq!(round[0].border_width, 0.0, "no outline by default");
}

/// A row's highlight in a fill of its own (a tree's selection), outlined (a drop target).
#[test]
fn a_row_highlight_may_take_its_own_fill_and_an_outline() {
    let p = Palette::dark();
    let r = Rect::new(0.0, 0.0, 100.0, 20.0);
    let own = RowHighlight::new(true, false).selected_fill(p.tree_selection);
    assert_eq!(own.fill(&p), Some(p.tree_selection));
    assert_eq!(own.inactive(true).fill(&p), Some(p.text.with_alpha(ROW_INACTIVE)));
    let tinted = RowHighlight::new(false, true).hover(HoverFill::Color(p.warning));
    assert_eq!(tinted.fill(&p), Some(p.warning));
    let target = quads_of(|l, _, c| {
        RowHighlight::new(true, false).outline(p.accent).paint(l, c, r);
    });
    assert_eq!((target[0].border_width, target[0].border_color), (2.0, p.accent));
    let none = quads_of(|l, _, c| RowHighlight::new(false, false).outline(p.accent).paint(l, c, r));
    assert!(none.is_empty(), "nothing lit, nothing drawn");
}

/// A switch off limits: dimmed, deaf to the mouse; enabled, the usual one.
#[test]
fn a_switch_off_limits_is_dimmed() {
    let p = Palette::dark();
    let usual = quads_of(|l, _, c| {
        switch(l, c, 0.0, 20.0, true, false);
    });
    let enabled = quads_of(|l, _, c| {
        switch_enabled(l, c, (0.0, 20.0), true, false, true);
    });
    assert_eq!(usual, enabled);
    let off = quads_of(|l, _, c| {
        switch_enabled(l, c, (0.0, 20.0), false, true, false);
    });
    assert_eq!(off[0].color, p.text.with_alpha(0.16 * DISABLED_ALPHA), "not lit by the hover");
    assert_eq!(off[1].color, Rgba::WHITE.with_alpha(DISABLED_ALPHA));
}

/// A checkbox: the defaults are [`checkbox`]'s; another side; brighter under the mouse.
#[test]
fn a_checkbox_has_sizes_and_a_hover() {
    let p = Palette::dark();
    let old = quads_of(|l, t, c| {
        checkbox(l, t, c, 0.0, 20.0, Check::Mixed, true);
    });
    let new = quads_of(|l, t, c| {
        Checkbox::new(Check::Mixed).paint(l, t, c, 0.0, 20.0);
    });
    assert_eq!(old, new);
    let big = quads_of(|l, t, c| {
        Checkbox::new(Check::On).size(20.0).paint(l, t, c, 0.0, 20.0);
    });
    assert_eq!(big[0].bounds.size.width, 40.0);
    let hovered = quads_of(|l, t, c| {
        Checkbox::new(Check::Off).hovered(true).paint(l, t, c, 0.0, 20.0);
    });
    assert_eq!(hovered[0].border_color, p.text_muted.with_alpha(0.95));
}

/// A bare radio: chosen, a disc and its dot; else a ring, brighter under the mouse.
#[test]
fn a_radio_is_a_disc_or_a_ring() {
    let p = Palette::dark();
    let on = quads_of(|l, _, c| {
        radio(l, c, 0.0, 20.0, Radio::new(true));
    });
    assert_eq!(on.len(), 2);
    assert_eq!((on[0].bounds.size.width, on[0].color), (36.0, p.accent));
    let ring = quads_of(|l, _, c| {
        radio(l, c, 0.0, 20.0, Radio::new(false).hovered(true).size(14.0));
    });
    assert_eq!(ring.len(), 1);
    assert_eq!(ring[0].bounds.size.width, 28.0);
    assert_eq!(ring[0].border_color, p.text_muted.with_alpha(0.6));
}

/// A divider in the cards' border or a color of the caller's.
#[test]
fn a_divider_takes_a_token_or_a_color() {
    let p = Palette::dark();
    let card = quads_of(|l, _, c| divider_card(l, c, 0.0, 100.0, 10.0));
    assert_eq!((card[0].color, card[0].bounds), (p.card_border, Rect::new(0.0, 10.0, 100.0, 2.0)));
    let own = quads_of(|l, _, c| divider_in(l, c, (0.0, 50.0, 4.0), Rgba::WHITE));
    assert_eq!(own[0].color, Rgba::WHITE);
    let tall = quads_of(|l, _, c| divider_vertical_in(l, c, (5.0, 0.0, 30.0), Rgba::BLACK));
    assert_eq!(tall[0].bounds, Rect::new(5.0, 0.0, 2.0, 30.0));
}

/// A dot is round, centered on its point, on whole pixels and scaled with the zoom.
#[test]
fn a_dot_is_round_and_centered() {
    let quads = quads_of(|l, _, c| {
        dot(l, c, Point::new(20.0, 10.0), 7.0, Rgba::WHITE);
    });
    let q = &quads[0];
    assert_eq!(q.bounds, Rect::new(13.0, 3.0, 14.0, 14.0));
    assert_eq!(q.corner_radius, 7.0);
    assert_eq!(q.color, Rgba::WHITE);
}

/// A badge with an icon and a muted run before its label: wider by them, the icon drawn
/// before the text, the muted run in the muted color.
#[test]
fn a_badge_may_have_an_icon_and_a_muted_run() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let icons = icons::Icons::builtin();
    let mut text = TextSystem::new();
    let plain = Badge::new("1 204").width(&mut text, &ctx);
    let iconed = Badge::new("1 204").icon(&icons, "lock").width(&mut text, &ctx);
    assert_eq!(iconed, plain + 16.0, "a 12 icon and its gap");
    let both = Badge::new("1 204").icon(&icons, "lock").lead("rows");
    let wide = both.width(&mut text, &ctx);
    assert!(wide > iconed + 4.0);
    let mut layer = Layer::default();
    let r = both.icon_color(palette.warning).paint(&mut layer, &mut text, &ctx, 0.0, 20.0);
    assert_eq!(r.size.width, wide);
    let first = layer.glyphs.first().map(|g| g.color);
    assert_eq!(first, Some(palette.warning), "the icon first, in its own color");
    assert!(layer.glyphs.iter().any(|g| g.color == palette.text_muted), "the muted run");
}

/// A row's outline is a line wide by default; asked, wider (never under a line).
#[test]
fn a_row_s_outline_may_be_wider() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 2.0);
    let r = Rect::new(0.0, 0.0, 100.0, 28.0);
    let width = |h: RowHighlight| {
        let mut layer = Layer::default();
        h.paint(&mut layer, &ctx, r);
        layer.quads[0].border_width
    };
    let lit = RowHighlight::new(true, false).outline(palette.accent);
    assert_eq!(width(lit), 2.0);
    assert_eq!(width(lit.outline_width(1.5)), 3.0);
    assert_eq!(width(lit.outline_width(0.1)), 2.0);
}

/// A value color (a request's method) colors the value and its chevron; by default both
/// keep the text's and the muted colors.
#[test]
fn a_dropdown_s_value_color_colors_its_value_and_chevron() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let icons = icons::Icons::builtin();
    let spec = DropdownSpec {
        value: "DELETE",
        open: false,
        hovered: false,
        enabled: true,
        note: None,
        placeholder: None,
    };
    let r = Rect::new(0.0, 0.0, 120.0, 30.0);
    let colors = |look: DropdownLook<'_>, text: &mut TextSystem| {
        let mut layer = Layer::default();
        dropdown_styled(&mut layer, text, &icons, &ctx, r, (spec, look));
        let value: Vec<_> =
            layer.glyphs.iter().filter(|g| !icons::is_icon(g.key)).map(|g| g.color).collect();
        let chevron: Vec<_> =
            layer.glyphs.iter().filter(|g| icons::is_icon(g.key)).map(|g| g.color).collect();
        (value, chevron)
    };
    let own = palette.danger;
    let (value, chevron) =
        colors(DropdownLook { value_color: Some(own), ..DropdownLook::default() }, &mut text);
    assert!(!value.is_empty() && value.iter().all(|c| *c == own), "{value:?}");
    assert!(!chevron.is_empty() && chevron.iter().all(|c| *c == own), "{chevron:?}");
    let (value, chevron) = colors(DropdownLook::default(), &mut text);
    assert!(value.iter().all(|c| *c == palette.text));
    assert!(chevron.iter().all(|c| *c == palette.text_muted));
}

/// A busy primary button with a key and a menu draws as one without them: its label after
/// the spinner, nothing over it.
#[test]
fn a_busy_primary_button_draws_nothing_over_its_spinner() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let icons = icons::Icons::builtin();
    let r = Rect::new(0.0, 0.0, 160.0, button_size::LARGE);
    let draw = |spec: ButtonSpec<'_>, text: &mut TextSystem| {
        let mut layer = Layer::default();
        button(&mut layer, text, &icons, &ctx, r, spec);
        let glyphs: Vec<(f32, f32)> =
            layer.glyphs.iter().map(|g| (g.origin.x, g.origin.y)).collect();
        (glyphs, layer.quads.len())
    };
    let plain = ButtonSpec::new(ButtonKind::Primary).label("Run").busy(true);
    let keyed = plain.key("↵").menu(true);
    assert_eq!(draw(keyed, &mut text), draw(plain, &mut text));
}

/// Every dialog button shows its spinner while busy, as the primary does: the danger one
/// keeps its fill, the secondary its border; neither draws its key over the spinner.
#[test]
fn a_busy_danger_or_secondary_button_shows_its_spinner() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let r = Rect::new(0.0, 0.0, 160.0, BUTTON_H);
    for kind in [ButtonKind::Danger, ButtonKind::Secondary] {
        let idle = ButtonSpec::new(kind).label("Drop").hovered(true);
        let (rest, busy) =
            (drawn(idle, r, &ctx, &mut text), drawn(idle.busy(true), r, &ctx, &mut text));
        assert!(busy.quads.len() > rest.quads.len(), "{kind:?}: its spinner");
        if kind == ButtonKind::Danger {
            assert_eq!(busy.quads[0].color, palette.danger, "filled, not lit while busy");
        } else {
            assert_eq!(busy.quads[0].color.a, 0.0, "transparent, not lit while busy");
        }
        let keyed = drawn(idle.busy(true).key("↵"), r, &ctx, &mut text);
        assert_eq!(keyed.quads.len(), busy.quads.len(), "{kind:?}: no key over the spinner");
        let half = drawn(idle.busy_amount(0.5), r, &ctx, &mut text);
        assert!(half.quads.len() > rest.quads.len(), "{kind:?}: a growing spinner");
    }
}

/// An outlined button: a border in its content's color (faint), with or without a back;
/// without the option a ghost has no border at rest, and a soft one keeps its own.
#[test]
fn an_outlined_button_is_bordered_in_its_tone() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let r = Rect::new(0.0, 0.0, 120.0, 30.0);
    let ghost = ButtonSpec::new(ButtonKind::Ghost).label("Remove").tone(Tone::Danger);
    assert!(drawn(ghost, r, &ctx, &mut text).quads.is_empty(), "a ghost at rest: no back");
    let outlined = drawn(ghost.outlined(), r, &ctx, &mut text);
    let border = outlined.quads.first().map(|q| (q.border_width, q.border_color));
    assert_eq!(border, Some((1.0, palette.danger.with_alpha(0.55))));
    let soft = drawn(ButtonSpec::new(ButtonKind::Soft).label("Add"), r, &ctx, &mut text);
    assert_eq!(soft.quads.first().map(|q| q.border_color), Some(palette.border));
}

/// An avatar: tinted, a circle faintly in its color with the initials in it; solid, a
/// rounded square in its color with white initials.
#[test]
fn an_avatar_shows_the_initials_in_its_look() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let color = Rgba::rgb(0x3366cc);
    let r = Rect::new(10.0, 10.0, 20.0, 20.0);
    let mut tinted = Layer::default();
    Avatar::new("seo-analyzer", color).paint(&mut tinted, &mut text, &ctx, r);
    assert_eq!(tinted.quads[0].color, color.with_alpha(0.16));
    assert_eq!(tinted.quads[0].corner_radius, 10.0);
    assert_eq!(tinted.glyphs.len(), 2, "S and A");
    assert!(tinted.glyphs.iter().all(|g| g.color == color));
    let mut solid = Layer::default();
    Avatar::new("seo-analyzer", color).solid().paint(&mut solid, &mut text, &ctx, r);
    assert_eq!((solid.quads[0].color, solid.quads[0].corner_radius), (color, 5.0));
    assert!(solid.glyphs.iter().all(|g| g.color == Rgba::WHITE));
}

/// A button's avatar takes its icon's place (counted in its width); its label in the font
/// it was given; from its left edge after its room when asked, else centered.
#[test]
fn a_button_takes_an_avatar_a_font_and_a_start() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let avatar = Avatar::new("docker", Rgba::rgb(0x22aa66)).solid();
    let bold = ctx.ui_font(0.0).with_weight(Font::BOLD);
    let plain = ButtonSpec::new(ButtonKind::Ghost).label("docker");
    let spec = plain.avatar(avatar).icon_size(20.0).font(&bold);
    let w = button_width(&mut text, &ctx, &spec, 28.0);
    assert!(w > button_width(&mut text, &ctx, &plain, 28.0) + 20.0, "the avatar and a gap");
    let r = Rect::new(0.0, 0.0, w + 40.0, 28.0);
    let centered = drawn(spec, r, &ctx, &mut text);
    assert_eq!(centered.quads.iter().filter(|q| q.color == avatar.color).count(), 1);
    let first = |layer: &Layer| {
        layer.quads.iter().find(|q| q.color == avatar.color).map(|q| q.bounds.min_x())
    };
    assert_eq!(first(&drawn(spec.start(8.0), r, &ctx, &mut text)), Some(8.0));
    assert!(first(&centered).is_some_and(|x| x > 8.0), "centered: further in");
    let label = |layer: &Layer| layer.glyphs.iter().filter(|g| g.color == palette.text).count();
    assert_eq!(label(&centered), 6, "the label in its font");
}

/// A ringed dot: a circle in each color behind it (wider by its ring), then the dot, all
/// on one center.
#[test]
fn a_ringed_dot_is_cut_out_of_what_it_stands_on() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut layer = Layer::default();
    let behind = [palette.window_background, palette.accent.with_alpha(0.1)];
    let at = Point::new(20.0, 20.0);
    let dot = ringed_dot(&mut layer, &ctx, at, (7.0, 1.5), palette.danger, &behind);
    assert_eq!(layer.quads.len(), 3);
    assert_eq!(layer.quads[2].color, palette.danger);
    assert_eq!(layer.quads[2].bounds, dot);
    assert!(layer.quads[0].bounds.size.width > dot.size.width, "the ring");
    let centers: Vec<_> = layer.quads.iter().map(|q| q.bounds.center()).collect();
    assert!(centers.iter().all(|c| (c.x - 20.0).abs() <= 1.0 && (c.y - 20.0).abs() <= 1.0));
}

/// The dialog's and the bars' buttons tell a screen reader what they are: a button named by
/// its label, greyed out while off, its key said with it; their labels aren't read again.
#[test]
fn the_older_buttons_tell_what_they_are() {
    use ui_kit::access::Role;
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let mut layer = Layer::default();
    let r = Rect::new(0.0, 0.0, 120.0, 32.0);
    let recording = ui_kit::access::begin();
    primary_button(&mut layer, &mut text, &ctx, r, "Save", true, false);
    secondary_button(&mut layer, &mut text, &ctx, r, "Cancel", false, false);
    let spec = KeyButtonSpec {
        right: 300.0,
        mid: 50.0,
        label: "Delete",
        key: "↵",
        enabled: true,
        hovered: false,
        danger: true,
    };
    primary_button_with_key(&mut layer, &mut text, &ctx, spec);
    let frame = recording.take();
    let nodes = frame.nodes();
    let names: Vec<_> = nodes.iter().map(|n| (n.role, n.name.to_string())).collect();
    assert_eq!(
        names,
        [
            (Role::Button, "Save".into()),
            (Role::Button, "Cancel".into()),
            (Role::Button, "Delete".into())
        ]
    );
    assert!(nodes[1].state.disabled);
    assert_eq!(nodes[2].shortcut.as_deref(), Some("↵"));
}

/// Progress, headings, a group's choices and the loader tell a screen reader what they are.
#[test]
fn progress_headings_choices_and_the_loader_are_told() {
    use ui_kit::access::Role;
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let mut layer = Layer::default();
    let recording = ui_kit::access::begin();
    let bar =
        ProgressBar { fraction: Some(0.42), label: Some("Indexing"), ..ProgressBar::default() };
    bar.paint(&mut layer, &mut text, &ctx, Rect::new(0.0, 0.0, 200.0, 30.0));
    section_title_in(
        &mut layer,
        &mut text,
        &ctx,
        (0.0, 40.0, 300.0),
        "Appearance",
        Some("The theme"),
    );
    choice(
        &mut layer,
        &mut text,
        &ctx,
        ChoiceSpec { x: 0.0, mid: 100.0, label: "Dark", selected: true, hovered: false },
    );
    loader(&mut layer, &mut text, &ctx, 0.0, 140.0, &Loader::with_text("Loading"));
    let frame = recording.take();
    let find = |role| frame.nodes().iter().find(|n| n.role == role).expect("a node");
    let p = find(Role::ProgressBar);
    assert_eq!((&*p.name, p.value.as_deref()), ("Indexing", Some("42%")));
    assert_eq!(&*find(Role::Heading).name, "Appearance");
    let c = find(Role::Radio);
    assert_eq!((&*c.name, c.state.checked), ("Dark", Some(true)));
    assert!(frame.nodes().iter().any(|n| n.role == Role::ProgressBar && &*n.name == "Loading"));
    let labels: Vec<_> = frame
        .nodes()
        .iter()
        .filter(|n| n.role == Role::Label)
        .map(|n| n.name.to_string())
        .collect();
    assert!(
        !labels.iter().any(|l| l == "Indexing" || l == "Appearance" || l == "Dark"),
        "{labels:?}"
    );
}

/// A choice in a grid's cell: filled in the accent when in force (its label white), tinted
/// under the mouse or the keys, its label centered and cut to the cell.
#[test]
fn a_choice_cell_fills_its_cell() {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let r = Rect::new(10.0, 10.0, 80.0, 28.0);
    let cell = |selected, active| ChoiceCell { label: "8.3", selected, hovered: false, active };
    let mut on = Layer::default();
    choice_cell(&mut on, &mut text, &ctx, r, cell(true, false));
    assert_eq!(on.quads[0].color, palette.accent);
    assert!(on.glyphs.iter().all(|g| g.color == Rgba::WHITE));
    let mut keyed = Layer::default();
    choice_cell(&mut keyed, &mut text, &ctx, r, cell(false, true));
    assert_eq!(keyed.quads[0].color, palette.accent.with_alpha(0.10));
    // Centered: as much room on its left as on its right.
    let first = keyed.glyphs.first().map_or(0.0, |g| g.origin.x);
    let w = text.shape_line("8.3", &ctx.ui_font(-0.5)).width;
    assert!(((first - r.min_x()) - (r.max_x() - first - w)).abs() <= 1.0);
    // A long label stays inside.
    let mut long = Layer::default();
    let label = "a version far too long for its cell";
    choice_cell(&mut long, &mut text, &ctx, r, ChoiceCell { label, ..cell(false, false) });
    assert!(long.glyphs.iter().all(|g| g.origin.x < r.max_x()));
}

/// A badge's icon size and color hold whichever comes first, the icon or them.
#[test]
fn a_badge_s_icon_options_hold_in_any_order() {
    let icons = icons::Icons::builtin();
    let before = Badge::new("x").icon_size(9.0).icon_color(Rgba::WHITE).icon(&icons, "check");
    let after = Badge::new("x").icon(&icons, "check").icon_size(9.0).icon_color(Rgba::WHITE);
    for b in [before, after] {
        let icon = b.icon.expect("its icon");
        assert_eq!((icon.size, icon.color), (9.0, Some(Rgba::WHITE)));
    }
}

/// The specs' builders: `new` the usual state, each method one field.
#[test]
fn the_specs_build_from_new() {
    let c = ChoiceSpec::new(4.0, 20.0, "Dark").selected(true);
    assert_eq!((c.x, c.mid, c.label, c.selected, c.hovered), (4.0, 20.0, "Dark", true, false));
    let labels = ["A", "B"];
    let s = SegmentedSpec::new(200.0, 16.0, &labels).selected(1).hovered(Some(0));
    assert_eq!((s.right, s.selected, s.hovered), (200.0, 1, Some(0)));
    let st = StepperSpec::new((0.0, 300.0), 20.0, &labels).current(1);
    assert_eq!((st.x, st.width, st.current), (0.0, 300.0, 1));
    let r = RadioCardSpec::new(Rect::new(0.0, 0.0, 10.0, 10.0), "T", "D").hovered(true);
    assert_eq!((r.title, r.detail, r.selected, r.hovered), ("T", "D", false, true));
    let d = DropdownSpec::new("local").note("3").placeholder("none").enabled(false);
    assert_eq!(
        (d.value, d.note, d.placeholder, d.enabled, d.open),
        ("local", Some("3"), Some("none"), false, false)
    );
    let look = DropdownLook::default().prefix("Tx:").compact(true);
    assert_eq!((look.prefix, look.compact, look.dot), (Some("Tx:"), true, None));
}

/// A name's initials: the first letters of its first two words, else its first two, in
/// upper case (a letter that grows doesn't overflow), `?` for none.
#[test]
fn initials_of_a_name() {
    assert_eq!(initials("docker.acme.test"), "DA");
    assert_eq!(initials("www-data"), "WD");
    assert_eq!(initials("root"), "RO");
    assert_eq!(initials("ßeta"), "SSE");
    assert_eq!(initials("--"), "?");
    assert_eq!(Initials::of("élan vital").as_str(), "ÉV");
}

/// To a screen reader: a number field a spin button with its number, a color's field and
/// swatch buttons with their color (what's drawn in them not read again), a wizard's steps
/// a list (the one under way chosen, those done checked), a toggle a toggle button.
#[test]
fn the_rest_of_the_controls_tell_what_they_are() {
    use ui_kit::access::Role;
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let icons = icons::Icons::builtin();
    let mut layer = Layer::default();
    let recording = ui_kit::access::begin();
    number_field(&mut layer, &mut text, &ctx, Rect::new(0.0, 0.0, 120.0, 28.0), "14", false);
    ColorField::new(Rgba::rgb(0x3b82f6), "#3b82f6").paint(
        &mut layer,
        &mut text,
        &icons,
        &ctx,
        Rect::new(0.0, 40.0, 160.0, 28.0),
    );
    let labels = ["Source", "Tables", "Done"];
    stepper(&mut layer, &mut text, &ctx, StepperSpec::new((0.0, 600.0), 100.0, &labels).current(1));
    let toggle = ButtonSpec::new(ButtonKind::Ghost).label("Aa").toggle(true);
    button(&mut layer, &mut text, &icons, &ctx, Rect::new(0.0, 140.0, 40.0, 28.0), toggle);
    let frame = recording.take();
    let n = frame.nodes();
    let spin = n.iter().find(|n| n.role == Role::SpinButton).expect("a spin button");
    assert_eq!(spin.value.as_deref(), Some("14"));
    assert!(
        !n.iter().any(|n| n.role == Role::Label && &*n.name == "14"),
        "its number not read again"
    );
    let color = n.iter().find(|n| n.role == Role::Button && n.value.as_deref() == Some("#3b82f6"));
    assert!(color.is_some(), "the color's field");
    assert_eq!(
        n.iter().filter(|n| n.role == Role::Button && n.value.is_some()).count(),
        1,
        "its swatch not again"
    );
    let steps: Vec<_> = n.iter().filter(|n| n.role == Role::ListItem).collect();
    assert_eq!(steps.len(), 3);
    assert_eq!((steps[0].state.checked, steps[1].state.selected), (Some(true), true));
    let t = n.iter().find(|n| n.role == Role::ToggleButton).expect("a toggle");
    assert_eq!((&*t.name, t.state.checked), ("Aa", Some(true)));
}

/// A loading state: the spinner's eight dots above its title, centered.
#[test]
fn a_loading_state_turns_above_its_title() {
    let area = Rect::new(0.0, 0.0, 400.0, 300.0);
    let quads = quads_of(|l, t, c| loading_state(l, t, c, area, "Opening", Some("why")));
    assert_eq!(quads.len(), 8, "eight dots");
    let middle = quads.iter().map(|q| q.bounds.center().x).sum::<f32>() / 8.0;
    assert!((middle - 200.0).abs() < 1.0, "centered: {middle}");
    assert!(quads.iter().all(|q| q.bounds.max_y() < 150.0), "above the middle");
}

/// A loading state tells a screen reader something is under way, named by its title.
#[test]
fn a_loading_state_is_told_as_progress() {
    use ui_kit::access::Role;
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let mut layer = Layer::default();
    let recording = ui_kit::access::begin();
    let area = Rect::new(0.0, 0.0, 400.0, 300.0);
    loading_state(&mut layer, &mut text, &ctx, area, "Opening", Some("why"));
    let frame = recording.take();
    let progress = frame.nodes().iter().find(|n| n.role == Role::ProgressBar).expect("told");
    assert_eq!(&*progress.name, "Opening");
}

/// Something loading is told once, named by its title: the title painted under it isn't
/// read again, its explanation is (under it); a loader with a text is named by it, over the
/// spinner and the text, the text not read again.
#[test]
fn a_loading_state_is_told_once_by_its_title() {
    use ui_kit::access::Role;
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let mut layer = Layer::default();
    let rect = Rect::new(0.0, 0.0, 400.0, 300.0);
    let recording = ui_kit::access::begin();
    loading_state(&mut layer, &mut text, &ctx, rect, "Starting the engine", Some("Once only"));
    let frame = recording.take();
    let nodes = frame.nodes();
    let said = |name: &str| nodes.iter().filter(|n| &*n.name == name).count();
    let bar = nodes.iter().find(|n| n.role == Role::ProgressBar).expect("told as under way");
    assert_eq!((&*bar.name, bar.bounds), ("Starting the engine", rect));
    assert_eq!(said("Starting the engine"), 1, "its title read once: {nodes:?}");
    let detail = nodes.iter().find(|n| &*n.name == "Once only").expect("its explanation");
    assert_eq!(detail.parent, bar.id, "read under it");

    let recording = ui_kit::access::begin();
    let drawn = loader(&mut layer, &mut text, &ctx, 10.0, 50.0, &Loader::with_text("Loading"));
    let frame = recording.take();
    let nodes = frame.nodes();
    assert_eq!(nodes.len(), 1, "one node, its text not read again: {nodes:?}");
    assert_eq!((nodes[0].role, &*nodes[0].name), (Role::ProgressBar, "Loading"));
    assert!((nodes[0].bounds.size.width - drawn.size.width).abs() < 0.5, "over all of it");
}
