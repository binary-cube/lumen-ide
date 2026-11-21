//! The right-click menu, drawn by us (not the system's): the same look on every system,
//! in the theme's colors — icons, labels, shortcuts, separators and submenus (opened on
//! hover, a click or →; ← closes one), with keyboard navigation (↑ ↓, ↵, esc). A click
//! outside closes it.

use std::time::Duration;

use icons::Icons;
use kernel::{Point, Rect};
use render_api::{Quad, Scene};
use text_layout::TextSystem;
use ui_kit::{draw_icon, draw_text};

use crate::RowLook;
use crate::anim::Appear;
use crate::ds::{self, Ctx, Elevation, radius};
use crate::palette::Palette;

/// A row of the menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuEntry {
    Item {
        /// What is run (a command id, as the native menus send it).
        id: String,
        label: String,
        /// As the system shows it (`⌘⇧C`, `Ctrl+Shift+C`).
        shortcut: Option<String>,
        icon: Option<&'static str>,
        /// A setting it turns on or off: how it stands, a small switch at the row's end
        /// (in place of a shortcut).
        on: Option<bool>,
        /// Greyed out (never run, never selected), checked (a ✓ at its end), a color dot
        /// in its icon's place.
        look: RowLook,
    },
    /// A row opening more rows beside it.
    Submenu {
        label: String,
        icon: Option<&'static str>,
        entries: Vec<MenuEntry>,
    },
    Separator,
}

impl MenuEntry {
    fn is_row(&self) -> bool {
        !matches!(self, MenuEntry::Separator)
    }

    /// A row the keys and the mouse can stop on (not a separator, not greyed out).
    fn selectable(&self) -> bool {
        match self {
            MenuEntry::Item { look, .. } => !look.disabled,
            MenuEntry::Submenu { .. } => true,
            MenuEntry::Separator => false,
        }
    }
}

/// The rows as they're shown: no empty section anywhere — separators at the start, at the
/// end or after another one dropped, and submenus left without rows dropped (in submenus
/// too), so whoever builds a menu may add a separator before each optional group.
pub fn tidy_menu(entries: Vec<MenuEntry>) -> Vec<MenuEntry> {
    let mut out: Vec<MenuEntry> = Vec::with_capacity(entries.len());
    for entry in entries {
        match entry {
            MenuEntry::Separator => {
                if out.last().is_some_and(MenuEntry::is_row) {
                    out.push(MenuEntry::Separator);
                }
            }
            MenuEntry::Submenu { label, icon, entries } => {
                let entries = tidy_menu(entries);
                if !entries.is_empty() {
                    out.push(MenuEntry::Submenu { label, icon, entries });
                }
            }
            item => out.push(item),
        }
    }
    if matches!(out.last(), Some(MenuEntry::Separator)) {
        out.pop();
    }
    out
}

/// What a click on the menu did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextAction {
    /// Run this item's id (the menu closes).
    Run(String),
    /// Run this setting's id (the menu stays, showing how it stands now).
    Switch(String),
    /// Inside, not on an item (or on a submenu's row: it opens).
    Inside,
    /// Outside: the menu closes.
    Close,
}

/// The widest a row's label is drawn (logical pixels): what's longer is shortened.
const MAX_LABEL_W: f32 = 420.0;

#[derive(Debug, Default)]
pub struct ContextMenu {
    open: bool,
    entries: Vec<MenuEntry>,
    at: Point,
    /// A submenu opens to the left of this x when it doesn't fit on the right (its parent's
    /// left edge).
    flip_at: Option<f32>,
    /// The button it was opened for, on the window's right side: it stands to its left.
    left_of: Option<Rect>,
    selected: Option<usize>,
    viewport: Rect,
    scale: f32,
    panel: Rect,
    /// The rows (from the last paint), with their entry index.
    rows: Vec<(Rect, usize)>,
    appear: Appear,
    /// The open submenu, and the entry it belongs to.
    child: Option<(usize, Box<ContextMenu>)>,
}

