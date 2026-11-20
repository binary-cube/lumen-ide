//! The keyboard: where it is (the focus — a toolbar control, a card, the table, the detail;
//! Tab / ⇧Tab go through them in the order they're drawn, with a visible ring), the row the
//! table's keys are on (the cursor: a group's header, a part, one of its programs, an
//! operation) and what each key does there — ↑ / ↓, Page Up / Page Down, Home / End through
//! the rows; → opens a group or a part's programs, ← folds it or goes up to its group;
//! Enter opens the detail (a page on a narrow page) or what the control does; the popovers'
//! rows with ↑ / ↓ and Enter; Esc in its order.

use std::sync::Arc;

use kernel::Rect;
use ui_components::KeyReach;
use ui_components::ViewerKey;

use super::summary::cards;
use super::toolbar::groupings;
use super::{Card, Hit, MenuAction, Popover, TaskManager};
use crate::layout::{DetailPlace, Tool};
use crate::model::{GroupKey, Item, ViewModel};

/// What has the keyboard in the view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Focus {
    Tool(Tool),
    /// The toolbar's "…".
    More,
    Card(Card),
    /// The columns' chooser (the "⋯" at the header's end).
    Chooser,
    Table,
    Detail,
}

/// A row of the list, by what it shows (it survives the looks and the sorting): a group's
/// header, a part (its key), one of its programs (the part's key, the program's id), an
/// operation (its identity).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RowRef {
    Group(GroupKey),
    Part(String),
    Process(String, u32),
    Operation(u128),
}

/// Row `i` of `model`, by what it shows.
pub(super) fn row_ref(model: &ViewModel, i: usize) -> Option<RowRef> {
    Some(match model.items.get(i)? {
        Item::Group { key, .. } => RowRef::Group(*key),
        Item::Part { index, .. } => RowRef::Part(model.frame.lines[*index].key.clone()),
        Item::Process { part, process, .. } => {
            let line = &model.frame.lines[*part];
            RowRef::Process(line.key.clone(), line.processes[*process].pid)
        }
        Item::Operation(o) => RowRef::Operation(o.activity.id),
    })
}

/// Whether row `i` of `model` is `row`.
pub(super) fn is_row(model: &ViewModel, i: usize, row: &RowRef) -> bool {
    match (&model.items[i], row) {
        (Item::Group { key, .. }, RowRef::Group(k)) => key == k,
        (Item::Part { index, .. }, RowRef::Part(k)) => model.frame.lines[*index].key == *k,
        (Item::Process { part, process, .. }, RowRef::Process(k, pid)) => {
            let line = &model.frame.lines[*part];
            line.key == *k && line.processes[*process].pid == *pid
        }
        (Item::Operation(o), RowRef::Operation(id)) => o.activity.id == *id,
        _ => false,
    }
}

/// The part row `row` is of (a program's, an operation's).
fn part_of(model: &ViewModel, row: &RowRef) -> Option<String> {
    match row {
        RowRef::Group(_) => None,
        RowRef::Part(key) | RowRef::Process(key, _) => Some(key.clone()),
        RowRef::Operation(id) => {
            model.operations().find(|o| o.activity.id == *id).map(|o| o.owner.clone())
        }
    }
}

impl TaskManager {
    /// Where the table's keys are: the cursor's row, else the selected part's (`None`:
    /// neither is listed). Worked out once per model and change of either.
    pub(super) fn cursor_index(&mut self, model: &ViewModel) -> Option<usize> {
        let stamp = (model.stamp, self.cursor_epoch, self.selection_epoch);
        if let Some((at, found)) = self.cursor_cache
            && at == stamp
        {
            return found;
        }
        let rows = 0..model.items.len();
        let found = match &self.cursor {
            Some(row) => rows.clone().find(|&i| is_row(model, i, row)),
            None => None,
        }
        .or_else(|| {
            let key = self.selected.as_deref()?;
            rows.into_iter().find(|&i| match &model.items[i] {
                Item::Part { index, .. } => model.frame.lines[*index].key == key,
                _ => false,
            })
        });
        self.cursor_cache = Some((stamp, found));
        found
    }

