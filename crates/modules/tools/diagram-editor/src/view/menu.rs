//! The right-click menu of the chosen cells (the cell under the mouse chosen first): an
//! edge's line (straight, at right angles, curved) and end; to the front or the back; group,
//! ungroup; duplicate; delete. Each row one step of the history; a style of two keys (a
//! curved line takes its routing away) one step too.

use i18n::t;
use kernel::Point;
use ui_components::ViewMenuEntry;

use super::DiagramView;

/// Style keys set (`None`: taken away).
type Sets = [(&'static str, Option<&'static str>); 2];

/// The edges' line kinds: its menu id, its label, what's set.
const LINES: [(&str, &str, Sets); 3] = [
    ("line:straight", "diagram_editor.line_straight", [("edgeStyle", None), ("curved", None)]),
    (
        "line:orthogonal",
        "diagram_editor.line_orthogonal",
        [("edgeStyle", Some("orthogonalEdgeStyle")), ("curved", None)],
    ),
    ("line:curved", "diagram_editor.line_curved", [("edgeStyle", None), ("curved", Some("1"))]),
];

/// The ends an edge's arrow can be.
const ENDS: [&str; 6] = ["none", "classic", "block", "open", "oval", "diamond"];

impl DiagramView {
    /// The chosen edges (their ids).
    fn chosen_edges(&self) -> Vec<String> {
        let Some(page) = self.diagram.as_ref().and_then(|d| d.doc.pages.get(self.page)) else {
            return Vec::new();
        };
        self.selection
            .iter()
            .filter(|id| page.cell(id).is_some_and(|c| c.kind == diagram_model::CellKind::Edge))
            .cloned()
            .collect()
    }

    /// The menu at `p` (`None`: nothing there or chosen).
    pub(super) fn menu_at(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        if !self.sheet().contains(p) {
            return None;
        }
        let at = self.camera.to_diagram(p, self.sheet(), self.scale);
        let near = 5.0 / self.camera.k(self.scale);
        if let Some(id) = self.drawing.as_ref().and_then(|(d, _)| crate::edit::hit(d, at, near))
            && !self.selection.contains(&id)
        {
            self.selection = vec![id];
        }
        // Nothing chosen: the page's own (its exports).
        if self.selection.is_empty() {
            return self.export_menu();
        }
        let locked = self.locked;
        let item = |id: &str, key: &str| ViewMenuEntry::item(id, t(key)).disabled_if(locked);
        let mut entries = Vec::new();
        let edges = self.chosen_edges();
        if let Some(first) =
            edges.first().and_then(|id| self.diagram.as_ref()?.doc.pages.get(self.page)?.cell(id))
        {
            let style = &first.style;
            let kind = if style.flag("curved") {
                "line:curved"
            } else if style.get("edgeStyle").is_some() {
                "line:orthogonal"
            } else {
                "line:straight"
            };
            let lines = LINES
                .iter()
                .map(|(id, label, _)| item(id, label).checked_if(*id == kind))
                .collect();
            entries.push(ViewMenuEntry::submenu(t("diagram_editor.line"), lines));
            let end = style.get("endArrow").unwrap_or("classic");
            let ends = ENDS
                .iter()
                .map(|e| {
                    item(&format!("end:{e}"), &format!("diagram_editor.end_{e}"))
                        .checked_if(*e == end)
                })
                .collect();
            entries.push(ViewMenuEntry::submenu(t("diagram_editor.end"), ends));
            entries.push(ViewMenuEntry::Separator);
        }
        entries.push(item("to-front", "diagram_editor.to_front"));
        entries.push(item("to-back", "diagram_editor.to_back"));
        entries.push(ViewMenuEntry::Separator);
        if self.selection.len() >= 2 {
            entries.push(item("group", "diagram_editor.group"));
        }
        let page = self.diagram.as_ref().and_then(|d| d.doc.pages.get(self.page));
        let is_group = self.selection.len() == 1
            && page.is_some_and(|p| {
                p.cells.iter().any(|c| c.parent.as_deref() == Some(self.selection[0].as_str()))
            });
        if is_group {
            entries.push(item("ungroup", "diagram_editor.ungroup"));
        }
        entries.push(item("duplicate", "diagram_editor.duplicate"));
        entries.push(item("delete", "diagram_editor.delete"));
        Some(entries)
    }

    /// A row of the menu chosen. `true`: it did something.
    pub(super) fn menu_command(&mut self, id: &str) -> bool {
        if self.locked {
            return false;
        }
        let page = self.page;
        let ids: Vec<String> = self.selection.clone();
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        if let Some((_, _, sets)) = LINES.iter().find(|(line, _, _)| *line == id) {
            let edges = self.chosen_edges();
            let edges: Vec<&str> = edges.iter().map(String::as_str).collect();
            return self.set_styles(&edges, sets);
        }
        if let Some(end) = id.strip_prefix("end:") {
            let edges = self.chosen_edges();
            let edges: Vec<&str> = edges.iter().map(String::as_str).collect();
            return self.set_styles(&edges, &[("endArrow", Some(end))]);
        }
        let Some(d) = self.diagram.as_mut() else { return false };
        match id {
            "to-front" | "to-back" => {
                let tx = diagram_model::ops::reorder(&d.doc, page, &refs, id == "to-front");
                d.edit(tx, None)
            }
            "group" => {
                let group = super::clipboard::new_id();
                let Some(tx) = diagram_model::ops::group(&d.doc, page, &refs, &group) else {
                    return false;
                };
                let done = d.edit(tx, None);
                if done {
                    self.selection = vec![group];
                }
                done
            }
            "ungroup" => {
                let Some(tx) =
                    refs.first().and_then(|g| diagram_model::ops::ungroup(&d.doc, page, g))
                else {
                    return false;
                };
                d.edit(tx, None)
            }
            "duplicate" => self.duplicate_chosen(),
            "delete" => self.delete_chosen(),
            _ => false,
        }
    }

    /// Style keys set on `ids` (`None`: taken away), as one step.
    fn set_styles(&mut self, ids: &[&str], sets: &[(&str, Option<&str>)]) -> bool {
        let page = self.page;
        let Some(d) = self.diagram.as_mut() else { return false };
        if ids.is_empty() {
            return false;
        }
        // One gesture: its steps one in the history.
        let gesture = kernel::id::new_uuid() as u64;
        let mut done = false;
        for (key, value) in sets {
            let tx = diagram_model::ops::set_style(&d.doc, page, ids, key, *value);
            done |= d.edit(tx, Some(gesture));
        }
        d.history.seal();
        done
    }
}
