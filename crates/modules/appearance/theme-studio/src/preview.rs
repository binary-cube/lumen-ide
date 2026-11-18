//! The live preview: a window with code, a button and the theme's colors as the draft has them.

use super::*;

/// The preview: a miniature editor (tabs, gutter, current line, selection, colored
/// code) with the work-in-progress theme's colors.
pub(crate) fn paint_preview(
    scene: &mut render_api::Scene,
    text: &mut TextSystem,
    ds: &Ctx<'_>,
    area: Rect,
    draft: &ThemeDraft,
    effects: Option<PreviewEffects<'_>>,
    backdrop: bool,
) {
    let ui = Font::new(ds.palette.ui_font.clone(), ds.px(12.0));
    let frame = preview_frame(ds, area);
    // The window's background, then its effects (behind the panels), then the rest.
    scene.push_layer(Some(area)).quad(
        Quad::filled(frame, draft_color(draft, "base"))
            .rounded(ds.px(radius::LG))
            .bordered(ds.line(), draft_color(draft, "line")),
    );
    if let Some(e) = effects {
        let inner = Rect::new(
            frame.min_x() + ds.line(),
            frame.min_y() + ds.line(),
            frame.size.width - 2.0 * ds.line(),
            frame.size.height - 2.0 * ds.line(),
        );
        let layer = scene.push_layer(Some(inner));
        let geometry = ui_components::decorations::Geometry {
            window: inner,
            scale: ds.scale,
            top_inset: 0.0,
            top_bar: 0.0,
            keep_clear: &[],
        };
        let (_, side, card) = preview_rects(ds, area);
        let covered = [card, side];
        let frame = ui_components::decorations::DecorationFrame {
            geometry,
            covered: &covered,
            editor: &covered[..1],
            time: e.time,
            icons: Some(e.icons),
        };
        e.painter.paint(&frame, layer);
    }
    let layer = scene.push_layer(Some(area));
    let card = paint_preview_window(layer, text, ds, area, draft, &ui);
    // Code (clipped to the card edges).
    let layer = scene.push_layer(Some(card));
    paint_preview_code(layer, text, ds, card, draft);
    paint_preview_button(layer, text, ds, card, draft, &ui);
    // A sample notification in the window's corner (its colors: the popups').
    let layer = scene.push_layer(Some(area));
    paint_preview_toast(layer, text, ds, area, draft, &ui);
    if backdrop {
        paint_preview_dialog(scene, text, ds, area, draft, &ui);
    }
}

/// A dialog over the preview's window, on the draft's backdrop (blurred too, for Glass):
/// shown while its backdrop is being changed.
fn paint_preview_dialog(
    scene: &mut render_api::Scene,
    text: &mut TextSystem,
    ds: &Ctx<'_>,
    area: Rect,
    draft: &ThemeDraft,
    ui: &Font,
) {
    let px = |v: f32| ds.px(v);
    let c = |k: &str| draft_color(draft, k);
    let frame = preview_frame(ds, area);
    let inner = Rect::new(
        frame.min_x() + ds.line(),
        frame.min_y() + ds.line(),
        frame.size.width - 2.0 * ds.line(),
        frame.size.height - 2.0 * ds.line(),
    );
    let style = ui_components::BackdropStyle::resolve(draft.backdrop, c("base"))
        .with_user(Default::default());
    let palette = ui_components::Palette { backdrop: style, ..ds.palette.clone() };
    let ctx = Ctx::new(&palette, ds.scale);
    let layer = scene.push_layer(Some(inner));
    ds::backdrop(layer, &ctx, inner);
    let label = text.shape_line(&t("studio.sample_dialog"), ui);
    let (w, h) = ((label.width + px(64.0)).min(inner.size.width - px(24.0)), px(64.0));
    let r = Rect::new(
        (inner.min_x() + (inner.size.width - w) / 2.0).round(),
        (inner.min_y() + (inner.size.height - h) / 2.0).round(),
        w,
        h,
    );
    layer.quad(
        Quad::filled(Rect::new(r.min_x(), r.min_y() + px(4.0), w, h), Rgba::BLACK.with_alpha(0.2))
            .rounded(px(10.0)),
    );
    // Its panel as the draft's floating panels are (frosted over the glass backdrop).
    let glass = ui_components::backdrop::SurfaceStyle::resolve(draft.floating);
    let blurred = style.blur_radius(ds.scale).is_some();
    let fill = if glass.is_glass() && blurred { glass.fill(c("panel")) } else { c("panel") };
    layer.quad(Quad::filled(r, fill).rounded(px(10.0)).bordered(ds.line(), c("line")));
    draw_text(
        layer,
        &label,
        (r.min_x() + (w - label.width) / 2.0).round(),
        ds::optical_baseline(r.min_y(), h, ui),
        c("fg"),
    );
}