    /// The table's keys moved to row `i`: its part selected (a program's part, an
    /// operation's) — the detail following where it's beside the list (on a page, Enter
    /// opens it) — and the row brought into view.
    pub(super) fn go_to(&mut self, i: usize) {
        let model = self.model();
        let Some(row) = row_ref(&model, i) else { return };
        if let Some(key) = part_of(&model, &row)
            && self.selected.as_deref() != Some(key.as_str())
        {
            self.last_selected = self.frame.line(&key).cloned().map(Arc::new);
            self.selected = Some(key);
            self.selection_epoch += 1;
        }
        self.detail_open = self.detail_open && self.place() != DetailPlace::Page;
        self.set_cursor(Some(row));
        self.reveal(i);
    }

    pub(super) fn set_cursor(&mut self, row: Option<RowRef>) {
        self.cursor = row;
        self.cursor_epoch += 1;
    }

    /// Row `i` scrolled into the list's view.
    fn reveal(&mut self, i: usize) {
        let Some(page) = self.layouts.last() else { return };
        let (row_h, h) = (page.row_height, page.rows.size.height);
        let top = i as f32 * row_h;
        if top < self.scroll {
            self.scroll = top;
        } else if top + row_h > self.scroll + h {
            self.scroll = (top + row_h - h).max(0.0);
        }
    }

    /// The rows a page holds (Page Up / Page Down move by one less).
    fn rows_per_page(&self) -> usize {
        self.layouts
            .last()
            .map_or(10, |p| (p.rows.size.height / p.row_height.max(1.0)).floor() as usize)
            .saturating_sub(1)
            .max(1)
    }

    /// A key moving through the rows: `true` if it was one.
    fn move_key(&mut self, key: ViewerKey) -> bool {
        let model = self.model();
        let count = model.items.len();
        if count == 0 {
            return matches!(
                key,
                ViewerKey::Up
                    | ViewerKey::Down
                    | ViewerKey::PageUp
                    | ViewerKey::PageDown
                    | ViewerKey::Home
                    | ViewerKey::End
            );
        }
        let at = self.cursor_index(&model);
        let page = self.rows_per_page();
        // From nowhere, ↓ goes to the first part (the busiest), ↑ to the last row.
        let first_part =
            || model.items.iter().position(|i| matches!(i, Item::Part { .. })).unwrap_or(0);
        let next = match (key, at) {
            (ViewerKey::Down, None) => first_part(),
            (ViewerKey::Down, Some(i)) => (i + 1).min(count - 1),
            (ViewerKey::Up, None) => count - 1,
            (ViewerKey::Up, Some(i)) => i.saturating_sub(1),
            (ViewerKey::PageDown, at) => at.map_or(0, |i| (i + page).min(count - 1)),
            (ViewerKey::PageUp, at) => at.map_or(0, |i| i.saturating_sub(page)),
            (ViewerKey::Home, _) => 0,
            (ViewerKey::End, _) => count - 1,
            _ => return false,
        };
        self.go_to(next);
        true
    }

    /// → on the table's row: a folded group opens, a part's programs are listed; already
    /// open, the first row in it.
    fn open_key(&mut self) -> bool {
        let model = self.model();
        let Some(i) = self.cursor_index(&model) else { return self.move_key(ViewerKey::Down) };
        match &model.items[i] {
            Item::Group { key, open: false, .. } => self.toggle_group(*key),
            Item::Part { index, .. }
                if !model.frame.lines[*index].processes.is_empty()
                    && !self.state.expanded.contains(&model.frame.lines[*index].key) =>
            {
                let key = model.frame.lines[*index].key.clone();
                self.toggle_expanded(&key);
            }
            Item::Group { .. } | Item::Part { .. }
                if matches!(
                    model.items.get(i + 1),
                    Some(Item::Part { .. } | Item::Process { .. } | Item::Operation(_))
                ) =>
            {
                self.go_to(i + 1);
            }
            _ => {}
        }
        true
    }

