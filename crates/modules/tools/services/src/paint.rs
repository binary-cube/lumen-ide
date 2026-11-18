//! The panel drawn: the filter and the tree's rows in view on the left (as the "Database"
//! window draws its own: the same rows, chevrons, guides, highlights and color marks), the
//! node's tabs and the active one's view, then the splitter over both; empty states where
//! there's nothing to show.

use icons::Icons;
use kernel::{Rect, Rgba};
use render_api::{Layer, Quad, Scene};
use text_layout::{Font, TextSystem};
use ui_components::ds::{self, Ctx, Loader, Tone};
use ui_components::tree::{self as tree_row, TreeList};
use ui_components::{ServiceState, SplitterStyle, ViewerPaint};
use ui_kit::{TextInputStyle, centered_baseline, draw_icon, draw_text};

use crate::panel::{
    FIELD_HEIGHT, FIELD_TOP, GAP, Hover, INDENT, LIST_TOP, PADDING, ROW_HEIGHT, ServicesPanel,
    TABS_HEIGHT, TabHit,
};
use crate::tree::Key;

impl ServicesPanel {
    pub(crate) fn paint_panel(&mut self, ctx: ViewerPaint<'_>) {
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let p = ctx.palette;
        let px = |v: f32| ds.px(v);
        let b = ctx.bounds;
        self.bounds = b;
        self.scale = ctx.scale;
        let base = ctx.scene.push_layer(Some(b));
        base.quad(Quad::filled(b, p.editor_background));
        if self.rows.is_empty() && self.filter.text().trim().is_empty() {
            // Nothing contributed yet: what shows here, said in the middle.
            self.tree_area = Rect::default();
            self.field = Rect::default();
            self.clear = None;
            self.list = Rect::default();
            self.tabs_bar = Rect::default();
            self.view_area = Rect::default();
            self.tab_hits.clear();
            self.splitter.set_bounds(Rect::default(), ctx.scale);
            paint_empty_hero(base, ctx.text, ctx.icons, &ds, b);
            return;
        }
        let tree_w = self.tree_px(b.size.width);
        self.tree_area = Rect::new(b.min_x(), b.min_y(), tree_w, b.size.height);
        let gap = px(GAP);
        self.splitter.set_style(SplitterStyle::from_palette(p));
        self.splitter.set_bounds(
            Rect::new(self.tree_area.max_x(), b.min_y(), gap, b.size.height),
            ctx.scale,
        );
        let right_x = self.tree_area.max_x() + gap;
        let right = Rect::new(right_x, b.min_y(), (b.max_x() - right_x).max(0.0), b.size.height);
        // The tree: the filter, its rows.
        let inset = px(PADDING);
        self.field = Rect::new(
            b.min_x() + inset,
            b.min_y() + px(FIELD_TOP),
            (tree_w - 2.0 * inset).max(0.0),
            px(FIELD_HEIGHT),
        );
        self.paint_field(base, ctx.text, ctx.icons, &ds, ctx.focused && !self.in_view_focus());
        let top = b.min_y() + px(LIST_TOP);
        self.list = Rect::new(b.min_x(), top, tree_w, (b.max_y() - top).max(0.0));
        if self.rows.is_empty() {
            let area = Rect::new(
                self.list.min_x(),
                self.list.min_y(),
                self.list.size.width,
                (self.list.size.height * 0.6).max(0.0),
            );
            ds::empty_state(base, ctx.text, &ds, area, &i18n::t("services.no_match"), None);
        } else {
            let h = px(ROW_HEIGHT);
            let content = self.rows.len() as f32 * h + px(12.0);
            self.scroll = self.scroll.clamp(0.0, (content - self.list.size.height).max(0.0));
            self.paint_rows(ctx.scene, ctx.text, ctx.icons, &ds);
            self.bar.paint(ctx.scene, p, ctx.scale, self.list, content, self.scroll);
        }
        // The node's tabs, the active one's view.
        let bar_h = px(TABS_HEIGHT);
        self.tabs_bar = Rect::new(right.min_x(), right.min_y(), right.size.width, bar_h);
        self.view_area = Rect::new(
            right.min_x(),
            right.min_y() + bar_h + ds.line(),
            right.size.width,
            (right.size.height - bar_h - ds.line()).max(0.0),
        );
        let layer = ctx.scene.push_layer(Some(right));
        self.paint_tabs(layer, ctx.text, ctx.icons, &ds);
        if self.tabs.is_empty() {
            self.view_area = Rect::default();
            let name = self
                .selected
                .and_then(|k| self.rows.iter().find(|r| r.key == k))
                .map(|r| r.label.clone());
            let (title, detail) = match name {
                Some(name) => (i18n::tr!("services.no_tabs", name = name.as_str()), None),
                None => (i18n::t("services.choose"), Some(i18n::t("services.choose_hint"))),
            };
            let area = Rect::new(
                right.min_x(),
                right.min_y(),
                right.size.width,
                (right.size.height * 0.85).max(0.0),
            );
            ds::empty_state(layer, ctx.text, &ds, area, &title, detail.as_deref());
            // Over both panes: its handle reaches into each.
            self.splitter.paint(ctx.scene);
            return;
        }
        let focused = ctx.focused && self.in_view_focus();
        let view_area = self.view_area;
        let (text, scene, palette, icons, waker) =
            (ctx.text, ctx.scene, ctx.palette, ctx.icons, ctx.waker);
        if let Some(view) = self.active_view() {
            view.paint(ViewerPaint {
                bounds: view_area,
                scale: ds.scale,
                text,
                scene: &mut *scene,
                palette,
                icons,
                focused,
                waker,
            });
        }
        // Over both panes: its handle reaches into each.
        self.splitter.paint(scene);
    }

