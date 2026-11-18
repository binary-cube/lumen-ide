//! A row of the list: a section's header, a recent search, a file or folder by name, a file with its line.

use super::*;

/// A section's header (`label`) at `top`, moving `top` below it.
pub(super) fn paint_header(
    layer: &mut Layer,
    text: &mut TextSystem,
    f: &Frame<'_>,
    fonts: &Fonts,
    top: &mut f32,
    label: &str,
) {
    let shaped = text.shape_line(label, &fonts.small);
    draw_text(
        layer,
        &shaped,
        f.panel.min_x() + f.ctx.px(22.0),
        centered_baseline(*top, f.header_h, &fonts.small_metrics),
        f.ctx.palette.text_muted,
    );
    *top += f.header_h;
}

/// A recent search's row: the magnifier and the query.
pub(super) fn paint_recent_row(
    layer: &mut Layer,
    text: &mut TextSystem,
    f: &Frame<'_>,
    fonts: &Fonts,
    rect: Rect,
    query: &str,
) {
    let (ctx, palette, icons, height) = (&f.ctx, f.ctx.palette, f.icons, rect.size.height);
    let px = |v: f32| ctx.px(v);
    let size = px(15.0);
    if let Some(id) = icons.id("search") {
        draw_icon(
            layer,
            icons,
            id,
            Rect::new(rect.min_x() + px(14.0), rect.min_y() + (height - size) / 2.0, size, size),
            palette.text_muted,
        );
    }
    let shaped = text.shape_line(query, &fonts.font);
    draw_text(
        layer,
        &shaped,
        rect.min_x() + px(40.0),
        centered_baseline(rect.min_y(), height, &fonts.metrics),
        palette.text,
    );
}

/// A row of a file or a folder matched by its name: its icon, its name (the matched letters
/// accented) and its parent folder. Returns where its name went when it was cut short (its
/// tooltip says all).
pub(super) fn paint_name_row(
    layer: &mut Layer,
    text: &mut TextSystem,
    f: &Frame<'_>,
    fonts: &Fonts,
    rect: Rect,
    folder: &FileMatch,
    (icon, color): (Option<IconId>, Rgba),
) -> Option<Rect> {
    let (ctx, palette, icons, height) = (&f.ctx, f.ctx.palette, f.icons, rect.size.height);
    let px = |v: f32| ctx.px(v);
    let size = px(16.0);
    if let Some(id) = icon {
        let at = (rect.min_y() + (height - size) / 2.0).round();
        let r = Rect::new(rect.min_x() + px(14.0), at, size, size);
        draw_icon(layer, icons, id, r, ctx.fade(color));
    }
    let baseline = centered_baseline(rect.min_y(), height, &fonts.metrics);
    let mut x = rect.min_x() + px(40.0);
    // The name, matched letters accented.
    let name = folder.name();
    let mut start = 0;
    let mut segments: Vec<(&str, bool)> = Vec::new();
    for &at in &folder.name_positions {
        let Some(c) = name.get(at..).and_then(|s| s.chars().next()) else {
            continue;
        };
        if at > start {
            segments.push((&name[start..at], false));
        }
        segments.push((&name[at..at + c.len_utf8()], true));
        start = at + c.len_utf8();
    }
    if start < name.len() {
        segments.push((&name[start..], false));
    }
    // Too long for the row: cut in the middle (unlit), the whole of it in its tooltip.
    let room = (rect.max_x() - px(space::LG) - x).max(0.0);
    let fitted = ds::ellipsize_file(text, name, &fonts.font, room);
    let cut = (fitted != name).then(|| Rect::new(x, rect.min_y(), room, height));
    if cut.is_some() {
        segments = vec![(fitted.as_str(), false)];
    }
    for (segment, matched) in segments {
        let shaped = text.shape_line(segment, &fonts.font);
        let color = if matched { palette.accent } else { palette.text };
        draw_text(layer, &shaped, x.round(), baseline, ctx.fade(color));
        x += shaped.width;
    }
    let dir = folder.dir();
    if !dir.is_empty() {
        let x = x + px(space::SM);
        let room = rect.max_x() - px(space::LG) - x;
        let dir = ds::ellipsize(text, dir, &fonts.small, room);
        let shaped = text.shape_line(&dir, &fonts.small);
        draw_text(
            layer,
            &shaped,
            x.round(),
            centered_baseline(rect.min_y(), height, &fonts.small_metrics),
            palette.text_muted,
        );
    }
    cut
}

