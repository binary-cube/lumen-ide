//! Badges and labelled rows: a tone's badge, a dot on a button, a file's row, a key cap.

use super::*;

/// What [`badge`] draws, and where.
#[derive(Debug, Clone, Copy)]
pub struct BadgeSpec<'a> {
    pub label: &'a str,
    pub x: f32,
    pub mid: f32,
    pub align_right: bool,
    pub tone: Tone,
}

/// A rounded badge with small text (e.g. `Linia 11`, `semantic`), vertically centered on
/// `mid`; `x` is the right edge if `align_right`. Returns the rectangle.
pub fn badge(layer: &mut Layer, text: &mut TextSystem, ctx: &Ctx<'_>, spec: BadgeSpec<'_>) -> Rect {
    let BadgeSpec { label, x, mid, align_right, tone } = spec;
    let font = ctx.ui_font(-1.5);
    let shaped = text.shape_line(label, &font);
    let (w, h) = (shaped.width + ctx.px(14.0), ctx.px(20.0));
    let left = if align_right { x - w } else { x };
    let rect = Rect::new(left.round(), (mid - h / 2.0).round(), w.round(), h);
    let (fill, color) = match tone {
        Tone::Neutral => (ctx.palette.text.with_alpha(0.05), ctx.palette.text_muted),
        t => (t.color(ctx.palette).with_alpha(0.12), t.color(ctx.palette)),
    };
    layer.quad(Quad::filled(rect, ctx.fade(fill)).rounded(h / 2.0));
    draw_text(
        layer,
        &shaped,
        rect.min_x() + ctx.px(7.0),
        optical_baseline(rect.min_y(), h, &font),
        ctx.fade(color),
    );
    rect
}

/// What a count means: its color by meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BadgeKind {
    /// Something to do (changes, uploads): the accent.
    #[default]
    Count,
    /// Something to look at: the warning color.
    Warning,
    /// Something failed: the error color.
    Error,
}

/// A dot on a button's icon corner (see [`dot_badge`]): there is something (files to
/// upload, changes), its count in the button's tooltip.
#[derive(Debug, Clone, Copy)]
pub struct DotBadge {
    pub kind: BadgeKind,
    /// The button is lit with the accent: the dot white.
    pub lit: bool,
    /// What is behind the button: the ring that sets it apart from the icon.
    pub outline: Rgba,
}

/// A dot badge's diameter, logical pixels.
pub const DOT_BADGE: f32 = 9.0;

/// A small dot centered on an icon's top right corner `at`, its color by meaning, with a
/// 2 px ring in what's behind (`outline`) so it reads cut out of the icon; on whole pixels,
/// scaled with the zoom. Returns its rectangle (without the ring).
pub fn dot_badge(layer: &mut Layer, ctx: &Ctx<'_>, at: Point, badge: DotBadge) -> Rect {
    let DotBadge { kind, lit, outline } = badge;
    let p = ctx.palette;
    let tone = match kind {
        BadgeKind::Count => p.accent,
        BadgeKind::Warning => crate::severity::Level::Warning.color(p),
        BadgeKind::Error => crate::severity::Level::Critical.color(p),
    };
    let d = ctx.px(DOT_BADGE);
    let ring = ctx.px(2.0).max(1.0);
    let rect = Rect::new((at.x - d / 2.0).round(), (at.y - d / 2.0).round(), d, d);
    let outer = Rect::new(rect.min_x() - ring, rect.min_y() - ring, d + 2.0 * ring, d + 2.0 * ring);
    layer.quad(Quad::filled(outer, outline).rounded(outer.size.height / 2.0));
    layer.quad(Quad::filled(rect, if lit { Rgba::WHITE } else { tone }).rounded(d / 2.0));
    rect
}

/// A file in a list (what's to upload, what changed): its state's mark (`M`, `A`, `!`…) in
/// its color, its name, its folder dimmed (cut in the middle), and a tag on the right (why
/// it's there), all cut with "…" to fit.
#[derive(Debug, Clone, Copy)]
pub struct FileRow<'a> {
    /// The path, relative (`src/app/Main.php`), `/`-separated.
    pub path: &'a str,
    pub mark: &'a str,
    pub color: Rgba,
    pub tag: Option<(&'a str, Tone)>,
}

