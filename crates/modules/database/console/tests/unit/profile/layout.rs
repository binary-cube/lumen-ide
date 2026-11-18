//! The profile's layout by its width: the panel beside the tree (wide), a drawer over it
//! (narrower), the findings above it (narrowest); the switcher a menu under 560 px; the
//! panel's width kept within bounds.

use super::*;

fn spec(width: f32, drawer: bool) -> LayoutSpec {
    LayoutSpec {
        bounds: Rect::new(0.0, 0.0, width * 2.0, 1000.0),
        scale: 2.0,
        panel_w: PANEL_W,
        drawer_open: drawer,
        compact_rows: 3,
    }
}

#[test]
fn the_panel_follows_the_width() {
    let wide = layout(&spec(1200.0, false));
    assert!(wide.mode_wide && !wide.drawer);
    assert_eq!(wide.panel.size.width, PANEL_W * 2.0);
    assert_eq!(wide.tree.max_x(), wide.split.min_x());
    assert_eq!(wide.split.max_x(), wide.panel.min_x());
    assert_eq!(wide.compact.size.height, 0.0);
    assert_eq!(wide.tree.min_y(), wide.summary.max_y());
    // Narrower: the tree whole, the panel a drawer over it once opened.
    let closed = layout(&spec(700.0, false));
    assert!(!closed.mode_wide && closed.panel.size.width == 0.0);
    assert_eq!(closed.tree.size.width, 1400.0);
    let open = layout(&spec(700.0, true));
    assert!(open.drawer && open.panel.max_x() == open.tree.max_x());
    assert!(!open.switcher_menu);
    // Narrowest: the findings above the tree, the switcher a menu.
    let stacked = layout(&spec(480.0, false));
    assert!(stacked.compact.size.height > 0.0 && stacked.switcher_menu);
    assert_eq!(stacked.tree.min_y(), stacked.compact.max_y());
    assert_eq!(Mode::of(819.0), Mode::Drawer);
    assert_eq!(Mode::of(820.0), Mode::Wide);
    assert_eq!(Mode::of(519.0), Mode::Stacked);
}

#[test]
fn the_panel_s_width_stays_within_bounds() {
    let mut s = spec(1200.0, false);
    s.panel_w = 50.0;
    assert_eq!(layout(&s).panel.size.width, PANEL_MIN * 2.0);
    s.panel_w = 5000.0;
    assert!(layout(&s).panel.size.width <= 2400.0 * 0.6 + 1.0);
}
