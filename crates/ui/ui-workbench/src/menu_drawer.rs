//! The app's menus in a drawer (where the system has no menu bar of ours: Windows, Linux;
//! on macOS when asked for): it slides in from the left under the top bar, over a dimmed
//! window, with the menus as categories (icon, title, chevron). The mouse resting on one —
//! or ← → ↵ — opens its submenu as a second panel beside it, the drawer widening smoothly;
//! a row there with a submenu of its own (Code › Folding ›) opens it as a third panel the
//! same way, beside it (over it, with a header back, where the window is too narrow for
//! three). The filter on top searches every submenu at once, each result with its path
//! (`Code › Folding › Collapse All`). Its footer says
//! which keys work. A click outside closes it (it's a menu, not a modal with the user's
//! work); Esc, the menu button and Alt / F10 do too. What it is is [`DrawerNav`]'s; this
//! draws it and tells what the mouse hit.

use std::time::{Duration, Instant};

use icons::Icons;
use kernel::{Point, Rect};
use render_api::{Layer, Quad, Scene};
use text_layout::{Font, TextSystem};
use ui_components::search_field::SearchField;
use ui_components::{BackdropRole, EdgeScrollbar, ScrollState};
use ui_kit::{KeyHint, draw_icon, draw_text};

use crate::anim::{Appear, Tween};
use crate::context_menu::MenuEntry;
use crate::ds::{self, Ctx, radius};
use crate::menu_nav::{DrawerMenu, DrawerNav, HoverIntent, Intent, NavFocus, NavKey, NavOutcome};
use crate::modal::{KeyFooter, paint_key_footer};
use crate::palette::Palette;

mod paint;
mod pointer;

/// The categories' panel, the submenu's and a row's own submenu's (logical pixels).
const MENUS_W: f32 = 300.0;
const ITEMS_W: f32 = 300.0;
const SUB_W: f32 = 300.0;
/// How long it slides in or out, and the submenu widens.
const SLIDE: Duration = Duration::from_millis(160);
/// The footer's height (the list modals').
const FOOTER_H: f32 = 44.0;
const PAD: f32 = 12.0;
const FIELD_H: f32 = 36.0;
/// Every row is as tall (the categories', the results', the submenu's).
const MENU_ROW: f32 = 34.0;
const ITEM_ROW: f32 = 34.0;
const ICON: f32 = 16.0;
/// A row's icon and its label, from the row's left edge (the same in both panels).
const ICON_AT: f32 = 12.0;
const LABEL_AT: f32 = 40.0;
const SEPARATOR: f32 = 9.0;

/// What a click on the drawer did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DrawerAction {
    /// Run this command (the drawer closes).
    Run(String),
    /// Outside its panels: it closes.
    Close,
    /// A scrollbar taken: a drag follows.
    Drag,
    /// Inside, nothing more (a category shown, the field clicked).
    Inside,
}