impl ContextMenu {
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens the menu with its top-left corner at `at` (moved to stay in the window).
    /// Its rows are tidied first ([`tidy_menu`]): no empty section is ever shown.
    pub fn open(&mut self, entries: Vec<MenuEntry>, at: Point) {
        let entries = tidy_menu(entries);
        if entries.is_empty() {
            return;
        }
        self.open = true;
        self.entries = entries;
        self.at = at;
        self.left_of = None;
        self.selected = None;
        self.child = None;
        self.appear.start(Duration::from_millis(90));
    }

    /// Opens the menu to the left of `button` (one on the window's right side), `gap` away,
    /// its top on the button's, or its bottom when it doesn't fit below: never over it,
    /// however wide its rows are.
    pub fn open_left_of(&mut self, entries: Vec<MenuEntry>, button: Rect, gap: f32) {
        self.open(entries, Point::new(button.min_x() - gap, button.min_y()));
        self.left_of = Some(button);
    }

    pub fn close(&mut self) {
        self.open = false;
        self.entries = Vec::new();
        self.rows.clear();
        self.child = None;
    }

    pub fn is_animating(&self) -> bool {
        self.open
            && (self.appear.animating()
                || self.child.as_ref().is_some_and(|(_, c)| c.is_animating()))
    }

    pub fn set_viewport(&mut self, viewport: Rect, scale: f32) {
        self.viewport = viewport;
        self.scale = scale;
        if let Some((_, child)) = &mut self.child {
            child.set_viewport(viewport, scale);
        }
    }

