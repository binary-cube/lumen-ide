//! Edges drawn and reshaped. Over a shape show its connection points (its sides' middles) and,
//! outside each side, an arrow (as draw.io's): a click on an arrow makes a shape like it
//! beside it, joined to it; an arrow dragged draws an edge — let go over another shape it joins
//! them, on the sheet a shape like it is made there, joined. A drag from a connection point,
//! or from the band just inside a shape's outline, draws an edge too (let go on the sheet: its
//! end loose there). While an edge is drawn, the shape it would join is lit. A chosen edge's
//! handles: an end dragged onto a shape joins it there (on the sheet: loose), a point dragged
//! moves (on the grid), a middle dragged puts a point in. Each one step of the history; while
//! dragging, only a line shows where it goes.

use kernel::{Point, Rect};
use render_api::Quad;
use text_layout::TextSystem;
use ui_components::ds::{self, ButtonKind, ButtonSpec, Ctx};

use super::DiagramView;
use super::gesture::Gesture;
use diagram_model::CellKind;

use crate::edit::{self, EdgeHandle};

/// A connection point's or an edge handle's side (logical pixels); caught a little wider.
const DOT: f32 = 10.0;
const DOT_CATCH: f32 = 16.0;
/// A shape's arrow: its side, and how far out of the shape (logical pixels).
const ARROW: f32 = 18.0;
const ARROW_GAP: f32 = 6.0;
/// The band just inside a shape's outline where a press draws an edge (logical pixels).
const BAND: f32 = 5.0;
/// How far from a shape the one made beside it goes (diagram units).
const BESIDE: f64 = 40.0;

/// A side of a shape (where its arrow points).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Side {
    Up,
    Right,
    Down,
    Left,
}

impl Side {
    const ALL: [Self; 4] = [Self::Up, Self::Right, Self::Down, Self::Left];

    fn icon(self) -> &'static str {
        match self {
            Self::Up => "arrow-up",
            Self::Right => "arrow-right",
            Self::Down => "arrow-down",
            Self::Left => "arrow-left",
        }
    }
}

/// A new edge's style: draw.io's default connector.
const EDGE_STYLE: &str =
    "edgeStyle=orthogonalEdgeStyle;rounded=0;orthogonalLoop=1;jettySize=auto;html=1;";

impl DiagramView {
    /// The shape under `p` for its connection points (`None`: none, or locked).
    pub(super) fn hover_shape(&mut self, p: Point) -> bool {
        let shape = if self.locked || self.gesture.is_some() || self.editing.is_some() {
            None
        } else if self.arrow_at(p).is_some() {
            // On its arrow (outside it): still the shape's.
            self.hovered.clone()
        } else {
            let at = self.camera.to_diagram(p, self.sheet(), self.scale);
            self.drawing.as_ref().and_then(|(d, _)| {
                d.boxes.iter().rev().find(|(_, r)| r.contains(at)).map(|(id, _)| id.clone())
            })
        };
        std::mem::replace(&mut self.hovered, shape.clone()) != shape
    }

    /// Shape `id`'s box on the screen.
    fn screen_box(&self, id: &str) -> Option<Rect> {
        let r = self.box_of(id)?;
        let a = self.camera.to_screen(r.origin, self.sheet(), self.scale);
        let b = self.camera.to_screen(Point::new(r.max_x(), r.max_y()), self.sheet(), self.scale);
        Some(Rect::new(a.x, a.y, b.x - a.x, b.y - a.y))
    }

    /// The hovered shape's arrows (screen), one out of each side.
    fn arrows(&self) -> Vec<(Side, Rect)> {
        // A drawing tool draws there instead.
        if self.draw != super::draw::Draw::Select {
            return Vec::new();
        }
        let Some(s) = self.hovered.as_deref().and_then(|id| self.screen_box(id)) else {
            return Vec::new();
        };
        let (size, gap) = (ARROW * self.scale, ARROW_GAP * self.scale);
        let c = s.center();
        Side::ALL
            .into_iter()
            .map(|side| {
                let (x, y) = match side {
                    Side::Up => (c.x - size / 2.0, s.min_y() - gap - size),
                    Side::Right => (s.max_x() + gap, c.y - size / 2.0),
                    Side::Down => (c.x - size / 2.0, s.max_y() + gap),
                    Side::Left => (s.min_x() - gap - size, c.y - size / 2.0),
                };
                (side, Rect::new(x, y, size, size))
            })
            .collect()
    }

