//! The edits a diagram is made with, as transactions made from the document (nothing changed
//! until it's applied): a cell added; cells deleted (with what they hold and the edges on
//! them), moved (the edges between them following), resized; their style or text set;
//! grouped and ungrouped; brought to the front or sent to the back; an edge's ends connected.

use std::collections::HashSet;

use crate::cell::{Cell, CellKind};
use crate::document::{Document, Page};
use crate::edit::{Change, Transaction};
use crate::geometry::Geometry;

/// A page's cell at `at`, with its place.
fn place(page: &Page, id: &str) -> Option<usize> {
    page.cells.iter().position(|c| c.id == id)
}

/// The cells `ids` and everything under them (their children, theirs…).
fn with_descendants(page: &Page, ids: &[&str]) -> HashSet<String> {
    let mut set: HashSet<String> = ids.iter().map(|s| (*s).to_owned()).collect();
    loop {
        let before = set.len();
        for c in &page.cells {
            if c.parent.as_ref().is_some_and(|p| set.contains(p)) {
                set.insert(c.id.clone());
            }
        }
        if set.len() == before {
            return set;
        }
    }
}

/// Replacements of the cells `edit` changes (those it leaves alone aren't in it). Only the
/// cells `touches` lets through are copied to be edited: a drag's step on a big page copies
/// what it moves, not the page.
fn replace_each(
    page_index: usize,
    page: &Page,
    touches: impl Fn(&Cell) -> bool,
    mut edit: impl FnMut(&mut Cell) -> bool,
) -> Transaction {
    let mut tx = Transaction::default();
    for (at, cell) in page.cells.iter().enumerate() {
        if !touches(cell) {
            continue;
        }
        let mut changed = cell.clone();
        if edit(&mut changed) && changed != *cell {
            tx.changes.push(Change::Replace { page: page_index, at, cell: Box::new(changed) });
        }
    }
    tx
}

/// Page setting `name` set to `value` (`None`: taken away); nothing if it's that already.
pub fn set_page_setting(
    doc: &Document,
    page: usize,
    name: &str,
    value: Option<&str>,
) -> Transaction {
    let Some(p) = doc.pages.get(page) else { return Transaction::default() };
    if p.setting(name) == value {
        return Transaction::default();
    }
    Transaction {
        changes: vec![Change::Setting {
            page,
            name: name.to_owned(),
            value: value.map(str::to_owned),
        }],
    }
}

/// `cell` added on top of the page.
pub fn add(doc: &Document, page: usize, cell: Cell) -> Transaction {
    let Some(p) = doc.pages.get(page) else { return Transaction::default() };
    Transaction { changes: vec![Change::Insert { page, at: p.cells.len(), cell: Box::new(cell) }] }
}

/// The cells `ids` deleted, with what they hold and the edges on any of them.
pub fn delete(doc: &Document, page: usize, ids: &[&str]) -> Transaction {
    let Some(p) = doc.pages.get(page) else { return Transaction::default() };
    let mut gone = with_descendants(p, ids);
    // An edge on a cell going goes too (and what it holds: its label).
    let edges: Vec<&str> = p
        .cells
        .iter()
        .filter(|c| c.kind == CellKind::Edge)
        .filter(|c| {
            [&c.source, &c.target].iter().any(|t| t.as_ref().is_some_and(|t| gone.contains(t)))
        })
        .map(|c| c.id.as_str())
        .collect();
    gone.extend(with_descendants(p, &edges));
    // Taken out from the last, so the places before stay right.
    let changes = (0..p.cells.len())
        .rev()
        .filter(|&at| gone.contains(&p.cells[at].id) && p.cells[at].kind != CellKind::Holder)
        .map(|at| Change::Remove { page, at })
        .collect();
    Transaction { changes }
}

