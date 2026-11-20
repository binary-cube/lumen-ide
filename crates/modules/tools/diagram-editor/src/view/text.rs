//! A cell's text edited in place: a double click opens the field over the shape (or at the
//! edge's middle), with its text as shown (an HTML label's lines, its tags left out); the
//! field takes the keys and the typing; Esc, a click outside or another press ends it and
//! the text is set (an HTML label written back as HTML: escaped, lines as `<br>`) — one
//! step of the history, nothing if it didn't change.

use kernel::{Point, Rect};
use ui_components::control::{Control, Press};
use ui_components::ds::Ctx;
use ui_components::{KeyReach, MultilineField, MultilineSpec, PressMods, ViewerKey};

use super::DiagramView;
use super::parts::Asked;
use crate::label;

/// A text being edited.
pub(super) struct Editing {
    id: String,
    field: MultilineField,
    /// The cell's text is HTML (written back as such).
    html: bool,
    /// Its text when the editing began (unchanged: nothing written).
    before: String,
    /// Where (diagram units).
    area: Rect,
    /// Where the field was last drawn (screen).
    drawn: Rect,
    /// A text being made: the cell added once the field ends with something in it (one
    /// step; left empty, nothing).
    pending: Option<diagram_model::Cell>,
}

/// A text as HTML: escaped, its lines as `<br>`.
fn as_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\n' => out.push_str("<br>"),
            c => out.push(c),
        }
    }
    out
}

impl DiagramView {
    /// Cell `id`'s text opened for editing. `false`: locked, or no such cell shown.
    pub(super) fn edit_text(&mut self, id: &str) -> bool {
        if self.locked {
            return false;
        }
        let Some(drawing) = self.drawing.as_ref().map(|(d, _)| std::sync::Arc::clone(d)) else {
            return false;
        };
        let Some(cell) =
            self.diagram.as_ref().and_then(|d| d.doc.pages.get(self.page)).and_then(|p| p.cell(id))
        else {
            return false;
        };
        let way = drawing.ways.iter().find(|(w, _)| w == id).map(|(_, way)| way[way.len() / 2]);
        let area = match drawing.boxes.iter().find(|(b, _)| b == id) {
            Some((_, r)) => *r,
            None => match way {
                Some(mid) => Rect::new(mid.x - 60.0, mid.y - 15.0, 120.0, 30.0),
                // Made just now (not drawn yet): its geometry, in its parent's box.
                None => {
                    let Some(g) = cell.geometry.as_ref().filter(|g| !g.relative) else {
                        return false;
                    };
                    let (ox, oy) = cell
                        .parent
                        .as_deref()
                        .and_then(|p| drawing.boxes.iter().find(|(b, _)| b == p))
                        .map_or((0.0, 0.0), |(_, r)| (r.min_x(), r.min_y()));
                    let (x, y, w, h) = g.bounds();
                    Rect::new(x as f32 + ox, y as f32 + oy, w as f32, h as f32)
                }
            },
        };
        let html = cell.style.flag("html");
        let before = label::plain(&cell.value, html);
        let mut field = MultilineField::new(&before);
        field.select_all();
        self.editing = Some(Editing {
            id: id.to_owned(),
            field,
            html,
            before,
            area,
            drawn: Rect::default(),
            pending: None,
        });
        self.selection = vec![id.to_owned()];
        true
    }

    /// The editing ended: its text set (as one step), if it changed.
    pub(super) fn end_text(&mut self) -> bool {
        let Some(e) = self.editing.take() else { return false };
        let text = e.field.text().to_owned();
        let value = if e.html { as_html(&text) } else { text.clone() };
        let page = self.page;
        // A text being made: added with what was typed (nothing typed: nothing made).
        if let Some(mut cell) = e.pending {
            if text.trim().is_empty() {
                self.selection.clear();
                return true;
            }
            cell.value = value;
            if let Some(d) = self.diagram.as_mut() {
                let tx = diagram_model::ops::add(&d.doc, page, cell);
                if d.edit(tx, None) {
                    self.selection = vec![e.id];
                }
            }
            return true;
        }
        if text == e.before {
            return true;
        }
        if let Some(d) = self.diagram.as_mut() {
            let tx = diagram_model::ops::set_value(&d.doc, page, &e.id, &value);
            d.edit(tx, None);
        }
        true
    }

    /// A text made at diagram point `at` (centered, on the grid), its field opened with its
    /// placeholder chosen (typing replaces it).
    pub(super) fn new_text_at(&mut self, at: Point) -> bool {
        let Some((s, e)) = super::draw::entry_index("text") else { return false };
        let size = crate::palette::SECTIONS[s].entries[e].size;
        self.new_text_in(Rect::new(at.x - size.0 / 2.0, at.y - size.1 / 2.0, size.0, size.1))
    }

