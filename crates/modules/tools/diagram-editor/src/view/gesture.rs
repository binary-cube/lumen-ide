//! Editing a diagram with the mouse and the keys. A press picks what's under it (a handle of
//! the one shape chosen, a cell, or the empty sheet: a band); a drag shows what it would do
//! — the cells' outlines where they'd go, snapped, with the guides they meet — and the
//! release does it, as one step of the history (draw.io's way: nothing is redrawn while the
//! mouse moves but the outlines). The keys delete, move by a point (⇧: a grid step) and let
//! go; a locked tab chooses but never changes.

use kernel::{Point, Rect};
use render_api::Quad;
use ui_components::{KeyReach, PressMods, ViewerKey};

use super::DiagramView;
use crate::edit::{self, Guide, Handle};

use crate::edit::union;

/// The mouse pressed and moving.
#[derive(Debug, Clone)]
pub(super) enum Gesture {
    /// The sheet dragged (⌥): its last point.
    Pan(Point),
    /// A band choosing what it holds (screen points).
    Band { from: Point, to: Point },
    /// The chosen cells moved (screen points); `moved`: past a nudge.
    Move { from: Point, to: Point, moved: bool },
    /// The one chosen shape's handle dragged.
    Resize { id: String, handle: Handle, from: Point, to: Point },
    /// A new edge drawn from a shape's connection point (or its outline); not moved past a
    /// nudge, it was a click (the shape chosen).
    Connect { source: String, from: Point, to: Point },
    /// A shape's arrow pressed: a click makes a shape beside it, a drag draws an edge.
    Arrow { source: String, side: super::connect::Side, from: Point, to: Point },
    /// The one chosen edge's handle dragged (an end, a point, a middle).
    Edge { id: String, handle: crate::edit::EdgeHandle, to: Point },
    /// A shape drawn with a drawing tool: the box dragged (`square`: ⇧ held at the press).
    Draw { from: Point, to: Point, square: bool },
    /// A loose line drawn with the line tool.
    Line { from: Point, to: Point },
    /// A shape of the palette carried to the sheet.
    Place { section: usize, entry: usize, from: Point, to: Point },
}

/// A press moving less than this (physical pixels) is a click, not a move.
pub(super) const NUDGE: f32 = 3.0;
/// How near (physical pixels) an edge or a guide catches.
const NEAR: f32 = 5.0;
/// A handle's side (logical pixels).
const HANDLE: f32 = 8.0;

impl DiagramView {
    /// Physical pixels per diagram unit now.
    fn k(&self) -> f32 {
        self.camera.k(self.scale)
    }

    /// Screen point `p` in the diagram.
    pub(super) fn at(&self, p: Point) -> Point {
        self.camera.to_diagram(p, self.sheet(), self.scale)
    }

    /// Diagram box `r` on the screen.
    fn on_screen(&self, r: Rect) -> Rect {
        let o = self.camera.to_screen(r.origin, self.sheet(), self.scale);
        let k = self.k();
        Rect::new(o.x, o.y, r.size.width * k, r.size.height * k)
    }

    /// The page's grid step, if its grid is on.
    /// Diagram point `p` on the grid (when it's on).
    pub(super) fn grid_point(&self, p: Point) -> Point {
        match self.grid() {
            Some(g) if g > 0.0 => Point::new((p.x / g).round() * g, (p.y / g).round() * g),
            _ => p,
        }
    }

    pub(super) fn grid(&self) -> Option<f32> {
        let page = self.diagram.as_ref()?.doc.pages.get(self.page)?;
        page.grid().then(|| page.grid_size() as f32)
    }

    /// The drawing shown now.
    fn shown(&self) -> Option<std::sync::Arc<crate::drawing::Drawing>> {
        self.drawing.as_ref().map(|(d, _)| std::sync::Arc::clone(d))
    }

    /// What's chosen, to look up (a walk over the page asks it of every cell: never a scan
    /// of the selection for each).
    pub(super) fn chosen(&self) -> std::collections::HashSet<&str> {
        self.selection.iter().map(String::as_str).collect()
    }

    /// The boxes of the chosen shapes.
    fn chosen_boxes(&self) -> Vec<(String, Rect)> {
        let Some(d) = self.drawing.as_ref().map(|(d, _)| d) else { return Vec::new() };
        if self.selection.is_empty() {
            return Vec::new();
        }
        // A few chosen: compared directly (cheaper than hashing every id of the page).
        if self.selection.len() <= 4 {
            return d.boxes.iter().filter(|(id, _)| self.selection.contains(id)).cloned().collect();
        }
        let chosen = self.chosen();
        d.boxes.iter().filter(|(id, _)| chosen.contains(id.as_str())).cloned().collect()
    }

