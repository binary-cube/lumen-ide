//! Where a profile's view puts its parts, by its width ([`layout`], worked out once per size
//! and state): the bar (40 px) and the summary under it; then the tree on the left and, on
//! the right past a splitter, the panel of findings ("What can be optimized") over the
//! details ([`Mode::Wide`], from [`WIDE`] px); narrower, the panel is a drawer opened from its
//! button, over the tree, not modal ([`Mode::Drawer`]); under [`STACKED`] px the findings go
//! above the tree as a compact list, the details in the drawer ([`Mode::Stacked`]). Under
//! [`SWITCHER_MENU`] px the views' switcher becomes a menu.

use kernel::Rect;

/// From this width (logical pixels) the panel sits beside the tree.
pub const WIDE: f32 = 820.0;
/// Under this width the findings go above the tree.
pub const STACKED: f32 = 520.0;
/// Under this width the views' switcher is a menu.
pub const SWITCHER_MENU: f32 = 560.0;
/// The bar's and the summary's heights (logical pixels).
pub const BAR_H: f32 = 40.0;
pub const SUMMARY_H: f32 = 34.0;
/// The panel's width when nothing chose one, and its least.
pub const PANEL_W: f32 = 340.0;
pub const PANEL_MIN: f32 = 280.0;
/// The splitter's strip.
pub const SPLIT_W: f32 = 6.0;
/// A compact finding's row (stacked).
pub const COMPACT_ROW: f32 = 28.0;

/// How the panel is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Wide,
    Drawer,
    Stacked,
}

impl Mode {
    pub fn of(width: f32) -> Self {
        if width >= WIDE {
            Mode::Wide
        } else if width >= STACKED {
            Mode::Drawer
        } else {
            Mode::Stacked
        }
    }
}

/// What the layout depends on (logical pixels, bits of floats: a key).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutSpec {
    pub bounds: Rect,
    pub scale: f32,
    /// The panel's width chosen.
    pub panel_w: f32,
    pub drawer_open: bool,
    /// Findings shown in the compact list (stacked).
    pub compact_rows: usize,
}

/// The parts' places (physical pixels).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ProfileLayout {
    pub mode_wide: bool,
    pub bar: Rect,
    pub summary: Rect,
    /// The compact findings (stacked; empty otherwise).
    pub compact: Rect,
    pub tree: Rect,
    /// The splitter's strip (wide only).
    pub split: Rect,
    /// The panel (beside, or the drawer over the tree; empty when closed).
    pub panel: Rect,
    pub drawer: bool,
    pub switcher_menu: bool,
}

pub fn layout(spec: &LayoutSpec) -> ProfileLayout {
    let b = spec.bounds;
    let s = spec.scale;
    let px = |v: f32| (v * s).round();
    let logical = b.size.width / s.max(0.01);
    let mode = Mode::of(logical);
    let bar = Rect::new(b.min_x(), b.min_y(), b.size.width, px(BAR_H));
    let summary = Rect::new(b.min_x(), bar.max_y(), b.size.width, px(SUMMARY_H));
    let mut top = summary.max_y();
    let bottom = b.max_y();
    let mut out = ProfileLayout {
        mode_wide: mode == Mode::Wide,
        bar,
        summary,
        switcher_menu: logical < SWITCHER_MENU,
        ..ProfileLayout::default()
    };
    if mode == Mode::Stacked && spec.compact_rows > 0 {
        let rows = spec.compact_rows.min(4) as f32;
        let h = (px(COMPACT_ROW) * rows + px(8.0)).min(((bottom - top) * 0.4).max(0.0));
        out.compact = Rect::new(b.min_x(), top, b.size.width, h.round());
        top += out.compact.size.height;
    }
    let body_h = (bottom - top).max(0.0);
    match mode {
        Mode::Wide => {
            let max = (b.size.width * 0.6).max(px(PANEL_MIN));
            let panel_w = px(spec.panel_w).clamp(px(PANEL_MIN).min(max), max);
            let split_w = px(SPLIT_W);
            let tree_w = (b.size.width - panel_w - split_w).max(0.0);
            out.tree = Rect::new(b.min_x(), top, tree_w, body_h);
            out.split = Rect::new(b.min_x() + tree_w, top, split_w, body_h);
            out.panel = Rect::new(out.split.max_x(), top, panel_w, body_h);
        }
        Mode::Drawer | Mode::Stacked => {
            out.tree = Rect::new(b.min_x(), top, b.size.width, body_h);
            if spec.drawer_open {
                let w = px(spec.panel_w.min(PANEL_W)).min(b.size.width - px(40.0)).max(0.0);
                out.panel = Rect::new(b.max_x() - w, top, w, body_h);
                out.drawer = true;
            }
        }
    }
    out
}

#[cfg(test)]
#[path = "../../tests/unit/profile/layout.rs"]
mod tests;