    /// A text to be made in box `r` (diagram units, put on the grid): its field opened with
    /// its placeholder chosen; the cell is added when the field ends with something typed
    /// (one step), never left empty.
    pub(super) fn new_text_in(&mut self, r: Rect) -> bool {
        if self.locked {
            return false;
        }
        let Some((s, e)) = super::draw::entry_index("text") else { return false };
        let Some(cell) = self.entry_cell(s, e, r) else { return false };
        let Some((x, y, w, h)) = cell.geometry.as_ref().map(diagram_model::Geometry::bounds) else {
            return false;
        };
        let before = label::plain(&cell.value, cell.style.flag("html"));
        let mut field = MultilineField::new(&before);
        field.select_all();
        self.selection.clear();
        self.editing = Some(Editing {
            id: cell.id.clone(),
            html: cell.style.flag("html"),
            field,
            before,
            area: Rect::new(x as f32, y as f32, w as f32, h as f32),
            drawn: Rect::default(),
            pending: Some(cell),
        });
        true
    }

    /// The field over its cell (at least wide and tall enough to type in).
    pub(super) fn paint_text_field(
        &mut self,
        scene: &mut render_api::Scene,
        text: &mut text_layout::TextSystem,
        ds: &Ctx<'_>,
    ) {
        let sheet = self.sheet();
        let (scale, camera) = (self.scale, self.camera);
        let Some(e) = self.editing.as_mut() else { return };
        let o = camera.to_screen(e.area.origin, sheet, scale);
        let k = camera.k(scale);
        let (w, h) = (
            (e.area.size.width * k).max(160.0 * scale),
            (e.area.size.height * k).max(48.0 * scale),
        );
        let r = Rect::new(
            o.x + (e.area.size.width * k - w) / 2.0,
            o.y + (e.area.size.height * k - h) / 2.0,
            w,
            h,
        );
        e.drawn = r;
        e.field.paint(
            scene,
            text,
            ds,
            r,
            MultilineSpec { placeholder: "", focused: true, subject_limit: None },
        );
    }
}

/// The text being edited as a control: a press in its field puts the caret (⇧ extends, two
/// a word, three a line) and a drag selects; it has the keys, the typing and the edit
/// commands while it's open (Tab included: nothing moves the focus away); Esc, or the focus
/// gone (a press anywhere else), asks the view to end it.
pub(super) struct TextPart<'a> {
    pub(super) editing: Option<&'a mut Editing>,
    pub(super) asked: &'a mut Option<Asked>,
}

impl Editing {
    /// Its field, as a control read from `&self` (its selection).
    pub(super) fn field_control(&self) -> &dyn Control {
        &self.field
    }
}

#[cfg(test)]
impl DiagramView {
    /// The text edited in place: what it has selected.
    pub(super) fn text_selected(&self) -> Option<String> {
        Control::selected_text(&self.editing.as_ref()?.field)
    }
}

impl TextPart<'_> {
    fn field(&mut self) -> Option<&mut MultilineField> {
        self.editing.as_mut().map(|e| &mut e.field)
    }
}

impl Control for TextPart<'_> {
    fn hit(&self, p: Point) -> bool {
        self.editing.as_ref().is_some_and(|e| e.drawn.contains(p))
    }

    fn press_with(&mut self, p: Point, clicks: u32, mods: PressMods) -> Press {
        let Some(field) = self.field() else { return Press::Ignored };
        field.press(p, clicks, mods.shift);
        Press::Held
    }

    fn drag(&mut self, p: Point) -> bool {
        self.field().is_some_and(|f| f.drag(p))
    }

    fn release(&mut self) -> bool {
        self.field().is_some_and(MultilineField::release)
    }

    fn focusable(&self) -> bool {
        self.editing.is_some()
    }

    fn set_focus(&mut self, focused: bool) {
        if !focused && self.editing.is_some() {
            *self.asked = Some(Asked::EndText);
        }
    }

    fn keeps_tab(&self) -> bool {
        true
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        if key == ViewerKey::Escape && self.editing.is_some() {
            *self.asked = Some(Asked::EndText);
            return true;
        }
        self.field().is_some_and(|f| Control::key(f, key, reach))
    }

    fn text(&mut self, typed: &str) -> bool {
        self.field().is_some_and(|f| Control::text(f, typed))
    }

    fn selected_text(&self) -> Option<String> {
        self.editing.as_ref().and_then(|e| Control::selected_text(&e.field))
    }

    fn command(&mut self, id: &str) -> bool {
        self.field().is_some_and(|f| Control::command(f, id))
    }
}

#[cfg(test)]
#[path = "../../tests/unit/view_text.rs"]
mod tests;