    /// The one shape chosen (its handles shown).
    fn single(&self) -> Option<(String, Rect)> {
        let [id] = self.selection.as_slice() else { return None };
        let d = self.drawing.as_ref().map(|(d, _)| d)?;
        d.boxes.iter().find(|(b, _)| b == id).cloned()
    }

    /// The move the drag makes (diagram units), snapped, and the guides it meets.
    fn move_delta(&self, from: Point, to: Point) -> (f32, f32, Vec<Guide>) {
        let k = self.k();
        let (dx, dy) = ((to.x - from.x) / k, (to.y - from.y) / k);
        let boxes = self.chosen_boxes();
        let Some(moving) = boxes.iter().map(|(_, r)| *r).reduce(union) else {
            return (dx, dy, Vec::new());
        };
        let chosen = self.chosen();
        let others: Vec<Rect> = self
            .drawing
            .as_ref()
            .map(|(d, _)| {
                d.boxes
                    .iter()
                    .filter(|(id, _)| !chosen.contains(id.as_str()))
                    .map(|(_, r)| *r)
                    .collect()
            })
            .unwrap_or_default();
        edit::snap_move(moving, &others, (dx, dy), self.grid(), NEAR / k)
    }

    /// A press on the sheet at `p`: what it starts.
    pub(super) fn press_sheet(&mut self, p: Point, mods: PressMods) {
        let Some(drawing) = self.shown() else { return };
        if mods.alt {
            self.gesture = Some(Gesture::Pan(p));
            return;
        }
        // A drawing tool's shape or line.
        if let Some(g) = self.press_draw(p, mods.shift) {
            self.gesture = Some(g);
            return;
        }
        // An edge drawn from a connection point, or the chosen edge's handle.
        if let Some(g) = self.press_edge(p) {
            self.gesture = Some(g);
            return;
        }
        if !self.locked
            && let Some((id, r)) = self.single()
        {
            let screen = self.on_screen(r);
            if let Some((handle, _)) =
                edit::handles(screen, HANDLE * self.scale).into_iter().find(|(_, h)| h.contains(p))
            {
                self.gesture = Some(Gesture::Resize { id, handle, from: p, to: p });
                return;
            }
        }
        let toggle = mods.shift || mods.primary;
        match edit::hit(&drawing, self.at(p), NEAR / self.k()) {
            Some(id) => {
                if toggle {
                    if let Some(i) = self.selection.iter().position(|s| *s == id) {
                        self.selection.remove(i);
                    } else {
                        self.selection.push(id);
                    }
                    return;
                }
                if !self.selection.contains(&id) {
                    self.selection = vec![id];
                }
                if !self.locked {
                    self.gesture = Some(Gesture::Move { from: p, to: p, moved: false });
                }
            }
            None => {
                if !toggle {
                    self.selection.clear();
                }
                self.gesture = Some(Gesture::Band { from: p, to: p });
            }
        }
    }

    /// The mouse moved to `p` while pressed. `true`: a redraw.
    pub(super) fn drag_to(&mut self, p: Point) -> bool {
        match self.gesture.as_mut() {
            Some(Gesture::Pan(last)) => {
                let (dx, dy) = (p.x - last.x, p.y - last.y);
                *last = p;
                self.camera.pan(dx, dy);
            }
            Some(
                Gesture::Band { to, .. }
                | Gesture::Resize { to, .. }
                | Gesture::Connect { to, .. }
                | Gesture::Arrow { to, .. }
                | Gesture::Draw { to, .. }
                | Gesture::Line { to, .. }
                | Gesture::Edge { to, .. }
                | Gesture::Place { to, .. },
            ) => *to = p,
            Some(Gesture::Move { from, to, moved }) => {
                *to = p;
                *moved |= (p.x - from.x).hypot(p.y - from.y) > NUDGE * self.scale;
            }
            None => return false,
        }
        true
    }

