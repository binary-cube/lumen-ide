//! The "Jump to file or function" field (top right) and its results, painted.

use super::*;

impl Canvas {
    /// The "Jump to file or function" field (top right) and its results.
    pub(crate) fn paint_search(
        &mut self,
        graph: &DepGraph,
        ds: &Ctx<'_>,
        text: &mut TextSystem,
        scene: &mut render_api::Scene,
        icons: &icons::Icons,
    ) {
        let px = |v: f32| ds.px(v);
        let b = self.bounds;
        let w = px(340.0).min(b.size.width - px(32.0));
        let field = Rect::new(b.max_x() - w - px(18.0), b.min_y() + px(16.0), w, px(40.0));
        let layer = scene.push_layer(Some(b));
        self.paint_search_field(Pen { layer, text, ds }, icons, field);
        if self.search.as_ref().is_some_and(|s| !s.query.trim().is_empty()) {
            let layer = scene.push_layer(Some(b));
            self.paint_search_results(Pen { layer, text, ds }, graph, field);
        }
    }

    /// The search field: what's typed (or the placeholder) and the caret, the ⌘K key, the ×
    /// that clears it.
    pub(crate) fn paint_search_field(
        &mut self,
        pen: Pen<'_, '_>,
        icons: &icons::Icons,
        field: Rect,
    ) {
        let Pen { layer, text, ds } = pen;
        let px = |v: f32| ds.px(v);
        ds::surface(layer, ds, field, ds::Elevation::Low, 20.0);
        let open = self.search.is_some();
        let input = ds::search_field(layer, icons, ds, field, open);
        self.paint_search_query(Pen { layer, text, ds }, field, input);
        let key = if cfg!(target_os = "macos") { "⌘K" } else { "Ctrl K" };
        let keycap = ds::keycap(
            layer,
            text,
            ds,
            key,
            field.max_x() - px(12.0),
            field.min_y() + field.size.height / 2.0,
            false,
        );
        self.hits.push((field, Hit::SearchField));
        // × (clears the text), left of the ⌘K key, while something is typed.
        if self.search.as_ref().is_some_and(|s| !s.query.is_empty()) {
            let cx = keycap.min_x() - px(16.0);
            let hovered = self.hover == Some(Hit::SearchClear);
            let r = ds::clear_button(
                layer,
                icons,
                ds,
                cx,
                field.min_y() + field.size.height / 2.0,
                hovered,
            );
            let pad = px(4.0);
            let r = Rect::new(
                r.min_x() - pad,
                r.min_y() - pad,
                r.size.width + 2.0 * pad,
                r.size.height + 2.0 * pad,
            );
            self.hits.push((r, Hit::SearchClear));
        }
    }

    /// What's typed in the search field at `input` (selected whole, the caret after it), or
    /// the placeholder (the caret before it, while open).
    pub(crate) fn paint_search_query(&self, pen: Pen<'_, '_>, field: Rect, input: Rect) {
        let Pen { layer, text, ds } = pen;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let font = ds.ui_font(-0.5);
        let base = ds::optical_baseline(field.min_y(), field.size.height, &font);
        match &self.search {
            Some(search) if !search.query.is_empty() => {
                let shaped = text.shape_line(&search.query, &font);
                if search.all {
                    let h = (field.size.height * 0.5).round();
                    layer.quad(
                        Quad::filled(
                            Rect::new(
                                input.min_x() - px(1.0),
                                (field.min_y() + (field.size.height - h) / 2.0).round(),
                                shaped.width + px(2.0),
                                h,
                            ),
                            p.selection,
                        )
                        .rounded(px(3.0)),
                    );
                }
                draw_text(layer, &shaped, input.min_x(), base, p.text);
                let caret_h = (field.size.height * 0.45).round();
                ui_kit::caret::paint(
                    layer,
                    self.search_caret(
                        input.min_x() + shaped.width + px(1.0),
                        field,
                        caret_h,
                        px(1.5).max(1.0),
                        p.caret,
                    ),
                );
            }
            _ => {
                let shaped = text.shape_line(&t("canvas.search"), &font);
                draw_text(layer, &shaped, input.min_x(), base, p.text_muted);
                if self.search.is_some() {
                    let caret_h = (field.size.height * 0.45).round();
                    ui_kit::caret::paint(
                        layer,
                        self.search_caret(input.min_x(), field, caret_h, px(1.5).max(1.0), p.caret),
                    );
                }
            }
        }
    }

    /// The search's results under its field (or "no results"): a glyph for the kind, the
    /// label, where it is.
    pub(crate) fn paint_search_results(&mut self, pen: Pen<'_, '_>, graph: &DepGraph, field: Rect) {
        let Pen { layer, text, ds } = pen;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let Some(search) = &self.search else { return };
        let font = ds.ui_font(-0.5);
        let row_h = px(44.0);
        let rows = search.results.len().max(1);
        let list = Rect::new(
            field.min_x(),
            field.max_y() + px(8.0),
            field.size.width,
            rows as f32 * row_h + px(12.0),
        );
        ds::surface(layer, ds, list, ds::Elevation::Medium, 12.0);
        if search.results.is_empty() {
            let shaped = text.shape_line(&t("canvas.no_results"), &font);
            draw_text(
                layer,
                &shaped,
                list.min_x() + px(16.0),
                ds::optical_baseline(list.min_y() + px(6.0), row_h, &font),
                p.text_muted,
            );
            return;
        }
        let small = ds.ui_font(-2.0);
        // The list first: its rows, pushed after, win.
        self.hits.push((list, Hit::SearchField));
        for (i, &entry) in search.results.iter().enumerate() {
            let e = &graph.catalog[entry];
            let r = Rect::new(
                list.min_x() + px(6.0),
                list.min_y() + px(6.0) + i as f32 * row_h,
                list.size.width - px(12.0),
                row_h,
            );
            let hovered = self.hover == Some(Hit::SearchResult(i));
            if i == search.selected {
                layer.quad(Quad::filled(r, p.accent.with_alpha(0.10)).rounded(px(8.0)));
            } else if hovered {
                layer.quad(Quad::filled(r, p.text.with_alpha(0.05)).rounded(px(8.0)));
            }
            let glyph = match e.kind {
                None => "▢",
                Some(k) if k.is_type() => "C",
                Some(_) => "ƒ",
            };
            let g = text.shape_line(glyph, &ds.code_font(-1.0));
            draw_text(
                layer,
                &g,
                r.min_x() + px(12.0),
                ds::optical_baseline(r.min_y(), row_h, &ds.code_font(-1.0)),
                p.accent,
            );
            let x = r.min_x() + px(34.0);
            let label = ds::ellipsize(text, &e.label, &font, r.size.width - px(46.0));
            let shaped = text.shape_line(&label, &font);
            draw_text(layer, &shaped, x, r.min_y() + px(19.0), p.text);
            let detail = match e.line {
                Some(line) => format!("{}:{}", e.detail, line + 1),
                None => e.detail.clone(),
            };
            let detail = ds::ellipsize(text, &detail, &small, r.size.width - px(46.0));
            let shaped = text.shape_line(&detail, &small);
            draw_text(layer, &shaped, x, r.min_y() + px(36.0), p.text_muted);
            self.hits.push((r, Hit::SearchResult(i)));
        }
    }
}