    /// The hovered shape's arrow under `p` (screen).
    pub(super) fn arrow_at(&self, p: Point) -> Option<(String, Side)> {
        if self.locked {
            return None;
        }
        let id = self.hovered.clone()?;
        let side = self.arrows().into_iter().find(|(_, r)| r.contains(p))?.0;
        Some((id, side))
    }

    /// The hovered shape, when `p` is in the band just inside its outline (not on a resize
    /// handle; a shape too small for a band has none).
    pub(super) fn outline_at(&self, p: Point) -> Option<String> {
        let id = self.hovered.as_deref()?;
        let s = self.screen_box(id)?;
        let band = BAND * self.scale;
        if self.locked
            || self.handle_at(p).is_some()
            || s.size.width < 6.0 * band
            || s.size.height < 6.0 * band
        {
            return None;
        }
        let inner = Rect::new(
            s.min_x() + band,
            s.min_y() + band,
            s.size.width - 2.0 * band,
            s.size.height - 2.0 * band,
        );
        (s.contains(p) && !inner.contains(p)).then(|| id.to_owned())
    }

    /// Its box (absolute).
    fn box_of(&self, id: &str) -> Option<Rect> {
        self.drawing.as_ref()?.0.boxes.iter().find(|(b, _)| b == id).map(|(_, r)| *r)
    }

    /// A dot `p` (diagram) as a square on the screen.
    fn dot(&self, p: Point) -> Rect {
        self.square(p, DOT)
    }

    /// A square of side `side` (logical pixels) around diagram point `p`, on the screen.
    fn square(&self, p: Point, side: f32) -> Rect {
        let s = self.camera.to_screen(p, self.sheet(), self.scale);
        let h = side * self.scale / 2.0;
        Rect::new(s.x - h, s.y - h, 2.0 * h, 2.0 * h)
    }

    /// The hovered shape's connection point under `p` (screen), caught a little around it.
    fn connection_at(&self, p: Point) -> Option<String> {
        // A chosen shape's resize handle there (the sides' middles): the handle's.
        if self.handle_at(p).is_some() {
            return None;
        }
        let id = self.hovered.as_ref()?;
        let r = self.box_of(id)?;
        edit::connection_points(r)
            .into_iter()
            .any(|c| self.square(c, DOT_CATCH).contains(p))
            .then(|| id.clone())
    }

    /// Edge `id`'s points, absolute (its own are its parent's: a group's corner added).
    fn edge_points(&self, id: &str) -> Vec<Point> {
        let Some(cell) =
            self.diagram.as_ref().and_then(|d| d.doc.pages.get(self.page)).and_then(|p| p.cell(id))
        else {
            return Vec::new();
        };
        let (ox, oy) = cell
            .parent
            .as_deref()
            .and_then(|p| self.box_of(p))
            .map_or((0.0, 0.0), |r| (r.min_x(), r.min_y()));
        cell.geometry.as_ref().map_or_else(Vec::new, |g| {
            g.points.iter().map(|p| Point::new(p.x as f32 + ox, p.y as f32 + oy)).collect()
        })
    }

    /// The one chosen edge and its handles.
    fn chosen_edge(&self) -> Option<(String, Vec<(EdgeHandle, Point)>)> {
        if self.selection.len() != 1 || self.locked {
            return None;
        }
        let id = &self.selection[0];
        let way = self
            .drawing
            .as_ref()?
            .0
            .ways
            .iter()
            .find(|(w, _)| w == id)
            .map(|(_, way)| way.clone())?;
        Some((id.clone(), edit::edge_handles(&way, &self.edge_points(id))))
    }