    /// ← on the table's row: an open group folds, a part's programs fold; else up to the
    /// row it's in (a program's part, a part's group).
    fn close_key(&mut self) -> bool {
        let model = self.model();
        let Some(i) = self.cursor_index(&model) else { return true };
        match &model.items[i] {
            Item::Group { key, open: true, .. } => self.toggle_group(*key),
            Item::Part { index, .. }
                if self.state.expanded.contains(&model.frame.lines[*index].key) =>
            {
                let key = model.frame.lines[*index].key.clone();
                self.toggle_expanded(&key);
            }
            Item::Group { .. } => {}
            Item::Part { .. } | Item::Process { .. } | Item::Operation(_) => {
                let process = matches!(model.items[i], Item::Process { .. });
                let up = (0..i).rev().find(|&j| match &model.items[j] {
                    Item::Part { .. } => process,
                    Item::Group { .. } => true,
                    _ => false,
                });
                if let Some(j) = up {
                    self.go_to(j);
                }
            }
        }
        true
    }

    /// Enter on the table: a group folds or opens; a row's part opens its detail (a page on
    /// a narrow page).
    fn enter_table(&mut self) {
        let model = self.model();
        let Some(i) = self.cursor_index(&model) else {
            self.move_key(ViewerKey::Down);
            return;
        };
        match &model.items[i] {
            Item::Group { key, .. } => self.toggle_group(*key),
            _ => {
                self.go_to(i);
                self.open_detail();
                if self.place() == DetailPlace::Page {
                    self.focus_on(Focus::Detail);
                }
            }
        }
    }

    /// Tab (`forward`) / ⇧Tab: the next / previous control in the order drawn.
    fn cycle(&mut self, forward: bool) {
        self.popover = None;
        if self.stops.is_empty() {
            return;
        }
        let n = self.stops.len();
        let at = self
            .stops
            .iter()
            .position(|(_, f)| *f == self.focus)
            .or_else(|| self.stops.iter().position(|(_, f)| *f == Focus::Table));
        let next = match at {
            Some(i) if forward => (i + 1) % n,
            Some(i) => (i + n - 1) % n,
            None => 0,
        };
        self.focus_on(self.stops[next].1);
        // A narrow page's filter, opened over the bar, closes once left (empty).
        if self.focus != Focus::Tool(Tool::Filter) && self.filter.text().is_empty() {
            self.filter_open = false;
        }
    }

    /// Enter on a control: what a click on it does.
    fn activate(&mut self) {
        match self.focus {
            Focus::Tool(Tool::Scope) => self.set_all(!self.all),
            Focus::Tool(Tool::Grouping) => self.step_grouping(true),
            Focus::Tool(Tool::Filter) => {
                self.focus_on(Focus::Table);
                self.filter_open = !self.filter.text().is_empty() && self.filter_open;
                let model = self.model();
                if self.cursor_index(&model).is_none() {
                    self.move_key(ViewerKey::Down);
                }
            }
            Focus::Tool(Tool::Pause) => self.set_paused(!self.paused),
            Focus::Tool(Tool::Interval) => self.open_popover(Popover::Interval),
            Focus::Tool(Tool::Sort) => self.open_popover(Popover::Sort),
            Focus::More => self.open_popover(Popover::More),
            Focus::Chooser => self.open_popover(Popover::Columns),
            Focus::Card(card) => {
                let column = card.column();
                if self.state.sort.0 != column {
                    self.sort_by(column);
                }
            }
            Focus::Table => self.enter_table(),
            Focus::Detail => {}
        }
    }

    /// The next (or previous) grouping offered.
    fn step_grouping(&mut self, forward: bool) {
        let offered = groupings(self.all);
        let now = self.state.grouping.on(&self.frame);
        let at = offered.iter().position(|g| *g == now).unwrap_or(0);
        let next = if forward { (at + 1) % offered.len() } else { at.saturating_sub(1) };
        self.set_grouping(offered[next]);
    }

    /// Popover `popover` opened under what opens it, its first row the keys' (`menu_at`).
    pub(super) fn open_popover(&mut self, popover: Popover) {
        let wanted = match popover {
            Popover::Columns => Focus::Chooser,
            Popover::More => Focus::More,
            Popover::Interval => Focus::Tool(Tool::Interval),
            Popover::Sort => Focus::Tool(Tool::Sort),
        };
        let anchor = self
            .hits
            .iter()
            .rev()
            .find(|(_, h)| *h == Hit::Open(popover))
            .map(|(r, _)| *r)
            .or_else(|| self.stops.iter().find(|(_, f)| *f == wanted).map(|(r, _)| *r))
            .unwrap_or_default();
        self.popover = Some((popover, anchor));
        self.refresh_menu();
        self.menu_at = self.menu.iter().position(|a| !matches!(a, MenuAction::Heading(_)));
    }

