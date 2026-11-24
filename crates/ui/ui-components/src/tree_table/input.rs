//! What the table answers: what's under the pointer, a press (choosing, folding, an edge
//! dragged), the wheel, the keys, the cursor and the tooltips; the table as a form's control.

use super::*;

/// A tree table as one of a view's controls: the same input as its own methods, what a press
/// did kept for [`TreeTable::take_event`].
impl crate::control::Control for TreeTable {
    fn area(&self) -> Option<Rect> {
        let all = Rect::new(
            self.header.min_x(),
            self.header.min_y(),
            self.header.size.width.max(self.body.size.width),
            self.body.max_y() - self.header.min_y(),
        );
        (all.size.width > 0.0 && all.size.height > 0.0).then_some(all)
    }

    fn press(&mut self, p: Point, clicks: u32) -> crate::control::Press {
        use crate::control::Press;
        let held = self.bar.contains(p) || matches!(self.hit(p), Some(Hit::Edge(_)));
        self.event = TreeTable::press(self, p, clicks);
        if held { Press::Held } else { Press::Took }
    }

    fn drag(&mut self, p: Point) -> bool {
        TreeTable::drag(self, p)
    }

    fn release(&mut self) -> bool {
        TreeTable::release(self)
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        match p {
            Some(p) => TreeTable::hover(self, p),
            None => self.leave(),
        }
    }

    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        self.cursor_at(p)
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        TreeTable::scroll(self, dx, dy)
    }

    fn focusable(&self) -> bool {
        true
    }

    fn key(&mut self, key: ViewerKey, _reach: crate::tool_panel::KeyReach) -> bool {
        TreeTable::key(self, key)
    }

    fn char_key(&mut self, key: char) -> bool {
        TreeTable::char_key(self, key)
    }

    fn tooltip(&self, p: Point) -> Option<Tip> {
        self.tooltip_at(p)
    }
}

impl TreeTable {
    pub(super) fn hit(&self, p: Point) -> Option<Hit> {
        if self.header.contains(p) {
            // A column's left edge (the tree's width follows the others': theirs are dragged).
            let edge = self.scale * 4.0;
            for (i, &(x, _)) in self.xs.iter().enumerate().skip(1) {
                if (p.x - (x - self.hscroll)).abs() <= edge {
                    return Some(Hit::Edge(i));
                }
            }
            let column = self.column_at(p.x)?;
            return Some(Hit::Header(column));
        }
        if !self.body.contains(p) {
            return None;
        }
        let shown = self.shown_cached();
        let slot =
            TreeList::new(self.row_h, shown.len()).row_at(p.y - self.body.min_y() + self.scroll)?;
        let row = shown[slot];
        let column = self.column_at(p.x).unwrap_or(0);
        let chevron = column == 0 && self.has_children(row) && {
            let x0 = self.xs[0].0 - self.hscroll;
            let cx = x0 + self.scale * (6.0 + INDENT * self.depth(row) as f32);
            p.x >= cx - self.scale * 2.0 && p.x <= cx + self.scale * 16.0
        };
        Some(Hit::Row { row, column, chevron })
    }

    fn column_at(&self, x: f32) -> Option<usize> {
        self.xs
            .iter()
            .position(|&(l, w)| x >= l - self.hscroll && x < l + w - self.hscroll)
            .or_else(|| (!self.xs.is_empty() && x >= self.body.min_x()).then(|| self.xs.len() - 1))
    }

    /// The selection kept in view.
    pub(super) fn bring_into_view(&mut self) {
        let Some(s) = self.selected else { return };
        let Some(slot) = self.shown_rows().iter().position(|&r| r == s) else { return };
        let top = slot as f32 * self.row_h;
        let height = self.body.size.height;
        if top < self.scroll {
            self.scroll = top;
        } else if top + self.row_h > self.scroll + height {
            self.scroll = top + self.row_h - height;
        }
    }

    /// A press at `p` (`clicks`: 2 opens or folds the row).
    pub fn press(&mut self, p: Point, clicks: u32) -> Option<TableEvent> {
        if let Some(offset) = self.bar.press(p) {
            self.scroll = offset;
            return None;
        }
        match self.hit(p)? {
            Hit::Edge(i) => {
                let w = self.columns[i].width;
                self.resizing = Some((i, p.x, w));
                None
            }
            Hit::Header(_) => None,
            Hit::Row { row, chevron, .. } => {
                if chevron || (clicks >= 2 && self.has_children(row)) {
                    self.selected = Some(row);
                    self.toggle(row);
                    return Some(TableEvent::Toggled(row));
                }
                self.selected = Some(row);
                Some(TableEvent::Selected(row))
            }
        }
    }

    /// The mouse moved with its button held: a column's edge or the scrollbar dragged.
    pub fn drag(&mut self, p: Point) -> bool {
        if let Some((i, from, width)) = self.resizing {
            // Its left edge dragged: leftwards widens it.
            let w = width - (p.x - from) / self.scale.max(0.01);
            let before = self.columns[i].width;
            self.set_width(i, w);
            return self.columns[i].width != before;
        }
        if let Some(offset) = self.bar.drag(p) {
            let changed = offset != self.scroll;
            self.scroll = offset;
            return changed;
        }
        false
    }

