//! The drawing tools: Select (choose, move, join — the usual), a shape drawn by dragging on
//! the sheet (a rectangle, an ellipse, a rhombus, a text: in the box dragged, ⇧ a square; a
//! click alone makes it its usual size there), or a line (from a shape to another it joins
//! them; on the sheet it's a loose line). After one, the tool goes back to Select; chosen by a
//! double click it stays, until Esc or Select.

use kernel::{Point, Rect};

use super::DiagramView;
use crate::palette::SECTIONS;

/// A tool of the bar's drawing group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Draw {
    #[default]
    Select,
    Rect,
    Ellipse,
    Rhombus,
    Text,
    Line,
}

/// A loose line's style: draw.io's plain arrow.
const LINE_STYLE: &str = "endArrow=classic;html=1;rounded=0;";

impl Draw {
    /// The palette's shape it draws.
    fn entry(self) -> Option<&'static str> {
        match self {
            Self::Rect => Some("rectangle"),
            Self::Ellipse => Some("ellipse"),
            Self::Rhombus => Some("diamond"),
            Self::Text => Some("text"),
            Self::Select | Self::Line => None,
        }
    }
}

/// Where the palette's shape `id` is (its section, its place).
pub(super) fn entry_index(id: &str) -> Option<(usize, usize)> {
    SECTIONS
        .iter()
        .enumerate()
        .find_map(|(s, section)| section.entries.iter().position(|e| e.id == id).map(|e| (s, e)))
}

/// The box dragged from `a` to `b`, a square when `square` (its side the larger, from `a`).
pub(super) fn dragged_box(a: Point, b: Point, square: bool) -> Rect {
    let (mut w, mut h) = (b.x - a.x, b.y - a.y);
    if square {
        let side = w.abs().max(h.abs());
        w = side.copysign(w);
        h = side.copysign(h);
    }
    Rect::new(a.x.min(a.x + w), a.y.min(a.y + h), w.abs(), h.abs())
}

impl DiagramView {
    /// The tool chosen (a double click keeps it after a shape).
    pub(super) fn choose_draw(&mut self, draw: Draw, kept: bool) {
        self.draw = draw;
        self.draw_kept = kept && draw != Draw::Select;
        self.gesture = None;
    }

    /// A shape drawn: the tool goes back to Select unless it's kept.
    pub(super) fn drawn(&mut self) {
        if !self.draw_kept {
            self.draw = Draw::Select;
        }
    }

    /// A press on the sheet with a drawing tool: what it starts (`None`: Select's own).
    pub(super) fn press_draw(&self, p: Point, square: bool) -> Option<super::gesture::Gesture> {
        use super::gesture::Gesture;
        if self.locked {
            return None;
        }
        match self.draw {
            Draw::Select => None,
            Draw::Line => Some(match self.shape_at(p, None) {
                Some(source) => Gesture::Connect { source, from: p, to: p },
                None => Gesture::Line { from: p, to: p },
            }),
            _ => Some(Gesture::Draw { from: p, to: p, square }),
        }
    }

    /// The shape dragged from `from` to `to` (screen) made — or, a click, its usual size
    /// there — chosen; a text's field opened.
    pub(super) fn finish_draw(&mut self, from: Point, to: Point, square: bool) -> bool {
        let Some((s, e)) = self.draw.entry().and_then(entry_index) else { return false };
        let (a, b) = (self.at(from), self.at(to));
        let clicked = (to.x - from.x).hypot(to.y - from.y) <= super::gesture::NUDGE * self.scale;
        let size = SECTIONS[s].entries[e].size;
        let r = if clicked {
            Rect::new(a.x - size.0 / 2.0, a.y - size.1 / 2.0, size.0, size.1)
        } else {
            dragged_box(a, b, square)
        };
        // A text: its field opened there (made once something's typed).
        let made =
            if self.draw == Draw::Text { self.new_text_in(r) } else { self.add_entry_in(s, e, r) };
        self.drawn();
        made
    }

    /// A loose line from `from` to `to` (screen), its end on a shape there joined to it.
    pub(super) fn finish_line(&mut self, from: Point, to: Point) -> bool {
        self.drawn();
        if (to.x - from.x).hypot(to.y - from.y) <= super::gesture::NUDGE * self.scale {
            return false;
        }
        let target = self.shape_at(to, None);
        let (a, b) = (self.grid_point(self.at(from)), self.grid_point(self.at(to)));
        let page = self.page;
        let Some(d) = self.diagram.as_mut() else { return false };
        let Some(layer) = d.doc.pages.get(page).and_then(super::clipboard::first_layer) else {
            return false;
        };
        let id = super::clipboard::new_id();
        let mut edge = diagram_model::Cell::new(id.clone(), diagram_model::CellKind::Edge, layer);
        edge.style = diagram_model::Style::parse(LINE_STYLE);
        let point = |p: Point| diagram_model::Point::new(f64::from(p.x), f64::from(p.y));
        let mut g = diagram_model::Geometry {
            relative: true,
            source_point: Some(point(a)),
            ..Default::default()
        };
        match target {
            Some(t) => edge.target = Some(t),
            None => g.target_point = Some(point(b)),
        }
        edge.geometry = Some(g);
        let tx = diagram_model::ops::add(&d.doc, page, edge);
        let done = d.edit(tx, None);
        if done {
            self.selection = vec![id];
        }
        done
    }
}

#[cfg(test)]
#[path = "../../tests/unit/view_draw.rs"]
mod tests;
