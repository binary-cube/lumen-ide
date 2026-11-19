//! Painting a tool window: its header, tabs and buttons.

use super::*;

impl Workbench {
    /// The window's card: each group's header (the window's name on the first, the views
    /// as tabs, "+", hide on the top right one) and its view, the separators between them.
    pub(in crate::workbench) fn paint_tool_window(
        &mut self,
        text: &mut TextSystem,
        scene: &mut Scene,
    ) {
        self.paint_tool_header(text, scene);
        let b = self.tools.bottom_bounds;
        let palette = self.palette.clone();
        let waker = self.waker.as_ref().map(|w| Arc::clone(&w.0));
        let icons = self.icons.clone();
        let scale = self.scale;
        let focused = self.focus == Focus::Panel && !self.keyboard_in_overlay();
        let Some(window) = self.tools.window() else { return };
        let places: Vec<Rect> =
            window.groups.iter().map(|g| self.tool_group_view_bounds(g.bounds)).collect();
        let current = window.current;
        let top = places.iter().map(|r| r.min_y()).fold(f32::MAX, f32::min);
        for (g, bounds) in places.into_iter().enumerate() {
            let Some(window) = self.tools.window_mut() else { return };
            if let Some(view) = window.groups[g].active_view_mut() {
                view.viewer.paint(ui_workbench::ViewerPaint {
                    bounds,
                    scale,
                    text,
                    scene,
                    palette: &palette,
                    icons: &icons,
                    focused: focused && g == current,
                    waker: waker.clone(),
                });
            }
        }
        // What the views drew past the card's rounded corners cut back.
        self.round_card_corners(scene, b, top.min(b.max_y()));
    }

    /// A tab dragged out of its strip: where it would go (an insertion line among another
    /// group's tabs, the part of a view it would take) and its pill by the pointer.
    pub(in crate::workbench) fn paint_tool_tab_drop(
        &self,
        text: &mut TextSystem,
        scene: &mut Scene,
    ) {
        let Some(drag) =
            self.tools.tab_drag.as_ref().filter(|d| d.state == TabDragState::Moving && !d.in_strip)
        else {
            return;
        };
        let Some(window) = self.tools.window() else { return };
        let Some(view) = window.views().find(|v| v.id == drag.view) else { return };
        let palette = &self.palette;
        {
            let layer = scene.push_layer(None);
            match drag.target {
                Some(ToolDrop::Strip { marker, .. }) => {
                    layer.quad(
                        Quad::filled(marker, palette.accent).rounded(marker.size.width / 2.0),
                    );
                }
                Some(ToolDrop::Into { zone, .. } | ToolDrop::Edge { zone, .. }) => {
                    super::splits::paint_drop_zone(layer, zone, self.scale, palette);
                }
                None => {}
            }
        }
        let icon = self
            .tools
            .bottom
            .as_deref()
            .and_then(|id| self.tool(id))
            .and_then(|t| self.icons.id(t.icon()))
            .map(|icon| (icon, palette.accent));
        let title = view.viewer.live_title().unwrap_or(&view.title);
        self.paint_drag_pill(text, scene, title, icon, drag.at);
    }

    /// The window's card and its groups' headers (the name, the views as tabs, the
    /// buttons) and separators, their places kept for the clicks.
    pub(in crate::workbench) fn paint_tool_header(
        &mut self,
        text: &mut TextSystem,
        scene: &mut Scene,
    ) {
        let b = self.tools.bottom_bounds;
        let header_h = self.tool_header_height();
        let palette = self.palette.clone();
        let ctx = ds::Ctx::new(&palette, self.scale);
        let id = self.tools.bottom.clone().unwrap_or_default();
        let mut hits = Vec::new();
        {
            let layer = scene.push_layer(Some(b));
            // Effects shown behind the panels: the card lets them through, faintly.
            let glass = self.chrome.decorations.glass.map_or(1.0, |g| g.panels);
            layer.quad(
                Quad::filled(b, palette.editor_background.with_alpha(glass))
                    .rounded(ctx.px(radius::CARD))
                    .bordered(ctx.line(), palette.card_border),
            );
            let groups: Vec<Rect> = self
                .tools
                .window()
                .map(|w| w.groups.iter().map(|g| g.bounds).collect())
                .unwrap_or_default();
            // Hide: on the top right group's header.
            let hide = groups
                .iter()
                .enumerate()
                .filter(|(_, r)| r.min_y() <= b.min_y() + 0.5)
                .max_by(|(_, a), (_, r)| a.max_x().total_cmp(&r.max_x()))
                .map_or(0, |(g, _)| g);
            for (g, bounds) in groups.into_iter().enumerate() {
                let header = ToolHeader {
                    ctx,
                    bounds,
                    height: header_h,
                    mid: bounds.min_y() + header_h / 2.0,
                    hover: self.tools.hover,
                };
                let x = if g == 0 {
                    self.paint_tool_title(layer, text, &header, &id)
                } else {
                    bounds.min_x() + ctx.px(8.0)
                };
                let x = self.paint_tool_tabs(layer, text, &header, g, x, &mut hits);
                self.paint_tool_buttons(layer, &header, g, x, g == hide, &mut hits);
            }
        }
        if let Some(window) = self.tools.window() {
            window.split.paint(scene);
        }
        self.tools.hits = hits;
    }

