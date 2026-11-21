//! The log as a view: what it reacts to (clicks, keys, scrolling, the panes' separators).

use super::*;

use ui_components::control::{Control, Controls, Divider, ScrollThumb, Shown, Thumb};
use ui_components::date_range::{RangeAnswer, RangePart};

use super::parts::{BarField, DatePart, FilesSeam};
use crate::parts::{Tune, Tuned};

/// The places of its parts in [`LogView::with_controls`]: the Date picker (over everything
/// while open), the files' separator, the files' tree, the panels' scrollbars, the
/// separators between the panels, then the filter bar's field (never hit: it only takes the
/// keys while typed in).
const DATE: usize = 0;
const SPLIT: usize = 1;
const TREE: usize = 2;
const SEPARATORS: std::ops::Range<usize> = 6..8;
const FIELD: usize = 8;
/// The open filter list's scrollbar.
const LIST_BAR: usize = 9;

impl LogView {
    /// Its parts used as controls, the topmost first, with their state of use (see the
    /// places above). The separators between the panels take the pointer only where nothing
    /// of the view is (`free`); the files' separator, the tree and the scrollbars only while
    /// no filter's list is open over them (`listless`: a press then only closes it). Where a
    /// separator was dragged to, the panels follow. The open Date picker has the focus (the
    /// part that had it gets it back when it closes). The bar's field typed in takes the keys
    /// over the focus, the text typed going on to the files' search while it has it. The
    /// bar's presses and its lists keep their own way.
    fn with_controls<R>(
        &mut self,
        (free, listless): (bool, bool),
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let mut at = [None; 2];
        let mut split_at = None;
        let files = !self.chosen.is_empty() && self.hits.iter().any(|(_, h)| *h == Hit::Files);
        let open = self.bar.picker.is_some();
        // The focus as it will be once the Date picker has taken it or given it back (below):
        // the files' search has the text while their tree has it.
        let refocus = match self.controls.focused() {
            Some(DATE) if !open => Some(self.before_picker.take()),
            focused if open && focused != Some(DATE) => {
                self.before_picker = focused;
                Some(Some(DATE))
            }
            _ => None,
        };
        let tree_shown = listless && files;
        let tree_focused = refocus.unwrap_or(self.controls.focused()) == Some(TREE);
        let text_elsewhere = tree_shown && tree_focused && self.tree.searching;
        let answer = {
            // The Date picker closed, the files' separator not made yet: their places take
            // nothing.
            let mut date = self.bar.picker.as_mut().map(|picker| DatePart {
                part: RangePart { picker, answer: &mut self.date_answer },
            });
            let mut seam = self.files_split.as_mut().map(|splitter| FilesSeam {
                splitter,
                at: &mut split_at,
                reset: &mut self.split_reset,
            });
            let split_shown = listless && !self.chosen.is_empty();
            let mut split = Shown { part: &mut seam, shown: split_shown };
            let mut tree = ui_components::changed_files::TreePart {
                tree: &mut self.tree,
                event: &mut self.opened,
            };
            let tune = Tune { keeps_tab: true, ..Tune::default() };
            let mut tree = Tuned { part: &mut tree, tune };
            let mut tree = Shown { part: &mut tree, shown: tree_shown };
            let mut details =
                ScrollThumb { bar: &mut self.detail_bar, scroll: &mut self.detail_scroll };
            let mut details = Shown { part: &mut details, shown: listless };
            let mut commits = Thumb { bar: &mut self.list_bar, offset: &mut self.scroll };
            let mut commits = Shown { part: &mut commits, shown: listless };
            let mut branches =
                Thumb { bar: &mut self.branches_bar, offset: &mut self.branches_scroll };
            let mut branches = Shown { part: &mut branches, shown: listless };
            let [first, second] = &mut self.splitters;
            let [at_first, at_second] = &mut at;
            let mut left = Divider { splitter: first, at: at_first };
            let mut left = Shown { part: &mut left, shown: free && listless };
            let mut right = Divider { splitter: second, at: at_second };
            let mut right = Shown { part: &mut right, shown: free && listless };
            // The open list's search (the field's) and its scrollbar (a part of its own).
            let (query, list_bar) = match self.bar.popup.as_mut() {
                Some(super::filters::Popup { query, bar, scroll, .. }) => {
                    (Some(query), Some(Thumb { bar, offset: scroll }))
                }
                None => (None, None),
            };
            let mut list_bar = list_bar;
            let mut field = BarField {
                focus: &mut self.bar.focus,
                text: &mut self.bar.text,
                branches: &mut self.bar.branch_query,
                query,
                picker_open: open,
                text_elsewhere,
                answer: &mut self.field_answer,
            };
            let mut parts: [&mut dyn Control; 10] = [
                &mut date,
                &mut split,
                &mut tree,
                &mut details,
                &mut commits,
                &mut branches,
                &mut left,
                &mut right,
                &mut field,
                &mut list_bar,
            ];
            if let Some(to) = refocus {
                self.controls.focus(&mut parts, to);
            }
            f(&mut self.controls, &mut parts)
        };
        for (i, x) in at.into_iter().enumerate() {
            if let Some(x) = x {
                self.drag_separator(i, x);
            }
        }
        if let Some(y) = split_at {
            self.drag_files_split(y);
        }
        answer
    }

