//! The screen as a view: what it reacts to, and how its parts are laid out.

use super::parts::{ListsAsked, ListsLane, MessagePart, PlacesLane};
use super::text_selection::CodeText;
use super::*;

/// The places of its parts in [`CommitView::with_controls`]: the dividers and the panes'
/// scrollbars come before the message.
const MESSAGE: usize = 7;
const CODE: usize = 8;
const PLACES: usize = 9;
const LISTS: usize = 10;

impl CommitView {
    /// Its parts used as controls, the topmost first, with their state of use: the
    /// dividers (each keeping its share in the project's sizes), the panes' scrollbars with
    /// their offsets, the message, the code's text; then the keyboard's lanes without a
    /// place of their own (the places' filter, the lists: their presses are the screen's,
    /// which gives them the focus). The message has the keys when no other lane does; the
    /// places' rows follow their filter.
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let sizes = self.sizes.as_ref();
        let [d0, d1, d2, d3] = &mut self.dividers.0;
        let [b0, b1, b2] = &mut self.bars;
        let [s0, s1, s2] = &mut self.scrolls;
        let (mut g0, mut g1) = (Grip { divider: d0, sizes }, Grip { divider: d1, sizes });
        let (mut g2, mut g3) = (Grip { divider: d2, sizes }, Grip { divider: d3, sizes });
        let mut t0 = Thumb { bar: b0, offset: s0 };
        let mut t1 = Thumb { bar: b1, offset: s1 };
        let mut t2 = Thumb { bar: b2, offset: s2 };
        let draft = self.repo.as_ref().map(|repo| repo.root());
        let mut message = MessagePart { field: &mut self.message, draft };
        let mut code = CodeText { code: &mut self.code, diff: self.diff.as_ref() };
        let mut edited = false;
        let filter = &mut self.places_filter;
        let mut places = PlacesLane { filter, edited: &mut edited, leave: &mut self.leave_places };
        let mut lists = ListsLane { asked: &mut self.lists_asked };
        let mut parts: [&mut dyn Control; 11] = [
            &mut g0,
            &mut g1,
            &mut g2,
            &mut g3,
            &mut t0,
            &mut t1,
            &mut t2,
            &mut message,
            &mut code,
            &mut places,
            &mut lists,
        ];
        if self.controls.focused().is_none() {
            self.controls.focus(&mut parts, Some(MESSAGE));
        }
        let answer = f(&mut self.controls, &mut parts);
        if edited {
            self.places_scroll = 0.0;
            self.lay_places();
        }
        answer
    }

    /// Where the keyboard goes: the lane with the focus.
    pub(super) fn keys(&self) -> Keys {
        match self.controls.focused() {
            Some(PLACES) => Keys::Places,
            Some(LISTS) => Keys::Lists,
            _ => Keys::Message,
        }
    }

    /// The keyboard given to `keys`' lane.
    pub(super) fn set_keys(&mut self, keys: Keys) {
        let lane = match keys {
            Keys::Message => MESSAGE,
            Keys::Lists => LISTS,
            Keys::Places => PLACES,
        };
        self.with_controls(|c, parts| c.focus(parts, Some(lane)));
    }

    /// A key the lists were asked (they have the keyboard): ↑ ↓ (pages, ends) through the
    /// files of both (⇧: choosing a run on the shown file's list), ↵ opens the shown one,
    /// Tab to the message.
    fn lists_key(&mut self, key: ViewerKey, reach: ui_components::KeyReach) {
        match key {
            // ⇧: the run chosen grows (or shrinks) along the shown file's list.
            ViewerKey::Up if reach.extend => self.extend(-1),
            ViewerKey::Down if reach.extend => self.extend(1),
            ViewerKey::Up => self.step(-1),
            ViewerKey::Down => self.step(1),
            ViewerKey::PageUp => self.step(-10),
            ViewerKey::PageDown => self.step(10),
            ViewerKey::Home => self.step(isize::MIN / 2),
            ViewerKey::End => self.step(isize::MAX / 2),
            ViewerKey::Enter => self.pending = self.open_shown(),
            ViewerKey::Tab => self.set_keys(Keys::Message),
            _ => {}
        }
    }

    /// Text typed or `pasted`, to the lane with the keys. In the lists, Space moves the
    /// shown file to the other list; anything else goes to the message (the keyboard with
    /// it). `true` if a lane took it.
    fn take_text(&mut self, text: &str, pasted: bool) -> bool {
        let give = |c: &mut Controls, parts: &mut [&mut dyn Control], text: &str| {
            if pasted { c.paste(parts, text) } else { c.text(parts, text) }
        };
        let answered = self.with_controls(|c, parts| give(c, parts, text));
        if answered == Some(LISTS)
            && let Some(ListsAsked::Text(typed)) = self.lists_asked.take()
        {
            if typed == " " {
                self.toggle_shown();
                return true;
            }
            self.set_keys(Keys::Message);
            return self.with_controls(|c, parts| give(c, parts, &typed)).is_some();
        }
        answered.is_some()
    }
}

