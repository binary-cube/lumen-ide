use super::*;
use crate::Palette;
use kernel::Point;

fn setup() -> (Palette, TextSystem, Layer, icons::Icons) {
    (Palette::dark(), TextSystem::new(), Layer::default(), icons::Icons::builtin())
}

fn row<'a>(width: f32, detail: Option<&'a str>, modified: bool) -> RowSpec<'a> {
    RowSpec {
        x: 100.0,
        y: 50.0,
        width,
        label: "Density",
        detail,
        control_w: 200.0,
        control_h: 22.0,
        modified,
        reset_hovered: false,
        highlight: 0.0,
    }
}

/// A row: its label on the left, its control on the right (vertically centered), "↺" after
/// the control only while modified — the control in the same place either way.
#[test]
fn a_row_puts_its_control_on_the_right_and_reset_after_it() {
    let (palette, mut text, mut layer, icons) = setup();
    let ctx = Ctx::new(&palette, 1.0);
    let plain = setting_row(&mut layer, &mut text, &ctx, &icons, row(600.0, None, false));
    assert_eq!(plain.rect, Rect::new(100.0, 50.0, 600.0, ROW_H));
    assert!(!plain.stacked);
    assert!(plain.reset.is_none());
    assert_eq!(plain.control.max_x(), 700.0 - ROW_PAD - RESET_W - 8.0);
    let mid = plain.control.min_y() + plain.control.size.height / 2.0;
    assert!((mid - (50.0 + ROW_H / 2.0)).abs() <= 1.0, "centered: {mid}");
    let changed =
        setting_row(&mut layer, &mut text, &ctx, &icons, row(600.0, Some("How many"), true));
    assert_eq!(changed.rect.size.height, ROW_H_DETAIL, "a description makes it taller");
    assert_eq!(changed.control.min_x(), plain.control.min_x(), "the control doesn't move");
    let reset = changed.reset.expect("↺ while modified");
    assert!(reset.min_x() >= changed.control.max_x(), "after the control");
    assert!(reset.max_x() <= 700.0 - ROW_PAD + 1.0);
}

/// Too narrow for the label beside the control: the label goes above it, the control under
/// it from the left edge, and the row grows to hold both.
#[test]
fn a_narrow_row_stacks_its_control_under_its_label() {
    let (palette, mut text, mut layer, icons) = setup();
    let ctx = Ctx::new(&palette, 2.0);
    let spec = row(380.0, Some("How many"), false);
    assert!(stacks(&ctx, spec.width, spec.control_w));
    let area = setting_row(&mut layer, &mut text, &ctx, &icons, spec);
    assert!(area.stacked);
    assert_eq!(area.control.min_x(), 100.0 + ctx.px(ROW_PAD));
    assert!(area.control.min_y() >= 50.0 + ctx.px(ROW_H_DETAIL) - 1.0, "under the text");
    assert!(area.rect.max_y() >= area.control.max_y(), "the row holds it");
    assert_eq!(area.rect.size.height, row_height(&mut text, &ctx, &spec));
}

/// A description too long for one line goes on two (the row grows by a line), never cut
/// short after its first; a short one keeps the row as it was.
#[test]
fn a_long_description_wraps_to_a_second_line() {
    let (palette, mut text, mut layer, icons) = setup();
    let ctx = Ctx::new(&palette, 1.0);
    let short =
        setting_row(&mut layer, &mut text, &ctx, &icons, row(600.0, Some("How many"), false));
    assert_eq!(short.rect.size.height, ROW_H_DETAIL);
    let long = "Behind everything: the top bar, the side strips, the gaps between the panels";
    let spec = row(600.0, Some(long), false);
    let area = setting_row(&mut layer, &mut text, &ctx, &icons, spec);
    assert_eq!(area.rect.size.height, ROW_H_DETAIL + DETAIL_LINE);
    assert_eq!(area.rect.size.height, row_height(&mut text, &ctx, &spec));
    assert!(!area.stacked);
}