    /// A press at `p` on what draws or reshapes an edge: the gesture it starts.
    pub(super) fn press_edge(&self, p: Point) -> Option<Gesture> {
        if self.locked {
            return None;
        }
        if let Some((id, handles)) = self.chosen_edge()
            && let Some((handle, _)) = handles.into_iter().find(|(_, at)| self.dot(*at).contains(p))
        {
            return Some(Gesture::Edge { id, handle, to: p });
        }
        if let Some((source, side)) = self.arrow_at(p) {
            return Some(Gesture::Arrow { source, side, from: p, to: p });
        }
        let source = self.connection_at(p).or_else(|| self.outline_at(p))?;
        Some(Gesture::Connect { source, from: p, to: p })
    }

    /// The shape under screen point `p` other than `but` (where an edge would join).
    pub(super) fn shape_at(&self, p: Point, but: Option<&str>) -> Option<String> {
        let at = self.camera.to_diagram(p, self.sheet(), self.scale);
        let d = &self.drawing.as_ref()?.0;
        d.boxes
            .iter()
            .rev()
            .find(|(id, r)| r.contains(at) && Some(id.as_str()) != but)
            .map(|(id, _)| id.clone())
    }

    /// A new edge from `source` to the shape at screen point `to` (or loose there): added and
    /// chosen.
    pub(super) fn finish_connect(&mut self, source: &str, to: Point) -> bool {
        let target = self.shape_at(to, Some(source));
        let at = self.grid_point(self.camera.to_diagram(to, self.sheet(), self.scale));
        let page = self.page;
        let Some(d) = self.diagram.as_mut() else { return false };
        let Some(layer) = d.doc.pages.get(page).and_then(super::clipboard::first_layer) else {
            return false;
        };
        let id = super::clipboard::new_id();
        let mut edge = diagram_model::Cell::new(id.clone(), diagram_model::CellKind::Edge, layer);
        edge.style = diagram_model::Style::parse(EDGE_STYLE);
        edge.source = Some(source.to_owned());
        let mut g = diagram_model::Geometry { relative: true, ..Default::default() };
        match target {
            Some(t) => edge.target = Some(t),
            None => {
                g.target_point = Some(diagram_model::Point::new(f64::from(at.x), f64::from(at.y)))
            }
        }
        edge.geometry = Some(g);
        let tx = diagram_model::ops::add(&d.doc, page, edge);
        let done = d.edit(tx, None);
        if done {
            self.selection = vec![id];
        }
        done
    }

    /// A shape like `source` (its style and size, no text) made beside it toward `side` — or
    /// centered at screen point `at` — and joined to it by an edge: one step, the new shape
    /// chosen.
    pub(super) fn make_beside(&mut self, source: &str, side: Side, at: Option<Point>) -> bool {
        let page = self.page;
        let at = at.map(|p| self.grid_point(self.camera.to_diagram(p, self.sheet(), self.scale)));
        let Some(p) = self.diagram.as_ref().and_then(|d| d.doc.pages.get(page)) else {
            return false;
        };
        let Some(layer) = super::clipboard::first_layer(p) else { return false };
        let Some(cell) = p.cell(source) else { return false };
        let Some(g) = cell.geometry.as_ref().filter(|_| cell.kind == CellKind::Vertex) else {
            return false;
        };
        let parent = cell.parent.clone().unwrap_or_else(|| layer.clone());
        let (w, h) = (g.width, g.height);
        let (x, y) = match at {
            // Its parent's corner taken away (a shape in a group is placed in it).
            Some(at) => {
                let (ox, oy) = self.box_of(&parent).map_or((0.0, 0.0), |r| (r.min_x(), r.min_y()));
                (f64::from(at.x - ox) - w / 2.0, f64::from(at.y - oy) - h / 2.0)
            }
            None => match side {
                Side::Right => (g.x + w + BESIDE, g.y),
                Side::Left => (g.x - BESIDE - w, g.y),
                Side::Down => (g.x, g.y + h + BESIDE),
                Side::Up => (g.x, g.y - BESIDE - h),
            },
        };
        // A group or a container is no shape to copy: a plain box beside it.
        let style = if cell.style.has_name("group") || cell.style.flag("container") {
            diagram_model::Style::parse("rounded=0;whiteSpace=wrap;html=1;")
        } else {
            cell.style.clone()
        };
        let shape_id = super::clipboard::new_id();
        let mut shape = diagram_model::Cell::new(shape_id.clone(), CellKind::Vertex, parent);
        shape.style = style;
        shape.geometry =
            Some(diagram_model::Geometry { x, y, width: w, height: h, ..Default::default() });
        let mut edge = diagram_model::Cell::new(super::clipboard::new_id(), CellKind::Edge, layer);
        edge.style = diagram_model::Style::parse(EDGE_STYLE);
        edge.source = Some(source.to_owned());
        edge.target = Some(shape_id.clone());
        edge.geometry = Some(diagram_model::Geometry { relative: true, ..Default::default() });
        let Some(d) = self.diagram.as_mut() else { return false };
        let mut tx = diagram_model::ops::add(&d.doc, page, shape);
        tx.changes.extend(diagram_model::ops::add(&d.doc, page, edge).changes);
        let done = d.edit(tx, None);
        if done {
            self.selection = vec![shape_id];
        }
        done
    }

