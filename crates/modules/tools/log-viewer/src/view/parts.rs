//! The log view's parts as a form's controls (`ui_components::control`): the bar's
//! buttons, the details under the grid (their text and ×), the grid itself. The view hands
//! them as a list and reads back which one answered; what each one means is the view's.

use kernel::{Point, Rect};
use ui_components::control::{Control, NOWHERE, Press};
use ui_components::data_grid::DataGrid;
use ui_components::tooltip::Tip;
use ui_components::{KeyReach, PressMods, ReadText, ViewMenuEntry, ViewerCursor, ViewerKey};

use super::Button;
use crate::source::LogSource;

/// The bar's buttons: a press takes them (the view does what the one there does), the one
/// under the mouse lit.
pub(super) struct BarButtons<'a> {
    pub buttons: &'a [(Rect, Button)],
    pub hover: &'a mut Option<Button>,
}

impl BarButtons<'_> {
    fn at(&self, p: Point) -> Option<Button> {
        self.buttons.iter().find(|(r, _)| r.contains(p)).map(|(_, b)| *b)
    }
}

impl Control for BarButtons<'_> {
    fn hit(&self, p: Point) -> bool {
        self.at(p).is_some()
    }

    fn press(&mut self, _p: Point, _clicks: u32) -> Press {
        Press::Took
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        let lit = p.and_then(|p| self.at(p));
        let changed = lit != *self.hover;
        *self.hover = lit;
        changed
    }

    fn cursor(&self, _p: Point) -> Option<ViewerCursor> {
        Some(ViewerCursor::Pointer)
    }
}

/// The details under the grid, while a row's are shown: their text (selected by a press and
/// a drag, ⇧ extending it; scrolled by the wheel, which never goes on to the grid under) and
/// × (a press on it is taken: the view closes them); their right-click menu is the text's,
/// then `extra`'s (the line's: made only when the menu opens).
pub(super) struct DetailsPart<'a> {
    pub read: Option<&'a mut ReadText>,
    pub area: Rect,
    pub close: Rect,
    pub close_hover: &'a mut bool,
    pub extra: fn() -> Vec<ViewMenuEntry>,
}

impl Control for DetailsPart<'_> {
    fn hit(&self, p: Point) -> bool {
        self.read.is_some() && self.area.contains(p)
    }

    fn press(&mut self, p: Point, clicks: u32) -> Press {
        self.press_with(p, clicks, PressMods::default())
    }

    fn press_with(&mut self, p: Point, clicks: u32, mods: PressMods) -> Press {
        if self.close.contains(p) {
            return Press::Took;
        }
        let held = self.read.as_mut().is_some_and(|r| ReadText::press(r, p, clicks, mods.shift));
        if held { Press::Held } else { Press::Took }
    }

    fn drag(&mut self, p: Point) -> bool {
        self.read.as_mut().is_some_and(|r| ReadText::drag(r, p))
    }

    fn release(&mut self) -> bool {
        self.read.as_mut().is_some_and(|r| ReadText::release(r))
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        let on_close = p.is_some_and(|p| self.close.contains(p));
        let changed = on_close != *self.close_hover;
        *self.close_hover = on_close;
        let text = self.read.as_mut().is_some_and(|r| ReadText::hover(r, p.unwrap_or(NOWHERE)));
        changed | text
    }

    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        if self.close.contains(p) {
            return Some(ViewerCursor::Pointer);
        }
        let over = self.read.as_ref().is_some_and(|r| r.over_text(p));
        Some(if over { ViewerCursor::Text } else { ViewerCursor::Arrow })
    }

    /// Taken even at their end: the wheel over them never scrolls the grid.
    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        if let Some(read) = &mut self.read {
            read.scroll_by(dy);
        }
        true
    }

    fn menu(&mut self, _p: Point) -> Option<Vec<ViewMenuEntry>> {
        let mut menu = self.read.as_ref().map(|r| ReadText::menu(r)).unwrap_or_default();
        menu.extend((self.extra)());
        Some(menu)
    }
}

/// The grid, under every other part: what none of them takes is its (a press inside it, the
/// mouse, the wheel, the menu and the tooltip anywhere), and it keeps the keys (Tab too:
/// it moves among its cells).
pub(super) struct GridPart<'a> {
    pub grid: Option<&'a mut DataGrid<LogSource>>,
    pub area: Rect,
}

impl Control for GridPart<'_> {
    fn hit(&self, _p: Point) -> bool {
        self.grid.is_some()
    }

    fn press(&mut self, p: Point, clicks: u32) -> Press {
        self.press_with(p, clicks, PressMods::default())
    }

    /// Inside it, its own press; elsewhere (the bar around the buttons) taken, nothing done.
    fn press_with(&mut self, p: Point, clicks: u32, mods: PressMods) -> Press {
        match &mut self.grid {
            Some(grid) if self.area.contains(p) => Control::press_with(*grid, p, clicks, mods),
            _ => Press::Took,
        }
    }

    fn keeps_tab(&self) -> bool {
        true
    }

    fn drag(&mut self, p: Point) -> bool {
        self.grid.as_mut().is_some_and(|g| Control::drag(*g, p))
    }

    fn release(&mut self) -> bool {
        self.grid.as_mut().is_some_and(|g| Control::release(*g))
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        self.grid.as_mut().is_some_and(|g| Control::hover(*g, p))
    }

    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        self.grid.as_ref().and_then(|g| Control::cursor(*g, p))
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        self.grid.as_mut().is_some_and(|g| Control::scroll(*g, dx, dy))
    }

    fn focusable(&self) -> bool {
        self.grid.is_some()
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        self.grid.as_mut().is_some_and(|g| Control::key(*g, key, reach))
    }

    fn text(&mut self, typed: &str) -> bool {
        self.grid.as_mut().is_some_and(|g| Control::text(*g, typed))
    }

    /// A character typed with no field open: it starts editing the cell (its own keys).
    fn char_key(&mut self, key: char) -> bool {
        self.grid.as_mut().is_some_and(|g| g.type_char(key))
    }

    fn command(&mut self, id: &str) -> bool {
        self.grid.as_mut().is_some_and(|g| Control::command(*g, id))
    }

    fn tooltip(&self, p: Point) -> Option<Tip> {
        self.grid.as_ref().and_then(|g| Control::tooltip(*g, p))
    }

    fn menu(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        self.grid.as_mut().and_then(|g| Control::menu(*g, p))
    }
}