/// The new slider: one line (as tall as its thumb, no row of labels), its value in words on
/// the right; a click anywhere on the track picks the nearest stop.
#[test]
fn a_value_slider_is_one_line_with_its_value_on_the_right() {
    let (palette, mut text, mut layer, _) = setup();
    let ctx = Ctx::new(&palette, 1.0);
    let area = value_slider(
        &mut layer,
        &mut text,
        &ctx,
        ValueSliderSpec {
            x: 0.0,
            mid: 30.0,
            width: 300.0,
            stops: 5,
            current: 2,
            value: "Medium",
            hovered: false,
        },
    );
    assert_eq!(area.stops.len(), 5);
    assert_eq!(area.rect.size.height, VALUE_SLIDER_H, "one line");
    assert!(area.rect.max_x() <= 300.0 - VALUE_W, "the value's room is kept");
    assert_eq!(area.nearest(0.0), Some(0));
    assert_eq!(area.nearest(area.rect.max_x()), Some(4));
    // The value is drawn right-aligned at the end of its width.
    let last = layer.glyphs.iter().map(|g| g.origin.x).fold(0.0, f32::max);
    assert!(last > 300.0 - VALUE_W && last < 300.0, "{last}");
    assert!(area.rect.contains(Point::new(10.0, 30.0)));
}

/// Keys step a slider: an arrow one stop, Shift a bigger step; never past its ends.
#[test]
fn keys_step_a_slider_within_its_ends() {
    assert_eq!(stepped(2, 5, 1, false), 3);
    assert_eq!(stepped(2, 5, -1, false), 1);
    assert_eq!(stepped(4, 5, 1, false), 4);
    assert_eq!(stepped(0, 5, -1, false), 0);
    assert_eq!(stepped(0, 21, 1, true), 5);
    assert_eq!(stepped(1, 5, 1, true), 3, "at least two");
    assert_eq!(stepped(19, 21, 1, true), 20);
    assert_eq!(stepped(0, 0, 1, false), 0);
}

/// A foldable card's header: the handle on the left, the switch, "⋯" and the chevron on the
/// right, none overlapping; the title and its summary between.
#[test]
fn a_fold_card_s_header_keeps_its_parts_apart() {
    let (palette, mut text, mut layer, icons) = setup();
    let ctx = Ctx::new(&palette, 2.0);
    let area = fold_card_head(
        &mut layer,
        &mut text,
        &ctx,
        &icons,
        FoldCardSpec {
            x: 10.0,
            y: 20.0,
            width: 900.0,
            icon: "layers",
            tint: palette.accent,
            title: "Matrix",
            summary: "Background · dense · slow",
            expanded: true,
            enabled: true,
            hovered: Some(FoldPart::Menu),
        },
    );
    assert_eq!(area.header, Rect::new(10.0, 20.0, 900.0, ctx.px(FOLD_HEAD)));
    for part in [area.handle, area.toggle, area.menu] {
        assert!(area.header.contains(part.origin), "{part:?}");
    }
    assert!(area.handle.max_x() < area.toggle.min_x());
    assert!(area.toggle.max_x() < area.menu.min_x());
    assert!(area.menu.max_x() < 910.0);
    assert!(layer.glyphs.len() > "MatrixBackground".len(), "the title and the summary");
    // Its background: ringed while dragged.
    let quad = fold_card_quad(&ctx, area.header, false, true);
    assert_eq!(quad.border_width, ctx.px(1.5));
}

/// A section inside a card: as wide as asked, its header the click target.
#[test]
fn a_fold_section_spans_its_width() {
    let (palette, mut text, mut layer, icons) = setup();
    let ctx = Ctx::new(&palette, 1.0);
    let r = fold_section(
        &mut LayerPainter::new(&mut layer, &mut text, &icons, ctx),
        (0.0, 0.0, 400.0),
        "Look",
        false,
        false,
    );
    assert_eq!(r, Rect::new(0.0, 0.0, 400.0, FOLD_SECTION));
    let line = layer.quads.last().unwrap().bounds;
    assert_eq!(line.max_x(), 400.0, "its line reaches the end");
}

/// A gallery's grid: as many columns as fit, sharing the width; cells in rows.
#[test]
fn a_grid_fits_its_columns_to_the_width() {
    let grid = Grid::new(0.0, 0.0, 620.0, 140.0, 120.0, 10.0);
    assert_eq!(grid.columns, 4);
    assert!(grid.cell_w >= 140.0);
    assert_eq!(grid.cell(4).min_y(), 130.0, "the second row");
    assert_eq!(grid.cell(5).min_x(), grid.cell(1).min_x());
    assert!(grid.cell(3).max_x() <= 620.0);
    assert_eq!(grid.height(0), 0.0);
    assert_eq!(grid.height(5), 250.0);
    assert_eq!(Grid::new(0.0, 0.0, 50.0, 140.0, 120.0, 10.0).columns, 1, "at least one");
}