    /// The open popover's rows for the page as it was last laid out.
    fn refresh_menu(&mut self) {
        let (Some((popover, _)), Some(page)) = (self.popover, self.layouts.last()) else {
            return;
        };
        let page = page.clone();
        self.menu_rows(popover, &page);
    }

    /// A key in an open popover: ↑ / ↓ / Home / End through its rows, Enter runs one, Esc
    /// and Tab close it. `None`: not one of them.
    fn popover_key(&mut self, key: ViewerKey) -> Option<bool> {
        self.popover?;
        self.refresh_menu();
        let rows: Vec<usize> = (0..self.menu.len())
            .filter(|&i| !matches!(self.menu[i], MenuAction::Heading(_)))
            .collect();
        let at = self.menu_at.and_then(|m| rows.iter().position(|&r| r == m));
        let pick = |i: usize| rows.get(i).copied();
        match key {
            ViewerKey::Down => {
                self.menu_at = pick(at.map_or(0, |i| (i + 1).min(rows.len().saturating_sub(1))))
            }
            ViewerKey::Up => self.menu_at = pick(at.map_or(0, |i| i.saturating_sub(1))),
            ViewerKey::Home | ViewerKey::PageUp => self.menu_at = pick(0),
            ViewerKey::End | ViewerKey::PageDown => {
                self.menu_at = pick(rows.len().saturating_sub(1))
            }
            ViewerKey::Enter => {
                if let Some(action) = self.menu_at.and_then(|i| self.menu.get(i).copied()) {
                    self.run_menu(action);
                }
            }
            ViewerKey::Escape => self.popover = None,
            ViewerKey::Tab => return None,
            _ => return Some(false),
        }
        Some(true)
    }

    /// Esc: the popover closes, else the filter empties, else the detail closes, else the
    /// selection goes, else the keys go back to the table. `false`: nothing to undo.
    pub(super) fn escape(&mut self) -> bool {
        if self.popover.take().is_some() {
            return true;
        }
        if !self.filter.text().is_empty() {
            self.clear_filter();
            self.focus_table_from_filter();
            return true;
        }
        if self.filter_open {
            self.filter_open = false;
            self.focus_table_from_filter();
            return true;
        }
        if self.detail_open() {
            self.close_detail();
            if self.focus == Focus::Detail {
                self.focus_on(Focus::Table);
            }
            return true;
        }
        if self.selected.is_some() || self.cursor.is_some() {
            self.selected = None;
            self.selection_epoch += 1;
            self.set_cursor(None);
            return true;
        }
        if self.focus != Focus::Table {
            self.focus_on(Focus::Table);
            return true;
        }
        false
    }

    fn focus_table_from_filter(&mut self) {
        if self.focus == Focus::Tool(Tool::Filter) {
            self.focus_on(Focus::Table);
        }
    }

    /// The filter has the keys (⌘F; typing): on a narrow page it opens over the bar.
    pub(super) fn focus_filter(&mut self) {
        self.popover = None;
        self.focus_on(Focus::Tool(Tool::Filter));
        if self.layouts.last().is_some_and(|l| l.filter_icon) {
            self.filter_open = true;
        }
    }

    /// The detail scrolled by a key.
    fn detail_key(&mut self, key: ViewerKey) -> bool {
        let line = self.layouts.last().map_or(40.0, |l| l.row_height * 1.5);
        let page = self.layouts.last().and_then(|l| l.detail).map_or(300.0, |d| d.size.height);
        let to = match key {
            ViewerKey::Down => self.detail_scroll + line,
            ViewerKey::Up => self.detail_scroll - line,
            ViewerKey::PageDown => self.detail_scroll + page * 0.9,
            ViewerKey::PageUp => self.detail_scroll - page * 0.9,
            ViewerKey::Home => 0.0,
            ViewerKey::End => self.detail_max,
            _ => return false,
        };
        self.detail_scroll = to.clamp(0.0, self.detail_max);
        true
    }

