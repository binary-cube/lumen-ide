//! The mouse moving over the window.

use super::*;

impl Workbench {
    /// The mouse cursor moved: returns (needs a redraw, the cursor icon).
    pub fn hover(&mut self, p: Point) -> (bool, CursorIcon) {
        // Another region under it emphasized: a frame.
        let emphasis = self.emphasis_pointer(p);
        let left = self.leave_side_panel(p);
        let (redraw, icon) = self.hover_over(p);
        (redraw || emphasis || left, icon)
    }

    /// The mouse went where the app has nothing (onto a page the app draws): what it lit
    /// goes out — a tab's or a button's hover, the side panel's, a tooltip — and a bar's
    /// window not pinned closes (it would stay open, and take the page's mouse where it
    /// is). `true`: a redraw.
    pub fn hover_cleared(&mut self) -> bool {
        let away = Point::new(f32::MIN, f32::MIN);
        let (mut redraw, _) = self.hover_over(away);
        redraw |= self.leave_side_panel(away);
        redraw |= self.tooltips.hide();
        if self.status_popover.is_open() && !self.status_popover.pinned() {
            self.status_popover.close();
            redraw = true;
        }
        self.pointer_left() | redraw
    }

    /// The side panel the mouse was over, told it left (it gets the point outside): what
    /// it lit under the mouse goes out, a scrollbar it held shown fades. Only the panel
    /// under the mouse hears it move otherwise. `true`: it changed.
    fn leave_side_panel(&mut self, p: Point) -> bool {
        let under = self.tools.side_at(p).map(|s| s.id);
        let left = std::mem::replace(&mut self.tools.hovered, under);
        match left.filter(|id| Some(*id) != under) {
            Some(id) => self
                .tools
                .sides
                .iter_mut()
                .find(|s| s.id == id)
                .is_some_and(|side| side.panel.hover(p) | side.header.hover(p)),
            None => false,
        }
    }

    /// [`Self::hover`] but the emphasis.
    pub(super) fn hover_over(&mut self, p: Point) -> (bool, CursorIcon) {
        // A wide item's tooltip shows by the mouse.
        self.tooltips.pointer(p);
        // The notifications follow the mouse wherever it goes (it leaves them too); on a
        // card, nothing under it gets the mouse.
        let mut floating = false;
        let mut over = None;
        for o in Overlay::ALL {
            if !o.slot().floats(self) || !self.overlay_is_open(o) {
                continue;
            }
            let (changed, icon) = o.slot().hover(self, p);
            floating |= changed;
            if o.slot().holds(self, p) {
                over = Some(icon);
            }
        }
        if let Some(icon) = over {
            return (self.tooltips.set(None) | floating, icon);
        }
        let (changed, icon) = self.hover_under_toasts(p);
        (changed | floating, icon)
    }

    /// The mouse at `p`, not on a notification.
    pub(super) fn hover_under_toasts(&mut self, p: Point) -> (bool, CursorIcon) {
        let blocked = self.modal_open();
        let mut redraw = self.reveal_top_bar(p);
        // Editors the mouse isn't over (other groups, hidden tabs) forget their hover, or
        // their scroll strip would stay widened.
        let current = self.active;
        let groups = self.editors.groups.iter_mut().map(|g| (g.active, &mut g.tabs));
        for (active, tabs) in std::iter::once((current, &mut self.tabs)).chain(groups) {
            for (i, tab) in tabs.iter_mut().enumerate() {
                if blocked || i != active || !tab.view.bounds().contains(p) {
                    redraw |= tab.view.unhover(&tab.doc, &tab.folds);
                }
            }
        }
        let (moved, over) = self.editors.split.hover((!blocked).then_some(p));
        redraw |= moved;
        if let Some(icon) = over {
            return (redraw, icon);
        }
        let (changed, icon) = self.hover_inner_any(p);
        (changed || redraw, icon)
    }

    pub(in crate::workbench) fn hover_inner_any(&mut self, p: Point) -> (bool, CursorIcon) {
        // Over another editor group: its hover (tabs, editor), without making it current.
        if !self.overlay_open()
            && let Some((redraw, icon)) = self.with_group_at(p, |this| {
                // The hover first: what's under the mouse is told where it is, then asked
                // its tooltip there.
                let (redraw, icon) = this.hover_inner(p);
                let tip = this.tooltip_at(p);
                let tip_changed = this.tooltips.set(tip);
                (redraw || tip_changed, icon)
            })
        {
            return (redraw, icon);
        }
        let (redraw, icon) = self.hover_inner(p);
        let tip = self.tooltip_at(p);
        let tip_changed = self.tooltips.set(tip);
        (redraw || tip_changed, icon)
    }

    /// Opening a search result from the home screen: a place typed after the name
    /// (`User.php:12`) is kept for when the file is shown.
    pub(in crate::workbench) fn note_home_place(&mut self, action: &ui_workbench::ProjectsAction) {
        if matches!(action, ui_workbench::ProjectsAction::OpenPath(_)) {
            self.pending_goto = workspace::split_location(self.home.picker.query.text().trim()).1;
        }
    }

    pub(super) fn hover_top_bar(&mut self, p: Point) -> (bool, CursorIcon) {
        let redraw = self.chrome.top_bar.hover(p);
        let icon = match self.chrome.top_bar.hit(p) {
            Some(TopBarHit::Empty) | None => CursorIcon::Default,
            Some(_) => CursorIcon::Pointer,
        };
        (redraw, icon)
    }

