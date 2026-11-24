//! Where each part of the window goes: the docks on the sides and at the bottom (their
//! widths and height, what's left to the editors), the editor groups, the side panels, the
//! status bar — worked out again when the window or one of them changes size.

use super::*;

impl Workbench {
    /// The left dock's width (logical): the project's, as dragged, else the default — the
    /// same for every tool window shown there.
    /// A width dragged is never narrower than its panel's header needs (one kept from
    /// before it was); the default stays as it is.
    pub(super) fn left_dock_width(&self) -> f32 {
        match self.data.sizes.get(workbench_model::sizes::DOCK_LEFT) {
            Some(kept) => kept.max(self.header_min(ui_workbench::Dock::Left)),
            None => TREE_WIDTH,
        }
    }

    /// The right dock's width (logical), as the left's.
    pub(super) fn right_dock_width(&self) -> f32 {
        match self.data.sizes.get(workbench_model::sizes::DOCK_RIGHT) {
            Some(kept) => kept.max(self.header_min(ui_workbench::Dock::Right)),
            None => SIDE_PANEL_WIDTH,
        }
    }

    /// The bottom dock's height (physical) in `available` (the height below the top bar,
    /// physical; `gap`: its separator's): the project's, as dragged, the editor keeping
    /// 120 px; never dragged, [`BOTTOM_SHARE`] of it, the editor keeping
    /// [`EDITOR_MIN_SHARE`]. The same for every tool window shown there.
    pub(super) fn bottom_dock_height(&self, available: f32, gap: f32) -> f32 {
        let s = self.scale;
        match self.data.sizes.get(workbench_model::sizes::DOCK_BOTTOM) {
            Some(height) => (height * s).round().min((available - gap - 120.0 * s).max(0.0)),
            None => {
                let most = (available * (1.0 - EDITOR_MIN_SHARE) - gap).max(0.0);
                (available * BOTTOM_SHARE).max(TERMINAL_MIN_HEIGHT * s).min(most).round()
            }
        }
    }

