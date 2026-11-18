//! Copy and paste as draw.io's: cells copied are an `<mxGraphModel>` (its root and layer,
//! then the cells), so what's copied here pastes in draw.io and back. Copied: the cells
//! chosen, what they hold and the edges between them. Pasted: new ids (their links among
//! themselves kept, ends outside them let loose), in the layer given, moved by an offset.

use std::collections::{HashMap, HashSet};

use crate::Error;
use crate::cell::{Cell, CellKind};
use crate::document::{Document, Page};
use crate::edit::{Change, Transaction};

/// The cells `ids` of `page` (with what they hold and the edges between them) as draw.io's
/// clipboard XML.
pub fn copy(page: &Page, ids: &[&str]) -> String {
    let mut chosen: HashSet<&str> = ids.iter().copied().collect();
    // What they hold and the edges between them (and what those hold: their labels), until
    // nothing more comes.
    loop {
        let before = chosen.len();
        for c in &page.cells {
            let inside = |end: &Option<String>| end.as_deref().is_some_and(|e| chosen.contains(e));
            let held = inside(&c.parent);
            let between = c.kind == CellKind::Edge && inside(&c.source) && inside(&c.target);
            if held || between {
                chosen.insert(&c.id);
            }
        }
        if chosen.len() == before {
            break;
        }
    }
    let mut clip = Page::new("", "");
    // The page's cells by id (a copied cell's parents are looked up there, never searched).
    let by_id: HashMap<&str, &Cell> = page.cells.iter().map(|c| (c.id.as_str(), c)).collect();
    for c in
        page.cells.iter().filter(|c| c.kind != CellKind::Holder && chosen.contains(c.id.as_str()))
    {
        let mut c = c.clone();
        // On top of what's copied: in the clipboard's layer, at its place on the page (a
        // group left behind no longer places it), as draw.io reads it.
        if !c.parent.as_deref().is_some_and(|p| chosen.contains(p)) {
            let (ox, oy) = offset(&by_id, c.parent.as_deref());
            c.parent = Some("1".into());
            if let Some(g) = c.geometry.as_mut().filter(|_| ox != 0.0 || oy != 0.0) {
                if !g.relative {
                    g.x += ox;
                    g.y += oy;
                }
                for pt in g.points.iter_mut().chain(&mut g.source_point).chain(&mut g.target_point)
                {
                    pt.x += ox;
                    pt.y += oy;
                }
            }
        }
        clip.cells.push(c);
    }
    clip.model_xml()
}

/// Where cell `parent`'s children are placed from on the page: the sum of its shapes' places
/// up to the layer (none for a layer). At most as deep as a file may nest.
fn offset<'a>(by_id: &HashMap<&str, &'a Cell>, mut parent: Option<&'a str>) -> (f64, f64) {
    let (mut x, mut y) = (0.0, 0.0);
    for _ in 0..64 {
        let Some(cell) = parent.and_then(|id| by_id.get(id)) else { break };
        if cell.kind != CellKind::Vertex {
            break;
        }
        if let Some(g) = cell.geometry.as_ref().filter(|g| !g.relative) {
            x += g.x;
            y += g.y;
        }
        parent = cell.parent.as_deref();
    }
    (x, y)
}

/// `text` (draw.io's clipboard XML, or a whole file: its first page) pasted on page `page`:
/// each cell a new id from `new_id`, its parent the layer `layer` unless it was inside
/// another pasted, moved by (`dx`, `dy`). The transaction and the new ids of the cells
/// pasted at the top (what to select).
pub fn paste(
    doc: &Document,
    page: usize,
    text: &str,
    layer: &str,
    (dx, dy): (f64, f64),
    new_id: &mut dyn FnMut() -> String,
) -> Result<(Transaction, Vec<String>), Error> {
    let pasted = Document::read(text)?;
    let Some(source) = pasted.pages.into_iter().next() else { return Ok(Default::default()) };
    let Some(target) = doc.pages.get(page) else { return Ok(Default::default()) };
    let cells: Vec<Cell> =
        source.cells.into_iter().filter(|c| c.kind != CellKind::Holder).collect();
    let taken: HashSet<&str> = target.cells.iter().map(|c| c.id.as_str()).collect();
    let mut ids: HashMap<String, String> = HashMap::new();
    let mut given: HashSet<String> = HashSet::new();
    for c in &cells {
        let mut id = new_id();
        while taken.contains(id.as_str()) || given.contains(&id) {
            id = new_id();
        }
        given.insert(id.clone());
        ids.insert(c.id.clone(), id);
    }
    let mut tx = Transaction::default();
    let mut top = Vec::new();
    // Each on top of the page, in the order copied.
    for (at, mut c) in (target.cells.len()..).zip(cells) {
        let inside = c.parent.as_ref().and_then(|p| ids.get(p)).cloned();
        let on_top = inside.is_none();
        c.id = ids[&c.id].clone();
        c.parent = Some(inside.unwrap_or_else(|| layer.to_owned()));
        c.source = c.source.as_ref().and_then(|s| ids.get(s)).cloned();
        c.target = c.target.as_ref().and_then(|t| ids.get(t)).cloned();
        if on_top && let Some(g) = c.geometry.as_mut() {
            if c.kind == CellKind::Vertex && !g.relative {
                g.x += dx;
                g.y += dy;
            }
            for pt in g.points.iter_mut().chain(&mut g.source_point).chain(&mut g.target_point) {
                pt.x += dx;
                pt.y += dy;
            }
        }
        if on_top {
            top.push(c.id.clone());
        }
        tx.changes.push(Change::Insert { page, at, cell: Box::new(c) });
    }
    Ok((tx, top))
}

#[cfg(test)]
#[path = "../tests/unit/clipboard.rs"]
mod tests;
