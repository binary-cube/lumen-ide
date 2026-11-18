//! A small bar over what's chosen, for the styles changed most: a pair of colors (draw.io's
//! schemes: a fill and its line — a shape takes both, an edge the line, a text the line as its
//! color; the first: the theme's), a dashed line; for edges, their way (straight, at right
//! angles, curved) and their end's arrow. Each one step; shown while nothing is dragged or
//! edited, above what's chosen (below it when there's no room).

use kernel::{Point, Rect, Rgba};
use render_api::Quad;
use ui_components::ds::Ctx;

use super::DiagramView;
use diagram_model::{Cell, CellKind};

/// The color pairs offered: (fill, line); the first, none (the theme's).
const SCHEMES: [(Option<&str>, Option<&str>); 8] = [
    (None, None),
    (Some("#dae8fc"), Some("#6c8ebf")),
    (Some("#d5e8d4"), Some("#82b366")),
    (Some("#ffe6cc"), Some("#d79b00")),
    (Some("#fff2cc"), Some("#d6b656")),
    (Some("#f8cecc"), Some("#b85450")),
    (Some("#e1d5e7"), Some("#9673a6")),
    (Some("#f5f5f5"), Some("#666666")),
];

/// An edge's way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Route {
    Straight,
    Elbow,
    Curved,
}

/// A button of the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Quick {
    Scheme(usize),
    Dashed,
    Route(Route),
    EndArrow,
}

impl Quick {
    fn icon(self) -> Option<&'static str> {
        Some(match self {
            Self::Scheme(_) => return None,
            Self::Dashed => "line-dashed",
            Self::Route(Route::Straight) => "route-straight",
            Self::Route(Route::Elbow) => "route-elbow",
            Self::Route(Route::Curved) => "route-curved",
            Self::EndArrow => "line-arrow",
        })
    }

    fn tip(self) -> &'static str {
        match self {
            Self::Scheme(0) => "diagram_editor.quick_theme_colors",
            Self::Scheme(_) => "diagram_editor.quick_colors",
            Self::Dashed => "diagram_editor.quick_dashed",
            Self::Route(Route::Straight) => "diagram_editor.quick_straight",
            Self::Route(Route::Elbow) => "diagram_editor.quick_elbow",
            Self::Route(Route::Curved) => "diagram_editor.quick_curved",
            Self::EndArrow => "diagram_editor.quick_end_arrow",
        }
    }
}

/// An edge's way as its style says.
fn route_of(cell: &Cell) -> Route {
    if cell.style.get("curved") == Some("1") {
        // (Curved: draw.io's rounded right angles, `edgeStyle` kept.)
        Route::Curved
    } else if cell.style.get("edgeStyle").is_some_and(|s| s != "none") {
        Route::Elbow
    } else {
        Route::Straight
    }
}

/// A text's own (no box): its color is what a line color sets.
fn is_text(cell: &Cell) -> bool {
    cell.style.shape() == Some("text") || cell.style.has_name("text")
}