    /// An editing or navigation key, and how far it reaches (⇧: Tab backwards, the filter's
    /// selection extended; ⌥ / Ctrl: by word). `true` if it was used.
    pub(super) fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        let navigation = !matches!(key, ViewerKey::Backspace | ViewerKey::Delete);
        if navigation {
            self.ring = true;
        }
        if let Some(used) = self.popover_key(key) {
            return used;
        }
        // The filter's own keys, while it has them (Tab is the ring's).
        if key != ViewerKey::Tab && self.part_key(key, reach) {
            return true;
        }
        let in_filter = self.focus == Focus::Tool(Tool::Filter);
        match key {
            ViewerKey::Tab => self.cycle(!reach.extend),
            ViewerKey::Escape => return self.escape(),
            ViewerKey::Backspace | ViewerKey::Delete => self.erase_in_filter(key, reach),
            ViewerKey::Left | ViewerKey::Right => {
                let right = key == ViewerKey::Right;
                match self.focus {
                    Focus::Table => {
                        return if right { self.open_key() } else { self.close_key() };
                    }
                    Focus::Tool(Tool::Scope) => self.set_all(right),
                    Focus::Tool(Tool::Grouping) => self.step_grouping(right),
                    Focus::Card(card) => self.step_card(card, right),
                    _ => return false,
                }
            }
            ViewerKey::Enter => self.activate(),
            ViewerKey::Up
            | ViewerKey::Down
            | ViewerKey::PageUp
            | ViewerKey::PageDown
            | ViewerKey::Home
            | ViewerKey::End => {
                if self.focus == Focus::Detail && self.detail_open() {
                    return self.detail_key(key);
                }
                // From the filter or a control, the rows: the table has the keys.
                if !in_filter {
                    self.focus_on(Focus::Table);
                }
                return self.move_key(key);
            }
        }
        true
    }

    /// ← / → on a card: the previous / next card has the keys.
    fn step_card(&mut self, card: Card, forward: bool) {
        let shown = cards(self.frame.gpu.is_some() || self.frame.lines.is_empty());
        let at = shown.iter().position(|c| *c == card).unwrap_or(0);
        let next = if forward { (at + 1).min(shown.len() - 1) } else { at.saturating_sub(1) };
        self.focus_on(Focus::Card(shown[next]));
    }

    /// What has the keyboard after a click on `hit` (`None`: unchanged).
    pub(super) fn focus_of(hit: Hit) -> Option<Focus> {
        Some(match hit {
            Hit::Scope(_) => Focus::Tool(Tool::Scope),
            Hit::Grouping(_) => Focus::Tool(Tool::Grouping),
            Hit::Filter | Hit::FilterIcon | Hit::FilterClear => Focus::Tool(Tool::Filter),
            Hit::Pause => Focus::Tool(Tool::Pause),
            Hit::Open(Popover::Interval) => Focus::Tool(Tool::Interval),
            Hit::Open(Popover::Sort) => Focus::Tool(Tool::Sort),
            Hit::Open(Popover::More) => Focus::More,
            Hit::Open(Popover::Columns) => Focus::Chooser,
            Hit::Card(c) | Hit::CardValue(c) | Hit::CardChart(c) => Focus::Card(c),
            Hit::Header(_)
            | Hit::Edge(_)
            | Hit::Group(_)
            | Hit::Row(_)
            | Hit::Expand(_)
            | Hit::ClearFilter
            | Hit::Resume
            | Hit::CloseDetail
            | Hit::Back => Focus::Table,
            Hit::DetailChart(_) | Hit::Action(_) => Focus::Detail,
            Hit::Menu(_) | Hit::PopoverArea => {
                return None;
            }
        })
    }

    /// The rectangle the focus ring goes around (`None`: none drawn — the cards draw their
    /// own).
    pub(super) fn ring_rect(&self) -> Option<Rect> {
        match self.focus {
            Focus::Card(_) => None,
            Focus::Table => self
                .cursor_rect
                .or_else(|| self.stops.iter().find(|(_, f)| *f == Focus::Table).map(|(r, _)| *r)),
            focus => self.stops.iter().find(|(_, f)| *f == focus).map(|(r, _)| *r),
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/view/keys.rs"]
mod tests;