    /// The filter: a pill with the magnifier, its text or placeholder, × with text.
    fn paint_field(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        icons: &Icons,
        ds: &Ctx<'_>,
        focused: bool,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let f = self.field;
        if f.size.width <= 0.0 {
            return;
        }
        let typing = self.typing && focused;
        let border = if typing { p.search_field_border_focused } else { p.search_field_border };
        layer.quad(
            Quad::filled(f, p.search_field_background)
                .rounded(f.size.height / 2.0)
                .bordered(ds.line(), border),
        );
        let icon = px(14.0);
        let icon_x = f.min_x() + px(12.0);
        if let Some(id) = icons.id("search") {
            let r =
                Rect::new(icon_x, (f.min_y() + (f.size.height - icon) / 2.0).round(), icon, icon);
            draw_icon(layer, icons, id, r, p.text_muted);
        }
        let mut right = f.max_x() - px(12.0);
        self.clear = None;
        if !self.filter.text().is_empty() {
            let s = px(18.0);
            let r = Rect::new(
                f.max_x() - px(7.0) - s,
                (f.min_y() + (f.size.height - s) / 2.0).round(),
                s,
                s,
            );
            let hovered = self.hover == Some(Hover::Clear);
            layer.quad(
                Quad::filled(r, p.text.with_alpha(if hovered { 0.12 } else { 0.06 }))
                    .rounded(s / 2.0),
            );
            if let Some(id) = icons.id("x") {
                let x = px(9.0);
                let ir = Rect::new(
                    (r.min_x() + (s - x) / 2.0).round(),
                    (r.min_y() + (s - x) / 2.0).round(),
                    x,
                    x,
                );
                draw_icon(layer, icons, id, ir, p.text_muted);
            }
            self.clear = Some(r);
            right = r.min_x() - px(6.0);
        }
        let text_x = icon_x + icon + px(8.0);
        let style = TextInputStyle {
            font: Font::new(p.ui_font.clone(), px(p.ui_font_size - 0.5)),
            background: Rgba::TRANSPARENT,
            border: Rgba::TRANSPARENT,
            border_focused: Rgba::TRANSPARENT,
            text: p.text,
            placeholder: p.text_muted,
            selection: p.selection,
            caret: p.caret,
            radius: 0.0,
            border_width: 0.0,
            padding: 0.0,
            caret_width: px(1.5).max(1.0),
            lead: 0.0,
        };
        let field = Rect::new(text_x, f.min_y(), (right - text_x).max(0.0), f.size.height);
        let placeholder = i18n::t("services.filter");
        self.filter.paint(layer, text, field, typing, &placeholder, &style);
    }