    /// The details' files have the keys.
    pub(super) fn files_focused(&self) -> bool {
        self.controls.focused() == Some(TREE)
    }

    /// The keys given to the details' files (a file's menu asked for).
    pub(super) fn focus_files(&mut self) {
        self.with_controls((true, true), |c, parts| c.focus(parts, Some(TREE)));
    }

    /// The keys taken from the details' files (a press elsewhere).
    fn unfocus_files(&mut self) {
        if self.files_focused() {
            self.with_controls((true, true), |c, parts| c.focus(parts, None));
        }
    }

    /// What the Date picker answered: a press outside it closes it (on another pill, that
    /// one's list opens).
    fn take_date_answer(&mut self, p: Point) {
        match self.date_answer.take() {
            Some(RangeAnswer::Chose(event)) => self.picker_event(event),
            Some(RangeAnswer::Outside) => {
                self.close_popup();
                if let Some(Hit::Pill(kind)) = self.hit(p)
                    && kind != filters::FilterKind::Date
                {
                    self.toggle_popup(kind);
                }
            }
            None => {}
        }
    }

    /// What the files' tree opened: the change beside.
    fn take_opened(&mut self) -> Option<ViewerAction> {
        match self.opened.take()? {
            crate::files_tree::TreeEvent::Open(i) => self.compare_file(i),
        }
    }

    /// Text typed or `pasted`: to the bar's field typed in (unless the files' search has
    /// it), else to the part with the focus; the Date picker open takes all of it. `true` if
    /// one took it.
    fn take_text(&mut self, text: &str, pasted: bool) -> bool {
        let took = self.with_controls((true, true), |c, parts| {
            if !pasted {
                return c.text(parts, text);
            }
            // A paste the field leaves (the files' search has the text) goes on to the part
            // with the focus, as typed text does.
            c.paste(parts, text).or_else(|| c.text(parts, text))
        });
        self.take_field_answer();
        took.is_some()
    }

    /// A key for the part taking the keys (the Date picker, the bar's field typed in, the
    /// files' tree): `true` if it took it.
    fn part_key(&mut self, key: ViewerKey, reach: ui_components::KeyReach) -> bool {
        // Nothing has the focus nor takes the keys (the open picker gets the focus as its
        // parts are made): Tab doesn't give it to a part either.
        if self.controls.focused().is_none()
            && self.bar.picker.is_none()
            && self.bar.focus.is_none()
        {
            return false;
        }
        let Some(part) = self.with_controls((true, true), |c, parts| c.key(parts, key, reach))
        else {
            return false;
        };
        match part {
            DATE => {
                if let Some(RangeAnswer::Chose(event)) = self.date_answer.take() {
                    self.picker_event(event);
                }
            }
            FIELD => self.take_field_answer(),
            _ => {
                if self.opened.is_some() {
                    self.pending = self.take_opened();
                }
            }
        }
        true
    }
}

