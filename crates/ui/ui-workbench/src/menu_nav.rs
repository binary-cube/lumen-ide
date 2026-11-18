//! The menu drawer's state, without drawing: the app's menus (the same as the native menu
//! bar's), the category chosen and the submenu shown beside it, the filter that searches
//! every submenu at once (each result with its path, `Refactor › Rename`), the keyboard's
//! way through them, and the hover's intent (a diagonal move toward the open submenu
//! doesn't switch the category on the way, as in good menus).

use std::time::{Duration, Instant};

use kernel::{Point, Rect};
use ui_kit::ListFilter;

use crate::context_menu::MenuEntry;

/// A top-level menu as the drawer shows it: its title, its icon (a name from the app's
/// set) and its rows (items and separators).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawerMenu {
    pub title: String,
    pub icon: Option<&'static str>,
    pub entries: Vec<MenuEntry>,
}

/// An item found by the filter: its menu, its row there and, in a submenu, its row in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Found {
    pub menu: usize,
    pub entry: usize,
    pub sub: Option<usize>,
}

/// Where the keyboard is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavFocus {
    /// On the categories (one may be selected, its submenu shown beside).
    Menus,
    /// In the submenu shown.
    Items,
    /// In a submenu's submenu (the third panel).
    Sub,
    /// On the filter's results.
    Results,
}

/// A key the drawer's lists act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavKey {
    Up,
    Down,
    Left,
    Right,
    Enter,
}

/// What a key did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavOutcome {
    /// Used (the selection moved, a submenu was entered or left).
    Used,
    /// Not for the lists (the filter's field may take it: ← → move its caret).
    Unused,
    /// Run this command (the drawer closes).
    Run(String),
}

/// The drawer's lists and where the keyboard and the mouse are in them.
#[derive(Debug, Clone, Default)]
pub struct DrawerNav {
    menus: Vec<DrawerMenu>,
    /// The category selected; its submenu is the one shown.
    menu: Option<usize>,
    /// The row selected in the submenu shown (only while the keyboard is in it, or the
    /// mouse is over it).
    item: Option<usize>,
    /// The keyboard is in the submenu.
    in_items: bool,
    /// The submenu's row whose own submenu is open beside it (the third panel), the row
    /// selected there, and whether the keyboard is in it.
    sub: Option<usize>,
    sub_item: Option<usize>,
    in_sub: bool,
    /// What the filter found, in the menus' order (empty without a filter).
    results: Vec<Found>,
    result: Option<usize>,
    filtered: bool,
    /// The rows that don't apply now (their ids), each with why (`None`: no reason told):
    /// greyed out, never run.
    disabled: Vec<(String, Option<String>)>,
}

/// The rows of `entries` that can be selected (not their separators).
fn selectable(entries: &[MenuEntry]) -> Vec<usize> {
    entries
        .iter()
        .enumerate()
        .filter(|(_, e)| !matches!(e, MenuEntry::Separator))
        .map(|(i, _)| i)
        .collect()
}

/// The row after (or before, `delta` < 0) `at` among `rows`, around the ends; the first
/// (or the last) when none is selected.
fn step(rows: &[usize], at: Option<usize>, delta: isize) -> Option<usize> {
    if rows.is_empty() {
        return at;
    }
    let k = at.and_then(|i| rows.iter().position(|&r| r == i));
    Some(
        rows[match k {
            Some(k) => ui_kit::step_index(k, delta, rows.len()),
            None if delta < 0 => rows.len() - 1,
            None => 0,
        }],
    )
}

/// The id of `entry`, if it's an item (not a submenu's row, nor a separator).
fn command_of(entry: Option<&MenuEntry>) -> Option<String> {
    match entry? {
        MenuEntry::Item { id, .. } => Some(id.clone()),
        _ => None,
    }
}

impl DrawerNav {
    pub fn new(menus: Vec<DrawerMenu>) -> Self {
        Self { menus, ..Self::default() }
    }

    pub fn menus(&self) -> &[DrawerMenu] {
        &self.menus
    }

    /// The rows that don't apply now, with why.
    pub fn set_disabled(&mut self, disabled: Vec<(String, Option<String>)>) {
        self.disabled = disabled;
    }