/// What the mouse is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Under {
    Menu(usize),
    Item(usize),
    /// A row of the third panel.
    SubItem(usize),
    /// The third panel's header, back to the submenu (a narrow window).
    SubBack,
    Result(usize),
}
#[derive(Debug)]
pub struct MenuDrawer {
    nav: DrawerNav,
    /// The filter's field (the app's search field: ×, ⌘A; it has the keyboard while the
    /// drawer is open, a part of its controls).
    pub query: SearchField,
    /// The filter the lists were last made with.
    filtered_by: String,
    intent: HoverIntent,
    open: bool,
    /// How far it's in (0: gone, 1: in), sliding.
    shown: Tween,
    /// How far the submenu's panel is out (0..1), widening.
    widen: Tween,
    /// The submenu's rows fading in after another category's replaced them.
    swap: Appear,
    /// The same for the third panel: a row's submenu shown after a rest on the row, how far
    /// out it is, its rows fading in after another's, the one last drawn and last shown
    /// (category, row).
    sub_intent: HoverIntent,
    widen_sub: Tween,
    sub_swap: Appear,
    sub_painted: Option<(usize, usize)>,
    sub_last_shown: Option<(usize, usize)>,
    /// The submenu last drawn (for the fade when it changes) and the last one shown (drawn
    /// while its panel folds back).
    painted: Option<usize>,
    last_shown: Option<usize>,
    area: Rect,
    scale: f32,
    /// The top bar's height, its menu button, the window's buttons in it.
    bar_h: f32,
    button: Rect,
    controls: Option<Rect>,
    under: Option<Under>,
    pointer: Option<Point>,
    /// The keyboard moved: its row is kept in view at the next paint.
    reveal: bool,
    /// From the last paint: the drawer's panels, the field, the lists' rooms and rows.
    panel: Rect,
    items_panel: Rect,
    field: Rect,
    menus_room: Rect,
    items_room: Rect,
    /// The third panel, its list's room, its header back (narrow), the window too narrow
    /// for three panels side by side.
    sub_panel: Rect,
    sub_room: Rect,
    sub_back: Rect,
    narrow: bool,
    menu_rows: Vec<(Rect, usize)>,
    item_rows: Vec<(Rect, usize)>,
    sub_rows: Vec<(Rect, usize)>,
    result_rows: Vec<(Rect, usize)>,
    menus_scroll: ScrollState,
    items_scroll: ScrollState,
    sub_scroll: ScrollState,
    menus_bar: EdgeScrollbar,
    items_bar: EdgeScrollbar,
    sub_bar: EdgeScrollbar,
    /// Its parts' state of use (a list's thumb held…).
    parts: ui_components::control::Controls,
}

impl MenuDrawer {
    /// Opens with `menus` (sliding in).
    pub fn new(menus: Vec<DrawerMenu>) -> Self {
        let mut drawer = Self {
            nav: DrawerNav::new(Vec::new()),
            query: SearchField::new(),
            filtered_by: String::new(),
            intent: HoverIntent::default(),
            open: false,
            shown: Tween::new(0.0, SLIDE),
            widen: Tween::new(0.0, SLIDE),
            swap: Appear::default(),
            sub_intent: HoverIntent::default(),
            widen_sub: Tween::new(0.0, SLIDE),
            sub_swap: Appear::default(),
            sub_painted: None,
            sub_last_shown: None,
            painted: None,
            last_shown: None,
            area: Rect::default(),
            scale: 1.0,
            bar_h: 0.0,
            button: Rect::default(),
            controls: None,
            under: None,
            pointer: None,
            reveal: false,
            panel: Rect::default(),
            items_panel: Rect::default(),
            field: Rect::default(),
            menus_room: Rect::default(),
            items_room: Rect::default(),
            sub_panel: Rect::default(),
            sub_room: Rect::default(),
            sub_back: Rect::default(),
            narrow: false,
            menu_rows: Vec::new(),
            item_rows: Vec::new(),
            sub_rows: Vec::new(),
            result_rows: Vec::new(),
            menus_scroll: ScrollState::default(),
            items_scroll: ScrollState::default(),
            sub_scroll: ScrollState::default(),
            menus_bar: EdgeScrollbar::default(),
            items_bar: EdgeScrollbar::default(),
            sub_bar: EdgeScrollbar::default(),
            parts: ui_components::control::Controls::default(),
        };
        drawer.reopen(menus);
        drawer
    }

    /// Opens again (also while it's sliding out), from the start: nothing chosen, no
    /// filter.
    pub fn reopen(&mut self, menus: Vec<DrawerMenu>) {
        self.nav = DrawerNav::new(menus);
        self.query.set_text("");
        self.filtered_by.clear();
        self.intent.reset();
        self.sub_intent.reset();
        self.under = None;
        self.reveal = false;
        self.menus_scroll.reset();
        self.items_scroll.reset();
        self.sub_scroll.reset();
        self.widen.set(0.0);
        self.widen_sub.set(0.0);
        self.painted = None;
        self.last_shown = None;
        self.sub_painted = None;
        self.sub_last_shown = None;
        self.open = true;
        self.shown.retarget(1.0);
    }