    /// The header's title and icon, as registered (the icon in the accent). Returns where
    /// the views' tabs start.
    pub(super) fn paint_tool_title(
        &self,
        layer: &mut render_api::Layer,
        text: &mut TextSystem,
        header: &ToolHeader,
        id: &str,
    ) -> f32 {
        let (ctx, b, palette) = (header.ctx, header.bounds, header.ctx.palette);
        let px = |v: f32| ctx.px(v);
        let strong = ctx.ui_font(-0.5).with_weight(text_layout::Font::SEMIBOLD);
        let tool = self.tool(id);
        let name = tool.map(|t| t.title()).unwrap_or_default();
        let shaped = text.shape_line(&name, &strong);
        let mut x = b.min_x() + px(14.0);
        if let Some(icon) = tool.and_then(|t| self.icons.id(t.icon())) {
            let s = px(15.0);
            let at = Rect::new(x, (header.mid - s / 2.0).round(), s, s);
            draw_icon(layer, &self.icons, icon, at, palette.accent);
            x += s + px(8.0);
        }
        draw_text(
            layer,
            &shaped,
            x,
            ds::optical_baseline(b.min_y(), header.height, &strong),
            palette.text,
        );
        x + shaped.width + px(12.0)
    }

    /// Group `g`'s views as tabs from `x` (the active one in the accent — softer in a group
    /// that isn't the current one; × on it and under the mouse, a pin on the pinned ones). A
    /// tab being dragged leaves its place empty and is drawn under the pointer, over the
    /// others. Returns where they end.
    pub(super) fn paint_tool_tabs(
        &self,
        layer: &mut render_api::Layer,
        text: &mut TextSystem,
        header: &ToolHeader,
        g: usize,
        mut x: f32,
        hits: &mut Vec<(Rect, ToolHit)>,
    ) -> f32 {
        let Some(window) = self.tools.window() else { return x };
        let Some(group) = window.groups.get(g) else { return x };
        let lit = g == window.current || window.groups.len() == 1;
        let dragged = self
            .tools
            .tab_drag
            .as_ref()
            .filter(|d| d.state == TabDragState::Moving && d.in_strip && d.group == g)
            .and_then(|d| Some((group.views.iter().position(|v| v.id == d.view)?, d)));
        let gap = header.ctx.px(4.0);
        let pill = |x: f32, width: f32| {
            Rect::new(x, header.mid - header.ctx.px(12.0), width, header.ctx.px(24.0))
        };
        for i in 0..group.views.len() {
            if let Some((at, drag)) = dragged
                && at == i
            {
                // Its place, kept empty (its width as when pressed).
                let slot = pill(x, drag.widths.get(at).copied().unwrap_or(0.0));
                hits.push((slot, ToolHit::View(g, i)));
                x = slot.max_x() + gap;
                continue;
            }
            let tab = self.paint_tool_tab(layer, text, header, (g, group, lit), (i, x), Some(hits));
            x = tab.max_x() + gap;
        }
        if let Some((at, drag)) = dragged {
            let left = drag.floating_left(at);
            let lifted = pill(left, drag.widths.get(at).copied().unwrap_or(0.0));
            ds::surface(layer, &header.ctx, lifted, ds::Elevation::Low, radius::MD);
            self.paint_tool_tab(layer, text, header, (g, group, lit), (at, left), None);
        }
        x
    }

