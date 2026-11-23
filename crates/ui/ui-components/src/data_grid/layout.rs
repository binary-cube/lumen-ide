//! Where the grid's columns are: their order (moved by dragging a header), which are
//! hidden, and how wide each is (dragged at its edge, fitted to its content) — in logical
//! pixels, kept by name so a table's layout comes back when it opens again
//! ([`SavedLayout`]).

use std::collections::HashMap;

/// The narrowest a column gets (logical pixels).
pub const MIN_WIDTH: f32 = 44.0;
/// The widest a column gets.
pub const MAX_WIDTH: f32 = 1600.0;
/// The widest a column gets fitted to its content by itself (wider: dragged).
pub const FIT_WIDTH: f32 = 480.0;
/// A new column's width before its content is measured.
pub const DEFAULT_WIDTH: f32 = 140.0;

/// A table's layout as kept between openings: by column name (names the table no longer
/// has are ignored when it's put back).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SavedLayout {
    pub widths: Vec<(String, f32)>,
    /// The columns' order (names); those not named go after, in the source's order.
    pub order: Vec<String>,
    pub hidden: Vec<String>,
    /// The rows' order chosen (by the grid; empty from [`ColumnLayout::save`]).
    pub sort: Vec<super::source::SortKey>,
    /// The `WHERE` field's text as applied (read again when put back).
    pub filter: String,
    /// The page size chosen in the grid (`Some(None)`: every row); `None`: the default.
    pub page: Option<Option<u32>>,
}

/// The columns as shown (see the module's doc).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ColumnLayout {
    names: Vec<String>,
    /// The source's columns in the order shown.
    order: Vec<usize>,
    hidden: Vec<bool>,
    /// Logical pixels, by the source's column.
    widths: Vec<f32>,
    /// Measured from content (not yet: the default width).
    measured: Vec<bool>,
}

impl ColumnLayout {
    /// The source's columns, in their order, none hidden.
    pub fn new(names: &[String]) -> Self {
        Self {
            names: names.to_vec(),
            order: (0..names.len()).collect(),
            hidden: vec![false; names.len()],
            widths: vec![DEFAULT_WIDTH; names.len()],
            measured: vec![false; names.len()],
        }
    }

