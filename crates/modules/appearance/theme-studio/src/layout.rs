//! The studio's frame: the header (its title, the search, the unsaved changes), the
//! categories (down the left, or as tabs when narrow), the chosen category's panel
//! (scrolled), the live preview (on the right, or behind a button when narrow) and the
//! footer (where the theme is used, Discard, Save).

use super::*;
use ui_components::form::{self, NavItem};

/// Below this width (logical pixels) the preview hides behind a button.
pub(crate) const PREVIEW_MIN: f32 = 760.0;
/// Below this width the categories become tabs.
pub(crate) const NAV_MIN: f32 = 1180.0;
/// The header's and the footer's heights; the navigation's width (logical pixels).
pub(crate) const HEADER_H: f32 = 64.0;
pub(crate) const FOOTER_H: f32 = 64.0;
pub(crate) const NAV_W: f32 = 212.0;
/// The search's row when narrow (logical pixels).
pub(crate) const SEARCH_ROW: f32 = 50.0;
/// Below this width the footer puts the scope above the buttons.
pub(crate) const FOOTER_MIN: f32 = 620.0;
/// The panel's content: its padding and widest (logical pixels).
pub(crate) const PANEL_PAD: f32 = 32.0;
pub(crate) const PANEL_MAX: f32 = 760.0;

/// Where the frame's parts are, for a size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Frame {
    pub header: Rect,
    /// The search's own row under the header (narrow: no room beside the title).
    pub search_row: Option<Rect>,
    /// The footer on two lines (narrow): the scope above the buttons.
    pub stacked_footer: bool,
    /// The navigation down the side (none: tabs at the panel's top).
    pub nav: Option<Rect>,
    pub panel: Rect,
    /// The preview beside the panel (none: behind a button).
    pub preview: Option<Rect>,
    pub footer: Rect,
}

impl Frame {
    pub(crate) fn of(b: Rect, scale: f32) -> Self {
        let px = |v: f32| (v * scale).round();
        let logical = b.size.width / scale.max(0.5);
        let narrow = logical < PREVIEW_MIN;
        let header = Rect::new(b.min_x(), b.min_y(), b.size.width, px(HEADER_H));
        let search_row =
            narrow.then(|| Rect::new(b.min_x(), header.max_y(), b.size.width, px(SEARCH_ROW)));
        let stacked_footer = logical < FOOTER_MIN;
        let footer_h = px(if stacked_footer { FOOTER_H + 48.0 } else { FOOTER_H });
        let footer = Rect::new(b.min_x(), b.max_y() - footer_h, b.size.width, footer_h);
        let top = search_row.map_or(header.max_y(), |r| r.max_y());
        let h = (footer.min_y() - top).max(0.0);
        let nav = (logical >= NAV_MIN).then(|| Rect::new(b.min_x(), top, px(NAV_W), h));
        let preview_w = if logical >= PREVIEW_MIN {
            (b.size.width * 0.36).clamp(px(300.0), px(600.0))
        } else {
            0.0
        };
        let preview =
            (preview_w > 0.0).then(|| Rect::new(b.max_x() - preview_w, top, preview_w, h));
        let x0 = nav.map_or(b.min_x(), |n| n.max_x());
        let x1 = preview.map_or(b.max_x(), |p| p.min_x());
        let panel = Rect::new(x0, top, (x1 - x0).max(0.0), h);
        Self { header, search_row, stacked_footer, nav, panel, preview, footer }
    }
}