    pub(in crate::workbench) fn hover_inner(&mut self, p: Point) -> (bool, CursorIcon) {
        // The overlay on top has the mouse.
        if let Some(o) = self.active_overlay() {
            return o.slot().hover(self, p);
        }
        // The splitters follow the mouse wherever it goes (left for a panel, they go dark).
        let splitters = self.hover_dividers(p);
        // The separator between a preview's halves: over both of them.
        let (lit, over) = self.hover_preview_split(p);
        if let Some(icon) = over {
            return (splitters | lit, icon);
        }
        let (redraw, icon) = self.hover_content(p);
        (redraw | splitters | lit, icon)
    }

    /// What's under the mouse, besides the splitters.
    pub(super) fn hover_content(&mut self, p: Point) -> (bool, CursorIcon) {
        // A module's bar above the text (told when the mouse leaves it, wherever it goes).
        if let Some(hovered) = self.hover_toolbar(p) {
            return hovered;
        }
        // The hidden top bar, come over the content (full screen).
        if self.chrome.top_bar_hides() && self.chrome.top_bar.contains(p) {
            return self.hover_top_bar(p);
        }
        if self.tabs.get(self.active).is_some_and(|t| {
            t.view.swatch_at(p).is_some()
                || t.view.vcs_change_at(p).is_some()
                || t.view.statement_mark_at(p).is_some()
        }) {
            return (false, CursorIcon::Pointer);
        }
        if self.home_visible() && self.editor_area.contains(p) {
            let redraw = self.home.picker.hover(p);
            let icon = if self.home.picker.over_clickable(p) {
                CursorIcon::Pointer
            } else {
                CursorIcon::Default
            };
            return (redraw, icon);
        }
        // Over an item: a redraw only when what's lit changed (not at each move along it).
        // An item with a window opens it; away from both, an unpinned one closes.
        let popover = self.hover_status_popover(p);
        let lit = self.status.hover(p) | popover;
        let on = self.status.hit(p).is_some();
        if lit || on {
            let icon = if on { CursorIcon::Pointer } else { CursorIcon::Default };
            return (lit, icon);
        }
        if self.chrome.top_bar.contains(p) {
            return self.hover_top_bar(p);
        }
        if let Some(hovered) = self.hover_tool_window(p) {
            return hovered;
        }
        let mut redraw = self.tab_bar.hover(p)
            | self.chrome.top_bar.hover(p)
            | self.tools.strip.hover(p)
            | self.tools.right_strip.hover(p)
            | self.breadcrumbs.hover(p);
        if self.tools.strip.hit(p).is_some()
            || self.tools.right_strip.hit(p).is_some()
            || self.breadcrumbs.hit(p).is_some()
        {
            return (redraw, CursorIcon::Pointer);
        }
        if let Some(side) = self.tools.side_at(p) {
            if side.header.contains(p) {
                let lit = side.header.hover(p);
                let icon = if side.header.hit(p).is_some() {
                    CursorIcon::Pointer
                } else {
                    CursorIcon::Default
                };
                return (redraw | lit, icon);
            }
            redraw |= side.header.hover(p) | side.panel.hover(p);
            let icon = match side.panel.cursor_at(p) {
                Some(cursor) => super::super::cursor_icon(cursor),
                None if side.panel.clickable(p) => CursorIcon::Pointer,
                None => CursorIcon::Default,
            };
            return (redraw, icon);
        }
        if let Some(cursor) = self.divider_cursor(p) {
            return (redraw, cursor);
        }
        // The find bar lies over the editor and the viewer.
        if let Some(tab) = self.tabs.get_mut(self.active).filter(|t| t.find_open) {
            redraw |= tab.find_bar.hover(p);
            if tab.find_bar.contains(p) {
                let icon = match tab.find_bar.hit(p) {
                    Some(FindHit::Field(_)) => CursorIcon::Text,
                    Some(_) => CursorIcon::Pointer,
                    None => CursorIcon::Default,
                };
                return (redraw, icon);
            }
        }
        // The viewer (its switcher and links).
        if let Some(v) = self.tabs.get_mut(self.active).and_then(|t| t.viewer.as_mut()) {
            if v.toggles.iter().any(|(r, _)| r.contains(p)) {
                return (redraw, CursorIcon::Pointer);
            }
            if v.viewer_visible() && v.bounds.contains(p) {
                let said = v.viewer.hover(p);
                // A view saying when it changed redraws only then (not at each move).
                let (changed, link) =
                    if v.viewer.hover_tells_changes() { (said, false) } else { (true, said) };
                let icon = match v.viewer.cursor_at(p) {
                    Some(cursor) => super::super::cursor_icon(cursor),
                    None if link => CursorIcon::Pointer,
                    None => CursorIcon::Default,
                };
                return (redraw | changed, icon);
            }
        }
        let mut over_marker = false;
        if let Some(tab) = self.tabs.get_mut(self.active).filter(|t| t.editor_visible()) {
            redraw |= tab.view.hover(p, &tab.doc, &tab.folds);
            over_marker = tab.view.fold_marker_at(p, &tab.doc, &tab.folds).is_some()
                || tab.view.fold_pill_at(p).is_some();
        }
        let over_scrollbar = self.tabs.get(self.active).is_some_and(|t| t.view.over_scrollbar(p));
        let icon = if over_scrollbar {
            CursorIcon::Default
        } else if over_marker {
            CursorIcon::Pointer
        } else if self.tabs.get(self.active).is_some_and(|t| t.view.bounds().contains(p)) {
            CursorIcon::Text
        } else if self.tab_bar.hit(p).is_some() {
            CursorIcon::Pointer
        } else {
            CursorIcon::Default
        };
        (redraw, icon)
    }
}