/// A file's row: its icon, name, line badge and folder, then the line's code below — all in
/// the row (a long name cut in the middle, its extension kept; its folder in what's left).
/// Returns where its name went when it was cut short (its tooltip says all of it).
pub(super) fn paint_file_row(
    layer: &mut Layer,
    text: &mut TextSystem,
    f: &Frame<'_>,
    fonts: &Fonts,
    rect: Rect,
    row: FileRow<'_>,
    file_icon: &dyn Fn(&str) -> (IconId, Rgba),
) -> Option<Rect> {
    let (ctx, palette, icons, hit) = (&f.ctx, f.ctx.palette, f.icons, row.hit);
    let px = |v: f32| ctx.px(v);
    let name = hit.path.rsplit('/').next().unwrap_or(&hit.path);
    let dir = hit.path.rsplit_once('/').map_or("", |(d, _)| d);
    let (id, color) = file_icon(name);
    let size = px(16.0);
    let first_mid = rect.min_y() + px(19.0);
    draw_icon(
        layer,
        icons,
        id,
        Rect::new(rect.min_x() + px(14.0), (first_mid - size / 2.0).round(), size, size),
        color,
    );
    let mut x = rect.min_x() + px(40.0);
    let right = rect.max_x() - px(space::LG);
    let baseline = centered_baseline(first_mid - px(10.0), px(20.0), &fonts.metrics);
    let (focus, snippet) = focus_line(hit, row.query);
    // The name has the room but the line's badge (and a gap); cut in the middle beyond.
    let line = format!("{}", focus + 1);
    let badge_w = text.shape_line(&line, &ctx.ui_font(-1.0)).width + px(14.0 + space::SM);
    let room = (right - x - badge_w - px(space::SM)).max(0.0);
    let shown = ds::ellipsize_file(text, name, &fonts.font, room);
    let shaped = text.shape_line(&shown, &fonts.font);
    draw_text(layer, &shaped, x, baseline, ctx.fade(palette.text));
    let cut = (shown != name).then(|| Rect::new(x, rect.min_y(), shaped.width, px(38.0)));
    x += shaped.width + px(space::SM);
    let badge = ds::badge(
        layer,
        text,
        ctx,
        ds::BadgeSpec { label: &line, x, mid: first_mid, align_right: false, tone: Tone::Accent },
    );
    x = badge.max_x() + px(space::SM);
    // The folder in what's left (its start, its end kept), nothing when there's no room.
    let dir = if right - x > px(40.0) {
        ds::ellipsize_path(text, dir, &fonts.small, right - x)
    } else {
        String::new()
    };
    let shaped = text.shape_line(&dir, &fonts.small);
    draw_text(
        layer,
        &shaped,
        x,
        centered_baseline(first_mid - px(10.0), px(20.0), &fonts.small_metrics),
        palette.text_muted,
    );
    let max = rect.size.width - px(40.0) - px(space::LG);
    let snippet = ds::ellipsize(text, snippet, &fonts.code, max);
    let shaped = text.shape_line(&snippet, &fonts.code);
    draw_text(
        layer,
        &shaped,
        rect.min_x() + px(40.0),
        centered_baseline(rect.min_y() + px(30.0), px(20.0), &fonts.code_metrics),
        palette.text_muted,
    );
    cut
}