    pub(super) fn layout(&mut self) {
        let (w, h) = (self.size.width as f32, self.size.height as f32);
        let s = self.scale;
        let margin = (CARD_MARGIN * s).round();
        let status_h = StatusBar::height(s);
        let body_h = (h - status_h).max(0.0);
        self.settings.set_viewport(Rect::new(0.0, 0.0, w, h), s);
        self.home.projects_menu.set_viewport(Rect::new(0.0, 0.0, w, h), s);
        self.choice_menu.set_viewport(Rect::new(0.0, 0.0, w, h), s);
        self.context_menu.set_viewport(Rect::new(0.0, 0.0, w, h), s);
        self.dialogs.dialog.set_viewport(Rect::new(0.0, 0.0, w, h), s);
        self.about.set_viewport(Rect::new(0.0, 0.0, w, h), s);
        self.place_menu_drawer();
        // The top bar (on macOS, in the title bar); the rest starts below it — or at the
        // top, the bar hidden (full screen: it comes over the content, see `paint`).
        let bar_h = TopBar::height(s);
        self.chrome.top_bar.set_bounds(Rect::new(0.0, 0.0, w, bar_h), s);
        let y0 = if self.chrome.top_bar_hides() { 0.0 } else { bar_h };
        let strip_w = ToolStrip::width(s);
        self.tools.strip.set_bounds(Rect::new(0.0, y0, strip_w, (body_h - y0).max(0.0)), s);
        // The right bar (the module panels), stuck to the window edge; with no panels, only the
        // usual margin remains.
        let right = self.support.tools.iter().any(|t| t.dock() == ui_workbench::Dock::Right);
        let right_w = if right { strip_w } else { 0.0 };
        self.tools
            .right_strip
            .set_bounds(Rect::new(w - right_w, y0, right_w, (body_h - y0).max(0.0)), s);
        let right_edge = w - if right_w > 0.0 { right_w } else { margin };
        // The top area (project + editor) and the terminal below it, split horizontally.
        let gap = (SPLITTER_WIDTH * s).round();
        let available = (body_h - y0).max(0.0);
        // The terminal or a module's tool window (Git): the same area, one at a time.
        let bottom = self.tools.bottom.is_some();
        let size = if bottom { Some(self.tools.bottom_size) } else { None };
        let (term_h, top_h) = match size {
            None => (0.0, available),
            // All of it: nothing above, no separator.
            Some(tool_windows::BottomSize::Full) => (available, 0.0),
            Some(tool_windows::BottomSize::Half) => {
                let half = ((available - gap) / 2.0).max(0.0).round();
                (half, (available - half - gap).max(0.0))
            }
            Some(tool_windows::BottomSize::Kept) => {
                let term_h = self.bottom_dock_height(available, gap);
                (term_h, (available - term_h - gap).max(0.0))
            }
        };
        let full = size == Some(tool_windows::BottomSize::Full);
        let full_w = (right_edge - strip_w).max(0.0);
        let splitter_w = if bottom && !full { full_w } else { 0.0 };
        self.tools.terminal_splitter.set_bounds(Rect::new(strip_w, y0 + top_h, splitter_w, gap), s);
        let below = if full { 0.0 } else { gap };
        self.tools.bottom_bounds = Rect::new(strip_w, y0 + top_h + below, full_w, term_h);
        self.layout_tool_groups();
        // The left side: the panel open there (the project panel, or one in its place).
        let left_panel = self.tools.sides.iter().any(|p| p.dock == ui_workbench::Dock::Left);
        let (tree_w, splitter_w) = if left_panel {
            ((self.left_dock_width() * s).round().min(w * 0.6), (SPLITTER_WIDTH * s).round())
        } else {
            (0.0, 0.0)
        };
        // Its card, next to the toolbar.
        self.tools.sidebar = Rect::new(strip_w, y0, tree_w, top_h);
        let border = s.round().max(1.0);
        self.tools
            .splitter
            .set_bounds(Rect::new(self.tools.sidebar.max_x(), y0, splitter_w, top_h), s);
        self.status.set_bounds(Rect::new(0.0, body_h, w, status_h), s);

        // The main card: rounded, with margins from the window (after the separator on the left).
        let left = if tree_w > 0.0 { self.tools.sidebar.max_x() + splitter_w } else { strip_w };
        // The right side panel (if open), as a separate card.
        let right_panel = self.tools.sides.iter().any(|p| p.dock == ui_workbench::Dock::Right);
        let side_w = if right_panel {
            (self.right_dock_width() * s).round().min(((right_edge - left) * 0.5).max(0.0))
        } else {
            0.0
        };
        let side_gap = if side_w > 0.0 { (SPLITTER_WIDTH * s).round() } else { 0.0 };
        let right_bounds = Rect::new(right_edge - side_w, y0, side_w, top_h);
        self.tools
            .side_splitter
            .set_bounds(Rect::new(right_bounds.min_x() - side_gap, y0, side_gap, top_h), s);
        // Each side panel's card, and its content below the header the app draws for it.
        let (sidebar, bottom_bounds) = (self.tools.sidebar, self.tools.bottom_bounds);
        for side in &mut self.tools.sides {
            side.bounds = match side.dock {
                ui_workbench::Dock::Left => sidebar,
                // A panel at the bottom: the bottom windows' area (and their header's height).
                ui_workbench::Dock::Bottom => bottom_bounds,
                _ => right_bounds,
            };
            let header_h = if side.dock == ui_workbench::Dock::Bottom {
                PanelHeader::bottom_height(s)
            } else {
                PanelHeader::height(s)
            };
            let b = side.bounds;
            let inner = Rect::new(
                b.min_x() + border,
                b.min_y() + border,
                (b.size.width - 2.0 * border).max(0.0),
                (b.size.height - 2.0 * border).max(0.0),
            );
            let header = if side.panel.header().is_some() { header_h } else { 0.0 };
            side.header.set_bounds(Rect::new(b.min_x(), b.min_y(), b.size.width, header), s);
            side.content = Rect::new(
                inner.min_x(),
                inner.min_y() + header,
                inner.size.width,
                (inner.size.height - header).max(0.0),
            );
        }
        self.card = Rect::new(left, y0, (right_edge - left - side_w - side_gap).max(0.0), top_h);
        let inner = Rect::new(
            self.card.min_x() + border,
            self.card.min_y() + border,
            (self.card.size.width - 2.0 * border).max(0.0),
            (self.card.size.height - 2.0 * border).max(0.0),
        );
        // The editor groups, following the split tree, with a strip (and handle) between them.
        let gap = (SPLIT_GAP * s).round();
        let palette = self.palette.clone();
        let rects = self.editors.split.layout(&self.editors.tree, inner, gap, s, &palette);
        for (g, rect) in rects {
            if g >= self.editors.groups.len() {
                continue;
            }
            self.editors.groups[g].bounds = rect;
            if g == self.editors.current {
                self.layout_group(rect);
            } else {
                self.swap_group(g);
                self.layout_group(rect);
                self.swap_group(g);
            }
        }
    }
}