    /// The mouse released: what the gesture does, done. `true`: a redraw.
    pub(super) fn finish(&mut self) -> bool {
        let Some(gesture) = self.gesture.take() else { return false };
        match gesture {
            Gesture::Pan(_) | Gesture::Move { moved: false, .. } => {}
            Gesture::Band { from, to } => {
                if let Some(d) = self.shown() {
                    let (a, b) = (self.at(from), self.at(to));
                    let band =
                        Rect::new(a.x.min(b.x), a.y.min(b.y), (a.x - b.x).abs(), (a.y - b.y).abs());
                    for id in edit::within(&d, band) {
                        if !self.selection.contains(&id) {
                            self.selection.push(id);
                        }
                    }
                }
            }
            Gesture::Move { from, to, .. } => {
                let (dx, dy, _) = self.move_delta(from, to);
                self.move_chosen(dx, dy);
            }
            Gesture::Connect { source, from, to } => {
                // A click on a shape's outline or point: the shape chosen, no edge.
                if (to.x - from.x).hypot(to.y - from.y) <= NUDGE * self.scale {
                    self.selection = vec![source];
                } else {
                    self.finish_connect(&source, to);
                }
                // Drawn with the line tool: it goes back to Select.
                if self.draw == super::draw::Draw::Line {
                    self.drawn();
                }
            }
            Gesture::Draw { from, to, square } => {
                self.finish_draw(from, to, square);
            }
            Gesture::Line { from, to } => {
                self.finish_line(from, to);
            }
            // A click: a shape beside it; let go on another shape: joined; on the sheet: a
            // shape like it there, joined.
            Gesture::Arrow { source, side, from, to } => {
                let dropped_on = self.shape_at(to, None);
                if (to.x - from.x).hypot(to.y - from.y) <= NUDGE * self.scale {
                    self.make_beside(&source, side, None);
                } else if dropped_on.as_deref() == Some(source.as_str()) {
                    // Brought back onto its own shape: nothing.
                } else if dropped_on.is_some() {
                    self.finish_connect(&source, to);
                } else {
                    self.make_beside(&source, side, Some(to));
                }
            }
            Gesture::Edge { id, handle, to } => {
                self.finish_edge(&id, handle, to);
            }
            Gesture::Place { section, entry, from, to } => {
                self.finish_place(section, entry, from, to);
            }
            Gesture::Resize { id, handle, from, to } => {
                let k = self.k();
                let Some((_, start)) = self.single() else { return true };
                let r = edit::resized(
                    start,
                    handle,
                    ((to.x - from.x) / k, (to.y - from.y) / k),
                    self.grid(),
                );
                self.resize(&id, start, r);
            }
        }
        true
    }

    /// The chosen cells moved by (`dx`, `dy`) diagram units, as one step.
    fn move_chosen(&mut self, dx: f32, dy: f32) -> bool {
        if self.locked || self.selection.is_empty() || (dx == 0.0 && dy == 0.0) {
            return false;
        }
        let page = self.page;
        let Some(d) = self.diagram.as_mut() else { return false };
        let ids: Vec<&str> = self.selection.iter().map(String::as_str).collect();
        let tx = diagram_model::ops::move_by(&d.doc, page, &ids, f64::from(dx), f64::from(dy));
        d.edit(tx, None)
    }

    /// Shape `id` (shown at `shown`) resized to `to` (both absolute).
    pub(super) fn resize(&mut self, id: &str, shown: Rect, to: Rect) -> bool {
        let page = self.page;
        let Some(d) = self.diagram.as_mut() else { return false };
        let Some(g) =
            d.doc.pages.get(page).and_then(|p| p.cell(id)).and_then(|c| c.geometry.as_ref())
        else {
            return false;
        };
        // Its own place is its parent's: the same offset as it's shown at.
        let (ox, oy) = (shown.min_x() - g.x as f32, shown.min_y() - g.y as f32);
        let bounds = (
            f64::from(to.min_x() - ox),
            f64::from(to.min_y() - oy),
            f64::from(to.size.width),
            f64::from(to.size.height),
        );
        let tx = diagram_model::ops::resize(&d.doc, page, id, bounds);
        d.edit(tx, None)
    }

    /// The chosen cells deleted (with what they hold and the edges on them).
    pub(super) fn delete_chosen(&mut self) -> bool {
        if self.locked || self.selection.is_empty() {
            return false;
        }
        let page = self.page;
        let Some(d) = self.diagram.as_mut() else { return false };
        let ids: Vec<&str> = self.selection.iter().map(String::as_str).collect();
        let tx = diagram_model::ops::delete(&d.doc, page, &ids);
        let done = d.edit(tx, None);
        self.selection.clear();
        done
    }

    /// An editing key: delete, move (⇧: by a grid step), let go. `true`: used.
    pub(super) fn key_edit(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        let step = if reach.extend { self.grid().unwrap_or(10.0) } else { 1.0 };
        match key {
            ViewerKey::Delete | ViewerKey::Backspace => self.delete_chosen(),
            // ↩: the one chosen cell's text edited.
            ViewerKey::Enter if self.selection.len() == 1 => {
                let id = self.selection[0].clone();
                self.edit_text(&id)
            }
            ViewerKey::Escape => {
                // A drag under way: only it ends (what's chosen stays); a drawing tool: back to
                // Select; else nothing chosen.
                if self.gesture.take().is_some() {
                    return true;
                }
                if self.draw != super::draw::Draw::Select {
                    self.choose_draw(super::draw::Draw::Select, false);
                    return true;
                }
                let had = !self.selection.is_empty();
                self.selection.clear();
                had
            }
            ViewerKey::Left => self.move_chosen(-step, 0.0),
            ViewerKey::Right => self.move_chosen(step, 0.0),
            ViewerKey::Up => self.move_chosen(0.0, -step),
            ViewerKey::Down => self.move_chosen(0.0, step),
            _ => false,
        }
    }