    /// Row `entry` doesn't apply now: `Some(why)` (`why` may be `None`); a submenu's row
    /// when none of its rows does (with its first one's why).
    pub fn disabled(&self, entry: &MenuEntry) -> Option<Option<&str>> {
        match entry {
            MenuEntry::Item { id, .. } => {
                self.disabled.iter().find(|(d, _)| d == id).map(|(_, why)| why.as_deref())
            }
            MenuEntry::Submenu { entries, .. } => {
                let mut rows = entries.iter().filter(|e| !matches!(e, MenuEntry::Separator));
                let first = self.disabled(rows.next()?)?;
                rows.all(|e| self.disabled(e).is_some()).then_some(first)
            }
            MenuEntry::Separator => None,
        }
    }

    /// The row the keyboard is on doesn't apply now.
    pub fn selected_disabled(&self) -> bool {
        let entry = match self.focus() {
            NavFocus::Results => self.selected_result().and_then(|r| self.entry(self.results[r])),
            NavFocus::Sub => self.selected_sub_item().and_then(|j| self.sub_entries()?.1.get(j)),
            NavFocus::Items => {
                self.selected_item().and_then(|i| self.menus[self.menu?].entries.get(i))
            }
            NavFocus::Menus => None,
        };
        entry.is_some_and(|e| self.disabled(e).is_some())
    }

    /// The command of `entry`, if it's an item that applies now.
    fn runnable(&self, entry: Option<&MenuEntry>) -> Option<String> {
        let entry = entry?;
        if self.disabled(entry).is_some() {
            return None;
        }
        command_of(Some(entry))
    }

    /// The category whose submenu is shown (none while filtering).
    pub fn open_menu(&self) -> Option<usize> {
        if self.filtered { None } else { self.menu }
    }

    /// The row selected in the submenu shown.
    pub fn selected_item(&self) -> Option<usize> {
        self.open_menu().and(self.item)
    }

    /// The submenu's row whose submenu is open in the third panel.
    pub fn open_sub(&self) -> Option<usize> {
        self.open_menu().and(self.sub)
    }

    /// The open submenu's rows (the third panel's) and its title.
    pub fn sub_entries(&self) -> Option<(&str, &[MenuEntry])> {
        match self.menus.get(self.open_menu()?)?.entries.get(self.open_sub()?)? {
            MenuEntry::Submenu { label, entries, .. } => Some((label, entries)),
            _ => None,
        }
    }

    /// The row selected in the third panel.
    pub fn selected_sub_item(&self) -> Option<usize> {
        self.open_sub().and(self.sub_item)
    }

    pub fn selected_result(&self) -> Option<usize> {
        if self.filtered { self.result } else { None }
    }

    pub fn focus(&self) -> NavFocus {
        if self.filtered {
            NavFocus::Results
        } else if self.in_sub && self.sub.is_some() {
            NavFocus::Sub
        } else if self.in_items {
            NavFocus::Items
        } else {
            NavFocus::Menus
        }
    }

    pub fn filtering(&self) -> bool {
        self.filtered
    }

    pub fn results(&self) -> &[Found] {
        &self.results
    }

    /// The entry found (`Item` rows only).
    pub fn entry(&self, found: Found) -> Option<&MenuEntry> {
        let entry = self.menus.get(found.menu)?.entries.get(found.entry)?;
        match (entry, found.sub) {
            (MenuEntry::Submenu { entries, .. }, Some(sub)) => entries.get(sub),
            (_, None) => Some(entry),
            _ => None,
        }
    }

    /// A result's path (its menu's title, and its submenu's: `Git › Compare With`) and its
    /// label.
    pub fn path(&self, found: Found) -> Option<(String, String)> {
        let menu = self.menus.get(found.menu)?;
        let mut path = menu.title.clone();
        if found.sub.is_some()
            && let Some(MenuEntry::Submenu { label, .. }) = menu.entries.get(found.entry)
        {
            path = format!("{path} › {label}");
        }
        match self.entry(found)? {
            MenuEntry::Item { label, .. } => Some((path, label.clone())),
            _ => None,
        }
    }