/// Paints `row` in `rect` (the row without its buttons), vertically centered.
pub fn file_row(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    rect: Rect,
    row: FileRow<'_>,
) {
    let (font, small) = (ctx.ui_font(0.0), ctx.ui_font(-1.0));
    let (top, h) = (rect.min_y(), rect.size.height);
    let mark = text.shape_line(row.mark, &small.clone().with_weight(Font::SEMIBOLD));
    let mark_x = rect.min_x() + ctx.px(4.0);
    draw_text(layer, &mark, mark_x, optical_baseline(top, h, &small), row.color);
    let mut right = rect.max_x();
    if let Some((tag, tone)) = row.tag {
        let tag = ellipsize(text, tag, &ctx.ui_font(-1.5), (rect.size.width * 0.4).max(0.0));
        if !tag.is_empty() {
            let mid = top + h / 2.0;
            let spec = BadgeSpec { label: &tag, x: right, mid, align_right: true, tone };
            right = badge(layer, text, ctx, spec).min_x() - ctx.px(space::MD);
        }
    }
    let (folder, name) = row.path.rsplit_once('/').unwrap_or(("", row.path));
    let x = rect.min_x() + ctx.px(26.0);
    let name = ellipsize(text, name, &font, (right - x).max(0.0));
    let shaped = text.shape_line(&name, &font);
    draw_text(layer, &shaped, x, optical_baseline(top, h, &font), ctx.palette.text);
    let fx = x + shaped.width + ctx.px(space::MD);
    if !folder.is_empty() && right - fx > ctx.px(24.0) {
        let folder = ellipsize_middle(text, folder, &small, right - fx);
        let shaped = text.shape_line(&folder, &small);
        draw_text(layer, &shaped, fx, optical_baseline(top, h, &small), ctx.palette.text_muted);
    }
}

/// A single key (e.g. `esc` in a field's corner), vertically centered on `mid`, right-aligned
/// at `right`. Returns the rectangle.
pub fn keycap(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    key: &str,
    right: f32,
    mid: f32,
    hovered: bool,
) -> Rect {
    let font = ctx.ui_font(-2.0);
    let shaped = text.shape_line(key, &font);
    let (w, h) = ((shaped.width + ctx.px(16.0)).max(ctx.px(28.0)), ctx.px(26.0));
    let rect = Rect::new((right - w).round(), (mid - h / 2.0).round(), w.round(), h);
    layer.quad(
        Quad::filled(
            rect,
            ctx.fade(if hovered {
                ctx.palette.text.with_alpha(0.06)
            } else {
                ctx.palette.popup_background
            }),
        )
        .rounded(ctx.px(radius::SM))
        .bordered(
            ctx.line(),
            ctx.fade(if hovered {
                ctx.palette.text_muted.with_alpha(0.5)
            } else {
                ctx.palette.border
            }),
        ),
    );
    draw_text(
        layer,
        &shaped,
        (rect.min_x() + (w - shaped.width) / 2.0).round(),
        optical_baseline(rect.min_y(), h, &font),
        ctx.fade(if hovered { ctx.palette.text } else { ctx.palette.text_muted }),
    );
    rect
}

/// A key drawn over a picture (About's illustration, a full-window scene): a dark,
/// translucent chip — night blue, a thin light border, near-white label — readable over
/// anything, a bright sky included. Its top-right corner at `top_right` (logical sizes:
/// 22 high, the label 11 with 8 on each side). Returns the rectangle.
pub fn overlay_key(
    layer: &mut Layer,
    ctx: &Ctx<'_>,
    shaped: &text_layout::ShapedLine,
    font: &Font,
    top_right: Point,
) -> Rect {
    let px = |v: f32| ctx.px(v);
    let h = px(22.0);
    let w = shaped.width + px(16.0);
    let rect = Rect::new((top_right.x - w).round(), top_right.y.round(), w.round(), h);
    layer.quad(
        Quad::filled(rect, ctx.fade(OVERLAY_KEY_FILL))
            .rounded(px(6.0))
            .bordered(ctx.line(), ctx.fade(Rgba::WHITE.with_alpha(0.28))),
    );
    draw_text(
        layer,
        shaped,
        rect.min_x() + px(8.0),
        optical_baseline(rect.min_y(), h, font),
        ctx.fade(Rgba::WHITE.with_alpha(0.95)),
    );
    rect
}