    /// What's chosen and what's being done: the cells' outlines, the one shape's handles; while
    /// dragging, where they'd go (and the guides met), the band.
    pub(super) fn paint_editing(
        &self,
        layer: &mut render_api::Layer,
        palette: &ui_components::Palette,
    ) {
        let accent = palette.accent;
        let line = self.scale.max(1.0).round();
        let outline = |layer: &mut render_api::Layer, r: Rect, w: f32| {
            for side in [
                Rect::new(r.min_x(), r.min_y(), r.size.width, w),
                Rect::new(r.min_x(), r.max_y() - w, r.size.width, w),
                Rect::new(r.min_x(), r.min_y(), w, r.size.height),
                Rect::new(r.max_x() - w, r.min_y(), w, r.size.height),
            ] {
                layer.quad(Quad::filled(side, accent));
            }
        };
        let Some(d) = self.drawing.as_ref().map(|(d, _)| d) else { return };
        for (_, r) in self.chosen_boxes() {
            outline(layer, self.on_screen(r), line);
        }
        // A chosen edge: its points marked.
        let chosen = self.chosen();
        for (_, way) in d.ways.iter().filter(|(id, _)| chosen.contains(id.as_str())) {
            for p in way {
                let s = self.camera.to_screen(*p, self.sheet(), self.scale);
                let h = 3.0 * self.scale;
                layer.quad(
                    Quad::filled(Rect::new(s.x - h, s.y - h, 2.0 * h, 2.0 * h), accent).rounded(h),
                );
            }
        }
        // The handles: not while its text is edited (the field is over it).
        if !self.locked
            && self.gesture.is_none()
            && self.editing.is_none()
            && let Some((_, r)) = self.single()
        {
            for (_, h) in edit::handles(self.on_screen(r), HANDLE * self.scale) {
                layer.quad(Quad::filled(h, accent).rounded(2.0 * self.scale));
                let inner = Rect::new(
                    h.min_x() + line,
                    h.min_y() + line,
                    h.size.width - 2.0 * line,
                    h.size.height - 2.0 * line,
                );
                layer.quad(Quad::filled(inner, palette.editor_background).rounded(self.scale));
            }
        }
        match &self.gesture {
            Some(Gesture::Move { from, to, moved: true }) => {
                let (dx, dy, guides) = self.move_delta(*from, *to);
                for (_, r) in self.chosen_boxes() {
                    let moved =
                        Rect::new(r.min_x() + dx, r.min_y() + dy, r.size.width, r.size.height);
                    outline(layer, self.on_screen(moved), line);
                }
                let sheet = self.sheet();
                for g in guides {
                    let guide = match g {
                        Guide::X(x) => {
                            let s = self.camera.to_screen(Point::new(x, 0.0), sheet, self.scale);
                            Rect::new(s.x, sheet.min_y(), line, sheet.size.height)
                        }
                        Guide::Y(y) => {
                            let s = self.camera.to_screen(Point::new(0.0, y), sheet, self.scale);
                            Rect::new(sheet.min_x(), s.y, sheet.size.width, line)
                        }
                    };
                    layer.quad(Quad::filled(guide, accent.with_alpha(0.7)));
                }
            }
            Some(Gesture::Resize { handle, from, to, .. }) => {
                if let Some((_, start)) = self.single() {
                    let k = self.k();
                    let r = edit::resized(
                        start,
                        *handle,
                        ((to.x - from.x) / k, (to.y - from.y) / k),
                        self.grid(),
                    );
                    outline(layer, self.on_screen(r), line);
                }
            }
            Some(Gesture::Band { from, to }) => {
                let r = Rect::new(
                    from.x.min(to.x),
                    from.y.min(to.y),
                    (from.x - to.x).abs(),
                    (from.y - to.y).abs(),
                );
                layer.quad(Quad::filled(r, accent.with_alpha(0.12)));
                outline(layer, r, line);
            }
            _ => {}
        }
    }

    /// The handle under `p` (the one chosen shape's), for its cursor.
    pub(super) fn handle_at(&self, p: Point) -> Option<Handle> {
        if self.locked {
            return None;
        }
        let (_, r) = self.single()?;
        edit::handles(self.on_screen(r), HANDLE * self.scale)
            .into_iter()
            .find(|(_, h)| h.contains(p))
            .map(|(h, _)| h)
    }
}