/// The effects drawn in the preview: their painter, the time, the app's icons.
pub(crate) struct PreviewEffects<'a> {
    pub painter: &'a mut dyn ui_components::decorations::WindowDecorations,
    pub time: f32,
    pub icons: &'a icons::Icons,
}

/// The preview window's parts in `area`: its frame, its side panel, its editor's card.
fn preview_rects(ds: &Ctx<'_>, area: Rect) -> (Rect, Rect, Rect) {
    let px = |v: f32| ds.px(v);
    let frame = preview_frame(ds, area);
    let side = Rect::new(
        frame.min_x() + px(12.0),
        frame.min_y() + px(12.0),
        px(150.0).min(frame.size.width * 0.32).round(),
        frame.size.height - px(24.0),
    );
    let card = Rect::new(
        side.max_x() + px(12.0),
        side.min_y(),
        frame.max_x() - side.max_x() - px(24.0),
        side.size.height,
    );
    (frame, side, card)
}

/// How opaque the preview's panels and editor are: the draft's effects' glass (the same
/// as the window's, its contrast kept).
fn glass_of(draft: &ThemeDraft) -> (f32, f32) {
    let Some(see) = crate::effects::resolved(draft).see_through() else { return (1.0, 1.0) };
    let c = |k: &str| draft_color(draft, k);
    let behind: Vec<(Rgba, f32)> = see.colors.iter().map(|(col, o)| (*col, o.min(1.0))).collect();
    let texts = [
        (c("fg"), ui_components::decorations::TEXT_CONTRAST),
        (c("muted"), ui_components::decorations::MUTED_CONTRAST),
    ];
    let alpha = |card: Rgba| {
        ui_components::decorations::glass_alpha(card, &texts, c("base"), &behind, see.card_opacity)
    };
    let panels = alpha(c("panel")).max(alpha(c("card")));
    (panels, if see.editor { alpha(c("card")) } else { 1.0 })
}

/// The preview window's frame in `area`.
fn preview_frame(ds: &Ctx<'_>, area: Rect) -> Rect {
    let pad = ds.px(32.0);
    Rect::new(
        area.min_x() + pad,
        area.min_y() + pad,
        area.size.width - 2.0 * pad,
        area.size.height - 2.0 * pad,
    )
}

/// A sample notification in the preview window's bottom-right corner: a card with the
/// accent's mark and a line of text.
fn paint_preview_toast(
    layer: &mut Layer,
    text: &mut TextSystem,
    ds: &Ctx<'_>,
    area: Rect,
    draft: &ThemeDraft,
    ui: &Font,
) {
    let px = |v: f32| ds.px(v);
    let c = |k: &str| draft_color(draft, k);
    let label = text.shape_line(&t("studio.sample_toast"), ui);
    let w = label.width + px(44.0);
    let h = px(36.0);
    // Inside the preview's window (its corner, as the app's toasts are): a sample, never
    // taken for the app's own notification.
    let frame = preview_frame(ds, area);
    let r = Rect::new(frame.max_x() - px(16.0) - w, frame.max_y() - px(16.0) - h, w, h);
    if r.min_x() < frame.min_x() + px(40.0) {
        return;
    }
    layer.quad(
        Quad::filled(Rect::new(r.min_x(), r.min_y() + px(3.0), w, h), Rgba::BLACK.with_alpha(0.18))
            .rounded(px(8.0)),
    );
    layer.quad(Quad::filled(r, c("panel")).rounded(px(8.0)).bordered(ds.line(), c("line")));
    let d = px(8.0);
    layer.quad(
        Quad::filled(
            Rect::new(r.min_x() + px(14.0), (r.min_y() + (h - d) / 2.0).round(), d, d),
            c("accent"),
        )
        .rounded(d / 2.0),
    );
    draw_text(layer, &label, r.min_x() + px(30.0), ds::optical_baseline(r.min_y(), h, ui), c("fg"));
}