impl DiagramView {
    /// Whether cell `id` is chosen: a few compared directly, many through a set (made once
    /// by the caller: everything chosen on a large page stays one pass over it).
    fn is_chosen(&self) -> impl Fn(&str) -> bool + '_ {
        let set = (self.selection.len() > 4).then(|| self.chosen());
        move |id: &str| match &set {
            Some(set) => set.contains(id),
            None => self.selection.iter().any(|s| s == id),
        }
    }

    /// The chosen cells (one pass over the page).
    fn chosen_cells(&self) -> Vec<&Cell> {
        let Some(page) = self.diagram.as_ref().and_then(|d| d.doc.pages.get(self.page)) else {
            return Vec::new();
        };
        if self.selection.is_empty() {
            return Vec::new();
        }
        let chosen = self.is_chosen();
        page.cells.iter().filter(|c| chosen(&c.id)).collect()
    }

    /// Where the bar goes now and its buttons (screen); `None`: not shown. Worked out at each
    /// paint ([`Self::paint_quick`]), read from there by the mouse.
    pub(super) fn quick_layout(&self) -> Option<(Rect, Vec<(Rect, Quick)>)> {
        if self.locked
            || self.selection.is_empty()
            || self.gesture.is_some()
            || self.editing.is_some()
            || self.draw != super::draw::Draw::Select
        {
            return None;
        }
        let (drawing, _) = self.drawing.as_ref()?;
        let to_screen = |p: Point| self.camera.to_screen(p, self.sheet(), self.scale);
        // Around everything chosen: shapes' boxes, edges' ways (one pass over the drawing).
        let is_chosen = self.is_chosen();
        let mut chosen: Option<Rect> = None;
        let mut add = |r: Rect| chosen = Some(chosen.map_or(r, |c| crate::edit::union(c, r)));
        for (_, r) in drawing.boxes.iter().filter(|(id, _)| is_chosen(id)) {
            let (a, b) = (to_screen(r.origin), to_screen(Point::new(r.max_x(), r.max_y())));
            add(Rect::new(a.x, a.y, b.x - a.x, b.y - a.y));
        }
        let mut edges = false;
        for (_, way) in drawing.ways.iter().filter(|(id, _)| is_chosen(id)) {
            edges = true;
            for p in way {
                let s = to_screen(*p);
                add(Rect::new(s.x, s.y, 0.0, 0.0));
            }
        }
        let chosen = chosen?;
        let mut buttons: Vec<Quick> = (0..SCHEMES.len()).map(Quick::Scheme).collect();
        buttons.push(Quick::Dashed);
        if edges {
            buttons.extend([
                Quick::Route(Route::Straight),
                Quick::Route(Route::Elbow),
                Quick::Route(Route::Curved),
                Quick::EndArrow,
            ]);
        }
        let px = |v: f32| (v * self.scale).round();
        let (size, gap, pad, split) = (px(22.0), px(3.0), px(5.0), px(8.0));
        let splits = if edges { 2.0 } else { 1.0 };
        let width = 2.0 * pad
            + buttons.len() as f32 * size
            + (buttons.len() - 1) as f32 * gap
            + splits * split;
        let height = size + 2.0 * pad;
        let sheet = self.sheet();
        // Clear of the shape's arrows (out of its sides, 24 px).
        let gap = px(30.0);
        let above = chosen.min_y() - gap - height;
        let y = if above >= sheet.min_y() + px(4.0) { above } else { chosen.max_y() + gap };
        let x = (chosen.center().x - width / 2.0)
            .min(sheet.max_x() - px(4.0) - width)
            .max(sheet.min_x() + px(4.0));
        let bar = Rect::new(x.round(), y.round(), width, height);
        let mut out = Vec::with_capacity(buttons.len());
        let mut at = bar.min_x() + pad;
        for b in buttons {
            // A gap between the colors, the line, the edge's way.
            if matches!(b, Quick::Dashed | Quick::Route(Route::Straight)) {
                at += split;
            }
            out.push((Rect::new(at, bar.min_y() + pad, size, size), b));
            at += size + gap;
        }
        Some((bar, out))
    }

    /// The bar's button under `p`.
    pub(super) fn quick_at(&self, p: Point) -> Option<(Rect, Quick)> {
        let (bar, buttons) = self.quick_bar.as_ref()?;
        if !bar.contains(p) {
            return None;
        }
        buttons.iter().find(|(r, _)| r.contains(p)).copied()
    }

    /// Over the bar (its buttons or between them), as last painted.
    pub(super) fn over_quick(&self, p: Point) -> bool {
        self.quick_bar.as_ref().is_some_and(|(bar, _)| bar.contains(p))
    }

    /// A press on the bar: its button done. `true`: it was on it.
    pub(super) fn press_quick(&mut self, p: Point) -> bool {
        if !self.over_quick(p) {
            return false;
        }
        if let Some((_, quick)) = self.quick_at(p) {
            self.quick(quick);
        }
        true
    }

    /// A button of the bar done on what's chosen, as one step.
    pub(super) fn quick(&mut self, quick: Quick) -> bool {
        let chosen = self.chosen_cells();
        let all_dashed = chosen.iter().all(|c| c.style.get("dashed") == Some("1"));
        let any_arrow = chosen
            .iter()
            .any(|c| c.kind == CellKind::Edge && c.style.get("endArrow") != Some("none"));
        let page = self.page;
        let ids: Vec<String> = self.selection.clone();
        let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        let Some(d) = self.diagram.as_mut() else { return false };
        let tx = diagram_model::ops::set_styles(&d.doc, page, &ids, |c| match quick {
            Quick::Scheme(i) => {
                let (fill, line) = SCHEMES.get(i).copied().unwrap_or((None, None));
                match c.kind {
                    CellKind::Edge => vec![("strokeColor", line)],
                    _ if is_text(c) => vec![("fontColor", line)],
                    _ => vec![("fillColor", fill), ("strokeColor", line)],
                }
            }
            Quick::Dashed => vec![("dashed", (!all_dashed).then_some("1"))],
            Quick::Route(route) if c.kind == CellKind::Edge => match route {
                Route::Straight => vec![("edgeStyle", None), ("curved", None)],
                Route::Elbow => vec![("edgeStyle", Some("orthogonalEdgeStyle")), ("curved", None)],
                // As draw.io's own "Curved": its right angles rounded into curves.
                Route::Curved => {
                    vec![("edgeStyle", Some("orthogonalEdgeStyle")), ("curved", Some("1"))]
                }
            },
            Quick::EndArrow if c.kind == CellKind::Edge => {
                vec![("endArrow", any_arrow.then_some("none"))]
            }
            _ => Vec::new(),
        });
        d.edit(tx, None)
    }

    /// A button's tooltip.
    pub(super) fn quick_tip(&self, p: Point) -> Option<(Rect, String)> {
        self.quick_at(p).map(|(r, q)| (r, i18n::t(q.tip())))
    }

    /// The bar drawn (over the sheet).
    pub(super) fn paint_quick(
        &mut self,
        scene: &mut render_api::Scene,
        ds: &Ctx<'_>,
        icons: &icons::Icons,
    ) {
        self.quick_bar = self.quick_layout();
        let Some((bar, buttons)) = self.quick_bar.clone() else { return };
        let p = ds.palette;
        let layer = scene.push_layer(Some(self.sheet()));
        layer.quad(
            Quad::filled(bar, p.popup_background)
                .rounded(ds.px(8.0))
                .bordered(ds.line(), p.popup_border),
        );
        let chosen = self.chosen_cells();
        let dashed = chosen.iter().all(|c| c.style.get("dashed") == Some("1"));
        let edges: Vec<&&Cell> = chosen.iter().filter(|c| c.kind == CellKind::Edge).collect();
        let route = edges.first().map(|c| route_of(c));
        let arrow = edges.iter().any(|c| c.style.get("endArrow") != Some("none"));
        for (r, quick) in buttons {
            let on = match quick {
                Quick::Dashed => dashed,
                Quick::Route(way) => route == Some(way),
                Quick::EndArrow => arrow,
                Quick::Scheme(_) => false,
            };
            if on {
                layer.quad(Quad::filled(r, p.tab_active_background).rounded(ds.px(5.0)));
            }
            match quick {
                Quick::Scheme(i) => {
                    let (fill, line) = SCHEMES[i];
                    let parse = |c: Option<&str>| {
                        c.and_then(|c| u32::from_str_radix(c.trim_start_matches('#'), 16).ok())
                            .map(Rgba::rgb)
                    };
                    let inset = ds.px(3.0);
                    let dot = Rect::new(
                        r.min_x() + inset,
                        r.min_y() + inset,
                        r.size.width - 2.0 * inset,
                        r.size.height - 2.0 * inset,
                    );
                    let fill = parse(fill).unwrap_or(p.editor_background);
                    let line = parse(line).unwrap_or(p.text_muted);
                    layer.quad(
                        Quad::filled(dot, fill)
                            .rounded(dot.size.width / 2.0)
                            .bordered(ds.px(2.0), line),
                    );
                }
                _ => {
                    if let Some(id) = quick.icon().and_then(|name| icons.id(name)) {
                        let inset = ds.px(4.0);
                        let inner = Rect::new(
                            r.min_x() + inset,
                            r.min_y() + inset,
                            r.size.width - 2.0 * inset,
                            r.size.height - 2.0 * inset,
                        );
                        ui_kit::draw_icon(layer, icons, id, inner, p.text);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/view_quick.rs"]
mod tests;