    /// The rows in view: highlights, then each row's guides, chevron, icon, name, detail and
    /// state (a spinner, an error's mark, an open transaction's badge).
    fn paint_rows(&self, scene: &mut Scene, text: &mut TextSystem, icons: &Icons, ds: &Ctx<'_>) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let b = self.list;
        let h = px(ROW_HEIGHT);
        let line = ds.line();
        let inset = px(8.0);
        let origin = b.min_y() + px(6.0) - self.scroll;
        let range = TreeList::new(h, self.rows.len()).visible(self.scroll, b.size.height);
        let pills = scene.push_layer(Some(b));
        for i in range.clone() {
            let top = origin + i as f32 * h;
            let pill = Rect::new(
                b.min_x() + inset,
                top + line,
                b.size.width - 2.0 * inset,
                h - 2.0 * line,
            );
            if self.selected == Some(self.rows[i].key) {
                tree_row::highlight(pills, pill, p.tree_selection, px(6.0));
            } else if self.hover == Some(Hover::Row(i)) {
                tree_row::highlight(pills, pill, p.tree_hover, px(6.0));
            }
        }
        let clip =
            Rect::new(b.min_x(), b.min_y(), (b.size.width - px(10.0)).max(0.0), b.size.height);
        let layer = scene.push_layer(Some(clip));
        let font = Font::new(p.ui_font.clone(), px(p.ui_font_size - 0.5));
        let small = ds.ui_font(-1.5);
        let metrics = text.metrics(&font);
        let indent = px(INDENT);
        let padding = px(PADDING);
        for i in range {
            let row = &self.rows[i];
            let top = origin + i as f32 * h;
            let mid = top + h / 2.0;
            let guide_x = |level: usize| b.min_x() + padding + level as f32 * indent + indent * 0.3;
            tree_row::guides(
                layer,
                1..usize::from(row.depth),
                guide_x,
                (top, h),
                line,
                p.indent_guide,
            );
            let x = b.min_x() + padding + f32::from(row.depth) * indent;
            let center = |size: f32| (top + (h - size) / 2.0).round();
            let chevron = px(12.0);
            if row.expandable {
                let at = Rect::new(x.round(), center(chevron), chevron, chevron);
                tree_row::chevron(layer, icons, at, row.expanded, p.text_muted);
            }
            let group = matches!(row.key, Key::Group(_));
            let mut color = if group { p.text_muted } else { p.accent };
            // A node's color: its icon in it, and a mark at the row's start.
            if let Some(mark) =
                row.color.as_deref().and_then(|c| ui_components::tag_colors::named(c, p))
            {
                color = mark;
                let bar =
                    Rect::new(b.min_x() + inset + px(2.0), top + px(7.0), px(3.0), h - px(14.0));
                layer.quad(Quad::filled(bar, mark).rounded(px(1.5)));
            }
            let size = px(15.0);
            let icon_rect = Rect::new((x + chevron + px(4.0)).round(), center(size), size, size);
            if let Some(id) = icons.id(row.icon).or_else(|| icons.id("file")) {
                draw_icon(layer, icons, id, icon_rect, color);
            }
            let baseline = centered_baseline(top, h, &metrics);
            let label_x = (icon_rect.max_x() + px(7.0)).round();
            let mut end = clip.max_x() - px(10.0);
            // The state at the end.
            match row.state {
                ServiceState::Running => {
                    let s = 13.0;
                    let at = end - px(s);
                    ds::loader(layer, text, ds, at, mid, &Loader { size: s, ..Loader::default() });
                    end = at - px(8.0);
                }
                ServiceState::Error => {
                    let s = px(14.0);
                    if let Some(id) = icons.id("circle-alert") {
                        draw_icon(layer, icons, id, Rect::new(end - s, center(s), s, s), p.danger);
                    }
                    end -= s + px(8.0);
                }
                ServiceState::TxOpen => {
                    let spec = ds::BadgeSpec {
                        label: &i18n::t("services.tx"),
                        x: end,
                        mid,
                        align_right: true,
                        tone: Tone::Accent,
                    };
                    let r = ds::badge(layer, text, ds, spec);
                    end = r.min_x() - px(8.0);
                }
                ServiceState::Idle => {}
            }
            let label_font = if group || row.depth == 1 {
                font.clone().with_weight(Font::SEMIBOLD)
            } else {
                font.clone()
            };
            let label_color = if group { p.text_muted } else { p.text };
            let room = (end - label_x).max(0.0);
            let fitted = ds::ellipsize(text, &row.label, &label_font, room);
            let shaped = text.shape_line(&fitted, &label_font);
            draw_text(layer, &shaped, label_x, baseline, label_color);
            if !row.detail.is_empty() && fitted == row.label {
                let x = label_x + shaped.width + px(8.0);
                let room = end - x;
                if room > px(24.0) {
                    let detail = ds::ellipsize(text, &row.detail, &small, room);
                    let shaped = text.shape_line(&detail, &small);
                    draw_text(layer, &shaped, x.round(), baseline, p.text_muted);
                }
            }
        }
    }