impl Viewer for CommitView {
    /// Word wrap: the changes' lines go on in rows under them (no scroll sideways then).
    fn set_text_options(&mut self, options: &ui_components::TextOptions) {
        self.wrap = options.word_wrap;
    }

    fn set_data(&mut self, data: &dyn std::any::Any) {
        if let Some(sizes) = data.downcast_ref::<LayoutSizes>() {
            for divider in &mut self.dividers.0 {
                divider.take_sizes(sizes);
            }
            self.sizes = Some(sizes.clone());
            return;
        }
        // Made again at a start (the Git window's Commit tab): its project's repository found
        // again.
        if let Some(restored) = data.downcast_ref::<ui_components::Restored>() {
            let repo = restored.project.as_deref().and_then(git::GitRepository::discover);
            match repo {
                Some(repo) => {
                    let data =
                        CommitData { repo: Arc::new(repo), project: restored.id, docked: true };
                    self.set_data(&data);
                    // The Git window made again from the session: its other fixed tab too.
                    self.pending = Some(crate::log_view::missing_fixed_tabs());
                }
                None => self.gone = true,
            }
            return;
        }
        let Some(data) = data.downcast_ref::<CommitData>() else { return };
        self.repo = Some(Arc::clone(&data.repo));
        self.project = data.project;
        self.docked = data.docked;
        // Its sections at once; what's in them comes with the lists.
        self.lay_places();
        self.generation = self.changes_generation().unwrap_or(0);
        // The draft written before (not committed yet).
        let draft = drafts().get(data.repo.root()).cloned();
        if let Some(draft) = draft
            && self.message.text().is_empty()
        {
            self.message.set_text(&draft);
        }
        self.refresh();
    }

    /// Opened again in another group (split): the same repository.
    fn mirror_data(&self) -> Option<Box<dyn std::any::Any>> {
        let repo = Arc::clone(self.repo.as_ref()?);
        Some(Box::new(CommitData { repo, project: self.project, docked: false }))
    }

    /// Docked, a fixed tab of the Git window (made again at the next start); else a tab.
    fn closable(&self) -> bool {
        !self.docked
    }

    fn restorable(&self) -> bool {
        self.docked
    }

