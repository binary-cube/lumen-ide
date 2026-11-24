//! The diagram painted: the cards in view and the edges between them, the controls in its
//! corner, the legend.

use super::card::{Look, paint_card, paint_folded};
use super::*;

impl GraphView {
    pub fn paint(&mut self, ctx: GraphPaint<'_, '_>) {
        let GraphPaint {
            area,
            scale,
            text,
            scene,
            palette: p,
            icons,
            report,
            findings,
            table,
            row_of,
            selected,
        } = ctx;
        let ds = Ctx::new(p, scale);
        self.area = area;
        self.view.bounds = area;
        self.view.scale = scale;
        self.controls.clear();
        self.tips.clear();
        self.worst = tree::worst_of(report, findings);
        self.lay_out(report, table, row_of);
        let base = scene.push_layer(Some(area));
        base.quad(Quad::filled(area, p.editor_background));
        let Some(laid) = self.laid().cloned() else { return };
        if laid.cards.is_empty() {
            ds::empty_state(base, text, &ds, area, &i18n::t("console.plan_empty"), None);
            return;
        }
        // Fitted once per profile and flow; the chosen step followed into view.
        if self.fitted != Some((report.profile.id, self.flow)) {
            let focus = selected.or_else(|| report.hot_path.last().copied());
            self.fit(focus);
            self.fitted = Some((report.profile.id, self.flow));
            self.last_selected = selected;
        } else if selected != self.last_selected {
            self.last_selected = selected;
            if let Some(r) = selected.and_then(|s| self.card_rect_of(s)) {
                let on = self.view.rect_to_screen(r);
                if !diagram::contains_rect(self.view.visible(), on) {
                    self.view.center_on(r);
                }
            }
        }
        let k = self.view.k();
        let zoom = self.view.zoom;
        // A dot grid: the canvas's surface, the same at any zoom.
        let origin = self.view.to_screen(Point::new(0.0, 0.0));
        diagram::dot_grid(base, p, area, origin, self.view.scale);
        let view_rect = Rect::new(
            self.view.to_diagram(Point::new(area.min_x(), area.min_y())).x,
            self.view.to_diagram(Point::new(area.min_x(), area.min_y())).y,
            area.size.width / k,
            area.size.height / k,
        );
        let filter = !table.filter().is_empty();
        let matches =
            |id: NodeId| !filter || row_of.get(id.index()).is_some_and(|&r| table.matches(r));
        // The edges first, under the cards: the thin ones, then the thick, the hot path last.
        let edges_layer = scene.push_layer(Some(area));
        let mut order: Vec<usize> = (0..laid.edges.len()).collect();
        order.sort_by(|&a, &b| {
            (laid.info[a].hot, laid.info[a].width)
                .partial_cmp(&(laid.info[b].hot, laid.info[b].width))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for &i in &order {
            let e = &laid.edges[i];
            if e.bounds().intersect(&diagram::inflate(view_rect, 20.0)).is_none()
                && !rect_holds(view_rect, e.bounds())
            {
                continue;
            }
            let info = &laid.info[i];
            let dim = match laid.cards[e.kind.0].item {
                Item::Step(c) => !matches(c),
                Item::Folded { .. } => false,
            };
            let mut color = if info.hot { p.accent } else { p.text_muted.with_alpha(0.55) };
            if dim {
                color = color.with_alpha(color.a * 0.35);
            }
            let style = EdgeStyle {
                color,
                dashed: matches!(laid.cards[e.kind.0].item, Item::Folded { .. }),
                head: Head::Arrow { hollow: None },
                width: info.width,
            };
            let points: Vec<Point> = e.points.iter().map(|&pt| self.view.to_screen(pt)).collect();
            diagram::edge(edges_layer, &points, e.dir, k, style);
        }
        // The labels of the rows, on a pill so they read over the lines.
        let label_font =
            Font::new(p.ui_font.clone(), ((p.ui_font_size - 2.5) * k).round().max(6.0));
        if zoom >= 0.4 {
            for info in &laid.info {
                let (Some(label), Some(at)) = (&info.label, info.label_at) else { continue };
                if at.intersect(&view_rect).is_none() {
                    continue;
                }
                let r = self.view.rect_to_screen(at);
                let color = if info.hot { p.accent } else { p.text_muted };
                // Its sizes in the diagram's units, zoomed with it.
                let pill = ds::Badge::new(label)
                    .color(color)
                    .fill(p.editor_background)
                    .border(color.with_alpha(EDGE_PILL_BORDER))
                    .font(&label_font)
                    .size(ds::BadgeSize::Custom(at.size.height))
                    .pad(PILL_PAD)
                    .zoom(k / ds.scale);
                let w = pill.width(text, &ds);
                let left = r.min_x() + (r.size.width - w) / 2.0;
                pill.paint(edges_layer, text, &ds, left, r.min_y() + r.size.height / 2.0);
            }
        }
        // The cards in view.
        let cards_layer = scene.push_layer(Some(area));
        let fonts = Fonts::new(p, k);
        for (ci, card) in laid.cards.iter().enumerate() {
            if card.rect.intersect(&view_rect).is_none() {
                continue;
            }
            let r = self.view.rect_to_screen(card.rect);
            let hovered = self.hover == Some(GraphHit::Card(ci));
            match card.item {
                Item::Folded { under, count } => {
                    paint_folded(
                        &mut ui_components::LayerPainter::new(cards_layer, text, icons, ds),
                        &fonts,
                        (r, k),
                        (report, under),
                        count,
                        hovered,
                    );
                    self.tips.push((r, i18n::t("profile.view.graph_open_folded")));
                }
                Item::Step(id) => {
                    let finding = self
                        .worst
                        .get(id.index())
                        .copied()
                        .flatten()
                        .and_then(|f| findings.get(f).copied());
                    let look = Look {
                        selected: selected == Some(id),
                        hovered,
                        hot: report.on_hot_path(id),
                        dim: !matches(id),
                        hot_spot: hot_spot(findings) == Some(id),
                    };
                    paint_card(
                        &mut ui_components::LayerPainter::new(cards_layer, text, icons, ds),
                        &fonts,
                        (r, k),
                        (report, id),
                        finding,
                        look,
                    );
                    self.tips.push((r, card_tip(report, id, finding)));
                }
            }
        }
        // The controls, floating in the corner: zoom, fit, center, the flow.
        let controls = scene.push_layer(Some(area));
        self.paint_controls(controls, text, icons, &ds, area, selected.is_some());
        self.paint_legend(controls, text, &ds, area);
    }

    fn paint_controls(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        area: Rect,
        can_center: bool,
    ) {
        let px = |v: f32| ds.px(v);
        let h = px(30.0);
        let font = ds.ui_font(-1.5);
        let pct = format!("{} %", (self.view.zoom * 100.0).round() as i64);
        let pct_w = text.shape_line("200 %", &font).width + px(14.0);
        let b = px(28.0);
        let w = b * 5.0 + pct_w + px(4.0) * 2.0 + px(10.0);
        let r = Rect::new(
            (area.max_x() - w - px(12.0)).round(),
            (area.max_y() - h - px(12.0)).round(),
            w.round(),
            h,
        );
        if r.min_x() < area.min_x() + px(8.0) || r.min_y() < area.min_y() + px(8.0) {
            return;
        }
        ds::surface(layer, ds, r, ds::Elevation::Low, px(radius::MD));
        let mut x = r.min_x() + px(3.0);
        let flow_icon = match self.flow {
            Flow::Down => "arrow-down",
            Flow::Right => "arrow-right",
        };
        let flow_tip = match self.flow {
            Flow::Down => i18n::t("profile.view.graph_flow_right"),
            Flow::Right => i18n::t("profile.view.graph_flow_down"),
        };
        let items: [(GraphHit, Option<&str>, String); 6] = [
            (GraphHit::ZoomOut, Some("minus"), i18n::t("profile.view.graph_zoom_out")),
            (GraphHit::Zoom, None, i18n::t("profile.view.graph_zoom_reset")),
            (GraphHit::ZoomIn, Some("plus"), i18n::t("profile.view.graph_zoom_in")),
            (GraphHit::Fit, Some("maximize-2"), i18n::t("profile.view.graph_fit")),
            (GraphHit::Center, Some("crosshair"), i18n::t("profile.view.graph_center")),
            (GraphHit::Flow, Some(flow_icon), flow_tip),
        ];
        for (i, (hit, icon, tip)) in items.into_iter().enumerate() {
            if i == 3 {
                ds::divider_vertical(
                    layer,
                    ds,
                    (x + px(2.0)).round(),
                    r.min_y() + px(7.0),
                    r.max_y() - px(7.0),
                );
                x += px(4.0);
            }
            let bw = if icon.is_none() { pct_w } else { b };
            let br = Rect::new(x, r.min_y() + px(2.0), bw, h - px(4.0));
            let enabled = hit != GraphHit::Center || can_center;
            let spec = ButtonSpec::new(ButtonKind::Ghost)
                .enabled(enabled)
                .hovered(self.hover == Some(hit));
            // The zoom's figure (a reset) reads as a label, muted; the rest are icons.
            let spec = match icon {
                Some(icon) => spec.icon(icon),
                None => spec.label(&pct).tone(Tone::Neutral),
            };
            ds::button(layer, text, icons, ds, br, spec);
            self.controls.push((br, hit, tip));
            x += bw;
        }
    }

    /// What the lines and colors mean, in the bottom left corner (when there's room).
    fn paint_legend(&mut self, layer: &mut Layer, text: &mut TextSystem, ds: &Ctx<'_>, area: Rect) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let font = ds.ui_font(-2.0);
        let rows = text.shape_line(&i18n::t("profile.view.graph_legend_rows"), &font);
        let hot = text.shape_line(&i18n::t("profile.view.graph_legend_hot"), &font);
        let heat = text.shape_line(&i18n::t("profile.view.graph_legend_heat"), &font);
        let sample = px(26.0);
        let ramp = px(44.0);
        let w = rows.width + hot.width + heat.width + sample * 2.0 + ramp + px(60.0);
        let controls_w = self.controls.first().map_or(0.0, |(r, ..)| area.max_x() - r.min_x());
        if w + controls_w + px(40.0) > area.size.width {
            return;
        }
        let h = px(26.0);
        let r = Rect::new(
            (area.min_x() + px(12.0)).round(),
            (area.max_y() - h - px(14.0)).round(),
            w.round(),
            h,
        );
        layer.quad(
            Quad::filled(r, p.editor_background.with_alpha(0.92))
                .rounded(px(radius::MD))
                .bordered(ds.line(), p.card_border),
        );
        let mid = (r.min_y() + h / 2.0).round();
        let base = ds::optical_baseline(r.min_y(), h, &font);
        let mut x = r.min_x() + px(10.0);
        // Thin to thick: the rows.
        for (i, t) in [1.0f32, 2.5, 4.5].iter().enumerate() {
            let tw = px(*t).max(1.0);
            layer.quad(Quad::filled(
                Rect::new(
                    (x + i as f32 * px(9.0)).round(),
                    (mid - tw / 2.0).round(),
                    px(7.0),
                    tw.round().max(1.0),
                ),
                p.text_muted,
            ));
        }
        x += sample + px(4.0);
        draw_text(layer, &rows, x.round(), base, p.text_muted);
        x += rows.width + px(12.0);
        let tw = px(2.5).round();
        layer.quad(Quad::filled(
            Rect::new(x.round(), (mid - tw / 2.0).round(), sample, tw),
            p.accent,
        ));
        x += sample + px(6.0);
        draw_text(layer, &hot, x.round(), base, p.text_muted);
        x += hot.width + px(12.0);
        let steps = 10;
        for i in 0..steps {
            let t = i as f32 / (steps - 1) as f32;
            let sw = ramp / steps as f32;
            layer.quad(Quad::filled(
                Rect::new((x + sw * i as f32).round(), mid - px(4.0), sw.ceil(), px(8.0)),
                p.heat(t),
            ));
        }
        x += ramp + px(6.0);
        draw_text(layer, &heat, x.round(), base, p.text_muted);
    }
}