    /// The same columns (by name, in order).
    pub fn same_columns(&self, names: &[String]) -> bool {
        self.names == names
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// The source's columns shown, in their order.
    pub fn visible(&self) -> Vec<usize> {
        let mut out = Vec::new();
        self.visible_into(&mut out);
        out
    }

    /// [`Self::visible`] written into `out` (a buffer kept by its owner: nothing allocated
    /// once it has grown).
    pub fn visible_into(&self, out: &mut Vec<usize>) {
        out.clear();
        out.extend(self.order.iter().copied().filter(|&c| !self.hidden[c]));
    }

    /// [`Self::edges`] times `scale` (physical pixels), written into `out` (a kept buffer).
    pub fn edges_into(&self, out: &mut Vec<f32>, scale: f32) {
        out.clear();
        let mut x = 0.0;
        for &col in self.order.iter().filter(|&&c| !self.hidden[c]) {
            out.push(x * scale);
            x += self.width(col);
        }
        out.push(x * scale);
    }

    pub fn is_hidden(&self, col: usize) -> bool {
        self.hidden.get(col).copied().unwrap_or(false)
    }

    pub fn width(&self, col: usize) -> f32 {
        self.widths.get(col).copied().unwrap_or(DEFAULT_WIDTH)
    }

    pub fn is_measured(&self, col: usize) -> bool {
        self.measured.get(col).copied().unwrap_or(false)
    }

    /// Column `col` to be fitted to its content again (as a new one).
    pub fn unmeasure(&mut self, col: usize) {
        if let Some(m) = self.measured.get_mut(col) {
            *m = false;
        }
    }

    /// Column `col`'s width (kept within its limits).
    pub fn set_width(&mut self, col: usize, width: f32) {
        if let Some(w) = self.widths.get_mut(col) {
            *w = width.clamp(MIN_WIDTH, MAX_WIDTH).round();
            self.measured[col] = true;
        }
    }

    /// Column `col` fitted to its content, `content` wide (its padding included), up to
    /// [`FIT_WIDTH`] (wider: by dragging).
    pub fn fit(&mut self, col: usize, content: f32) {
        self.set_width(col, content.min(FIT_WIDTH));
    }

    /// Hides column `col` (never the last one shown).
    pub fn hide(&mut self, col: usize) -> bool {
        if col >= self.hidden.len() || self.hidden[col] || self.visible().len() <= 1 {
            return false;
        }
        self.hidden[col] = true;
        true
    }

    pub fn show(&mut self, col: usize) {
        if let Some(h) = self.hidden.get_mut(col) {
            *h = false;
        }
    }

    pub fn show_all(&mut self) {
        self.hidden.iter_mut().for_each(|h| *h = false);
    }

    /// The column shown at place `from` moved to place `to` (both among those shown).
    pub fn move_column(&mut self, from: usize, to: usize) {
        let visible = self.visible();
        let (Some(&col), Some(&target)) = (visible.get(from), visible.get(to)) else { return };
        if from == to {
            return;
        }
        let at = self.order.iter().position(|&c| c == col).unwrap_or(0);
        self.order.remove(at);
        let target_at = self.order.iter().position(|&c| c == target).unwrap_or(0);
        let insert = if to > from { target_at + 1 } else { target_at };
        self.order.insert(insert.min(self.order.len()), col);
    }

    /// The x of each shown column's left edge from 0 (logical), and the end of the last.
    pub fn edges(&self) -> Vec<f32> {
        let mut x = 0.0;
        let mut out = Vec::with_capacity(self.order.len() + 1);
        for col in self.visible() {
            out.push(x);
            x += self.width(col);
        }
        out.push(x);
        out
    }

    /// What `x` (logical, from the first column's left) is over: the place of the column
    /// shown there, and whether it's at its right edge (within `grip`, for resizing).
    pub fn at(&self, x: f32, grip: f32) -> Option<(usize, bool)> {
        let edges = self.edges();
        for i in 0..edges.len().saturating_sub(1) {
            let right = edges[i + 1];
            if (x - right).abs() <= grip {
                return Some((i, true));
            }
            if x >= edges[i] && x < right {
                return Some((i, false));
            }
        }
        None
    }

    /// The layout kept for the table.
    pub fn save(&self) -> SavedLayout {
        SavedLayout {
            widths: (0..self.names.len())
                .filter(|&c| self.measured[c])
                .map(|c| (self.names[c].clone(), self.widths[c]))
                .collect(),
            order: self.order.iter().map(|&c| self.names[c].clone()).collect(),
            hidden: (0..self.names.len())
                .filter(|&c| self.hidden[c])
                .map(|c| self.names[c].clone())
                .collect(),
            ..SavedLayout::default()
        }
    }

    /// A kept layout put back (names the table no longer has are left out; new columns
    /// come after, in the source's order).
    pub fn restore(&mut self, saved: &SavedLayout) {
        let by_name: HashMap<String, usize> =
            self.names.iter().enumerate().map(|(i, n)| (n.clone(), i)).collect();
        let mut order: Vec<usize> =
            saved.order.iter().filter_map(|n| by_name.get(n).copied()).collect();
        for c in 0..self.names.len() {
            if !order.contains(&c) {
                order.push(c);
            }
        }
        self.order = order;
        for (name, width) in &saved.widths {
            if let Some(&c) = by_name.get(name) {
                self.set_width(c, *width);
            }
        }
        for name in &saved.hidden {
            if let Some(&c) = by_name.get(name) {
                self.hidden[c] = true;
            }
        }
        if self.visible().is_empty() {
            self.show_all();
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/data_grid/layout.rs"]
mod tests;