/// [`overlay_key`]'s fill: night blue, 62 %.
pub const OVERLAY_KEY_FILL: Rgba = Rgba::rgb(0x0d0f24).with_alpha(0.62);

/// [`overlay_key`]'s label font at `ctx`'s scale.
pub fn overlay_key_font(ctx: &Ctx<'_>) -> Font {
    Font::new(ctx.palette.ui_font.clone(), ctx.px(11.0))
}

/// A corner of an area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    #[default]
    BottomRight,
}

/// What [`glass_badge`] draws: an icon on its tile, a title and what it's about, on a pane of
/// frosted glass in a corner. Not a notification: it only says what's on screen.
#[derive(Debug, Clone, Copy)]
pub struct GlassBadge<'a> {
    /// The icon's name in the app's set.
    pub icon: &'a str,
    /// A picture shown instead of the icon, larger and on no tile (the app's logo).
    pub image: Option<&'a render_api::ImageData>,
    pub title: &'a str,
    /// The smaller line under the title.
    pub detail: Option<&'a str>,
    pub corner: Corner,
    /// How far from the corner (logical pixels).
    pub margin: f32,
    /// What the glass and the words are tinted with (light over a dark screen).
    pub tint: Rgba,
    /// How much of it shows (0..1).
    pub opacity: f32,
}

impl<'a> GlassBadge<'a> {
    /// A light pane in the bottom right corner.
    pub fn new(icon: &'a str, title: &'a str) -> Self {
        Self {
            icon,
            image: None,
            title,
            detail: None,
            corner: Corner::default(),
            margin: 24.0,
            tint: Rgba::WHITE,
            opacity: 1.0,
        }
    }
}

/// Where a pane of `size` goes in `area`, `margin` from `corner`.
pub fn in_corner(area: Rect, size: (f32, f32), corner: Corner, margin: f32) -> Point {
    let (left, top) = (area.min_x() + margin, area.min_y() + margin);
    let (right, bottom) = (area.max_x() - margin - size.0, area.max_y() - margin - size.1);
    match corner {
        Corner::TopLeft => Point::new(left, top),
        Corner::TopRight => Point::new(right, top),
        Corner::BottomLeft => Point::new(left, bottom),
        Corner::BottomRight => Point::new(right, bottom),
    }
}

/// A glass badge's picture (its side), padding and the gap after the picture.
fn glass_badge_parts(ctx: &Ctx<'_>, badge: &GlassBadge<'_>) -> (f32, f32, f32) {
    let tile = ctx.px(if badge.image.is_some() { 40.0 } else { 32.0 });
    (tile, ctx.px(10.0), ctx.px(11.0))
}

/// The size [`glass_badge`] draws `badge` at (physical pixels).
pub fn glass_badge_size(
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    badge: &GlassBadge<'_>,
) -> (f32, f32) {
    let title = text.shape_line(badge.title, &ctx.ui_font(0.5).with_weight(Font::SEMIBOLD));
    let detail = badge.detail.map_or(0.0, |d| text.shape_line(d, &ctx.ui_font(-1.5)).width);
    let (tile, pad, gap) = glass_badge_parts(ctx, badge);
    let words = title.width.max(detail);
    ((pad + tile + gap + words + ctx.px(16.0)).round(), (pad * 2.0 + tile).round())
}