    /// The filter typed: every submenu's items (their submenus' too) whose label, or path
    /// with it, contains it, regardless of case and diacritics ("folding": every row of
    /// Code › Folding); the first one selected. Empty: the categories.
    pub fn set_query(&mut self, typed: &str) {
        let filter = ListFilter::new(typed);
        self.filtered = !filter.is_empty();
        self.results.clear();
        self.result = None;
        if !self.filtered {
            return;
        }
        for (m, menu) in self.menus.iter().enumerate() {
            for (e, entry) in menu.entries.iter().enumerate() {
                match entry {
                    MenuEntry::Item { label, .. }
                        if filter.matches(label)
                            || filter.matches(&format!("{} {label}", menu.title)) =>
                    {
                        self.results.push(Found { menu: m, entry: e, sub: None });
                    }
                    MenuEntry::Submenu { label: title, entries, .. } => {
                        for (s, row) in entries.iter().enumerate() {
                            if let MenuEntry::Item { label, .. } = row
                                && (filter.matches(label)
                                    || filter.matches(&format!("{title} {label}"))
                                    || filter.matches(&format!("{} {title} {label}", menu.title)))
                            {
                                self.results.push(Found { menu: m, entry: e, sub: Some(s) });
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        self.result = (!self.results.is_empty()).then_some(0);
    }

    /// The rows of menu `m` that can be selected (not its separators).
    fn rows(&self, m: usize) -> Vec<usize> {
        self.menus.get(m).map_or_else(Vec::new, |menu| selectable(&menu.entries))
    }

    /// The command of the row selected where the keyboard is, if it's an item.
    fn chosen(&self) -> Option<String> {
        if self.filtered {
            return self.runnable(self.entry(self.results[self.result?]));
        }
        if self.focus() == NavFocus::Sub {
            return self.runnable(self.sub_entries()?.1.get(self.sub_item?));
        }
        self.runnable(self.menus.get(self.menu?)?.entries.get(self.item?))
    }

    /// Row `i` of the open category is a submenu's.
    fn is_submenu(&self, i: usize) -> bool {
        let entry = self.open_menu().and_then(|m| self.menus[m].entries.get(i));
        matches!(entry, Some(MenuEntry::Submenu { .. }))
    }

    /// The third panel closed (the keyboard back in the submenu, if it was there).
    fn close_sub(&mut self) {
        self.sub = None;
        self.sub_item = None;
        self.in_sub = false;
    }

    /// A key: ↑ ↓ through the categories, the submenu's items, the third panel's or the
    /// results (around the ends); → / ↵ into the selected category's submenu or a row's
    /// own submenu, ← back out one level; ↵ on an item runs it.
    pub fn key(&mut self, key: NavKey) -> NavOutcome {
        if self.filtered {
            return match key {
                NavKey::Up | NavKey::Down if !self.results.is_empty() => {
                    let delta = if key == NavKey::Up { -1 } else { 1 };
                    self.result = Some(match self.result {
                        Some(r) => ui_kit::step_index(r, delta, self.results.len()),
                        None => 0,
                    });
                    NavOutcome::Used
                }
                NavKey::Enter => self.chosen().map_or(NavOutcome::Used, NavOutcome::Run),
                NavKey::Up | NavKey::Down => NavOutcome::Used,
                NavKey::Left | NavKey::Right => NavOutcome::Unused,
            };
        }
        let delta = if key == NavKey::Up { -1 } else { 1 };
        match (key, self.focus()) {
            (NavKey::Up | NavKey::Down, NavFocus::Menus) => {
                let len = self.menus.len();
                if len > 0 {
                    self.menu = Some(match self.menu {
                        Some(m) => ui_kit::step_index(m, delta, len),
                        None if delta < 0 => len - 1,
                        None => 0,
                    });
                    self.item = None;
                    self.close_sub();
                }
                NavOutcome::Used
            }
            (NavKey::Right | NavKey::Enter, NavFocus::Menus) => {
                let Some(m) = self.menu else {
                    // Nothing chosen yet: the first category.
                    if key == NavKey::Enter && !self.menus.is_empty() {
                        self.menu = Some(0);
                    }
                    return NavOutcome::Used;
                };
                if let Some(&first) = self.rows(m).first() {
                    self.in_items = true;
                    self.item = Some(first);
                    self.close_sub();
                }
                NavOutcome::Used
            }
            (NavKey::Left, NavFocus::Menus) => NavOutcome::Used,
            (NavKey::Up | NavKey::Down, NavFocus::Items) => {
                let Some(m) = self.menu else { return NavOutcome::Used };
                self.item = step(&self.rows(m), self.item, delta);
                self.close_sub();
                NavOutcome::Used
            }
            (NavKey::Left, NavFocus::Items) => {
                self.in_items = false;
                self.item = None;
                self.close_sub();
                NavOutcome::Used
            }
            (NavKey::Right | NavKey::Enter, NavFocus::Items) => {
                match self.item {
                    // A submenu's row: into its submenu, on its first row.
                    Some(i) if self.is_submenu(i) => {
                        self.sub = Some(i);
                        self.in_sub = true;
                        self.sub_item =
                            self.sub_entries().and_then(|(_, e)| step(&selectable(e), None, 1));
                        NavOutcome::Used
                    }
                    _ if key == NavKey::Enter => {
                        self.chosen().map_or(NavOutcome::Used, NavOutcome::Run)
                    }
                    _ => NavOutcome::Used,
                }
            }
            (NavKey::Up | NavKey::Down, NavFocus::Sub) => {
                let rows = self.sub_entries().map_or_else(Vec::new, |(_, e)| selectable(e));
                self.sub_item = step(&rows, self.sub_item, delta);
                NavOutcome::Used
            }
            (NavKey::Left, NavFocus::Sub) => {
                self.close_sub();
                NavOutcome::Used
            }
            (NavKey::Enter, NavFocus::Sub) => {
                self.chosen().map_or(NavOutcome::Used, NavOutcome::Run)
            }
            (NavKey::Right, NavFocus::Sub) => NavOutcome::Used,
            (_, NavFocus::Results) => NavOutcome::Unused,
        }
    }

    /// The category `m` shown (the mouse rested on it, or clicked it): its submenu beside,
    /// the keyboard back on the categories. `true` if that changed.
    pub fn show_menu(&mut self, m: usize) -> bool {
        if m >= self.menus.len() || (self.menu == Some(m) && !self.in_items) {
            return false;
        }
        let other = self.menu != Some(m);
        let changed = other || self.in_items;
        self.menu = Some(m);
        if other {
            self.item = None;
            self.close_sub();
        }
        self.in_items = false;
        self.in_sub = false;
        changed
    }

    /// The mouse in the submenu shown, over its row `i` (`None`: between rows, on its title,
    /// on a separator): the keyboard follows it there — the category stays open, the row
    /// under the mouse selected (between rows, the last one stays). `true` if that changed.
    pub fn hover_item(&mut self, i: Option<usize>) -> bool {
        let Some(m) = self.open_menu() else { return false };
        let row = i.filter(|&i| {
            self.menus[m].entries.get(i).is_some_and(|e| !matches!(e, MenuEntry::Separator))
        });
        let item = row.or(self.item);
        let changed = item != self.item || !self.in_items || self.in_sub;
        self.item = item;
        self.in_items = true;
        self.in_sub = false;
        changed
    }

    /// Row `i`'s own submenu shown beside it (the mouse rested on it, or clicked it); a row
    /// without one closes the one shown. `true` if that changed.
    pub fn show_sub(&mut self, i: usize) -> bool {
        let sub = self.is_submenu(i).then_some(i);
        if sub == self.sub {
            return false;
        }
        self.sub = sub;
        self.sub_item = None;
        self.in_sub = false;
        true
    }

    /// The mouse in the third panel, over its row `j` (`None`: between rows): the keyboard
    /// follows it there, as in the submenu. `true` if that changed.
    pub fn hover_sub_item(&mut self, j: Option<usize>) -> bool {
        let Some((_, entries)) = self.sub_entries() else { return false };
        let row = j.filter(|&j| entries.get(j).is_some_and(|e| !matches!(e, MenuEntry::Separator)));
        let sub_item = row.or(self.sub_item);
        let changed = sub_item != self.sub_item || !self.in_sub || self.item != self.sub;
        self.sub_item = sub_item;
        self.item = self.sub;
        self.in_items = true;
        self.in_sub = true;
        changed
    }

    /// Back from the third panel (its header, in a narrow window). `true` if it was open.
    pub fn leave_sub(&mut self) -> bool {
        let open = self.sub.is_some();
        if open {
            self.item = self.sub;
            self.in_items = true;
            self.close_sub();
        }
        open
    }

    /// The mouse over result `r`: it's selected. `true` if that changed.
    pub fn hover_result(&mut self, r: usize) -> bool {
        if !self.filtered || r >= self.results.len() || self.result == Some(r) {
            return false;
        }
        self.result = Some(r);
        true
    }

    /// The command of the submenu's row `i` (a click), if it's an item.
    pub fn item_command(&self, i: usize) -> Option<String> {
        self.runnable(self.menus.get(self.open_menu()?)?.entries.get(i))
    }

    /// The command of the third panel's row `j` (a click), if it's an item.
    pub fn sub_item_command(&self, j: usize) -> Option<String> {
        self.runnable(self.sub_entries()?.1.get(j))
    }

    /// Row `i` of the submenu shown opens a submenu of its own.
    pub fn item_is_submenu(&self, i: usize) -> bool {
        self.is_submenu(i)
    }

    /// The command of result `r` (a click).
    pub fn result_command(&self, r: usize) -> Option<String> {
        self.runnable(self.entry(*self.results.get(r)?))
    }
}

/// How long the mouse must rest on another category before its submenu replaces the one
/// shown (a small delay: crossing the list doesn't flash every submenu).
pub const SWITCH_DELAY: Duration = Duration::from_millis(70);
/// The same while the mouse is heading for the open submenu (inside the intent triangle):
/// long enough to get there across the categories in between.
pub const AIM_DELAY: Duration = Duration::from_millis(300);

/// What the mouse's move asks of the submenu shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// Show this category's submenu now.
    Show(usize),
    /// Maybe later: ask again at this time ([`HoverIntent::due`]).
    Wait(Instant),
    /// Nothing changes.
    Keep,
}

/// The hover's intent over the categories: another category's submenu replaces the shown
/// one only after a short rest, and not while the mouse moves toward the shown submenu (a
/// diagonal across the categories in between) — the triangle from where it was to the
/// submenu's near edge.
#[derive(Debug, Clone, Copy, Default)]
pub struct HoverIntent {
    /// Where the mouse was at its last move.
    last: Option<Point>,
    /// The category waiting to be shown, and when.
    pending: Option<(usize, Instant)>,
}

impl HoverIntent {
    /// The mouse moved to `p`, over category `row` (`None`: not over one), category `open`
    /// shown in `submenu` (its panel's area).
    pub fn moved(
        &mut self,
        p: Point,
        row: Option<usize>,
        open: Option<usize>,
        submenu: Option<Rect>,
        now: Instant,
    ) -> Intent {
        let from = self.last.replace(p);
        let Some(row) = row else {
            self.pending = None;
            return Intent::Keep;
        };
        if Some(row) == open {
            self.pending = None;
            return Intent::Keep;
        }
        if open.is_none() {
            // Nothing shown yet: at once.
            self.pending = None;
            return Intent::Show(row);
        }
        // A submenu shown but not out yet (widening): no triangle, the short wait still.
        let aiming = from.zip(submenu).is_some_and(|(from, submenu)| aims_at(from, p, submenu));
        // Heading for the submenu: each move toward it puts the switch off (it comes once
        // the mouse stops on the way); otherwise a wait already running keeps its time.
        let delay = if aiming { AIM_DELAY } else { SWITCH_DELAY };
        let at = match self.pending {
            Some((r, at)) if r == row && !aiming => at.min(now + delay),
            _ => now + delay,
        };
        self.pending = Some((row, at));
        Intent::Wait(at)
    }

    /// The category whose time came, if any (taken).
    pub fn due(&mut self, now: Instant) -> Option<usize> {
        match self.pending {
            Some((row, at)) if at <= now => {
                self.pending = None;
                Some(row)
            }
            _ => None,
        }
    }

    /// When a waiting category is due (a redraw then).
    pub fn deadline(&self) -> Option<Instant> {
        self.pending.map(|(_, at)| at)
    }

    /// Forgotten (the drawer closed, the keyboard took over).
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// The move from `from` to `to` heads for `target` (a submenu on the right): `to` lies in
/// the triangle between `from` and the target's near (left) edge, a little widened so a
/// slightly wobbly hand still counts.
pub fn aims_at(from: Point, to: Point, target: Rect) -> bool {
    if to.x <= from.x || from.x >= target.min_x() {
        return false;
    }
    let slack = (target.size.height * 0.05).max(4.0);
    let a = Point::new(target.min_x(), target.min_y() - slack);
    let b = Point::new(target.min_x(), target.max_y() + slack);
    in_triangle(to, from, a, b)
}

/// `p` inside (or on an edge of) the triangle `a b c`.
fn in_triangle(p: Point, a: Point, b: Point, c: Point) -> bool {
    let cross =
        |o: Point, u: Point, v: Point| (u.x - o.x) * (v.y - o.y) - (u.y - o.y) * (v.x - o.x);
    let d1 = cross(a, b, p);
    let d2 = cross(b, c, p);
    let d3 = cross(c, a, p);
    let negative = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let positive = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(negative && positive)
}

#[cfg(test)]
#[path = "../tests/unit/menu_nav.rs"]
mod tests;