/// The cells `ids` moved by (`dx`, `dy`): shapes (one inside another moved goes with it,
/// not twice), an edge's own points, and the points of the edges between moved shapes.
pub fn move_by(doc: &Document, page: usize, ids: &[&str], dx: f64, dy: f64) -> Transaction {
    let Some(p) = doc.pages.get(page) else { return Transaction::default() };
    let chosen: HashSet<&str> = ids.iter().copied().collect();
    let moved = with_descendants(p, ids);
    // Inside a moved one: it moves with it (its place is its parent's).
    let carried = |c: &Cell| c.parent.as_ref().is_some_and(|parent| moved.contains(parent));
    let shift = |g: &mut Geometry| {
        for pt in g.points.iter_mut().chain(&mut g.source_point).chain(&mut g.target_point) {
            pt.x += dx;
            pt.y += dy;
        }
    };
    // What moves (decided before a cell is copied: on a big page only those are): a chosen
    // shape, a chosen edge, an edge between moved shapes — never one inside a moved one.
    let moves = |c: &Cell| {
        let Some(g) = c.geometry.as_ref() else { return false };
        if carried(c) {
            return false;
        }
        let own = chosen.contains(c.id.as_str());
        let end_moves = |end: &Option<String>| end.as_ref().is_none_or(|e| moved.contains(e));
        let attached = c.source.is_some() || c.target.is_some();
        let between = attached && end_moves(&c.source) && end_moves(&c.target);
        match c.kind {
            CellKind::Vertex => own && !g.relative,
            CellKind::Edge => own || between,
            CellKind::Holder => false,
        }
    };
    replace_each(page, p, moves, |c| {
        let kind = c.kind;
        let Some(g) = c.geometry.as_mut() else { return false };
        match kind {
            CellKind::Vertex => {
                g.x += dx;
                g.y += dy;
            }
            _ => shift(g),
        }
        true
    })
}

/// Cell `id`'s box set to (`x`, `y`, `width`, `height`) (sizes at least 1).
pub fn resize(doc: &Document, page: usize, id: &str, bounds: (f64, f64, f64, f64)) -> Transaction {
    let Some(p) = doc.pages.get(page) else { return Transaction::default() };
    let (x, y, width, height) = bounds;
    replace_each(
        page,
        p,
        |c| c.id == id,
        |c| {
            if c.id != id || c.kind != CellKind::Vertex {
                return false;
            }
            let g = c.geometry.get_or_insert_with(Geometry::default);
            (g.x, g.y, g.width, g.height) = (x, y, width.max(1.0), height.max(1.0));
            true
        },
    )
}

/// Style key `key` of the cells `ids` set to `value` (`None`: taken away).
pub fn set_style(
    doc: &Document,
    page: usize,
    ids: &[&str],
    key: &str,
    value: Option<&str>,
) -> Transaction {
    let Some(p) = doc.pages.get(page) else { return Transaction::default() };
    let chosen: HashSet<&str> = ids.iter().copied().collect();
    replace_each(
        page,
        p,
        |c| chosen.contains(c.id.as_str()),
        |c| {
            match value {
                Some(v) => c.style.set(key, v),
                None => c.style.remove(key),
            }
            true
        },
    )
}

/// Several style keys of the cells `ids` set at once (`None`: taken away), as one step;
/// `keys` gives each cell its own (a shape's fill, an edge's line only).
pub fn set_styles<'a>(
    doc: &Document,
    page: usize,
    ids: &[&str],
    keys: impl Fn(&Cell) -> Vec<(&'a str, Option<&'a str>)>,
) -> Transaction {
    let Some(p) = doc.pages.get(page) else { return Transaction::default() };
    let chosen: HashSet<&str> = ids.iter().copied().collect();
    replace_each(
        page,
        p,
        |c| chosen.contains(c.id.as_str()),
        |c| {
            for (key, value) in keys(c) {
                match value {
                    Some(v) => c.style.set(key, v),
                    None => c.style.remove(key),
                }
            }
            true
        },
    )
}