/// A token's color in the draft (grey when it has none).
pub(crate) fn draft_color(draft: &ThemeDraft, key: &str) -> Rgba {
    draft.get(key).unwrap_or(Rgba::rgb(0x888888))
}

/// The preview's window: the background, the sidebar with its files, the editor card and
/// its tabs; returns the card.
pub(crate) fn paint_preview_window(
    layer: &mut Layer,
    text: &mut TextSystem,
    ds: &Ctx<'_>,
    area: Rect,
    draft: &ThemeDraft,
    ui: &Font,
) -> Rect {
    let px = |v: f32| ds.px(v);
    let c = |k: &str| draft_color(draft, k);
    let (_, side, card) = preview_rects(ds, area);
    // Effects shown through the panels (the draft's): the cards let them through, as the
    // window's will.
    let (panels, editor) = glass_of(draft);
    layer.quad(
        Quad::filled(side, c("panel").with_alpha(panels))
            .rounded(px(radius::MD))
            .bordered(ds.line(), c("line")),
    );
    for (i, name) in ["src", "class.php", "index.php", "style.css"].iter().enumerate() {
        // Kept inside the side panel, cut short when narrow.
        let name = ds::ellipsize(text, name, ui, side.size.width - px(22.0));
        let shaped = text.shape_line(&name, ui);
        let y = side.min_y() + px(18.0) + i as f32 * px(24.0);
        if i == 1 {
            layer.quad(
                Quad::filled(
                    Rect::new(
                        side.min_x() + px(6.0),
                        y - px(4.0),
                        side.size.width - px(12.0),
                        px(22.0),
                    ),
                    c("accent").with_alpha(0.18),
                )
                .rounded(px(5.0)),
            );
        }
        draw_text(
            layer,
            &shaped,
            side.min_x() + px(14.0),
            y + px(12.0),
            if i == 1 { c("fg") } else { c("muted") },
        );
    }
    layer.quad(
        Quad::filled(card, c("card").with_alpha(editor))
            .rounded(px(radius::MD))
            .bordered(ds.line(), c("line")),
    );
    // Tabs.
    let tab = text.shape_line("class.php", ui);
    let tx = card.min_x() + px(16.0);
    draw_text(layer, &tab, tx, card.min_y() + px(24.0), c("fg"));
    layer.quad(Quad::filled(
        Rect::new(tx - px(6.0), card.min_y() + px(34.0), tab.width + px(12.0), px(2.0)),
        c("accent"),
    ));
    // The second tab, when it fits in the card.
    let other = text.shape_line("index.php", ui);
    let ox = tx + tab.width + px(28.0);
    if ox + other.width <= card.max_x() - px(12.0) {
        draw_text(layer, &other, ox, card.min_y() + px(24.0), c("muted"));
    }
    layer.quad(Quad::filled(
        Rect::new(card.min_x(), card.min_y() + px(36.0), card.size.width, ds.line()),
        c("line"),
    ));
    card
}