    /// Slides out.
    pub fn close(&mut self) {
        self.open = false;
        self.intent.reset();
        self.sub_intent.reset();
        self.parts = ui_components::control::Controls::default();
        self.shown.retarget(0.0);
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Closed and gone from the screen: nothing left to draw.
    pub fn is_gone(&self) -> bool {
        !self.open && !self.shown.animating()
    }

    /// Its transitions ended at once: in, its submenu out (tests, snapshots).
    pub fn settle(&mut self) {
        self.shown.set(if self.open { 1.0 } else { 0.0 });
        let out = self.open && self.nav.open_menu().is_some();
        self.widen.set(if out { 1.0 } else { 0.0 });
        let sub = self.open && self.nav.open_sub().is_some();
        self.widen_sub.set(if sub { 1.0 } else { 0.0 });
        self.swap = Appear::default();
        self.sub_swap = Appear::default();
    }

    pub fn is_animating(&self) -> bool {
        self.shown.animating()
            || self.widen.animating()
            || self.swap.animating()
            || self.widen_sub.animating()
            || self.sub_swap.animating()
    }

    /// When a category (or a row's submenu) the mouse rests on is due to be shown (one
    /// redraw then).
    pub fn deadline(&self) -> Option<Instant> {
        match (self.intent.deadline(), self.sub_intent.deadline()) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// Its room: the whole window, top to bottom (over the top bar too).
    pub fn set_area(&mut self, area: Rect, scale: f32) {
        self.area = area;
        self.scale = scale;
    }

    /// The top bar it covers: its height (the drawer's header row), where the bar's menu
    /// button is (drawn there again, as if it stayed put while the drawer slides under it)
    /// and the window's own buttons drawn in it (Windows: left clear of the dimming).
    pub fn set_bar(&mut self, height: f32, button: Rect, controls: Option<Rect>) {
        self.bar_h = height;
        self.button = button;
        self.controls = controls;
    }

    pub fn nav(&self) -> &DrawerNav {
        &self.nav
    }

    /// The rows that don't apply now (their ids), each with why: greyed out, never run.
    pub fn set_disabled(&mut self, disabled: Vec<(String, Option<String>)>) {
        self.nav.set_disabled(disabled);
    }

    pub fn has_query(&self) -> bool {
        !self.query.text().is_empty()
    }

    pub fn clear_query(&mut self) {
        self.query.set_text("");
        self.sync_query();
    }

    pub fn type_text(&mut self, typed: &str) {
        ui_components::control::Control::text(&mut self.query, typed);
        self.sync_query();
    }

    pub fn backspace(&mut self, word: bool) {
        self.query.input.backspace(word);
        self.sync_query();
    }

    pub fn delete(&mut self) {
        self.query.input.delete();
        self.sync_query();
    }

    /// The lists made again if the field's text changed (typed, pasted, cut, undone).
    fn sync_query(&mut self) {
        if self.query.text() != self.filtered_by {
            self.filtered_by = self.query.text().to_owned();
            self.nav.set_query(&self.filtered_by);
            self.menus_scroll.reset();
            self.reveal = true;
        }
    }

    /// A key for the lists ([`DrawerNav::key`]); the mouse's pending switch dropped.
    pub fn key(&mut self, key: NavKey) -> NavOutcome {
        self.sync_query();
        let outcome = self.nav.key(key);
        if outcome == NavOutcome::Used {
            self.intent.reset();
            self.sub_intent.reset();
            self.reveal = true;
        }
        outcome
    }
}

/// How far a row that doesn't apply is faded.
const DISABLED: f32 = 0.42;

#[cfg(test)]
#[path = "../tests/unit/menu_drawer.rs"]
mod tests;