impl Studio {
    /// The whole view, for one frame.
    pub(crate) fn paint_frame(
        &mut self,
        mut ctx: ViewerPaint<'_>,
        input: &StudioInput,
        draft: &ThemeDraft,
    ) {
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let p = ctx.palette;
        let b = ctx.bounds;
        let frame = Frame::of(b, ctx.scale);
        let base = self.base_draft.clone().unwrap_or_else(|| draft.clone());
        let entries = model::catalog(draft, input, false);

        // ── The panel (scrolled; under the rest). ──
        self.list = frame.panel;
        let mut panel = frame.panel;
        let tabs = frame.nav.is_none();
        let tab_h = if tabs { ds.px(form::NAV_TAB) + ds.line() + ds.px(4.0) } else { 0.0 };
        panel = Rect::new(
            panel.min_x(),
            panel.min_y() + tab_h,
            panel.size.width,
            panel.size.height - tab_h,
        );
        self.list = panel;
        self.clamp_scroll();
        self.paint_panel(&mut ctx, input, draft, &base);
        // Its scrollbar (not over the gallery, which is laid out on its own).
        if !self.gallery_open {
            self.bar.set_auto_hiding(true);
            let scrolled = self.scrolled();
            self.bar.paint(ctx.scene, p, ctx.scale, panel, self.content_h, scrolled);
        }

        // ── The navigation: down the side, or tabs over the panel. ──
        let items: Vec<(Category, String)> =
            Category::shown(draft).map(|c| (c, c.label())).collect();
        let nav_items: Vec<NavItem<'_>> = items
            .iter()
            .map(|(c, label)| NavItem {
                icon: c.icon(),
                label,
                count: (*c == Category::Effects).then_some(draft.decorations.layers.len()),
                dot: model::category_changed(&entries, draft, &base, *c),
            })
            .collect();
        let selected = items.iter().position(|(c, _)| *c == self.category).unwrap_or(0);
        let hovered = items.iter().position(|(c, _)| self.hover == Some(Hit::Category(*c)));
        let mut nav_hits = Vec::new();
        if let Some(nav) = frame.nav {
            let layer = ctx.scene.push_layer(Some(nav));
            layer.quad(Quad::filled(nav, p.sidebar_background.with_alpha(0.0)));
            ds::divider_vertical(layer, &ds, nav.max_x() - ds.line(), nav.min_y(), nav.max_y());
            let pad = ds.px(14.0);
            form::nav_list(
                &mut LayerPainter::new(layer, ctx.text, ctx.icons, ds),
                form::NavPlace::new(
                    nav.min_x() + pad,
                    nav.min_y() + ds.px(18.0),
                    nav.size.width - 2.0 * pad,
                ),
                form::Nav::new(&nav_items, selected, hovered),
                |i, r| nav_hits.push((r, i)),
            );
        } else {
            let strip =
                Rect::new(frame.panel.min_x(), frame.panel.min_y(), frame.panel.size.width, tab_h);
            let layer = ctx.scene.push_layer(Some(strip));
            layer.quad(Quad::filled(strip, p.editor_background));
            let pad = ds.px(16.0);
            form::nav_tabs(
                &mut LayerPainter::new(layer, ctx.text, ctx.icons, ds),
                (strip.min_x() + pad, strip.min_y() + ds.px(4.0), strip.size.width - 2.0 * pad),
                form::Nav::new(&nav_items, selected, hovered),
                |i, r| nav_hits.push((r, i)),
            );
        }
        for (r, i) in nav_hits {
            self.hits.push((r, Hit::Category(items[i].0)));
        }

        // ── The preview: beside, or over the panel when asked for (narrow). ──
        let preview = frame.preview.or_else(|| self.preview_open.then_some(frame.panel));
        if let Some(area) = preview {
            let layer = ctx.scene.push_layer(Some(area));
            layer.quad(Quad::filled(area, p.editor_background));
            if frame.preview.is_some() {
                ds::divider_vertical(layer, &ds, area.min_x(), area.min_y(), area.max_y());
            }
            self.paint_preview_pane(&mut ctx, area, input, draft);
        }

        // ── The header: the title, the search, the changes. ──
        self.paint_header(&mut ctx, frame, &entries, draft);

        // ── The footer: where it is used, and the buttons. ──
        let footer = frame.footer;
        let layer = ctx.scene.push_layer(Some(footer));
        layer.quad(Quad::filled(footer, p.editor_background));
        ds::divider(layer, &ds, footer.min_x(), footer.max_x(), footer.min_y());
        let pad = ds.px(24.0);
        let changed = model::changes(&entries, draft, &base) > 0;
        if frame.stacked_footer {
            let half = footer.size.height / 2.0;
            let top =
                Rect::new(footer.min_x(), footer.min_y() + ds.px(4.0), footer.size.width, half);
            let bottom = Rect::new(
                footer.min_x(),
                footer.min_y() + half - ds.px(4.0),
                footer.size.width,
                half,
            );
            self.paint_buttons(layer, ctx.text, ds, bottom, pad, changed);
            self.paint_scope(layer, ctx.text, ds, top, pad, top.max_x() - pad);
        } else {
            let buttons = self.paint_buttons(layer, ctx.text, ds, footer, pad, changed);
            self.paint_scope(layer, ctx.text, ds, footer, pad, buttons);
        }

        // Over the panel: the gallery of effects, an effect's menu.
        self.paint_gallery(ctx.scene, ctx.text, ctx.icons, &ds, draft);
        self.paint_menu(ctx.scene, ctx.text, ctx.icons, &ds);
        // Color picker, above everything.
        self.picker.set_viewport(b, ctx.scale);
        self.picker.paint(ctx.text, ctx.scene, ctx.icons, p);
    }