/// The preview's code: line numbers, the current line, a selection and the colored tokens.
pub(crate) fn paint_preview_code(
    layer: &mut Layer,
    text: &mut TextSystem,
    ds: &Ctx<'_>,
    card: Rect,
    draft: &ThemeDraft,
) {
    let px = |v: f32| ds.px(v);
    let c = |k: &str| draft_color(draft, k);
    let code = Font::new(ds.palette.code_font.clone(), px(13.0));
    let k = |s: &str| (s.to_owned(), "syntax.keyword");
    let lines: Vec<Vec<(String, &str)>> = vec![
        vec![("<?php".to_owned(), "syntax.keyword")],
        vec![],
        vec![(format!("// {}", i18n::t("studio.sample_comment")), "syntax.comment")],
        vec![k("final class "), ("Car".to_owned(), "syntax.type"), (" {".to_owned(), "code")],
        vec![
            ("    ".to_owned(), "code"),
            k("private int "),
            ("$speed".to_owned(), "syntax.variable"),
            (" = ".to_owned(), "code"),
            ("42".to_owned(), "syntax.number"),
            (";".to_owned(), "code"),
        ],
        vec![],
        vec![
            ("    ".to_owned(), "code"),
            k("public function "),
            ("drive".to_owned(), "syntax.function"),
            ("(".to_owned(), "code"),
            ("string ".to_owned(), "syntax.type"),
            ("$to".to_owned(), "syntax.variable"),
            ("): ".to_owned(), "code"),
            ("string".to_owned(), "syntax.type"),
        ],
        vec![("    {".to_owned(), "code")],
        vec![
            ("        ".to_owned(), "code"),
            k("return "),
            ("\"Spre \"".to_owned(), "syntax.string"),
            (" . ".to_owned(), "code"),
            ("$this".to_owned(), "syntax.variable"),
            ("->".to_owned(), "code"),
            ("speed".to_owned(), "syntax.property"),
            (";".to_owned(), "code"),
        ],
        vec![("    }".to_owned(), "code")],
        vec![("}".to_owned(), "code")],
    ];
    let row = px(22.0);
    let mut y = card.min_y() + px(48.0);
    for (i, line) in lines.iter().enumerate() {
        if i == 4 {
            layer.quad(Quad::filled(
                Rect::new(card.min_x() + ds.line(), y, card.size.width - 2.0 * ds.line(), row),
                c("fg").with_alpha(0.04),
            ));
        }
        let n = text.shape_line(&(i + 1).to_string(), &code);
        let base = ds::optical_baseline(y, row, &code);
        draw_text(layer, &n, card.min_x() + px(36.0) - n.width, base, c("muted").with_alpha(0.7));
        let mut x = card.min_x() + px(52.0);
        for (s, key) in line {
            let shaped = text.shape_line(s, &code);
            if i == 6 && key == &"syntax.function" {
                layer.quad(
                    Quad::filled(
                        Rect::new(x, y + px(2.0), shaped.width, row - px(4.0)),
                        c("accent").with_alpha(0.25),
                    )
                    .rounded(px(3.0)),
                );
            }
            draw_text(layer, &shaped, x, base, c(key));
            x += shaped.width;
        }
        y += row;
    }
}

/// A button and a badge in the accent color, in the card's corner.
pub(crate) fn paint_preview_button(
    layer: &mut Layer,
    text: &mut TextSystem,
    ds: &Ctx<'_>,
    card: Rect,
    draft: &ThemeDraft,
    ui: &Font,
) {
    let px = |v: f32| ds.px(v);
    // Under the code (11 lines from its top), when there's room.
    if card.size.height < px(48.0 + 11.0 * 22.0 + 52.0) {
        return;
    }
    let chip = Rect::new(card.min_x() + px(52.0), card.max_y() - px(44.0), px(100.0), px(28.0));
    layer.quad(Quad::filled(chip, draft_color(draft, "accent")).rounded(px(7.0)));
    let label = text.shape_line(&t("studio.token.accent"), ui);
    draw_text(
        layer,
        &label,
        chip.min_x() + (chip.size.width - label.width) / 2.0,
        ds::optical_baseline(chip.min_y(), chip.size.height, ui),
        Rgba::WHITE,
    );
}