impl Viewer for LogView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        if let Some(sizes) = data.downcast_ref::<workbench_model::LayoutSizes>() {
            self.take_sizes(sizes);
            return;
        }
        // Made again at a start (its project's repository found again; a file's history by
        // its path): read when first shown.
        if let Some(restored) = data.downcast_ref::<ui_components::Restored>() {
            let repo = restored.project.as_deref().and_then(git::GitRepository::discover);
            let Some(repo) = repo else {
                self.gone = true;
                return;
            };
            self.repo = Some(Arc::new(repo));
            self.queries.clone_from(&restored.queries);
            self.path.clone_from(&restored.memento);
            self.stale = true;
            // The Git window made again from the session: its other fixed tab too.
            self.pending = Some(super::missing_fixed_tabs());
            return;
        }
        let Some(data) = data.downcast_ref::<LogData>() else { return };
        // Told again what it was just made with (the Git window opening on it): its first
        // page is already being read.
        let same = self.repo.as_ref().is_some_and(|r| r.root() == data.repo.root())
            && self.path == data.path
            && data.commit.is_none()
            && data.reference.is_none();
        if same && self.rows.is_empty() && self.loading.is_some() {
            return;
        }
        self.repo = Some(Arc::clone(&data.repo));
        self.queries.clone_from(&data.queries);
        self.path.clone_from(&data.path);
        self.wanted.clone_from(&data.commit);
        self.filter = LogFilter::default();
        if let Some(reference) = &data.reference {
            self.filter.references = vec![reference.clone()];
        }
        self.bar = filters::FilterBar::default();
        self.refs = None;
        self.branch_states.clear();
        self.remotes.clear();
        // Another repository's maybe: its own shared references (taken at the next paint).
        self.shared = None;
        self.shared_taken = 0;
        self.branch_rows.clear();
        self.branch_selected = None;
        self.reload();
    }

    /// The whole log is a fixed tab of the Git window; a file's history closes.
    fn closable(&self) -> bool {
        self.path.is_some()
    }

    /// Made again at the next start, in its place (a file's history: by its path).
    fn restorable(&self) -> bool {
        true
    }

    /// The whole log: found by its own name whatever language its tab was named in.
    fn panel_key(&self) -> Option<&'static str> {
        self.path.is_none().then_some(LOG_KEY)
    }

    fn memento(&self) -> Option<String> {
        self.path.clone()
    }

    /// Made again where there's no repository any more: it goes.
    fn closed(&self) -> bool {
        self.gone
    }

    fn poll(&mut self) -> bool {
        let mut busy = self.take_refs()
            | self.poll_filters()
            | self.bar.lists_loading.is_some()
            | self.bar.paths_matching.is_some();
        if let Some(action) = self.opening.poll() {
            self.pending = Some(action);
            busy = true;
        }
        busy |= self.opening.is_busy();
        if let Some(rx) = &self.loading {
            match rx.try_recv() {
                Ok(page) => {
                    self.loading = None;
                    self.add_page(page);
                    self.find_wanted();
                    // Looking further for the commit asked for.
                    busy = self.loading.is_some();
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => busy = true,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.loading = None;
                    self.complete = true;
                }
            }
        }
        if let Some(rx) = &self.details_loading {
            match rx.try_recv() {
                Ok(chosen) => {
                    self.details_loading = None;
                    self.take_chosen(chosen);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => busy = true,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.details_loading = None,
            }
        }
        busy
    }

    /// Hidden a while: everything is let go (the commits, the graph, the references, the
    /// details) and read again when it's shown.
    fn purge(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.rows = Vec::new();
        self.graph = None;
        self.loading = None;
        self.clear_selection_state();
        self.refs = None;
        self.shared = None;
        self.shared_taken = 0;
        self.branch_rows = Vec::new();
        self.message = None;
        // The lists read for the filters go too (read again when opened).
        self.bar.lists = None;
        self.bar.popup = None;
        self.stale = true;
    }

    /// One of the filter bar's fields (the branches panel's, the files' search) has the
    /// keys.
    fn wants_text(&self) -> bool {
        self.bar.focus.is_some()
            || self.bar.picker.is_some()
            || (self.files_focused() && self.tree.searching)
    }

    /// To the part with the focus (the Date picker: its field, or Space picking the day
    /// the keys are on; the files' search), else to the filter bar's field with the keys.
    fn text_input(&mut self, typed: &str) -> bool {
        self.take_text(typed, false)
    }

    /// Its first line, as typed (to the part with the focus as pasted).
    fn paste(&mut self, text: &str) -> bool {
        self.take_text(text.lines().next().unwrap_or_default(), true)
    }

    /// What's selected in the field with the keys (read from `&self`: each its own).
    fn selected_text(&self) -> Option<String> {
        if let Some(picker) = &self.bar.picker {
            return picker.selected_text();
        }
        if self.files_focused() && self.tree.searching {
            return self.tree.selected_text();
        }
        self.field_selection()
    }

    /// The Date picker open has every key; then the filter bar's field typed in; the files'
    /// search with its reach (⌥ by word, ⇧ selecting).
    fn edit_key_with(&mut self, key: ViewerKey, reach: ui_components::KeyReach) -> bool {
        // The picker open (it has the focus as its parts are made).
        if self.bar.picker.is_some() {
            self.part_key(key, reach);
            return true;
        }
        if (self.bar.focus.is_some() || self.tree.searching) && self.part_key(key, reach) {
            return true;
        }
        self.edit_key(key)
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        if self.bar.picker.is_some() {
            return self.edit_key_with(key, ui_components::KeyReach::default());
        }
        // A field of the bar typed in: it has every key.
        if self.bar.focus.is_some() {
            return self.part_key(key, ui_components::KeyReach::default());
        }
        // A list open without a search (the order's): its keys, as its footer says.
        if let Some(popup) = &self.bar.popup {
            match key {
                ViewerKey::Escape => self.close_popup(),
                ViewerKey::Up => self.step_popup(-1),
                ViewerKey::Down => self.step_popup(1),
                ViewerKey::Enter => {
                    let row = popup.active.or_else(|| {
                        popup.rows.iter().position(|r| r.choice != super::filters::Choice::Header)
                    });
                    if let Some(i) = row {
                        self.choose_row(i);
                    }
                }
                _ => return false,
            }
            return true;
        }
        // The files' tree has the keys: its own (↵: the change beside).
        if self.part_key(key, ui_components::KeyReach::default()) {
            return true;
        }
        let last = self.rows.len().saturating_sub(1);
        let next = match (key, self.selected) {
            (ViewerKey::Down, Some(i)) => (i + 1).min(last),
            (ViewerKey::Up, Some(i)) => i.saturating_sub(1),
            (ViewerKey::Down | ViewerKey::Up, None) => 0,
            (ViewerKey::Home, _) => 0,
            (ViewerKey::End, _) => last,
            // Enter: the (first shown) file's changes, beside.
            (ViewerKey::Enter, Some(_)) => {
                let first = self.tree.selected_file().or((!self.files.is_empty()).then_some(0));
                self.pending = first.and_then(|i| self.compare_file(i));
                return self.pending.is_some();
            }
            (ViewerKey::Escape, Some(_)) => {
                self.clear_selection_state();
                return true;
            }
            _ => return false,
        };
        if self.rows.is_empty() {
            return false;
        }
        self.select(next);
        self.reveal(next);
        true
    }

    /// Its action: `reload` (the module's command); the rows of its menus.
    fn command(&mut self, id: &str) -> bool {
        if id == "reload" {
            self.reload();
            return true;
        }
        // The app's edit commands, for the part taking the keys (the picker's fields, the
        // bar's field typed in, the files' search).
        if self.bar.picker.is_some() {
            return self.with_controls((true, true), |c, parts| c.command(parts, id)).is_some();
        }
        let took = self.with_controls((true, true), |c, parts| c.command(parts, id));
        self.take_field_answer();
        if took.is_some() {
            return true;
        }
        // A filter's list opened (`filter:branch`…), the text filter set (`filter-text:…`).
        if let Some(text) = id.strip_prefix("filter-text:") {
            self.bar.text.set_text(text);
            self.apply_text();
            return true;
        }
        if let Some(name) = id.strip_prefix("filter:") {
            let kind = filters::FilterKind::ALL.into_iter().find(|k| k.name() == name);
            if let Some(kind) = kind {
                self.toggle_popup(kind);
            }
            return kind.is_some();
        }
        self.menu_command(id)
    }

    /// A right click: a commit's menu, or a file's (of the details).
    fn context_menu(&mut self, p: Point) -> Option<Vec<ui_components::ViewMenuEntry>> {
        self.menu_at(p)
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.pending.take()
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        // Over the open list: it scrolls.
        if let Some(popup) = self.bar.popup.as_mut().filter(|p| p.rect.contains(self.pointer)) {
            let before = popup.scroll;
            popup.scroll = (popup.scroll + dy).max(0.0);
            return popup.scroll != before;
        }
        // Over the files: they scroll (at their end, nothing under them does).
        let pointer = self.pointer;
        let (scrolled, over_files) = self.with_controls((true, true), |c, parts| {
            let over_files = parts[TREE].hit(pointer);
            (c.scroll(parts, pointer, dx, dy), over_files)
        });
        if scrolled.is_some() || over_files {
            return scrolled.is_some();
        }
        // Over the details: they scroll (never past their end).
        if self.details_panel.contains(self.pointer) {
            return self.detail_scroll.scroll_by(dy);
        }
        // Over the branches: they scroll.
        if self.branches.contains(self.pointer) {
            let before = self.branches_scroll;
            let max = (self.branches_content() - self.branch_list().size.height).max(0.0);
            self.branches_scroll = (self.branches_scroll + dy).clamp(0.0, max);
            return self.branches_scroll != before;
        }
        let before = self.scroll;
        self.scroll += dy;
        self.clamp_scroll();
        self.scroll != before
    }

    fn drag(&mut self, p: Point) -> bool {
        if self.drag_field(p) {
            return true;
        }
        // A separator held follows the pointer even where it can't go further.
        let dragged = self.with_controls((true, true), |c, parts| c.drag(parts, p));
        dragged.is_some_and(|(i, moved)| moved || SEPARATORS.contains(&i))
    }

    fn release(&mut self) -> bool {
        self.field_drag = false;
        let Some((i, released)) = self.with_controls((true, true), |c, parts| c.release(parts))
        else {
            return false;
        };
        // A separator let go: the widths and the files' share are kept.
        if released && (SEPARATORS.contains(&i) || i == SPLIT) {
            self.keep_widths();
        }
        released
    }

    fn is_animating(&self) -> bool {
        // Typing in the text filter: frames until it applies.
        self.controls.is_animating() || self.bar.typed_at.is_some()
    }

    /// Its scrollbars fading in or out: a frame when they next change.
    fn frame_deadline(&self) -> Option<std::time::Instant> {
        self.controls.next_frame()
    }

    fn is_loading(&self) -> bool {
        // The loader turns while something is read.
        self.loading.is_some() || self.details_loading.is_some() || self.bar.lists_loading.is_some()
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        self.press(p, 1)
    }

    fn press(&mut self, p: Point, clicks: u32) -> Option<ViewerAction> {
        self.press_with(p, clicks, ui_components::PressMods::default())
    }

    fn press_with(
        &mut self,
        p: Point,
        clicks: u32,
        mods: ui_components::PressMods,
    ) -> Option<ViewerAction> {
        // Its parts: the Date filter's picker (a press outside closes it; another pill: that
        // one opens), the separator between the details' files and the commit (over both),
        // the files' tree (a double click: the change beside; Tree / List kept), a panel's
        // scrollbar (its thumb taken, or the content jumped there), where nothing else is a
        // separator grabbed. An open filter's list keeps them from a press (it closes).
        let hit = self.hit(p);
        let gates = (hit.is_none(), self.bar.popup.is_none());
        let flat = self.tree.flat;
        let pressed = self.with_controls(gates, |c, parts| {
            let had = c.focused();
            let pressed = c.press(parts, p, clicks);
            if pressed.is_none() {
                c.focus(parts, had);
            }
            pressed
        });
        match pressed {
            // The open list's thumb: its search keeps the keys.
            Some((LIST_BAR, _)) => return None,
            Some((DATE, _)) => {
                self.take_date_answer(p);
                return None;
            }
            Some((SPLIT, _)) => {
                if std::mem::take(&mut self.split_reset) {
                    self.reset_files_split();
                }
                return None;
            }
            Some((TREE, _)) => {
                self.bar.focus = None;
                if self.tree.flat != flat
                    && let Some(sizes) = &self.sizes
                {
                    let mode = if self.tree.flat { 1.0 } else { 0.0 };
                    sizes.set(crate::files_tree::LIST_MODE, mode);
                }
                return self.take_opened();
            }
            Some(_) => {
                self.bar.focus = None;
                self.unfocus_files();
                return None;
            }
            None => {}
        }
        // Outside the open list: it closes (and the press does nothing else).
        if self.bar.popup.is_some()
            && !matches!(hit, Some(Hit::Popup | Hit::PopupQuery | Hit::PopupRow(_)))
        {
            let open = self.bar.popup.as_ref().and_then(|p| p.kind);
            self.close_popup();
            // Another pill: its list opens instead.
            if let Some(Hit::Pill(kind)) = hit
                && Some(kind) != open
            {
                self.toggle_popup(kind);
            }
            return None;
        }
        // A field keeps the keys only while it's pressed into.
        if !matches!(hit, Some(Hit::TextField | Hit::PopupQuery | Hit::BranchSearch)) {
            self.bar.focus = None;
        }
        if let Some(hit) = hit
            && self.press_filter(hit, p, clicks)
        {
            self.unfocus_files();
            return None;
        }
        // The files where their tree takes nothing: they keep the keys.
        if hit == Some(Hit::Files) {
            self.focus_files();
            return None;
        }
        self.unfocus_files();
        let hit = hit?;
        match hit {
            // A folder opens / closes; a branch is chosen, its log shown; a double click
            // switches to it.
            Hit::Branch(i) => {
                self.branch_selected = Some(i);
                if clicks >= 2 {
                    let menu = self.menu_ref(i);
                    let wanted = menu
                        .filter(|on| !on.current)
                        .and_then(|on| crate::switch::Wanted::of(&on.name, on.kind?));
                    return wanted.map(crate::ref_menu::switch_action);
                }
                self.choose_branch(i);
                None
            }
            Hit::BranchSync(_) => None,
            Hit::AllFiles => {
                self.all_files = !self.all_files;
                self.refresh_files();
                None
            }
            Hit::Row(i) => {
                self.select_with(i, mods);
                None
            }
            Hit::Command(id) => Some(ViewerAction::RunCommand(id.to_owned())),
            Hit::Refresh => {
                self.reload();
                None
            }
            Hit::CloseDetails => {
                self.clear_selection_state();
                None
            }
            _ => None,
        }
    }

    /// A branch's commits to push and to pull, over its arrows; the files' buttons.
    fn tooltip_at(&self, p: Point) -> Option<ui_components::Tip> {
        if let Some(tip) = self.controls.tooltip_at(p) {
            return Some(tip);
        }
        let (r, i) = self.hits.iter().rev().find_map(|(r, h)| match h {
            Hit::BranchSync(i) if r.contains(p) => Some((*r, *i)),
            _ => None,
        })?;
        let BranchItem::Ref { name, .. } = &self.branch_rows.get(i)?.item else { return None };
        let state = self.branch_states.iter().find(|s| s.branch == *name)?;
        let upstream = state.upstream.as_deref().unwrap_or_default();
        Some(ui_components::Tip::new(
            r,
            tr!(
                "git.branch_sync_tip",
                ahead = state.ahead,
                behind = state.behind,
                upstream = upstream
            ),
        ))
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        // The Date picker open (the arrow outside it), the separators (one dragged keeps its
        // cursor wherever the pointer goes: the drag notes it).
        if let Some(cursor) = self.controls.cursor_at(p) {
            return Some(cursor);
        }
        let hit = self.hit(p);
        // The search fields: the text's cursor, a hand on their ×.
        let field = match hit? {
            Hit::TextField => Some(&self.bar.text),
            Hit::BranchSearch => Some(&self.bar.branch_query),
            Hit::PopupQuery => self.bar.popup.as_ref().map(|popup| &popup.query),
            _ => None,
        };
        Some(field.and_then(|f| Control::cursor(f, p)).unwrap_or(ViewerCursor::Pointer))
    }

    fn hover(&mut self, p: Point) -> bool {
        self.pointer = p;
        let hover = self.hit(p);
        // A separator lights up only where nothing else is.
        let parts = self.with_controls((hover.is_none(), true), |c, parts| c.hover(parts, Some(p)));
        // The Date picker open has the mouse over everything.
        if self.controls.hovered() == Some(DATE) {
            return parts;
        }
        let fields = self.hover_search_fields(p);
        let changed = (hover != self.hover) | parts | fields;
        self.hover = hover;
        changed
    }

    fn status(&self) -> Option<String> {
        let count = self.rows.len();
        Some(if self.complete {
            tr!("git.commits", count = count)
        } else {
            tr!("git.commits_more", count = count)
        })
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        if ctx.waker.is_some() {
            self.waker.clone_from(&ctx.waker);
        }
        self.opening.set_waker(&ctx.waker);
        self.follow_refs();
        self.take_refs();
        if std::mem::take(&mut self.stale) {
            self.reload();
        }
        self.hits.clear();
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let p = ctx.palette;
        let px = |v: f32| ds.px(v);
        let b = ctx.bounds;
        let layer = ctx.scene.push_layer(Some(b));
        layer.quad(Quad::filled(b, p.editor_background).rounded(px(radius::CARD)));

        let toolbar_h = px(52.0);
        self.paint_toolbar(
            layer,
            ctx.text,
            ctx.icons,
            &ds,
            Rect::new(b.min_x(), b.min_y(), b.size.width, toolbar_h),
        );

        self.bounds = b;
        self.scale = ctx.scale;
        // The panels under the toolbar, each in its own column (the splitters resize them
        // whole): the whole log's have a head — the branches' filter over the branches, the
        // filter bar over the commits; the details have their own.
        let body_top = b.min_y() + toolbar_h + ds.line();
        let body_h = (b.max_y() - body_top).max(0.0);
        let width = |w: f32, default: f32| px(if w > 0.0 { w } else { default });
        // The list keeps at least 40% of the width: the panels give way (the branches first).
        // The details always show (a commit to choose when none is), unless it's too narrow.
        let mut details_w = if b.size.width >= px(520.0) {
            width(self.details_w, DETAILS_WIDTH).min(b.size.width * 0.4)
        } else {
            0.0
        };
        // Too narrow (the view beside a comparison): the branches step aside.
        let mut branches_w =
            if self.path.is_none() && !self.branch_rows.is_empty() && b.size.width >= px(640.0) {
                width(self.branches_w, BRANCHES_WIDTH).min(b.size.width * 0.3)
            } else {
                0.0
            };
        let room = b.size.width * 0.6;
        if branches_w + details_w > room {
            branches_w = branches_w.min((room - details_w).max(b.size.width * 0.18));
            details_w = details_w.min(room - branches_w);
        }
        // A seam between the panels, the separator's alone: grabbing it never takes a
        // panel's scrollbar (on its edge) nor a row.
        let seam = px(super::SEAM);
        let left_seam = if branches_w > 0.0 { seam } else { 0.0 };
        let right_seam = if details_w > 0.0 { seam } else { 0.0 };
        let list_x = b.min_x() + branches_w + left_seam;
        let list_w = (b.size.width - branches_w - details_w - left_seam - right_seam).max(0.0);
        // The heads take the filter bar's lines (two when the commits' column is narrow).
        let lines =
            if self.path.is_none() { self.filter_bar_lines(ctx.text, &ds, list_w) } else { 0 };
        let head_h =
            if lines > 0 { px(filter_bar::BAR_H) * f32::from(lines) + ds.line() } else { 0.0 };
        let rows_top = body_top + head_h;
        let rows_h = (b.max_y() - rows_top).max(0.0);
        self.branches = Rect::new(b.min_x(), rows_top, branches_w, rows_h);
        self.list = Rect::new(list_x, rows_top, list_w, rows_h);
        // The tints: the side panels whole, the commits' head; each up to the middle of its
        // seam, under the splitter (which then stands where the colors meet).
        let tint = filter_bar::panel_tint(p);
        let left_edge = if branches_w > 0.0 { list_x - seam / 2.0 } else { b.min_x() };
        let right_edge = if details_w > 0.0 { list_x + list_w + seam / 2.0 } else { b.max_x() };
        if branches_w > 0.0 {
            let side = Rect::new(b.min_x(), body_top, left_edge - b.min_x(), body_h);
            layer.quad(Quad::filled(side, tint));
        }
        if details_w > 0.0 {
            let side = Rect::new(right_edge, body_top, b.max_x() - right_edge, body_h);
            layer.quad(Quad::filled(side, tint));
        }
        if lines > 0 {
            let head = Rect::new(left_edge, body_top, right_edge - left_edge, head_h - ds.line());
            layer.quad(Quad::filled(head, tint));
            // One line under the heads, across the seams between them (the details' too when
            // its head is in their row).
            let end =
                if details_w > 0.0 && self.tree.head_in_row() { b.max_x() } else { right_edge };
            ds::divider_card(layer, &ds, b.min_x(), end, head.max_y());
        }
        if lines > 0 {
            let bar_h = head_h - ds.line();
            let bar = Rect::new(list_x, body_top, list_w, bar_h);
            self.paint_filter_bar(layer, ctx.text, ctx.icons, &ds, (bar, lines));
            if branches_w > 0.0 {
                let head = Rect::new(b.min_x(), body_top, branches_w, bar_h);
                self.paint_branches_head(layer, ctx.text, ctx.icons, &ds, head);
            }
        }
        let font = ds.ui_font(-0.5);
        self.row_h = (font.size * 2.1).round();
        if std::mem::take(&mut self.reveal_selected)
            && let Some(i) = self.selected
        {
            self.reveal(i);
        }
        self.clamp_scroll();
        if self.rows.is_empty() && (self.loading.is_some() || self.stale) {
            // Reading the first page: the loader, centered.
            let label = t("git.log_loading");
            let spinner = ds::Loader::with_text(&label);
            let w = spinner.width(ctx.text, &ds);
            let mid = self.list.min_y() + self.list.size.height / 2.0;
            let x = self.list.min_x() + (self.list.size.width - w) / 2.0;
            ds::loader(layer, ctx.text, &ds, x, mid, &spinner);
        } else if self.rows.is_empty() && !self.filter.is_empty() {
            // Filtered to nothing: what's filtered by.
            let summary = self.filters_summary();
            let title = t("git.log_nothing_found");
            ds::empty_state(layer, ctx.text, &ds, self.list, &title, Some(&summary));
        } else if self.rows.is_empty() {
            ds::empty_state(layer, ctx.text, &ds, self.list, &t("git.log_empty"), None);
        } else {
            let list_layer = ctx.scene.push_layer(Some(self.list));
            self.paint_rows(list_layer, ctx.text, &ds, &font);
            let content = self.list_content();
            self.list_bar.set_auto_hiding(true);
            self.list_bar.paint(ctx.scene, p, ctx.scale, self.list, content, self.scroll);
        }
        // The next page, before the list runs out.
        let last_visible = ((self.scroll + self.list.size.height) / self.row_h.max(1.0)) as usize;
        if last_visible + AHEAD >= self.rows.len() {
            self.load_more();
        }
        if branches_w > 0.0 {
            self.paint_branches(ctx.scene, ctx.text, ctx.icons, &ds);
            let content = self.branches_content();
            let area = self.branch_list();
            self.branches_bar.set_auto_hiding(true);
            self.branches_bar.paint(ctx.scene, p, ctx.scale, area, content, self.branches_scroll);
        }
        if details_w <= 0.0 {
            self.details_panel = Rect::default();
            self.detail_room = Rect::default();
        }
        if details_w > 0.0 {
            // The files' head in the filter bar's row, aligned with it (as the branches').
            self.tree.head_row = (lines > 0).then(|| ui_components::changed_files::HeadRow {
                row: filter_bar::BAR_H,
                total: head_h / ds.px(1.0),
            });
            let panel = Rect::new(self.list.max_x() + right_seam, body_top, details_w, body_h);
            self.paint_details(ctx.scene, ctx.text, ctx.icons, &ds, panel);
        }
        // The separators: each in its seam.
        let style = SplitterStyle::from_palette(p);
        let edges = [
            (branches_w > 0.0).then(|| self.branches.max_x()),
            (details_w > 0.0).then(|| self.list.max_x()),
        ];
        for (splitter, edge) in self.splitters.iter_mut().zip(edges) {
            let area = edge.map_or_else(Rect::default, |x| Rect::new(x, body_top, seam, body_h));
            splitter.set_style(style.clone());
            splitter.set_bounds(area, ctx.scale);
            if edge.is_some() {
                splitter.paint(ctx.scene);
            }
        }
        // The open filter's list (or the Date picker), over everything.
        if self.bar.popup.is_some() {
            self.paint_popup(ctx.scene, ctx.text, ctx.icons, &ds, b);
        }
        let anchor = self.bar.pills.iter().find(|(k, _)| *k == filters::FilterKind::Date);
        if let (Some((_, anchor)), Some(picker)) = (anchor.copied(), self.bar.picker.as_mut()) {
            picker.paint(ctx.scene, ctx.text, ctx.icons, &ds, anchor, b);
        }
        self.with_controls((true, true), |c, parts| c.settle(parts));
    }
}