    /// The header: the studio's title on the left, the search in the middle, how many
    /// changes are unsaved on the right (and, narrow, the preview's button).
    fn paint_header(
        &mut self,
        ctx: &mut ViewerPaint<'_>,
        frame: Frame,
        entries: &[model::Entry],
        draft: &ThemeDraft,
    ) {
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let (scene, text, icons) = (&mut *ctx.scene, &mut *ctx.text, ctx.icons);
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let h = frame.header;
        let whole = frame
            .search_row
            .map_or(h, |r| Rect::new(h.min_x(), h.min_y(), h.size.width, r.max_y() - h.min_y()));
        let layer = scene.push_layer(Some(whole));
        layer.quad(Quad::filled(whole, p.editor_background));
        if frame.search_row.is_none() {
            ds::divider(layer, &ds, h.min_x(), h.max_x(), h.max_y() - ds.line());
        }
        let mid = h.min_y() + h.size.height / 2.0;
        let pad = px(24.0);
        // The title, with the theme it starts from under it.
        let title_font = ds.ui_font(2.0).with_weight(Font::SEMIBOLD);
        let small = ds.ui_font(-2.0);
        let title = text.shape_line(&t("studio.title"), &title_font);
        draw_text(
            layer,
            &title,
            h.min_x() + pad,
            ds::optical_baseline((mid - px(19.0)).round(), px(22.0), &title_font),
            p.text,
        );
        let base_name = self
            .input
            .as_ref()
            .and_then(|i| i.themes.get(self.base))
            .map_or_else(String::new, |th| th.name.clone());
        let sub = i18n::tr!("studio.based_on", theme = base_name.as_str());
        let sub = ds::ellipsize(text, &sub, &small, px(240.0));
        let sub = text.shape_line(&sub, &small);
        draw_text(
            layer,
            &sub,
            h.min_x() + pad,
            ds::optical_baseline((mid + px(2.0)).round(), px(18.0), &small),
            p.text_muted,
        );
        let left_end = h.min_x() + pad + title.width.max(sub.width) + px(24.0);
        // On the right: the changes (and the preview's button, narrow).
        let mut right = h.max_x() - pad;
        if frame.preview.is_none() {
            let label = t("studio.preview_button");
            let w = ds::primary_width(text, &ds, &[&label]).min(px(140.0));
            let r = Rect::new(right - w, (mid - px(16.0)).round(), w, px(32.0));
            if self.preview_open {
                layer.quad(
                    Quad::filled(r, p.accent.with_alpha(0.12))
                        .rounded(px(radius::MD))
                        .bordered(ds.line(), p.accent.with_alpha(0.6)),
                );
                let shaped = text.shape_line(&label, &ds::button_font(&ds));
                draw_text(
                    layer,
                    &shaped,
                    (r.min_x() + (w - shaped.width) / 2.0).round(),
                    ds::optical_baseline(r.min_y(), r.size.height, &ds::button_font(&ds)),
                    p.accent,
                );
            } else {
                ds::secondary_button(
                    layer,
                    text,
                    &ds,
                    r,
                    &label,
                    true,
                    self.hover == Some(Hit::PreviewOpen),
                );
            }
            self.hits.push((r, Hit::PreviewOpen));
            right = r.min_x() - px(16.0);
        }
        let base = self.base_draft.as_ref().unwrap_or(draft);
        let n = model::changes(entries, draft, base);
        let label =
            if n == 0 { t("studio.no_changes") } else { i18n::tr!("studio.changes", n = n as i64) };
        let font = ds.ui_font(-1.0);
        let shaped = text.shape_line(&label, &font);
        let lx = right - shaped.width;
        draw_text(
            layer,
            &shaped,
            lx,
            ds::optical_baseline((mid - px(10.0)).round(), px(20.0), &font),
            if n == 0 { p.text_muted } else { p.text },
        );
        if n > 0 {
            let d = px(8.0);
            layer.quad(
                Quad::filled(Rect::new(lx - px(8.0) - d, (mid - d / 2.0).round(), d, d), p.accent)
                    .rounded(d / 2.0),
            );
        }
        right = lx - px(32.0);
        // The search: in the middle (as wide as there's room for, at most 360), or on its
        // own row under the header when narrow.
        let place = match frame.search_row {
            Some(row) => {
                ds::divider(layer, &ds, row.min_x(), row.max_x(), row.max_y() - ds.line());
                let w = row.size.width - 2.0 * pad;
                Some(Rect::new(row.min_x() + pad, (row.min_y() + px(4.0)).round(), w, px(34.0)))
            }
            None => {
                let w = (right - left_end).min(px(360.0));
                let x = (h.min_x() + (h.size.width - w) / 2.0).max(left_end).min(right - w);
                (w > px(140.0)).then(|| Rect::new(x.round(), (mid - px(17.0)).round(), w, px(34.0)))
            }
        };
        if let Some(r) = place {
            let mid = r.min_y() + r.size.height / 2.0;
            let inner = ds::search_field(layer, icons, &ds, r, self.editing_query);
            self.hits.push((r, Hit::Search));
            if !self.query.text().is_empty() {
                let c = ds::clear_button(
                    layer,
                    icons,
                    &ds,
                    r.max_x() - px(18.0),
                    mid,
                    self.hover == Some(Hit::ClearSearch),
                );
                self.hits.push((c, Hit::ClearSearch));
            }
            let field = Rect::new(
                inner.min_x(),
                r.min_y(),
                (inner.size.width - px(16.0)).max(0.0),
                r.size.height,
            );
            self.query.paint(
                scene.push_layer(Some(field)),
                text,
                field,
                self.editing_query,
                &t("studio.search"),
                &field_style(&ds, 0.0),
            );
        }
    }