/// A gallery's card: the thumbnail's room at its top, inside it; the title under it.
#[test]
fn a_gallery_card_keeps_room_for_its_thumbnail() {
    let (palette, mut text, mut layer, _) = setup();
    let ctx = Ctx::new(&palette, 1.0);
    let rect = Rect::new(0.0, 0.0, 160.0, 130.0);
    let thumb = gallery_card(
        &mut layer,
        &mut text,
        &ctx,
        GalleryCardSpec {
            rect,
            thumb_h: 72.0,
            title: "Aurora",
            detail: Some("Bands of light"),
            badge: Some(("light", Tone::Neutral)),
            selected: true,
            hovered: false,
        },
    );
    assert!(rect.contains(thumb.origin) && thumb.max_x() <= rect.max_x());
    assert_eq!(thumb.size.height, 72.0);
    let below = layer.glyphs.iter().all(|g| g.origin.y > thumb.max_y());
    assert!(below, "the title and the line under the thumbnail");
    assert!(layer.quads[0].border_width > ctx.line(), "selected: ringed");
}

/// The categories down the side: a row each, in order; the chosen one on a pill.
#[test]
fn the_categories_are_listed_and_tabbed() {
    let (palette, mut text, mut layer, icons) = setup();
    let ctx = Ctx::new(&palette, 1.0);
    let items = [
        NavItem { icon: "gear", label: "General", count: None, dot: true },
        NavItem { icon: "palette", label: "Interface", count: None, dot: false },
        NavItem { icon: "layers", label: "Effects", count: Some(2), dot: false },
    ];
    let mut rows = Vec::new();
    let bottom = nav_list(
        &mut LayerPainter::new(&mut layer, &mut text, &icons, ctx),
        NavPlace::new(0.0, 0.0, 200.0),
        Nav::new(&items, 2, None),
        |i, r| rows.push((i, r)),
    );
    assert_eq!(rows.len(), 3);
    assert!(rows.windows(2).all(|w| w[0].1.max_y() < w[1].1.min_y()));
    assert!(bottom >= rows[2].1.max_y());
    // As tabs: side by side, within the width.
    let mut tabs = Vec::new();
    let mut layer = Layer::default();
    nav_tabs(
        &mut LayerPainter::new(&mut layer, &mut text, &icons, ctx),
        (0.0, 0.0, 600.0),
        Nav::new(&items, 0, None),
        |i, r| tabs.push((i, r)),
    );
    assert_eq!(tabs.len(), 3);
    assert!(tabs.windows(2).all(|w| w[0].1.max_x() <= w[1].1.min_x()));
    // Too narrow for every name: only the chosen tab keeps it (the others: their icon).
    let mut narrow = Vec::new();
    nav_tabs(
        &mut LayerPainter::new(&mut layer, &mut text, &icons, ctx),
        (0.0, 0.0, 150.0),
        Nav::new(&items, 0, None),
        |_, r| narrow.push(r),
    );
    assert!(narrow[1].size.width < tabs[1].1.size.width);
    assert_eq!(narrow[0].size.width, tabs[0].1.size.width);
}

/// A group's caption: its name, then its line on the same row, from right after the name to
/// the edge; the open page's group in the accent (name and line), the others muted.
#[test]
fn a_group_caption_has_its_line_and_the_open_one_the_accent() {
    let (palette, mut text, mut layer, icons) = setup();
    let ctx = Ctx::new(&palette, 1.0);
    let below = nav_caption(
        &mut LayerPainter::new(&mut layer, &mut text, &icons, ctx),
        (10.0, 100.0, 180.0),
        "Editing",
        false,
    );
    assert_eq!(below, 100.0 + NAV_CAPTION);
    let name_end = layer.glyphs.iter().map(|g| g.origin.x).fold(0.0, f32::max);
    let line = &layer.quads;
    assert!(!line.is_empty());
    assert!(line[0].bounds.min_x() > name_end, "after the name");
    assert_eq!(line.last().unwrap().bounds.max_x(), 190.0);
    let mid = line[0].bounds.min_y();
    assert!(mid > 100.0 && mid < below, "on the caption's row");
    assert!(layer.glyphs.iter().all(|g| g.color.r == palette.text_muted.r));
    let mut open = Layer::default();
    nav_caption(
        &mut LayerPainter::new(&mut open, &mut text, &icons, ctx),
        (10.0, 100.0, 180.0),
        "Editing",
        true,
    );
    assert!(open.glyphs.iter().all(|g| g.color.r == palette.accent.r));
    assert_eq!(open.quads[0].color.r, palette.accent.r);
}
