//! The "Jump to file or function" field (top right) and its results, painted.

use super::*;
use ui_components::search_field::{SearchExtras, SearchField};

/// A result row's corners (logical pixels) and its hover's alpha.
const RESULT_RADIUS: f32 = 8.0;
const RESULT_HOVER_ALPHA: f32 = 0.05;

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
        if self.search.as_ref().is_some_and(|s| !s.query().trim().is_empty()) {
            let layer = scene.push_layer(Some(b));
            self.paint_search_results(Pen { layer, text, ds }, graph, field);
        }
    }

    /// The search field: the app's (what's typed or the placeholder, the caret while open,
    /// × once there is text), on a raised surface; the ⌘K key always in the room it keeps on
    /// its right (× left of it).
    pub(crate) fn paint_search_field(
        &mut self,
        pen: Pen<'_, '_>,
        icons: &icons::Icons,
        field: Rect,
    ) {
        let Pen { layer, text, ds } = pen;
        ds::surface(layer, ds, field, ds::Elevation::Low, 20.0);
        self.hits.push((field, Hit::SearchField));
        let placeholder = t("canvas.search");
        let key = if cfg!(target_os = "macos") { "⌘K" } else { "Ctrl K" };
        // The key cap's room, in the field's logical pixels.
        let trailing = ds::keycap_width(text, ds, key) / ds.px(1.0).max(f32::EPSILON);
        let extras = SearchExtras { trailing, invalid: false };
        let room = match &mut self.search {
            Some(search) => {
                search.field.paint_with(layer, text, icons, ds, (field, true), &placeholder, extras)
            }
            None => {
                // Closed: an empty field to open (no caret, nothing kept).
                let mut closed = SearchField::new();
                closed.paint_with(layer, text, icons, ds, (field, false), &placeholder, extras)
            }
        };
        let mid = room.min_y() + room.size.height / 2.0;
        ds::keycap(layer, text, ds, key, room.max_x(), mid, false);
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
            ds::RowHighlight::new(i == search.selected, hovered)
                .radius(RESULT_RADIUS)
                .hover(ds::HoverFill::Faint(RESULT_HOVER_ALPHA))
                .paint(layer, ds, r);
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