/// Cell `id`'s text set.
pub fn set_value(doc: &Document, page: usize, id: &str, value: &str) -> Transaction {
    let Some(p) = doc.pages.get(page) else { return Transaction::default() };
    replace_each(
        page,
        p,
        |c| c.id == id,
        |c| {
            if c.id != id {
                return false;
            }
            c.value = value.to_owned();
            true
        },
    )
}

/// Cell `id` changed by `change` (its ends, its points, its style together): one step.
pub fn update(
    doc: &Document,
    page: usize,
    id: &str,
    change: impl FnOnce(&mut Cell),
) -> Transaction {
    let Some(p) = doc.pages.get(page) else { return Transaction::default() };
    let mut change = Some(change);
    replace_each(
        page,
        p,
        |c| c.id == id,
        |c| {
            if c.id != id {
                return false;
            }
            if let Some(change) = change.take() {
                change(c);
            }
            true
        },
    )
}

/// Cell `id`'s geometry set (an edge's points, its loose ends; a shape's box).
pub fn set_geometry(doc: &Document, page: usize, id: &str, geometry: Geometry) -> Transaction {
    let Some(p) = doc.pages.get(page) else { return Transaction::default() };
    replace_each(
        page,
        p,
        |c| c.id == id,
        |c| {
            if c.id != id {
                return false;
            }
            c.geometry = Some(geometry.clone());
            true
        },
    )
}

/// Edge `id`'s ends set (`None`: loose; its point there kept as it was drawn).
pub fn connect(
    doc: &Document,
    page: usize,
    id: &str,
    source: Option<&str>,
    target: Option<&str>,
) -> Transaction {
    let Some(p) = doc.pages.get(page) else { return Transaction::default() };
    replace_each(
        page,
        p,
        |c| c.id == id,
        |c| {
            if c.id != id || c.kind != CellKind::Edge {
                return false;
            }
            c.source = source.map(str::to_owned);
            c.target = target.map(str::to_owned);
            true
        },
    )
}

/// The shapes `ids` (of one parent) grouped in a new group `group_id`, in the place of the
/// first: its box around theirs, their places now inside it. `None`: fewer than two shapes,
/// or not of one parent.
pub fn group(doc: &Document, page: usize, ids: &[&str], group_id: &str) -> Option<Transaction> {
    let p = doc.pages.get(page)?;
    let chosen: HashSet<&str> = ids.iter().copied().collect();
    let members: Vec<(usize, &Cell)> = p
        .cells
        .iter()
        .enumerate()
        .filter(|(_, c)| chosen.contains(c.id.as_str()) && c.kind == CellKind::Vertex)
        .collect();
    let parent = members.first()?.1.parent.clone()?;
    if members.len() < 2 || members.iter().any(|(_, c)| c.parent.as_ref() != Some(&parent)) {
        return None;
    }
    let boxes: Vec<(f64, f64, f64, f64)> =
        members.iter().filter_map(|(_, c)| c.geometry.as_ref().map(Geometry::bounds)).collect();
    // No box to put around them.
    if boxes.is_empty() {
        return None;
    }
    let x = boxes.iter().map(|b| b.0).fold(f64::INFINITY, f64::min);
    let y = boxes.iter().map(|b| b.1).fold(f64::INFINITY, f64::min);
    let right = boxes.iter().map(|b| b.0 + b.2).fold(f64::NEG_INFINITY, f64::max);
    let bottom = boxes.iter().map(|b| b.1 + b.3).fold(f64::NEG_INFINITY, f64::max);
    let mut g = Cell::new(group_id, CellKind::Vertex, parent);
    g.style = crate::Style::parse("group");
    g.geometry =
        Some(Geometry { x, y, width: right - x, height: bottom - y, ..Geometry::default() });
    let mut tx = Transaction::default();
    for (at, cell) in &members {
        let mut inside = (*cell).clone();
        inside.parent = Some(group_id.to_owned());
        if let Some(geo) = inside.geometry.as_mut() {
            geo.x -= x;
            geo.y -= y;
        }
        tx.changes.push(Change::Replace { page, at: *at, cell: Box::new(inside) });
    }
    // In the first one's place: under the cells it holds (drawn before them).
    tx.changes.push(Change::Insert { page, at: members[0].0, cell: Box::new(g) });
    Some(tx)
}