    /// The node's tabs as pills (the active one in the accent; × on it and under the mouse,
    /// a pin on the pinned ones), and the line under them.
    fn paint_tabs(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        icons: &Icons,
        ds: &Ctx<'_>,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let bar = self.tabs_bar;
        let mid = bar.min_y() + bar.size.height / 2.0;
        self.tab_hits.clear();
        layer.quad(Quad::filled(
            Rect::new(bar.min_x(), bar.max_y(), bar.size.width, ds.line()),
            p.card_border,
        ));
        let font = ds.ui_font(-0.5);
        let active = self.active_tab();
        let mut x = bar.min_x() + px(6.0);
        let tabs = self.tabs.clone();
        for (i, tab) in tabs.iter().enumerate() {
            let shaped = text.shape_line(&tab.title, &font);
            let icon = px(14.0);
            let close = px(14.0);
            let ends = tab.pinned || tab.closable;
            let w = px(10.0)
                + icon
                + px(6.0)
                + shaped.width
                + if ends { px(8.0) + close } else { 0.0 }
                + px(8.0);
            let pill = Rect::new(x, (mid - px(12.0)).round(), w.round(), px(24.0));
            if pill.min_x() > bar.max_x() {
                break;
            }
            let on = active == Some(tab.id);
            let hovered =
                matches!(self.hover, Some(Hover::Tab(TabHit::Tab(h) | TabHit::Close(h))) if h == i);
            let fill = if on {
                p.accent.with_alpha(0.14)
            } else if hovered {
                p.text.with_alpha(0.06)
            } else {
                Rgba::TRANSPARENT
            };
            layer.quad(Quad::filled(pill, fill).rounded(px(ds::radius::MD)));
            let color = if on { p.accent } else { p.text };
            let ix = pill.min_x() + px(10.0);
            if let Some(id) = icons.id(tab.icon) {
                let r = Rect::new(ix, (mid - icon / 2.0).round(), icon, icon);
                draw_icon(layer, icons, id, r, if on { p.accent } else { p.text_muted });
            }
            let tx = ix + icon + px(6.0);
            draw_text(
                layer,
                &shaped,
                tx,
                ds::optical_baseline(pill.min_y(), pill.size.height, &font),
                color,
            );
            self.tab_hits.push((pill, TabHit::Tab(i)));
            let close_rect = Rect::new(
                pill.max_x() - close - px(6.0),
                (mid - close / 2.0).round(),
                close,
                close,
            );
            let inner = Rect::new(
                close_rect.min_x() + px(2.0),
                close_rect.min_y() + px(2.0),
                close - px(4.0),
                close - px(4.0),
            );
            if tab.pinned {
                if let Some(id) = icons.id("pin") {
                    draw_icon(layer, icons, id, inner, p.text_muted);
                }
            } else if tab.closable && (on || hovered) {
                let lit = self.hover == Some(Hover::Tab(TabHit::Close(i)));
                if lit {
                    layer.quad(
                        Quad::filled(close_rect, p.text.with_alpha(0.1)).rounded(close / 2.0),
                    );
                }
                if let Some(id) = icons.id("x") {
                    draw_icon(layer, icons, id, inner, if lit { p.text } else { p.text_muted });
                }
                self.tab_hits.push((close_rect, TabHit::Close(i)));
            }
            x = pill.max_x() + px(4.0);
        }
    }

    /// The keys are the view's (clicked into).
    pub(crate) fn in_view_focus(&self) -> bool {
        self.in_view
    }

    /// What a row's state mark means, under the mouse.
    pub(crate) fn state_tip(&self, p: kernel::Point) -> Option<ui_components::tooltip::Tip> {
        let i = self.row_at(p)?;
        let row = &self.rows[i];
        let key = match row.state {
            ServiceState::Running => "services.running",
            ServiceState::Error => "services.failed",
            ServiceState::TxOpen => "services.tx_open",
            ServiceState::Idle => return None,
        };
        let px = |v: f32| (v * self.scale).round();
        let h = px(ROW_HEIGHT);
        let top = self.list.min_y() + px(6.0) - self.scroll + i as f32 * h;
        let end = self.list.max_x() - px(20.0);
        let anchor = Rect::new(end - px(40.0), top, px(40.0), h);
        anchor.contains(p).then(|| ui_components::tooltip::Tip::new(anchor, i18n::t(key)))
    }
}

/// Nothing contributed: the window's icon, what shows here and where it comes from.
fn paint_empty_hero(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &Icons,
    ds: &Ctx<'_>,
    b: Rect,
) {
    let p = ds.palette;
    let px = |v: f32| ds.px(v);
    let size = px(40.0);
    let circle = px(64.0);
    let cy = b.min_y() + b.size.height * 0.42;
    let ring = Rect::new(
        (b.min_x() + (b.size.width - circle) / 2.0).round(),
        (cy - circle / 2.0 - px(18.0)).round(),
        circle,
        circle,
    );
    layer.quad(Quad::filled(ring, p.accent.with_alpha(0.1)).rounded(circle / 2.0));
    if let Some(id) = icons.id("services") {
        let r = Rect::new(
            (ring.min_x() + (circle - size) / 2.0).round(),
            (ring.min_y() + (circle - size) / 2.0).round(),
            size,
            size,
        );
        draw_icon(layer, icons, id, r, p.accent);
    }
    let area = Rect::new(b.min_x(), ring.max_y() + px(10.0), b.size.width, px(60.0));
    ds::empty_state(
        layer,
        text,
        ds,
        area,
        &i18n::t("services.empty"),
        Some(&i18n::t("services.empty_hint")),
    );
}
