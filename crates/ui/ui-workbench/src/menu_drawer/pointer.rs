//! The mouse over the drawer: what a press hits, the row under it, the hover, the scroll and its
//! bar.

use super::*;
use ui_components::control::{Control, Controls, ScrollThumb};

impl MenuDrawer {
    /// What a press at `p` does.
    pub fn hit(&mut self, p: Point) -> DrawerAction {
        // Its menu button (where the bar's is) closes it, as the bar's does.
        if !self.open || self.button.contains(p) {
            return DrawerAction::Close;
        }
        // A part that holds the mouse (a list's thumb, or its track: the list jumps there).
        if self.with_controls(|c, parts| c.press(parts, p, 1)).is_some() {
            return DrawerAction::Drag;
        }
        if !self.panel.contains(p) {
            return DrawerAction::Close;
        }
        match self.row_at(p) {
            Some(Under::Menu(m)) => {
                self.nav.show_menu(m);
                self.intent.reset();
                DrawerAction::Inside
            }
            // A row with a submenu of its own: shown at once.
            Some(Under::Item(i)) if self.nav.item_is_submenu(i) => {
                self.nav.hover_item(Some(i));
                self.nav.show_sub(i);
                self.sub_intent.reset();
                DrawerAction::Inside
            }
            Some(Under::Item(i)) => {
                self.nav.item_command(i).map_or(DrawerAction::Inside, DrawerAction::Run)
            }
            Some(Under::SubItem(j)) => {
                self.nav.sub_item_command(j).map_or(DrawerAction::Inside, DrawerAction::Run)
            }
            Some(Under::SubBack) => {
                self.nav.leave_sub();
                DrawerAction::Inside
            }
            Some(Under::Result(r)) => {
                self.nav.result_command(r).map_or(DrawerAction::Inside, DrawerAction::Run)
            }
            None => DrawerAction::Inside,
        }
    }

    /// The row under `p` (only the rows shown in their lists' rooms).
    pub(super) fn row_at(&self, p: Point) -> Option<Under> {
        let find = |rows: &[(Rect, usize)], room: Rect| {
            room.contains(p).then(|| rows.iter().find(|(r, _)| r.contains(p)).map(|(_, i)| *i))?
        };
        // The third panel is on top (over the submenu, in a narrow window).
        if self.sub_panel.contains(p) {
            if self.sub_back.contains(p) {
                return Some(Under::SubBack);
            }
            return find(&self.sub_rows, self.sub_room).map(Under::SubItem);
        }
        if let Some(i) = find(&self.item_rows, self.items_room) {
            return Some(Under::Item(i));
        }
        if let Some(r) = find(&self.result_rows, self.menus_room) {
            return Some(Under::Result(r));
        }
        find(&self.menu_rows, self.menus_room).map(Under::Menu)
    }

    /// The mouse at `p`: what's under it lit, a category's submenu shown (after a rest, not
    /// on the way to the open one). `true` if a redraw is needed.
    pub fn hover(&mut self, p: Point) -> bool {
        self.pointer = Some(p);
        let mut changed = self.with_controls(|c, parts| c.hover(parts, Some(p)));
        if !self.open {
            return changed;
        }
        let under = self.row_at(p);
        changed |= under != self.under;
        self.under = under;
        let submenu = (self.items_panel.size.width > 0.0).then_some(self.items_panel);
        let row = match under {
            Some(Under::Menu(m)) => Some(m),
            _ => None,
        };
        match self.intent.moved(p, row, self.nav.open_menu(), submenu, Instant::now()) {
            Intent::Show(m) => changed |= self.nav.show_menu(m),
            // Back on the open category: the keyboard there again.
            Intent::Keep if row.is_some() && row == self.nav.open_menu() => {
                changed |= self.nav.show_menu(row.unwrap_or_default());
            }
            Intent::Wait(_) | Intent::Keep => {}
        }
        // A row's own submenu: as the categories', after a short rest on its row, not on
        // the way to the one shown.
        let sub_panel = (self.sub_panel.size.width > 0.0).then_some(self.sub_panel);
        let item = match under {
            Some(Under::Item(i)) => Some(i),
            _ => None,
        };
        if let Intent::Show(i) =
            self.sub_intent.moved(p, item, self.nav.open_sub(), sub_panel, Instant::now())
        {
            changed |= self.nav.show_sub(i);
        }
        match under {
            Some(Under::Item(i)) => changed |= self.nav.hover_item(Some(i)),
            Some(Under::SubItem(j)) => changed |= self.nav.hover_sub_item(Some(j)),
            Some(Under::Result(r)) => changed |= self.nav.hover_result(r),
            _ if self.sub_panel.contains(p) => changed |= self.nav.hover_sub_item(None),
            _ if self.items_panel.contains(p) => changed |= self.nav.hover_item(None),
            _ => {}
        }
        changed
    }

    /// `p` is on the drawer (its panels or its menu button).
    pub fn contains(&self, p: Point) -> bool {
        self.panel.contains(p) || self.button.contains(p)
    }

    /// Where the shown submenu's row `entry` was last drawn (within its list's room).
    pub fn item_rect(&self, entry: usize) -> Option<Rect> {
        self.item_rows.iter().find(|(_, i)| *i == entry).map(|(r, _)| *r)
    }

    /// Where the third panel's row `entry` was last drawn (within its list's room).
    pub fn sub_item_rect(&self, entry: usize) -> Option<Rect> {
        self.sub_rows.iter().find(|(_, i)| *i == entry).map(|(r, _)| *r)
    }

    /// Something clickable under `p` (the pointer).
    pub fn over_clickable(&self, p: Point) -> bool {
        self.row_at(p).is_some_and(|u| match u {
            Under::Item(i) => self.nav.item_command(i).is_some() || self.nav.item_is_submenu(i),
            Under::SubItem(j) => self.nav.sub_item_command(j).is_some(),
            _ => true,
        })
    }

    /// The scroll, on the list under the mouse. `true` if it moved.
    pub fn scroll_by(&mut self, dy: f32) -> bool {
        let over = |r: Rect| self.pointer.is_some_and(|p| r.contains(p));
        if over(self.sub_panel) {
            self.sub_scroll.scroll_by(dy)
        } else if over(self.items_panel) {
            self.items_scroll.scroll_by(dy)
        } else {
            self.menus_scroll.scroll_by(dy)
        }
    }

    /// Its parts that take the mouse (each list's thumb), with their state of use: pressed,
    /// held, hovered alike (`ui_components::control`).
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let mut menus = ScrollThumb { bar: &mut self.menus_bar, scroll: &mut self.menus_scroll };
        let mut items = ScrollThumb { bar: &mut self.items_bar, scroll: &mut self.items_scroll };
        let mut sub = ScrollThumb { bar: &mut self.sub_bar, scroll: &mut self.sub_scroll };
        let mut parts: [&mut dyn Control; 3] = [&mut menus, &mut items, &mut sub];
        f(&mut self.parts, &mut parts)
    }

    /// One of its parts holds the mouse.
    pub fn scrollbar_held(&self) -> bool {
        self.parts.held()
    }

    /// The part holding the mouse follows it to `p`. `true` if what it shows moved.
    pub fn drag_scrollbar(&mut self, p: Point) -> bool {
        self.with_controls(|c, parts| c.drag(parts, p)).is_some_and(|(_, moved)| moved)
    }

    /// The mouse released: `true` if a part held it.
    pub fn release(&mut self) -> bool {
        self.with_controls(|c, parts| c.release(parts)).is_some()
    }
}