    /// Edge `id`'s `handle` let go at screen point `to`: an end joined to the shape there (or
    /// loose there), a point moved there, a point put in there.
    pub(super) fn finish_edge(&mut self, id: &str, handle: EdgeHandle, to: Point) -> bool {
        let at = self.grid_point(self.camera.to_diagram(to, self.sheet(), self.scale));
        let page = self.page;
        let (other_end, parent_box) = {
            let Some(cell) =
                self.diagram.as_ref().and_then(|d| d.doc.pages.get(page)).and_then(|p| p.cell(id))
            else {
                return false;
            };
            let other = match handle {
                EdgeHandle::Source => cell.target.clone(),
                _ => cell.source.clone(),
            };
            (other, cell.parent.as_deref().and_then(|p| self.box_of(p)))
        };
        let shape = matches!(handle, EdgeHandle::Source | EdgeHandle::Target)
            .then(|| self.shape_at(to, other_end.as_deref()))
            .flatten();
        let (ox, oy) = parent_box.map_or((0.0, 0.0), |r| (r.min_x(), r.min_y()));
        let local = diagram_model::Point::new(f64::from(at.x - ox), f64::from(at.y - oy));
        let Some(d) = self.diagram.as_mut() else { return false };
        let tx = diagram_model::ops::update(&d.doc, page, id, |c| {
            let g = c.geometry.get_or_insert_with(|| diagram_model::Geometry {
                relative: true,
                ..Default::default()
            });
            match handle {
                EdgeHandle::Source => {
                    g.source_point = shape.is_none().then_some(local);
                    c.source = shape.clone();
                }
                EdgeHandle::Target => {
                    g.target_point = shape.is_none().then_some(local);
                    c.target = shape.clone();
                }
                EdgeHandle::Point(i) => {
                    if let Some(p) = g.points.get_mut(i) {
                        *p = local;
                    }
                }
                EdgeHandle::Insert(i) => g.points.insert(i.min(g.points.len()), local),
            }
        });
        d.edit(tx, None)
    }