    pub fn release(&mut self) -> bool {
        self.resizing.take().is_some() | self.bar.release()
    }

    pub fn hover(&mut self, p: Point) -> bool {
        let bar = self.bar.hover(p);
        let hit = self.hit(p);
        let changed = hit != self.hover;
        self.hover = hit;
        changed || bar
    }

    /// The pointer left it.
    pub fn leave(&mut self) -> bool {
        self.hover.take().is_some()
    }

    pub fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        let shown = self.shown_rows().len();
        let (before, hbefore) = (self.scroll, self.hscroll);
        self.scroll = (self.scroll + dy).clamp(0.0, self.max_scroll(shown));
        self.hscroll = (self.hscroll + dx).clamp(0.0, self.max_hscroll());
        self.scroll != before || self.hscroll != hbefore
    }

    /// The keys of a tree: ↑ ↓, Home / End, PgUp / PgDn move the choice; → opens (or goes
    /// to the first child), ← folds (or goes to the parent), ↵ opens or folds.
    pub fn key(&mut self, key: ViewerKey) -> bool {
        let shown = self.visible();
        if shown.is_empty() {
            return false;
        }
        let at = self.selected.and_then(|s| shown.iter().position(|&r| r == s));
        let page = ((self.body.size.height / self.row_h.max(1.0)).floor() as usize).max(1);
        let last = shown.len() - 1;
        let to = |slot: usize| Some(shown[slot.min(last)]);
        let next = match key {
            ViewerKey::Down => at.map_or(to(0), |a| to(a + 1)),
            ViewerKey::Up => at.map_or(to(last), |a| to(a.saturating_sub(1))),
            ViewerKey::PageDown => at.map_or(to(0), |a| to(a + page)),
            ViewerKey::PageUp => at.map_or(to(0), |a| to(a.saturating_sub(page))),
            ViewerKey::Home => to(0),
            ViewerKey::End => to(last),
            ViewerKey::Right => {
                let Some(s) = self.selected else { return false };
                if self.has_children(s) && !self.is_open(s) {
                    self.set_open(s, true);
                    return true;
                }
                if self.has_children(s) { Some(s + 1) } else { return false }
            }
            ViewerKey::Left => {
                let Some(s) = self.selected else { return false };
                if self.has_children(s) && self.is_open(s) {
                    self.set_open(s, false);
                    return true;
                }
                match self.rows[s].parent {
                    Some(p) => Some(p),
                    None => return false,
                }
            }
            ViewerKey::Enter => {
                let Some(s) = self.selected else { return false };
                return self.toggle(s);
            }
            _ => return false,
        };
        let changed = next != self.selected;
        self.selected = next;
        self.reveal = true;
        changed
    }

    /// A typed key: `*` opens everything under the chosen row (everything, when none is).
    pub fn char_key(&mut self, c: char) -> bool {
        if c != '*' {
            return false;
        }
        match self.selected {
            None => self.set_all_open(true),
            Some(s) => {
                // Rows come parents first: a row is under `s` when its parent is.
                let mut under = vec![false; self.rows.len()];
                for i in s..self.rows.len() {
                    under[i] = i == s || self.rows[i].parent.is_some_and(|p| under[p]);
                    if under[i] && self.children[i] {
                        self.open[i] = true;
                    }
                }
                self.open_generation += 1;
            }
        }
        true
    }

    pub fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        match self.hit(p)? {
            Hit::Edge(_) => Some(ViewerCursor::ResizeHorizontal),
            Hit::Row { chevron: true, .. } => Some(ViewerCursor::Pointer),
            _ => None,
        }
    }

    /// The tooltip at `p`: a column's title says what it is; a cell, its own (else its text,
    /// when it's cut).
    pub fn tooltip_at(&self, p: Point) -> Option<Tip> {
        // The gutter's sign: what it means.
        if self.gutter > 0.0
            && p.x < self.body.min_x() + self.gutter
            && let Some(Hit::Row { row, .. }) = self.hit(p)
        {
            let slot = self.shown_cached().iter().position(|&r| r == row)?;
            let r = Rect::new(
                self.body.min_x(),
                self.body.min_y() + slot as f32 * self.row_h - self.scroll,
                self.gutter,
                self.row_h,
            );
            return Some(Tip::new(r, self.rows.get(row)?.sign_tip.clone()?));
        }
        match self.hit(p)? {
            Hit::Header(i) => {
                let c = self.columns.get(i)?;
                let (x, w) = self.xs[i];
                let r =
                    Rect::new(x - self.hscroll, self.header.min_y(), w, self.header.size.height);
                Some(Tip::new(r, c.tip.clone()?))
            }
            Hit::Row { row, column, .. } => {
                let slot = self.shown_cached().iter().position(|&r| r == row)?;
                let cell = self.rows.get(row)?.cells.get(column)?;
                let (x, w) = self.xs[column];
                let r = Rect::new(
                    x - self.hscroll,
                    self.body.min_y() + slot as f32 * self.row_h - self.scroll,
                    w,
                    self.row_h,
                );
                let tip = cell.tip.clone().or_else(|| {
                    let plain = cell.plain();
                    (plain.chars().count() > 24).then_some(plain)
                })?;
                Some(Tip::new(r, tip))
            }
            Hit::Edge(_) => None,
        }
    }
}