/// Draws `badge` in `area`. The frost is painted, not a backdrop blur: a blur would keep a copy
/// of the window for as long as the pane is up. Returns the pane.
pub fn glass_badge(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &icons::Icons,
    ctx: &Ctx<'_>,
    area: Rect,
    badge: &GlassBadge<'_>,
) -> Rect {
    let px = |v: f32| ctx.px(v);
    let shown = |alpha: f32| badge.tint.with_alpha(alpha * badge.opacity);
    let title_font = ctx.ui_font(0.5).with_weight(Font::SEMIBOLD);
    let detail_font = ctx.ui_font(-1.5);
    let title = text.shape_line(badge.title, &title_font);
    let detail = badge.detail.map(|d| text.shape_line(d, &detail_font));
    let (tile, pad, gap) = glass_badge_parts(ctx, badge);
    let size = glass_badge_size(text, ctx, badge);
    let at = in_corner(area, size, badge.corner, px(badge.margin));
    let (x, y) = (at.x.round(), at.y.round());
    let pane = Rect::new(x, y, size.0, size.1);
    // Painted frost: a badge may stay up for hours (the dimmed screen).
    let look =
        crate::screen_widgets::GlassLook { tint: badge.tint, opacity: badge.opacity, night: false };
    crate::screen_widgets::glass_pane(layer, ctx, pane, look);
    let tile_at = Rect::new(x + pad, y + pad, tile, tile);
    if let Some(image) = badge.image {
        layer.images.push(render_api::ImageInstance {
            image: image.clone(),
            bounds: tile_at,
            opacity: badge.opacity.clamp(0.0, 1.0),
        });
    } else {
        layer.quad(Quad::filled(tile_at, shown(0.1)).rounded(px(radius::LG)));
    }
    if let Some(id) = icons.id(badge.icon).filter(|_| badge.image.is_none()) {
        let side = px(18.0);
        let inset = ((tile - side) / 2.0).round();
        let icon = Rect::new(tile_at.min_x() + inset, tile_at.min_y() + inset, side, side);
        ui_kit::draw_icon(layer, icons, id, icon, shown(0.9));
    }
    let words_x = tile_at.max_x() + gap;
    match &detail {
        Some(detail) => {
            let line = tile / 2.0;
            draw_text(
                layer,
                &title,
                words_x,
                optical_baseline(y + pad, line, &title_font),
                shown(0.92),
            );
            let below = y + pad + line;
            draw_text(
                layer,
                detail,
                words_x,
                optical_baseline(below, line, &detail_font),
                shown(0.58),
            );
        }
        None => {
            draw_text(
                layer,
                &title,
                words_x,
                optical_baseline(y + pad, tile, &title_font),
                shown(0.92),
            );
        }
    }
    pane
}

/// A changed file's badge's side, logical pixels.
pub const CHANGE_BADGE: f32 = 16.0;

/// A changed file's state as a small rounded square in its color (the theme's
/// `vcs.badge.*`) with its sign over it (`…` modified, `+` added, `−` deleted, `→` renamed,
/// `?` untracked, `!` conflicted), in `rect` (a square, on whole pixels).
pub fn change_badge(
    layer: &mut Layer,
    icons: &icons::Icons,
    ctx: &Ctx<'_>,
    rect: Rect,
    state: crate::changed_files::ChangeState,
) {
    let (fill, icon) = change_badge_look(state, ctx.palette);
    let r = ctx.px(radius::SM).min(rect.size.height / 2.0);
    layer.quad(Quad::filled(rect, ctx.fade(fill)).rounded(r));
    if let Some(id) = icons.id(icon) {
        let s = (rect.size.height * 0.72).round();
        let at = Rect::new(
            (rect.min_x() + (rect.size.width - s) / 2.0).round(),
            (rect.min_y() + (rect.size.height - s) / 2.0).round(),
            s,
            s,
        );
        ui_kit::draw_icon(layer, icons, id, at, ctx.fade(sign_on(fill)));
    }
}

/// A state's badge color and the icon of its sign.
pub fn change_badge_look(
    state: crate::changed_files::ChangeState,
    p: &Palette,
) -> (Rgba, &'static str) {
    use crate::changed_files::ChangeState;
    let b = &p.vcs_badges;
    match state {
        ChangeState::Modified => (b.modified, "mark-modified"),
        ChangeState::Added => (b.added, "mark-added"),
        ChangeState::Deleted => (b.deleted, "mark-deleted"),
        ChangeState::Renamed => (b.renamed, "mark-renamed"),
        ChangeState::Untracked => (b.untracked, "mark-untracked"),
        ChangeState::Conflicted => (b.conflicted, "mark-conflicted"),
        ChangeState::Failed => (p.warning, "mark-conflicted"),
    }
}

/// The sign's color over `fill`: white on a dark fill, nearly black on a light one.
pub fn sign_on(fill: Rgba) -> Rgba {
    let luminance = 0.2126 * fill.r + 0.7152 * fill.g + 0.0722 * fill.b;
    if luminance > 0.6 { Rgba::rgb(0x1f2329) } else { Rgba::WHITE }
}
