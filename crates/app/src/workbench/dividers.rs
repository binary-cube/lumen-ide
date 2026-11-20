//! The separators beside the docks' panels (the bottom one, the right one, the left one)
//! as controls (`ui_workbench::control::Divider`): pressed, held, dragged, hovered and let
//! go alike, none named where the mouse is routed. What a drag means (each dock's size,
//! within its limits) and a double click (back to its default) is said once per edge.

use super::*;
use ui_workbench::control::{Absent, Control, Controls, Divider, Press};

/// A dock's edge with a separator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Edge {
    Bottom,
    Right,
    Left,
}

/// The edges in the separators' order (the list handed to [`Controls`]).
const EDGES: [Edge; 3] = [Edge::Bottom, Edge::Right, Edge::Left];

impl Workbench {
    /// The separators as controls, with where a drag puts each one's line.
    fn with_dividers<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> (R, [Option<f32>; 3]) {
        let right = self.tools.side_on(ui_workbench::Dock::Right).is_some();
        let [mut bottom_at, mut right_at, mut left_at] = [None; 3];
        let tools = &mut self.tools;
        let mut bottom = Divider { splitter: &mut tools.terminal_splitter, at: &mut bottom_at };
        let mut side = Divider { splitter: &mut tools.side_splitter, at: &mut right_at };
        let mut left = Divider { splitter: &mut tools.splitter, at: &mut left_at };
        let mut absent = Absent;
        let side: &mut dyn Control = if right { &mut side } else { &mut absent };
        let mut parts: [&mut dyn Control; 3] = [&mut bottom, side, &mut left];
        let answer = f(&mut self.dividers, &mut parts);
        (answer, [bottom_at, right_at, left_at])
    }

    /// A press on a separator: its drag starts; a double click puts its dock back to its
    /// default size. `None`: not on one.
    pub(super) fn press_divider(&mut self, p: Point, clicks: u32) -> Option<MouseResult> {
        let (pressed, _) = self.with_dividers(|c, parts| c.press(parts, p, clicks));
        let (i, press) = pressed?;
        if press != Press::Held {
            return None;
        }
        let edge = *EDGES.get(i)?;
        if clicks == 2 {
            self.with_dividers(|c, parts| c.release(parts));
            self.reset_edge(edge);
            return Some(MouseResult::Redraw);
        }
        Some(MouseResult::StartDrag)
    }

    /// The held separator follows the mouse: its dock takes the size it's dragged to.
    /// `None`: none is held.
    pub(super) fn drag_divider(&mut self, p: Point) -> Option<bool> {
        let (dragged, at) = self.with_dividers(|c, parts| c.drag(parts, p));
        let (i, _) = dragged?;
        let (edge, center) = (*EDGES.get(i)?, at.get(i).copied().flatten()?);
        self.resize_edge(edge, center);
        Some(true)
    }

    /// The mouse let go: `true` if a separator was held.
    pub(super) fn release_divider(&mut self) -> bool {
        self.with_dividers(|c, parts| c.release(parts)).0.is_some()
    }

    /// The mouse at `p`: the separators follow it wherever it goes (left for a panel, they
    /// go dark). `true`: a redraw.
    pub(super) fn hover_dividers(&mut self, p: Point) -> bool {
        self.with_dividers(|c, parts| c.hover(parts, Some(p))).0
    }

    /// The cursor over a separator at `p` (the way it resizes).
    pub(super) fn divider_cursor(&mut self, p: Point) -> Option<CursorIcon> {
        let (cursor, _) = self.with_dividers(|c, parts| c.cursor(parts, p));
        cursor.map(|c| match c {
            ui_workbench::ViewerCursor::ResizeVertical => CursorIcon::ResizeVertical,
            _ => CursorIcon::ResizeHorizontal,
        })
    }

    /// A separator is moving (lighting up, going dark).
    pub(super) fn dividers_animating(&self) -> bool {
        let tools = &self.tools;
        [&tools.terminal_splitter, &tools.side_splitter, &tools.splitter]
            .iter()
            .any(|s| s.is_animating())
    }

    /// The dock at `edge` back to its default size.
    fn reset_edge(&mut self, edge: Edge) {
        let key = match edge {
            Edge::Bottom => {
                self.tools.bottom_size = tool_windows::BottomSize::Kept;
                workbench_model::sizes::DOCK_BOTTOM
            }
            Edge::Right => workbench_model::sizes::DOCK_RIGHT,
            Edge::Left => workbench_model::sizes::DOCK_LEFT,
        };
        self.data.sizes.remove(key);
        self.layout();
    }

    /// The dock at `edge` sized so its separator's line is at `center` (physical pixels on
    /// its axis), within its limits.
    fn resize_edge(&mut self, edge: Edge, center: f32) {
        let s = self.scale;
        let (key, size) = match edge {
            Edge::Bottom => {
                // Dragged: its own size again, where it's dragged to. The bottom panel spans
                // from the separator to the bottom (above the status bar).
                self.tools.bottom_size = tool_windows::BottomSize::Kept;
                let bottom = self.size.height as f32 - StatusBar::height(s);
                let height = (bottom - center - SPLITTER_WIDTH * s / 2.0) / s;
                let max = (bottom / s * 0.75).max(TERMINAL_MIN_HEIGHT);
                (workbench_model::sizes::DOCK_BOTTOM, height.clamp(TERMINAL_MIN_HEIGHT, max))
            }
            Edge::Right => {
                // The right panel: from the separator to the right toolbar, never narrower
                // than its header needs (its title and buttons side by side).
                let right = self
                    .tools
                    .side_on(ui_workbench::Dock::Right)
                    .map_or(0.0, |side| side.bounds.max_x());
                let width = (right - center - SPLITTER_WIDTH * s / 2.0) / s;
                let least = SIDE_PANEL_MIN.max(self.header_min(ui_workbench::Dock::Right));
                let max = (self.size.width as f32 / s * 0.5).max(least);
                (workbench_model::sizes::DOCK_RIGHT, width.clamp(least, max))
            }
            Edge::Left => {
                // The panel starts after the tool strip, not at the window's edge.
                let left = self.tools.sidebar.min_x();
                let width = (center - left - SPLITTER_WIDTH * s / 2.0) / s;
                let least = TREE_MIN_WIDTH.max(self.header_min(ui_workbench::Dock::Left));
                let max = (self.size.width as f32 / s * 0.6).max(least);
                (workbench_model::sizes::DOCK_LEFT, width.clamp(least, max))
            }
        };
        let size = size.round();
        if self.data.sizes.get(key) != Some(size) {
            self.data.sizes.set(key, size);
            self.layout();
        }
    }
}