    /// Tab `i` of group `g` from `x` (`lit`: its group is the current one); its hits
    /// (itself, its ×) added to `hits` when given. Returns its pill.
    pub(super) fn paint_tool_tab(
        &self,
        layer: &mut render_api::Layer,
        text: &mut TextSystem,
        header: &ToolHeader,
        (g, group, lit): (usize, &ToolGroup, bool),
        (i, x): (usize, f32),
        mut hits: Option<&mut Vec<(Rect, ToolHit)>>,
    ) -> Rect {
        let (ctx, palette, mid, hover) = (header.ctx, header.ctx.palette, header.mid, header.hover);
        let px = |v: f32| ctx.px(v);
        let font = ctx.ui_font(-0.5);
        let view = &group.views[i];
        // The name it goes by now (a terminal's program), else the one it was opened with.
        let title = view.viewer.live_title().unwrap_or(&view.title);
        let shaped = text.shape_line(title, &font);
        let close = px(14.0);
        // A fixed tab (neither pinned nor closable) has no room for a × (nor a pin).
        let slot = view.pinned || view.closable();
        let w = shaped.width + px(20.0) + if slot { close + px(4.0) } else { 0.0 };
        let pill = Rect::new(x, mid - px(12.0), w, px(24.0));
        let on = i == group.active;
        let fill = if on && lit {
            palette.accent.with_alpha(0.14)
        } else if on {
            palette.text.with_alpha(0.08)
        } else if hover == Some(ToolHit::View(g, i)) {
            palette.text.with_alpha(0.06)
        } else {
            Rgba::TRANSPARENT
        };
        layer.quad(Quad::filled(pill, fill).rounded(px(radius::MD)));
        let color = if on && lit { palette.accent } else { palette.text };
        draw_text(
            layer,
            &shaped,
            x + px(10.0),
            ds::optical_baseline(pill.min_y(), pill.size.height, &font),
            color,
        );
        if let Some(hits) = hits.as_deref_mut() {
            hits.push((pill, ToolHit::View(g, i)));
        }
        // ×: on the active one and under the mouse.
        let close_rect = Rect::new(pill.max_x() - close - px(6.0), mid - close / 2.0, close, close);
        let inner = Rect::new(
            close_rect.min_x() + px(2.0),
            close_rect.min_y() + px(2.0),
            close - px(4.0),
            close - px(4.0),
        );
        // A pinned one shows a pin instead (no stray click closes it; its menu does).
        if view.pinned
            && let Some(icon) = self.icons.id("pin")
        {
            draw_icon(layer, &self.icons, icon, inner, palette.text_muted);
        }
        let hovered = matches!(hover, Some(ToolHit::View(hg, h) | ToolHit::CloseView(hg, h)) if hg == g && h == i);
        let shows_close = !view.pinned && view.closable() && (on || hovered);
        if shows_close && let Some(icon) = self.icons.id("x") {
            let lit = hover == Some(ToolHit::CloseView(g, i));
            if lit {
                layer.quad(
                    Quad::filled(close_rect, palette.text.with_alpha(0.1)).rounded(close / 2.0),
                );
            }
            let color = if lit { palette.text } else { palette.text_muted };
            draw_icon(layer, &self.icons, icon, inner, color);
            if let Some(hits) = hits {
                hits.push((close_rect, ToolHit::CloseView(g, i)));
            }
        }
        pill
    }

    /// A group's header buttons: "+" after its tabs (from `x`), if the window makes views
    /// (the terminal: a shell); on the right (`hide`: this group's header has them) half the
    /// height, all of it, the window's options ("⋮"), hide; then the line under the header.
    pub(super) fn paint_tool_buttons(
        &self,
        layer: &mut render_api::Layer,
        header: &ToolHeader,
        g: usize,
        x: f32,
        hide: bool,
        hits: &mut Vec<(Rect, ToolHit)>,
    ) {
        let (ctx, b, palette, mid) = (header.ctx, header.bounds, header.ctx.palette, header.mid);
        let px = |v: f32| ctx.px(v);
        // A button: its hover background (lit while it's in effect) and its icon, inset.
        let lit_button =
            |layer: &mut render_api::Layer, at: Rect, hit: ToolHit, icon: &str, on: bool| {
                if on {
                    layer.quad(
                        Quad::filled(at, palette.accent.with_alpha(0.14)).rounded(px(radius::MD)),
                    );
                } else if header.hover == Some(hit) {
                    layer.quad(
                        Quad::filled(at, palette.text.with_alpha(0.08)).rounded(px(radius::MD)),
                    );
                }
                if let Some(icon) = self.icons.id(icon) {
                    let inner = Rect::new(
                        at.min_x() + px(5.0),
                        at.min_y() + px(5.0),
                        at.size.width - px(10.0),
                        at.size.height - px(10.0),
                    );
                    let color = if on { palette.accent } else { palette.text_muted };
                    draw_icon(layer, &self.icons, icon, inner, color);
                }
            };
        let button = |layer: &mut render_api::Layer, at: Rect, hit: ToolHit, icon: &str| {
            lit_button(layer, at, hit, icon, false);
        };
        let size = px(24.0);
        if self.tool_new_command().is_some() {
            let plus = Rect::new(x + px(2.0), mid - size / 2.0, size, size);
            button(layer, plus, ToolHit::New(g), "plus");
            hits.push((plus, ToolHit::New(g)));
        }
        if hide {
            let at = Rect::new(b.max_x() - px(10.0) - size, mid - size / 2.0, size, size);
            button(layer, at, ToolHit::Hide, "minus");
            hits.push((at, ToolHit::Hide));
            // Before it: the window's options, all the height (its icon says which way a
            // press goes), half of it.
            let more = Rect::new(at.min_x() - px(4.0) - size, at.min_y(), size, size);
            button(layer, more, ToolHit::More, "more-vertical");
            hits.push((more, ToolHit::More));
            let now = self.tools.bottom_size;
            let full = Rect::new(more.min_x() - px(4.0) - size, at.min_y(), size, size);
            let icon = if now == BottomSize::Full { "minimize-2" } else { "maximize-2" };
            lit_button(layer, full, ToolHit::Full, icon, false);
            hits.push((full, ToolHit::Full));
            let half = Rect::new(full.min_x() - px(4.0) - size, at.min_y(), size, size);
            lit_button(layer, half, ToolHit::Half, "view-split-down", now == BottomSize::Half);
            hits.push((half, ToolHit::Half));
        }
        layer.quad(Quad::filled(
            Rect::new(b.min_x(), b.min_y() + header.height, b.size.width, ctx.line()),
            palette.card_border,
        ));
    }
}