    /// The footer's scope switch (only this project | everywhere) and its note, which keeps
    /// left of `limit` (the buttons): on one line, on two when narrower, gone when too narrow.
    pub(crate) fn paint_scope(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        ds: Ctx<'_>,
        footer: Rect,
        pad: f32,
        limit: f32,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let mid = footer.min_y() + footer.size.height / 2.0;
        let labels = [t("studio.scope_project"), t("studio.scope_global")];
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let width = ds::segmented_width(text, &ds, &refs);
        let selected = usize::from(self.global);
        let hovered = match &self.hover {
            Some(Hit::Scope(g)) => Some(usize::from(*g)),
            _ => None,
        };
        let mut segs = Vec::with_capacity(2);
        ds::segmented_with(
            layer,
            text,
            &ds,
            ds::SegmentedSpec {
                right: footer.min_x() + pad + width,
                mid,
                labels: &refs,
                selected,
                hovered,
            },
            |i, r| segs.push((r, Hit::Scope(i == 1))),
        );
        self.hits.extend(segs);
        let note_font = ds.ui_font(-2.0);
        let x = footer.min_x() + pad + width + px(14.0);
        let room = limit - px(16.0) - x;
        if room > px(120.0) {
            let lines = ds::wrap(text, &t("studio.saved_globally"), &note_font, room, 2);
            let line_h = px(17.0);
            let top = mid - line_h * lines.len() as f32 / 2.0;
            for (i, line) in lines.iter().enumerate() {
                let shaped = text.shape_line(line, &note_font);
                draw_text(
                    layer,
                    &shaped,
                    x,
                    ds::optical_baseline((top + line_h * i as f32).round(), line_h, &note_font),
                    p.text_muted,
                );
            }
        }
    }

    /// The footer's buttons, on the right: Discard (Close when nothing changed), then Save;
    /// returns where they start.
    pub(crate) fn paint_buttons(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        ds: Ctx<'_>,
        footer: Rect,
        pad: f32,
        changed: bool,
    ) -> f32 {
        let px = |v: f32| ds.px(v);
        let mid = footer.min_y() + footer.size.height / 2.0;
        let (save_label, discard_label) =
            (t("studio.save"), t(if changed { "studio.cancel" } else { "studio.close" }));
        let bh = px(ds::BUTTON_H);
        let sw = ds::primary_width(text, &ds, &[&save_label]);
        let dw = ds::primary_width(text, &ds, &[&t("studio.cancel"), &t("studio.close")]);
        let save = Rect::new(footer.max_x() - pad - sw, (mid - bh / 2.0).round(), sw, bh);
        let discard = Rect::new(save.min_x() - px(10.0) - dw, save.min_y(), dw, bh);
        ds::primary_button(
            layer,
            text,
            &ds,
            save,
            &save_label,
            true,
            self.hover == Some(Hit::Save),
        );
        ds::secondary_button(
            layer,
            text,
            &ds,
            discard,
            &discard_label,
            true,
            self.hover == Some(Hit::Discard),
        );
        self.hits.push((save, Hit::Save));
        self.hits.push((discard, Hit::Discard));
        discard.min_x()
    }