    fn rows_indices(&self) -> impl Iterator<Item = usize> + '_ {
        self.entries.iter().enumerate().filter(|(_, e)| e.selectable()).map(|(i, _)| i)
    }

    /// The deepest open menu (where the keyboard acts).
    fn deepest(&mut self) -> &mut ContextMenu {
        match self.child {
            Some((_, ref mut child)) => child.deepest(),
            None => self,
        }
    }

    /// Opens entry `i`'s submenu beside its row (from the last paint). `false` if it isn't
    /// one.
    fn open_child(&mut self, i: usize) -> bool {
        if self.child.as_ref().is_some_and(|(at, _)| *at == i) {
            return true;
        }
        let Some(MenuEntry::Submenu { entries, .. }) = self.entries.get(i) else { return false };
        let row = self.rows.iter().find(|(_, e)| *e == i).map(|(r, _)| *r).unwrap_or_default();
        let pad = (5.0 * self.scale).round();
        let mut child = ContextMenu::default();
        child.set_viewport(self.viewport, self.scale);
        child.open(entries.clone(), Point::new(self.panel.max_x() - pad / 2.0, row.min_y() - pad));
        child.flip_at = Some(self.panel.min_x() + pad / 2.0);
        self.child = Some((i, Box::new(child)));
        true
    }

    /// The next / previous row (keyboard), skipping separators.
    pub fn step(&mut self, delta: isize) {
        let menu = self.deepest();
        let rows: Vec<usize> = menu.rows_indices().collect();
        if rows.is_empty() {
            return;
        }
        let at = menu.selected.and_then(|s| rows.iter().position(|&i| i == s));
        let next = match at {
            Some(k) => ui_kit::step_index(k, delta, rows.len()),
            None if delta < 0 => rows.len() - 1,
            None => 0,
        };
        menu.selected = Some(rows[next]);
    }

    /// → opens the selected submenu (its first row selected); ← closes the deepest one.
    pub fn enter_submenu(&mut self, open: bool) -> bool {
        if !open {
            return self.close_deepest_child();
        }
        let parent = self.deepest();
        let Some(i) = parent.selected else { return false };
        if !parent.open_child(i) {
            return false;
        }
        parent.step(1);
        true
    }

    fn close_deepest_child(&mut self) -> bool {
        match &mut self.child {
            Some((_, child)) if child.child.is_some() => child.close_deepest_child(),
            Some(_) => {
                self.child = None;
                true
            }
            None => false,
        }
    }

    /// The selected item's id (Enter); a selected submenu opens instead (`None`).
    /// The keys' current row is a setting's (run, it leaves the menu open).
    pub fn on_switch(&mut self) -> bool {
        let menu = self.deepest();
        menu.selected
            .and_then(|i| menu.entries.get(i))
            .is_some_and(|e| matches!(e, MenuEntry::Item { on: Some(_), .. }))
    }

    /// The settings' rows brought up to date (`state`: how the row's id stands now).
    pub fn refresh_switches(&mut self, state: &dyn Fn(&str) -> Option<bool>) {
        for entry in &mut self.entries {
            if let MenuEntry::Item { id, on: on @ Some(_), .. } = entry
                && let Some(now) = state(id)
            {
                *on = Some(now);
            }
        }
        if let Some((_, child)) = &mut self.child {
            child.refresh_switches(state);
        }
    }

    pub fn confirm(&mut self) -> Option<String> {
        let menu = self.deepest();
        let entry = menu.entries.get(menu.selected?)?.clone();
        match entry {
            MenuEntry::Item { look, .. } if look.disabled => None,
            MenuEntry::Item { id, .. } => Some(id),
            MenuEntry::Submenu { .. } => {
                self.enter_submenu(true);
                None
            }
            MenuEntry::Separator => None,
        }
    }

    pub fn hit(&mut self, p: Point) -> Option<ContextAction> {
        if !self.open {
            return None;
        }
        if let Some((_, child)) = &mut self.child
            && child.contains(p)
        {
            return child.hit(p);
        }
        if !self.panel.contains(p) {
            return Some(ContextAction::Close);
        }
        let entry = self.rows.iter().find(|(r, _)| r.contains(p)).map(|(_, i)| *i);
        Some(match entry.and_then(|i| self.entries.get(i).map(|e| (i, e))) {
            // Greyed out: nothing runs, the menu stays.
            Some((_, MenuEntry::Item { look, .. })) if look.disabled => ContextAction::Inside,
            Some((_, MenuEntry::Item { id, on: Some(_), .. })) => ContextAction::Switch(id.clone()),
            Some((_, MenuEntry::Item { id, .. })) => ContextAction::Run(id.clone()),
            Some((i, MenuEntry::Submenu { .. })) => {
                self.selected = Some(i);
                self.open_child(i);
                ContextAction::Inside
            }
            _ => ContextAction::Inside,
        })
    }

    /// The menu or one of its open submenus is under `p`.
    fn contains(&self, p: Point) -> bool {
        self.panel.contains(p) || self.child.as_ref().is_some_and(|(_, c)| c.contains(p))
    }

    pub fn hover(&mut self, p: Point) -> bool {
        if let Some((_, child)) = &mut self.child
            && child.contains(p)
        {
            return child.hover(p);
        }
        let hover = self.rows.iter().find(|(r, _)| r.contains(p)).map(|(_, i)| *i);
        let Some(i) = hover else { return false };
        // A greyed-out row isn't lit: whatever was stays.
        if self.entries.get(i).is_some_and(|e| !e.selectable()) {
            return false;
        }
        let changed = Some(i) != self.selected;
        self.selected = Some(i);
        if changed {
            // Another row: its submenu opens, any other closes.
            self.child = None;
            if matches!(self.entries.get(i), Some(MenuEntry::Submenu { .. })) {
                self.open_child(i);
            }
        }
        changed
    }

    pub fn over_item(&self, p: Point) -> bool {
        self.rows.iter().any(|(r, _)| r.contains(p))
            || self.child.as_ref().is_some_and(|(_, c)| c.over_item(p))
    }

    pub fn paint(
        &mut self,
        icons: &Icons,
        text: &mut TextSystem,
        scene: &mut Scene,
        palette: &Palette,
    ) {
        self.rows.clear();
        if !self.open {
            return;
        }
        let shown = self.appear.value();
        let ctx = Ctx::new(palette, self.scale).with_opacity(shown);
        let px = |v: f32| ctx.px(v);
        let font = ctx.ui_font(-0.5);
        let key_font = ctx.ui_font(-1.0);
        let row_h = px(30.0);
        let separator_h = px(9.0);
        let pad = px(5.0);
        let icon_w = px(30.0);
        let max_label = px(MAX_LABEL_W);

        // Width: the widest label and shortcut (a submenu's row has its chevron there).
        let mut label_w: f32 = 0.0;
        let mut key_w: f32 = 0.0;
        let mut height = pad * 2.0;
        for entry in &self.entries {
            match entry {
                MenuEntry::Item { label, shortcut, on, look, .. } => {
                    label_w = label_w.max(text.shape_line(label, &font).width.min(max_label));
                    if on.is_some() {
                        key_w = key_w.max(px(ds::SMALL_SWITCH_W));
                    } else if let Some(key) = shortcut {
                        key_w = key_w.max(text.shape_line(key, &key_font).width);
                    }
                    if look.checked {
                        key_w = key_w.max(px(14.0));
                    }
                    height += row_h;
                }
                MenuEntry::Submenu { label, .. } => {
                    label_w = label_w.max(text.shape_line(label, &font).width.min(max_label));
                    key_w = key_w.max(px(12.0));
                    height += row_h;
                }
                MenuEntry::Separator => height += separator_h,
            }
        }
        let gap = if key_w > 0.0 { px(28.0) } else { 0.0 };
        let width = (icon_w + label_w + gap + key_w + px(14.0) + pad * 2.0)
            .clamp(px(180.0), self.viewport.size.width - px(16.0));
        let v = self.viewport;
        let mut x = if self.left_of.is_some() { self.at.x - width } else { self.at.x };
        // A submenu that doesn't fit on its parent's right opens on its left.
        if let Some(left) = self.flip_at
            && x + width > v.max_x() - px(8.0)
        {
            x = left - width;
        }
        let x = x.min(v.max_x() - width - px(8.0)).max(v.min_x() + px(8.0));
        // Above the pointer when it doesn't fit below.
        let y = if self.at.y + height > v.max_y() - px(8.0) {
            if let Some(button) = self.left_of {
                (button.max_y() - height).max(v.min_y() + px(8.0))
            } else if self.flip_at.is_some() {
                (v.max_y() - px(8.0) - height).max(v.min_y() + px(8.0))
            } else {
                (self.at.y - height).max(v.min_y() + px(8.0))
            }
        } else {
            self.at.y
        };
        self.panel = Rect::new(x.round(), y.round(), width.round(), height.round());
        let p = self.panel;

        let layer = scene.push_layer(None);
        ds::surface_with(layer, &ctx, p, Elevation::Medium, radius::MD, palette.popup_background);
        let layer = scene.push_layer(Some(p));
        let mut top = p.min_y() + pad;
        let open_child = self.child.as_ref().map(|(i, _)| *i);
        for (i, entry) in self.entries.iter().enumerate() {
            let (label, shortcut, icon, submenu, on, look) = match entry {
                MenuEntry::Separator => {
                    let y = (top + separator_h / 2.0).round();
                    ds::divider(layer, &ctx, p.min_x() + px(10.0), p.max_x() - px(10.0), y);
                    top += separator_h;
                    continue;
                }
                MenuEntry::Item { label, shortcut, icon, on, look, .. } => {
                    (label, shortcut.as_deref(), icon, false, *on, *look)
                }
                MenuEntry::Submenu { label, icon, .. } => {
                    (label, None, icon, true, None, RowLook::default())
                }
            };
            let row = Rect::new(p.min_x() + pad, top, p.size.width - pad * 2.0, row_h);
            let selected = (self.selected == Some(i) || open_child == Some(i)) && !look.disabled;
            // A greyed-out row: its icon, label and keys faded.
            let dim = |c: kernel::Rgba| if look.disabled { c.with_alpha(c.a * 0.45) } else { c };
            let (label_ink, icon_ink) = row_ink(look, selected, palette);
            ds::RowHighlight::new(selected, false)
                .selected_alpha(ROW_LIT)
                .radius(radius::SM)
                .paint(layer, &ctx, row);
            if let Some(rgb) = look.swatch {
                // A label's color: a dot in the icon's place.
                let size = px(12.0);
                let dot = Rect::new(
                    row.min_x() + px(10.5),
                    (top + (row_h - size) / 2.0).round(),
                    size,
                    size,
                );
                layer.quad(Quad::filled(dot, ctx.fade(dim(kernel::Rgba::rgb(rgb)))).rounded(size));
            } else if let Some(id) = icon.and_then(|name| icons.id(name)) {
                let size = px(15.0);
                draw_icon(
                    layer,
                    icons,
                    id,
                    Rect::new(
                        row.min_x() + px(9.0),
                        (top + (row_h - size) / 2.0).round(),
                        size,
                        size,
                    ),
                    ctx.fade(dim(icon_ink)),
                );
            }
            // A file's long name keeps its start, its end and its extension.
            let fitted = ds::ellipsize_file(text, label, &font, max_label);
            let shaped = text.shape_line(&fitted, &font);
            draw_text(
                layer,
                &shaped,
                row.min_x() + icon_w,
                ds::optical_baseline(top, row_h, &font),
                ctx.fade(dim(label_ink)),
            );
            if let Some(on) = on {
                let x = (row.max_x() - px(10.0) - px(ds::SMALL_SWITCH_W)).round();
                ds::small_switch(layer, &ctx, x, top + row_h / 2.0, on);
            } else if let Some(key) = shortcut {
                let shaped = text.shape_line(key, &key_font);
                draw_text(
                    layer,
                    &shaped,
                    (row.max_x() - px(10.0) - shaped.width).round(),
                    ds::optical_baseline(top, row_h, &key_font),
                    ctx.fade(dim(palette.text_muted)),
                );
            } else if look.checked
                && let Some(id) = icons.id("check")
            {
                // The choice made: a ✓ at the row's end.
                let size = px(14.0);
                draw_icon(
                    layer,
                    icons,
                    id,
                    Rect::new(
                        (row.max_x() - px(9.0) - size).round(),
                        (top + (row_h - size) / 2.0).round(),
                        size,
                        size,
                    ),
                    ctx.fade(dim(palette.text)),
                );
            }
            if submenu && let Some(id) = icons.id("chevron-right") {
                let size = px(13.0);
                draw_icon(
                    layer,
                    icons,
                    id,
                    Rect::new(
                        (row.max_x() - px(8.0) - size).round(),
                        (top + (row_h - size) / 2.0).round(),
                        size,
                        size,
                    ),
                    ctx.fade(if selected { palette.text } else { palette.text_muted }),
                );
            }
            self.rows.push((row, i));
            top += row_h;
        }
        if let Some((_, child)) = &mut self.child {
            child.paint(icons, text, scene, palette);
        }
    }
}

/// The colors of a row's label and icon: a dangerous one's both in the danger color, else
/// the text's and a muted icon (in the accent while selected).
fn row_ink(look: RowLook, selected: bool, palette: &Palette) -> (kernel::Rgba, kernel::Rgba) {
    if look.danger {
        return (palette.danger, palette.danger);
    }
    (palette.text, if selected { palette.accent } else { palette.text_muted })
}

/// The row under the mouse or the keys (the accent's alpha).
const ROW_LIT: f32 = 0.14;

#[cfg(test)]
#[path = "../tests/unit/context_menu.rs"]
mod tests;