    fn panel_key(&self) -> Option<&'static str> {
        self.docked.then_some(KEY)
    }

    /// Made again where there's no repository any more: it goes.
    fn closed(&self) -> bool {
        self.gone
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.pending.take()
    }

    fn is_animating(&self) -> bool {
        self.controls.is_animating()
    }

    fn tooltip_at(&self, p: Point) -> Option<ui_components::Tip> {
        let (r, state) = self.badges.iter().rev().find(|(r, _)| r.contains(p))?;
        Some(ui_components::Tip::new(*r, crate::files_tree::state_name(*state)))
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        // A divider dragged keeps its cursor wherever the pointer goes (the parts answer
        // only where the mouse hovered).
        let divider = self.dividers.0.iter().find_map(|divider| divider.cursor(p));
        divider.or_else(|| {
            // A hunk's buttons are over the code: theirs first.
            let hit = self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h);
            if matches!(hit, Some(Hit::StageHunk(_) | Hit::DiscardHunk(_))) {
                return Some(ViewerCursor::Pointer);
            }
            if let Some(cursor) = self.controls.cursor_at(p) {
                return Some(cursor);
            }
            Some(match hit? {
                Hit::Message => ViewerCursor::Text,
                Hit::Commit if !self.can_commit() => ViewerCursor::Arrow,
                _ => ViewerCursor::Pointer,
            })
        })
    }

    fn context_menu(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        let hit = self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)?;
        let (i, staged) = match hit {
            Hit::Row(i, staged) | Hit::Check(i, staged) => (i, staged),
            // A branch's or a tag's: the same menu as the log's branches panel.
            Hit::Place(i) => {
                if self.stash_at(i).is_some() {
                    return Some(crate::stash_menu::rows(i));
                }
                let on = self.menu_ref(i)?;
                return Some(crate::ref_menu::rows(i, &on, &self.remotes));
            }
            _ => return None,
        };
        if !self.among_marked(i, staged) {
            self.select(i, staged);
        }
        self.file_menu(i, staged)
    }

    fn poll(&mut self) -> bool {
        let mut changed = false;
        let opened = self.opening.poll();
        if opened.is_some() {
            self.pending = opened;
            changed = true;
        }
        // The files changed (here or outside): the lists read again.
        if self.status_rx.is_none()
            && let Some(generation) = self.changes_generation()
            && generation != self.generation
        {
            self.generation = generation;
            self.refresh();
        }
        // The last commit's message (Amend): it takes the message's place.
        if let Some(rx) = &self.amend_rx
            && let Ok((message, pushed)) = rx.try_recv()
        {
            self.amend_rx = None;
            self.amend_pushed = pushed;
            if let Some(message) = message {
                self.message.set_text(&message);
            }
            changed = true;
        }
        changed |= self.take_places();
        if let Some(rx) = &self.status_rx
            && let Ok((branch, result)) = rx.try_recv()
        {
            self.status_rx = None;
            self.branch = branch;
            self.lay_places();
            match result {
                Ok(files) => {
                    self.files = files;
                    self.reselect();
                }
                Err(e) => self.outcome = Some((Self::vcs_error(&e), true)),
            }
            changed = true;
        }
        if let Some(rx) = &self.diff_rx
            && let Ok(diff) = rx.try_recv()
        {
            self.diff_rx = None;
            self.diff = Some(diff);
            // Places in what was shown: they mean nothing in the new one.
            self.lines.clear();
            self.line_anchor = None;
            self.code.clear();
            changed = true;
        }
        if let Some(rx) = &self.act_rx {
            match rx.try_recv() {
                Ok(done) => {
                    self.act_rx = None;
                    self.asking = false;
                    if let Err(e) = done {
                        self.outcome = Some((Self::vcs_error(&e), true));
                    }
                    self.refresh();
                    self.tell_status();
                    changed = true;
                }
                // Answered "no": nothing to read again.
                Err(TryRecvError::Disconnected) => {
                    self.act_rx = None;
                    self.asking = false;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        if let Some(rx) = &self.commit_rx
            && let Ok(result) = rx.try_recv()
        {
            self.commit_rx = None;
            self.outcome = Some(match result {
                Ok(id) => {
                    // Committed: its draft goes (and amending ends).
                    self.message.set_text("");
                    self.amend = false;
                    self.amend_pushed = false;
                    self.before_amend = None;
                    if let Some(repo) = &self.repo {
                        drafts().remove(repo.root());
                    }
                    (tr!("git.committed", id = id.chars().take(7).collect::<String>()), false)
                }
                Err(CommitError::NothingSelected) => (t("git.nothing_selected"), true),
                Err(CommitError::NoMessage) => (t("git.no_message"), true),
                Err(CommitError::Vcs(e)) => (Self::vcs_error(&e), true),
            });
            self.refresh();
            self.tell_status();
            changed = true;
        }
        changed
            || self.status_rx.is_some()
            || self.commit_rx.is_some()
            || self.amend_rx.is_some()
            || self.diff_rx.is_some()
            || self.opening.is_busy()
            || (self.act_rx.is_some() && !self.asking)
    }

    fn wants_text(&self) -> bool {
        true
    }

    /// The message takes several lines (a summary, a blank line, the details).
    fn field_takes_lines(&self) -> bool {
        self.keys() == Keys::Message
    }

    fn text_input(&mut self, text: &str) -> bool {
        self.take_text(text, false)
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        self.edit_key_with(key, ui_components::KeyReach::default())
    }

    /// To the lane with the keys ([`super::parts`]); Tab moves between the message and the
    /// lists (to the lists only when there are files), Esc or Tab from the places' filter
    /// back to the message. ⌘ / Ctrl+↵ commits (the view's `commit`).
    fn edit_key_with(&mut self, key: ViewerKey, reach: ui_components::KeyReach) -> bool {
        match self.with_controls(|c, parts| c.key(parts, key, reach)) {
            Some(MESSAGE) if key == ViewerKey::Tab => {
                if !self.files.is_empty() {
                    self.set_keys(Keys::Lists);
                }
            }
            Some(PLACES) if std::mem::take(&mut self.leave_places) => {
                self.set_keys(Keys::Message);
            }
            Some(LISTS) => {
                if let Some(ListsAsked::Key(key, reach)) = self.lists_asked.take() {
                    self.lists_key(key, reach);
                }
            }
            Some(_) => {}
            None => return false,
        }
        true
    }

    fn paste(&mut self, text: &str) -> bool {
        self.take_text(text, true)
    }

    /// Read from `&self`: the lane with the keys' own.
    fn selected_text(&self) -> Option<String> {
        if self.keys() == Keys::Places {
            let selected = self.places_filter.input.selected_text();
            return (!selected.is_empty()).then(|| selected.to_owned());
        }
        if let Some(code) = self.selected_code() {
            return Some(code);
        }
        let selected = self.message.selected_text();
        (!selected.is_empty()).then(|| selected.to_owned())
    }

    /// The app's edit commands, for the message.
    fn command(&mut self, id: &str) -> bool {
        // The places' filter with the keyboard: the edit commands are its, and nothing else
        // is taken meanwhile.
        if self.keys() == Keys::Places {
            return self.with_controls(|c, parts| c.command(parts, id)).is_some();
        }
        // The message with the keyboard: its edit commands.
        if self.with_controls(|c, parts| c.command(parts, id)).is_some() {
            return true;
        }
        if self.file_command(id) {
            return true;
        }
        // A row of a stash's menu, of a branch's.
        if let Some(action) = crate::stash_menu::chosen(id, |i| self.stash_at(i)) {
            self.pending = action;
            return true;
        }
        if let Some(action) = crate::ref_menu::chosen(id, |i| self.menu_ref(i), &self.remotes) {
            self.pending = action;
            return true;
        }
        match id {
            "select_all" if self.keys() == Keys::Lists => {
                self.mark_all();
                true
            }
            // ⌘ / Ctrl+↵ (the module's command).
            "commit" => self.commit(),
            // The lists with the keyboard: the message still takes the other edits.
            _ => {
                let draft = self.repo.as_ref().map(|repo| repo.root());
                MessagePart { field: &mut self.message, draft }.command(id)
            }
        }
    }

    fn pointer_down(
        &mut self,
        p: Point,
        clicks: u32,
        mods: ui_components::PressMods,
    ) -> Option<ViewerAction> {
        self.mods = mods;
        let action = self.press(p, clicks);
        self.mods = ui_components::PressMods::default();
        action
    }

    /// Docked in the Git window (a bottom window's view): the same press, its modifiers
    /// kept (⇧ / ⌘ choose several files there too).
    fn press_with(
        &mut self,
        p: Point,
        clicks: u32,
        mods: ui_components::PressMods,
    ) -> Option<ViewerAction> {
        self.pointer_down(p, clicks, mods)
    }

    fn press(&mut self, p: Point, clicks: u32) -> Option<ViewerAction> {
        // A divider (two clicks: back where it starts), a pane's scrollbar, the message (two
        // clicks: a word; three: its line), the code's text. A press on none of them keeps
        // the keyboard where it is (the screen's own parts move it: [`Self::click`]).
        let pressed = self.with_controls(|c, parts| {
            let had = c.focused();
            let pressed = c.press(parts, p, clicks);
            if pressed.is_none() {
                c.focus(parts, had);
            }
            pressed
        });
        match pressed {
            // A divider, a scrollbar, the code: the code's selection stays theirs.
            Some((i, _)) if i < MESSAGE || i == CODE => return None,
            // Anywhere else a selection of the code ends.
            Some(_) => {
                self.code.clear();
                return None;
            }
            None => self.code.clear(),
        }
        // A branch or a tag of the places, twice: switched to; a stash: brought back.
        if clicks == 2
            && let Some(Hit::Place(i)) =
                self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)
        {
            return self.place_twice(i);
        }
        if clicks == 2
            && let Some(open) = self.open_at(p)
        {
            return Some(open);
        }
        self.click(p)
    }

    fn drag(&mut self, p: Point) -> bool {
        self.with_controls(|c, parts| c.drag(parts, p)).is_some_and(|(_, moved)| moved)
    }

    fn release(&mut self) -> bool {
        self.with_controls(|c, parts| c.release(parts)).is_some_and(|(_, changed)| changed)
    }

    /// The panes' scrollbars and the message's fading: a frame when they next change.
    fn frame_deadline(&self) -> Option<std::time::Instant> {
        self.controls.next_frame()
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        let hit = self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)?;
        // A file or a list clicked: the keyboard goes through the lists; the places' filter
        // (a control: its presses are its own) keeps it only until something else is clicked.
        if matches!(hit, Hit::Check(..) | Hit::Row(..) | Hit::All(_)) {
            self.set_keys(Keys::Lists);
        } else if self.keys() == Keys::Places {
            self.set_keys(Keys::Message);
        }
        match hit {
            Hit::Check(i, staged) => self.toggle(i, staged),
            Hit::Row(i, staged) => self.pick(i, staged),
            Hit::All(staged) => self.toggle_all(staged),
            Hit::StageHunk(i) => self.toggle_hunk(i),
            Hit::Line(h, l) => self.pick_line(h, l),
            Hit::DiscardHunk(i) => return self.discard_hunk(i),
            Hit::Refresh => self.refresh(),
            Hit::Stash => {
                return Some(ViewerAction::WithHost(ui_components::WithHost::once(|host| {
                    crate::stash_dialog::open_push(
                        host,
                        crate::stash_dialog::StashPreset::default(),
                    );
                })));
            }
            Hit::Amend => self.set_amend(!self.amend),
            Hit::Commit => {
                self.commit();
            }
            Hit::Message => {
                self.message.press(p, 1, false);
            }
            Hit::Place(i) => return self.pick_place(i),
        }
        None
    }

    fn hover(&mut self, p: Point) -> bool {
        self.pointer = p;
        let hover = self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h);
        let changed = hover != self.hover;
        self.hover = hover;
        self.with_controls(|c, parts| c.hover(parts, Some(p))) | changed
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        // Over the places: they scroll.
        if self.docked && self.places_area.contains(self.pointer) {
            let before = self.places_scroll;
            self.places_scroll = (before + dy).max(0.0);
            return self.places_scroll != before;
        }
        // Over the message: its own lines scroll.
        let pointer = self.pointer;
        if self.with_controls(|c, parts| c.scroll(parts, pointer, dx, dy)).is_some() {
            return true;
        }
        let Some(pane) = self.panes.iter().position(|r| r.contains(self.pointer)) else {
            return false;
        };
        let (before, sideways) = (self.scrolls[pane], self.diff_x);
        self.scrolls[pane] = (before + dy).max(0.0);
        if pane == DIFF {
            self.diff_x = (sideways + dx).clamp(0.0, self.diff_overflow);
        }
        self.scrolls[pane] != before || self.diff_x != sideways
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        if ctx.waker.is_some() {
            self.waker.clone_from(&ctx.waker);
        }
        self.hits.clear();
        self.badges.clear();
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let p = ctx.palette;
        let px = |v: f32| ds.px(v);
        let b = ctx.bounds;
        // Rounded like the editor's card it fills (its bottom corners show).
        let card = Quad::filled(b, p.editor_background).rounded(px(radius::CARD));
        ctx.scene.push_layer(Some(b)).quad(card);
        self.opening.set_waker(&ctx.waker);
        let (style, strip) = (SplitterStyle::from_palette(ctx.palette), px(24.0));
        // Docked: the places on the left, the screen in the rest.
        let b = if self.docked {
            let inner = Rect::new(
                b.min_x() + px(8.0),
                b.min_y() + px(8.0),
                b.size.width - px(8.0),
                b.size.height - px(16.0),
            );
            let (side, rest) = self.dividers.0[3].split(inner, px(12.0), style.clone(), ctx.scale);
            self.paint_places(ctx.scene, ctx.text, ctx.icons, &ds, side);
            Rect::new(rest.min_x() - px(8.0), b.min_y(), rest.size.width + px(8.0), b.size.height)
        } else {
            self.dividers.0[3].hide();
            b
        };
        let layer = ctx.scene.push_layer(Some(b));
        let width = b.size.width - px(32.0);
        let x0 = b.min_x() + px(16.0);
        let top = b.min_y() + px(12.0);
        let page = Page { x0, width, small: ds.ui_font(-1.0) };
        self.paint_header(layer, ctx.text, ctx.icons, &ds, &page, top);
        let body_top = top + px(40.0);
        let bottom = b.max_y() - px(12.0);
        let area = Rect::new(x0, body_top, width, (bottom - body_top).max(0.0));
        let [lists, sections, footer, _] = &mut self.dividers.0;
        // Short: the message in a column beside the rest (the files and changes keep the
        // height); else under them, its share dragged.
        let (upper, message) = if area.size.height < px(COMPACT_H) {
            let column = px(MESSAGE_COLUMN).min(area.size.width * 0.4).round();
            let gap = px(space::LG);
            let upper = Rect::new(
                area.min_x(),
                area.min_y(),
                area.size.width - column - gap,
                area.size.height,
            );
            let message = Rect::new(upper.max_x() + gap, area.min_y(), column, area.size.height);
            // No divider between them (it stays where the tall layout had it otherwise).
            footer.hide();
            (upper, message)
        } else {
            footer.split(area, strip, style.clone(), ctx.scale)
        };
        let (files, changes) = lists.split(upper, strip, style.clone(), ctx.scale);
        let (staged, unstaged) = sections.split(files, strip, style, ctx.scale);
        for (card, on) in [(staged, true), (unstaged, false)] {
            self.paint_section(ctx.scene, ctx.text, ctx.icons, &ds, (card, on));
        }
        self.paint_diff(ctx.scene, ctx.text, ctx.icons, &ds, changes);
        self.paint_message(ctx.scene, ctx.text, &ds, message);
        for divider in &self.dividers.0 {
            divider.paint(ctx.scene);
        }
        self.with_controls(|c, parts| c.settle(parts));
        self.reveal = false;
    }
}