/// Group `id` undone: what it holds back in its parent, at their places there; the group
/// taken out. `None`: not a group (nothing inside it).
pub fn ungroup(doc: &Document, page: usize, id: &str) -> Option<Transaction> {
    let p = doc.pages.get(page)?;
    let at = place(p, id)?;
    let group = &p.cells[at];
    let (gx, gy) = group.geometry.as_ref().map_or((0.0, 0.0), |g| (g.x, g.y));
    let mut tx = replace_each(
        page,
        p,
        |c| c.parent.as_deref() == Some(id),
        |c| {
            if c.parent.as_deref() != Some(id) {
                return false;
            }
            c.parent.clone_from(&group.parent);
            let edge = c.kind == CellKind::Edge;
            if let Some(g) = c.geometry.as_mut() {
                if !g.relative {
                    g.x += gx;
                    g.y += gy;
                } else if edge {
                    // An edge's points (and loose ends) were the group's too.
                    for pt in
                        g.points.iter_mut().chain(&mut g.source_point).chain(&mut g.target_point)
                    {
                        pt.x += gx;
                        pt.y += gy;
                    }
                }
            }
            true
        },
    );
    if tx.is_empty() {
        return None;
    }
    tx.changes.push(Change::Remove { page, at });
    Some(tx)
}

/// The cells `ids` brought to the front (`true`: drawn last) or sent to the back (drawn
/// first among their siblings, each before its own parent's first other child), keeping
/// their order among themselves.
pub fn reorder(doc: &Document, page: usize, ids: &[&str], front: bool) -> Transaction {
    let Some(p) = doc.pages.get(page) else { return Transaction::default() };
    let wanted: HashSet<&str> = ids.iter().copied().collect();
    let moving: Vec<usize> = (0..p.cells.len())
        .filter(|&at| {
            wanted.contains(p.cells[at].id.as_str()) && p.cells[at].kind != CellKind::Holder
        })
        .collect();
    if moving.is_empty() {
        return Transaction::default();
    }
    let is_moving: HashSet<usize> = moving.iter().copied().collect();
    // The order after: the others as they are, the moving ones placed.
    let mut order: Vec<usize> = Vec::with_capacity(p.cells.len());
    if front {
        order.extend((0..p.cells.len()).filter(|at| !is_moving.contains(at)));
        order.extend(&moving);
    } else {
        // Each parent's moving children, waiting for that parent's first other child.
        let mut waiting: std::collections::HashMap<Option<&str>, Vec<usize>> =
            std::collections::HashMap::new();
        for &at in &moving {
            waiting.entry(p.cells[at].parent.as_deref()).or_default().push(at);
        }
        for (at, cell) in p.cells.iter().enumerate() {
            if is_moving.contains(&at) {
                continue;
            }
            if cell.kind != CellKind::Holder
                && let Some(before) = waiting.remove(&cell.parent.as_deref())
            {
                order.extend(before);
            }
            order.push(at);
        }
        // Without another child: at the end, as they were.
        let mut rest: Vec<usize> = waiting.into_values().flatten().collect();
        rest.sort_unstable();
        order.extend(rest);
    }
    let mut tx = Transaction::default();
    // Taken out from the last (the places before stay right), then put back at their places
    // after, in order (each one's place counts only what's before it, there already).
    for &at in moving.iter().rev() {
        tx.changes.push(Change::Remove { page, at });
    }
    for (to, &at) in order.iter().enumerate() {
        if is_moving.contains(&at) {
            tx.changes.push(Change::Insert { page, at: to, cell: Box::new(p.cells[at].clone()) });
        }
    }
    tx
}

#[cfg(test)]
#[path = "../tests/unit/ops.rs"]
mod tests;
