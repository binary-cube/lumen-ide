//! A preview's page: its form at the top, its list of places below it, the keys at the
//! bottom. When its group is too short for the form and a few rows of the list (a group
//! below the editor, a small window), the page scrolls: down, the form goes up first, then
//! the list scrolls; up, the list first, then the form comes back. A scrollbar on the right
//! shows (and drags) the page.

use kernel::{Point, Rect};
use render_api::Scene;
use ui_components::{EdgeScrollbar, Palette};

use crate::plan_list::PlanList;

/// The list's height the page scrolls for (logical pixels): "every place" and a few rows.
pub const MIN_LIST: f32 = 150.0;

#[derive(Debug, Default)]
pub struct PageScroll {
    offset: f32,
    /// How far the page scrolls (from the last paint).
    max: f32,
    /// Above the keys (from the last paint).
    area: Rect,
    bar: EdgeScrollbar,
}

impl PageScroll {
    /// The page in bounds `b` above keys `footer_h` high, and where its content starts: `top`
    /// below its top, scrolled.
    pub fn begin(&mut self, b: Rect, footer_h: f32, top: f32) -> (Rect, f32) {
        self.area =
            Rect::new(b.min_x(), b.min_y(), b.size.width, (b.size.height - footer_h).max(0.0));
        self.offset = self.offset.min(self.max);
        (self.area, self.area.min_y() + top - self.offset)
    }

    /// The list's area: from `y` (where the form ends, scrolled) to `bottom`; the page scrolls
    /// so that at least `min_list` of it shows.
    pub fn list_area(&mut self, x0: f32, width: f32, y: f32, bottom: f32, min_list: f32) -> Rect {
        let min = min_list.min((bottom - self.area.min_y()).max(0.0));
        self.max = (y + self.offset + min - bottom).max(0.0);
        Rect::new(x0, y, width, (bottom - y).max(0.0))
    }

    /// Its scrollbar (nothing when the page fits).
    pub fn paint_bar(&mut self, scene: &mut Scene, palette: &Palette, scale: f32) {
        let content = self.area.size.height + self.max;
        self.bar.paint(scene, palette, scale, self.area, content, self.offset);
    }

    /// Point `p` is on the page (not on the keys under it).
    pub fn contains(&self, p: Point) -> bool {
        self.area.contains(p)
    }

    /// Scrolls the page and `list` by `dy` (see the module's doc). `true` if either moved.
    pub fn scroll(&mut self, list: &mut PlanList, dy: f32) -> bool {
        let before = (self.offset, list.offset());
        if dy > 0.0 {
            let page = (self.offset + dy).min(self.max).max(self.offset);
            let rest = dy - (page - self.offset);
            self.offset = page;
            list.scroll(rest);
        } else {
            let list_before = list.offset();
            list.scroll(dy);
            let rest = dy + (list_before - list.offset());
            self.offset = (self.offset + rest).max(0.0);
        }
        before != (self.offset, list.offset())
    }

    /// A press on its scrollbar (`true`: taken).
    pub fn press(&mut self, p: Point) -> bool {
        let Some(offset) = self.bar.press(p) else { return false };
        self.offset = offset.clamp(0.0, self.max);
        true
    }

    pub fn drag(&mut self, p: Point) -> bool {
        let Some(offset) = self.bar.drag(p) else { return false };
        let changed = offset != self.offset;
        self.offset = offset.clamp(0.0, self.max);
        changed
    }

    pub fn release(&mut self) -> bool {
        self.bar.release()
    }

    pub fn hover(&mut self, p: Point) -> bool {
        self.bar.hover(p)
    }
}

#[cfg(test)]
#[path = "../tests/unit/page.rs"]
mod tests;