    /// The hovered shape's connection points; the chosen edge's handles; a new edge or an end
    /// being dragged: its line.
    pub(super) fn paint_connections(
        &self,
        layer: &mut render_api::Layer,
        text: &mut TextSystem,
        palette: &ui_components::Palette,
        icons: &icons::Icons,
    ) {
        let accent = palette.accent;
        // The shape a drawn edge would join: lit all round.
        let target = match &self.gesture {
            Some(Gesture::Connect { source, to, .. } | Gesture::Arrow { source, to, .. }) => {
                self.shape_at(*to, Some(source))
            }
            Some(Gesture::Edge { handle: EdgeHandle::Source | EdgeHandle::Target, to, .. }) => {
                self.shape_at(*to, None)
            }
            _ => None,
        };
        if let Some(r) = target.as_deref().and_then(|id| self.screen_box(id)) {
            let w = (2.0 * self.scale).max(1.0);
            let r = Rect::new(
                r.min_x() - w,
                r.min_y() - w,
                r.size.width + 2.0 * w,
                r.size.height + 2.0 * w,
            );
            layer.quad(
                Quad::filled(r, accent.with_alpha(0.12))
                    .bordered(w, accent)
                    .rounded(3.0 * self.scale),
            );
        }
        // The hovered shape's arrows (a soft disc, the arrow in the accent).
        if self.gesture.is_none() {
            let ds = Ctx::new(palette, self.scale);
            for (side, r) in self.arrows() {
                let spec = ButtonSpec::new(ButtonKind::Ghost).icon(side.icon()).round().on(true);
                ds::button(layer, text, icons, &ds, r, spec);
            }
        }
        let dot = |layer: &mut render_api::Layer, r: Rect, filled: bool| {
            layer.quad(Quad::filled(r, accent).rounded(r.size.width / 2.0));
            if !filled {
                let w = self.scale.max(1.0) * 1.5;
                let inner = Rect::new(
                    r.min_x() + w,
                    r.min_y() + w,
                    r.size.width - 2.0 * w,
                    r.size.height - 2.0 * w,
                );
                layer.quad(
                    Quad::filled(inner, palette.editor_background).rounded(inner.size.width / 2.0),
                );
            }
        };
        if self.gesture.is_none()
            && let Some(r) = self.hovered.as_deref().and_then(|id| self.box_of(id))
        {
            for c in edit::connection_points(r) {
                dot(layer, self.dot(c), false);
            }
        }
        if self.gesture.is_none()
            && let Some((_, handles)) = self.chosen_edge()
        {
            for (h, at) in handles {
                dot(layer, self.dot(at), !matches!(h, EdgeHandle::Insert(_)));
            }
        }
        let line = |layer: &mut render_api::Layer, a: Point, b: Point| {
            // A dotted line from `a` to `b` (screen): squares along it.
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            let len = dx.hypot(dy);
            let step = 6.0 * self.scale;
            let n = (len / step).floor() as usize;
            let s = self.scale.max(1.0) * 1.5;
            for i in 0..=n {
                let t = if len > 0.0 { i as f32 * step / len } else { 0.0 };
                layer.quad(Quad::filled(
                    Rect::new(a.x + dx * t - s / 2.0, a.y + dy * t - s / 2.0, s, s),
                    accent,
                ));
            }
        };
        match &self.gesture {
            Some(Gesture::Connect { source, to, .. } | Gesture::Arrow { source, to, .. }) => {
                if let Some(r) = self.box_of(source) {
                    let from = self.camera.to_screen(r.center(), self.sheet(), self.scale);
                    line(layer, from, *to);
                }
            }
            Some(Gesture::Line { from, to }) => line(layer, *from, *to),
            Some(Gesture::Draw { from, to, square }) => {
                let r = super::draw::dragged_box(*from, *to, *square);
                let w = self.scale.max(1.0) * 1.5;
                layer.quad(Quad::filled(r, accent.with_alpha(0.08)).bordered(w, accent).rounded(w));
            }
            Some(Gesture::Edge { id, handle, to }) => {
                if let Some((_, handles)) = self.chosen_edge().filter(|(e, _)| e == id) {
                    let anchor = match handle {
                        EdgeHandle::Source => {
                            handles.iter().find(|(h, _)| *h == EdgeHandle::Target)
                        }
                        _ => handles.iter().find(|(h, _)| *h == EdgeHandle::Source),
                    };
                    if let Some((_, a)) = anchor {
                        line(layer, self.camera.to_screen(*a, self.sheet(), self.scale), *to);
                    }
                }
            }
            _ => {}
        }
    }
}
