//! What every split area shares (the editor's groups, a bottom window's groups): the
//! separators between the groups of a [`SplitTree`] as `Splitter`s (the three-dot handle,
//! its hover), dragging them, how far a press must move to drag a tab, and how a drop zone
//! is drawn. The tree is the model; this is its separators' look and feel.

use super::*;
use crate::split_tree::{Divider, SplitTree};

/// How far (logical pixels) a pressed tab must move before it's dragged: less is a click.
pub(super) const DRAG_THRESHOLD: f32 = 6.0;

/// The separators of a split tree as laid out, with the one being dragged.
#[derive(Debug, Default)]
pub(super) struct SplitHandles {
    pub dividers: Vec<Divider>,
    /// One `Splitter` per separator (reused across layouts, so the hover stays smooth).
    pub handles: Vec<Splitter>,
    /// The dragged separator.
    pub dragging: Option<usize>,
}

impl SplitHandles {
    /// Lays `tree` out in `rect` (separators `gap` thick): each group's area; the
    /// separators' handles follow.
    pub fn layout(
        &mut self,
        tree: &SplitTree,
        rect: Rect,
        gap: f32,
        scale: f32,
        palette: &Palette,
    ) -> Vec<(usize, Rect)> {
        let (rects, dividers) = tree.layout(rect, gap);
        self.handles.truncate(dividers.len());
        let style = SplitterStyle::from_palette(palette);
        for (i, d) in dividers.iter().enumerate() {
            let orientation = match d.dir {
                Split::Right => Orientation::Vertical,
                Split::Down => Orientation::Horizontal,
            };
            if self.handles.get(i).is_none_or(|h| h.orientation() != orientation) {
                let handle = Splitter::new(style.clone(), orientation);
                if i < self.handles.len() {
                    self.handles[i] = handle;
                } else {
                    self.handles.push(handle);
                }
            }
            self.handles[i].set_style(style.clone());
            self.handles[i].set_bounds(d.rect, scale);
        }
        self.dividers = dividers;
        rects
    }

    /// A press at `p`: the separator under it is dragged from now on. Its index, if any.
    pub fn begin_drag(&mut self, p: Point) -> Option<usize> {
        let i = self.handles.iter_mut().position(|h| h.begin_drag(p))?;
        self.dragging = Some(i);
        Some(i)
    }

    /// The dragged separator moved to `p` (`tree`'s ratios follow). `None` if none is
    /// dragged; `Some(true)`: the layout changed.
    pub fn drag(&mut self, tree: &mut SplitTree, p: Point) -> Option<bool> {
        let i = self.dragging?;
        let center = self.handles.get(i).and_then(|h| h.drag_to(p));
        let (Some(divider), Some(center)) = (self.dividers.get(i).cloned(), center) else {
            return Some(false);
        };
        tree.drag(&divider, Point::new(center, center));
        Some(true)
    }

    /// The button released: `true` if a separator was dragged.
    pub fn end_drag(&mut self) -> bool {
        let Some(i) = self.dragging.take() else { return false };
        if let Some(handle) = self.handles.get_mut(i) {
            handle.end_drag();
        }
        true
    }

    /// The mouse at `p` (`None`: away, or blocked by a modal): the separators' hover.
    /// Returns whether one changed and the cursor over one.
    pub fn hover(&mut self, p: Option<Point>) -> (bool, Option<CursorIcon>) {
        let mut redraw = false;
        let mut over = None;
        for (i, handle) in self.handles.iter_mut().enumerate() {
            let inside = p.is_some_and(|p| handle.contains(p));
            redraw |= handle.hover(match p {
                Some(p) if inside => p,
                _ => Point::new(f32::MIN, f32::MIN),
            });
            if inside {
                over = Some(i);
            }
        }
        let icon = over.map(|i| match self.dividers[i].dir {
            Split::Right => CursorIcon::ResizeHorizontal,
            Split::Down => CursorIcon::ResizeVertical,
        });
        (redraw, icon)
    }

    pub fn paint(&self, scene: &mut Scene) {
        for handle in &self.handles {
            handle.paint(scene);
        }
    }

    pub fn animating(&self) -> bool {
        self.handles.iter().any(Splitter::is_animating)
    }
}

/// Where a dragged tab would go, drawn over a group's area (or the part of it a split
/// there would take): a faint accent fill with an accent border.
pub(super) fn paint_drop_zone(layer: &mut render_api::Layer, zone: Rect, scale: f32, p: &Palette) {
    layer.quad(
        Quad::filled(zone, p.accent.with_alpha(0.06))
            .bordered((2.0 * scale).round(), p.accent.with_alpha(0.5)),
    );
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/splits.rs"]
mod tests;