    /// The preview's pane: its title and switches (moving / still; dark / light for a theme
    /// with both), then the window in miniature.
    fn paint_preview_pane(
        &mut self,
        ctx: &mut ViewerPaint<'_>,
        area: Rect,
        input: &StudioInput,
        draft: &ThemeDraft,
    ) {
        let ds = &Ctx::new(ctx.palette, ctx.scale);
        let (scene, text, icons) = (&mut *ctx.scene, &mut *ctx.text, ctx.icons);
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let pad = px(24.0);
        let layer = scene.push_layer(Some(area));
        let font = ds.ui_font(-1.5).with_weight(Font::SEMIBOLD);
        let title = text.shape_line(&t("studio.preview").to_uppercase(), &font);
        let title_top = area.min_y() + px(22.0);
        draw_text(
            layer,
            &title,
            area.min_x() + pad,
            ds::optical_baseline(title_top, px(20.0), &font),
            p.text_muted,
        );
        // The switches, on a row under the title: moving / still, then dark / light.
        let mut head_mid = title_top + px(20.0) + px(26.0);
        let mut x = area.min_x() + pad;
        let mut segs: Vec<(Rect, Hit)> = Vec::new();
        let labels = [t("studio.preview_moving"), t("studio.preview_still")];
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let hovered = match &self.hover {
            Some(Hit::PreviewMoving(m)) => Some(usize::from(!*m)),
            _ => None,
        };
        let width = ds::segmented_width(text, ds, &refs);
        ds::segmented_with(
            layer,
            text,
            ds,
            ds::SegmentedSpec {
                right: x + width,
                mid: head_mid,
                labels: &refs,
                selected: usize::from(self.preview_still),
                hovered,
            },
            |i, r| segs.push((r, Hit::PreviewMoving(i == 0))),
        );
        x += width + px(10.0);
        if self.sibling(input).is_some() {
            let labels = [t("studio.preview_dark"), t("studio.preview_light")];
            let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
            let width = ds::segmented_width(text, ds, &refs);
            let hovered = match &self.hover {
                Some(Hit::PreviewLight(l)) => Some(usize::from(*l)),
                _ => None,
            };
            // Which variant shows: the draft's, or the other one.
            let light = rows::dark(draft) == self.preview_light;
            // No room beside: on a row of its own.
            if x + width > area.max_x() - pad {
                x = area.min_x() + pad;
                head_mid += px(42.0);
            }
            {
                ds::segmented_with(
                    layer,
                    text,
                    ds,
                    ds::SegmentedSpec {
                        right: x + width,
                        mid: head_mid,
                        labels: &refs,
                        selected: usize::from(light),
                        hovered,
                    },
                    |i, r| segs.push((r, Hit::PreviewLight(i == 1))),
                );
            }
        }
        self.hits.extend(segs);
        let window = Rect::new(
            area.min_x(),
            head_mid + px(4.0),
            area.size.width,
            area.max_y() - head_mid - px(4.0),
        );
        let shown = self.preview_draft(input, draft);
        let shown = shown.as_ref().unwrap_or(draft);
        self.sync_live(shown);
        let time = self.anim_time();
        let effects =
            self.live.as_mut().map(|l| PreviewEffects { painter: l.painter.as_mut(), time, icons });
        let backdrop = self.backdrop_shown;
        paint_preview(scene, text, ds, window, shown, effects, backdrop);
    }

    /// The other variant of the starting theme (`…-dark` / `…-light`), if there is one.
    pub(crate) fn sibling(&self, input: &StudioInput) -> Option<usize> {
        let id = &input.themes.get(self.base)?.id;
        let other = if let Some(stem) = id.strip_suffix("-dark") {
            format!("{stem}-light")
        } else {
            format!("{}-dark", id.strip_suffix("-light")?)
        };
        input.themes.iter().position(|th| th.id == other)
    }

    /// What the preview shows: the draft, or (the other variant asked for) that variant's
    /// colors with the draft's effects.
    fn preview_draft(&self, input: &StudioInput, draft: &ThemeDraft) -> Option<ThemeDraft> {
        if !self.preview_light {
            return None;
        }
        let other = self.thumbs.get(self.sibling(input)?)?.clone()?;
        let mut shown = other;
        shown.decorations = draft.decorations.clone();
        Some(shown)
    }
}
