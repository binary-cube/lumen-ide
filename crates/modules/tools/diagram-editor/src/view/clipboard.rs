//! Copy, cut, paste and duplicate as draw.io's XML: what's copied here pastes in draw.io and
//! back. Pasted cells get ids of their own, go in the page's first layer, a step down and
//! right of where they were, and become the ones chosen; a duplicate is a copy pasted in one
//! step.

use super::DiagramView;

/// How far a paste or a duplicate lands from the cells it came from (diagram units).
const STEP: f64 = 10.0;

/// A cell id that won't repeat (draw.io's are any unique text).
pub(super) fn new_id() -> String {
    let hex = format!("{:032x}", kernel::id::new_uuid());
    hex[..20].to_owned()
}

impl DiagramView {
    /// The chosen cells as draw.io's clipboard XML (`None`: nothing chosen).
    pub(super) fn copy_chosen(&self) -> Option<String> {
        if self.selection.is_empty() {
            return None;
        }
        let page = self.diagram.as_ref()?.doc.pages.get(self.page)?;
        let ids: Vec<&str> = self.selection.iter().map(String::as_str).collect();
        Some(diagram_model::clipboard::copy(page, &ids))
    }

    /// `text` pasted (draw.io's XML, or a whole file: its first page), a step from where it
    /// was; what's pasted chosen. `false`: it isn't a diagram (nothing done).
    pub(super) fn paste_cells(&mut self, text: &str) -> bool {
        if self.locked {
            return false;
        }
        let page = self.page;
        let Some(d) = self.diagram.as_mut() else { return false };
        let Some(layer) = d.doc.pages.get(page).and_then(first_layer) else { return false };
        let Ok((tx, top)) =
            diagram_model::clipboard::paste(&d.doc, page, text, &layer, (STEP, STEP), &mut new_id)
        else {
            return false;
        };
        if !d.edit(tx, None) {
            return false;
        }
        self.selection = top;
        true
    }

    /// The chosen cells copied and pasted a step away, as one edit.
    pub(super) fn duplicate_chosen(&mut self) -> bool {
        match self.copy_chosen() {
            Some(xml) => self.paste_cells(&xml),
            None => false,
        }
    }
}

/// The page's first layer (the root's first child): where pasted cells go.
pub(super) fn first_layer(page: &diagram_model::Page) -> Option<String> {
    let root = page.cells.iter().find(|c| c.parent.is_none())?;
    page.cells.iter().find(|c| c.parent.as_deref() == Some(root.id.as_str())).map(|c| c.id.clone())
}
