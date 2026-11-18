//! The log as a view: what it reacts to (clicks, keys, scrolling, the panes' separators).

use super::*;

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
            || (self.files_focus && self.tree.searching)
    }

    fn text_input(&mut self, typed: &str) -> bool {
        // The picker: its field, or Space picking the day the keys are on.
        if let Some(picker) = &mut self.bar.picker {
            return picker.text_input(typed) || (typed == " " && picker.space());
        }
        if self.files_focus && self.tree.text_input(typed) {
            return true;
        }
        self.filter_text_input(typed)
    }

    fn paste(&mut self, text: &str) -> bool {
        let line = text.lines().next().unwrap_or_default();
        if let Some(picker) = &mut self.bar.picker {
            return picker.text_input(line);
        }
        if self.files_focus && self.tree.text_input(line) {
            return true;
        }
        self.filter_text_input(line)
    }

    /// What's selected in the field with the keys.
    fn selected_text(&self) -> Option<String> {
        if let Some(picker) = &self.bar.picker {
            return picker.selected_text();
        }
        if self.files_focus && self.tree.searching {
            return self.tree.selected_text();
        }
        self.field_selection()
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: ui_components::KeyReach) -> bool {
        if let Some(picker) = &mut self.bar.picker {
            if let Some(event) = picker.key(key, reach) {
                self.picker_event(event);
            }
            return true;
        }
        if self.filter_key(key, reach) {
            return true;
        }
        if self.files_focus
            && self.tree.searching
            && let Some(event) = self.tree.key_with(key, reach)
        {
            if let Some(crate::files_tree::TreeEvent::Open(i)) = event {
                self.pending = self.compare_file(i);
            }
            return true;
        }
        self.edit_key(key)
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        if self.bar.picker.is_some() {
            return self.edit_key_with(key, ui_components::KeyReach::default());
        }
        if self.bar.focus.is_some() {
            return self.filter_key(key, ui_components::KeyReach::default());
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
        if self.files_focus
            && let Some(event) = self.tree.key(key)
        {
            if let Some(crate::files_tree::TreeEvent::Open(i)) = event {
                self.pending = self.compare_file(i);
            }
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
        // The app's edit commands, for the field with the keys (the picker's, the files'
        // search too).
        if let Some(picker) = &mut self.bar.picker {
            return picker.command(id);
        }
        if self.field_command(id) {
            return true;
        }
        if self.files_focus && self.tree.command(id) {
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

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        // Over the open list: it scrolls.
        if let Some(popup) = self.bar.popup.as_mut().filter(|p| p.rect.contains(self.pointer)) {
            let before = popup.scroll;
            popup.scroll = (popup.scroll + dy).max(0.0);
            return popup.scroll != before;
        }
        // Over the files: they scroll.
        if self.tree.contains(self.pointer) && !self.chosen.is_empty() {
            return self.tree.scroll_by(dy);
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
        if self.tree.drag(p) {
            return true;
        }
        if self.detail_bar.is_dragging() {
            return self.detail_bar.drag(p).is_some_and(|o| self.detail_scroll.scroll_to(o));
        }
        if let Some(offset) = self.list_bar.drag(p) {
            self.scroll = offset;
            return true;
        }
        if let Some(offset) = self.branches_bar.drag(p) {
            self.branches_scroll = offset;
            return true;
        }
        if let Some(y) = self.files_split.as_ref().and_then(|s| s.drag_to(p)) {
            self.drag_files_split(y);
            return true;
        }
        let Some(i) = self.splitters.iter().position(Splitter::is_dragging) else { return false };
        if let Some(x) = self.splitters[i].drag_to(p) {
            self.drag_separator(i, x);
        }
        true
    }

    fn release(&mut self) -> bool {
        self.field_drag = false;
        if self.tree.release()
            | self.detail_bar.release()
            | self.list_bar.release()
            | self.branches_bar.release()
        {
            return true;
        }
        let released = self.splitters.iter_mut().fold(false, |any, s| s.end_drag() | any)
            | self.files_split.as_mut().is_some_and(Splitter::end_drag);
        if released {
            self.keep_widths();
        }
        released
    }

    fn is_animating(&self) -> bool {
        // Typing in the text filter: frames until it applies.
        self.splitters.iter().any(Splitter::is_animating)
            || self.files_split.as_ref().is_some_and(Splitter::is_animating)
            || self.bar.typed_at.is_some()
    }

    /// Its scrollbars fading in or out: a frame when they next change.
    fn frame_deadline(&self) -> Option<std::time::Instant> {
        [self.list_bar.next_frame(), self.branches_bar.next_frame(), self.tree.next_frame()]
            .into_iter()
            .flatten()
            .min()
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
        // The Date filter's picker: its parts; a press outside closes it (another pill: that
        // one opens).
        if let Some(picker) = &mut self.bar.picker {
            match picker.press(p) {
                Some(Some(event)) => self.picker_event(event),
                Some(None) => {}
                None => {
                    self.close_popup();
                    if let Some(Hit::Pill(kind)) = self.hit(p)
                        && kind != filters::FilterKind::Date
                    {
                        self.toggle_popup(kind);
                    }
                }
            }
            return None;
        }
        // The separator between the details' files and the commit (over both).
        if self.bar.popup.is_none()
            && !self.chosen.is_empty()
            && let Some(split) = self.files_split.as_mut().filter(|s| s.contains(p))
        {
            if clicks >= 2 {
                self.reset_files_split();
            } else {
                split.begin_drag(p);
            }
            return None;
        }
        let hit = self.hit(p);
        // Outside the open list: it closes (and the press does nothing else).
        if self.bar.popup.is_some()
            && !matches!(
                hit,
                Some(Hit::Popup | Hit::PopupQuery | Hit::ClearPopupQuery | Hit::PopupRow(_))
            )
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
        if !matches!(
            hit,
            Some(Hit::TextField | Hit::PopupQuery | Hit::ClearPopupQuery | Hit::BranchSearch)
        ) {
            self.bar.focus = None;
        }
        if let Some(hit) = hit
            && self.press_filter(hit, p, clicks)
        {
            self.files_focus = false;
            return None;
        }
        // The files: their tree (a double click: the change beside); Tree / List kept.
        if hit == Some(Hit::Files) {
            self.files_focus = true;
            let flat = self.tree.flat;
            let event = self.tree.press(p, clicks).flatten();
            if self.tree.flat != flat
                && let Some(sizes) = &self.sizes
            {
                sizes.set(crate::files_tree::LIST_MODE, if self.tree.flat { 1.0 } else { 0.0 });
            }
            return match event {
                Some(crate::files_tree::TreeEvent::Open(i)) => self.compare_file(i),
                None => None,
            };
        }
        self.files_focus = false;
        // The details' scrollbar: its thumb taken (or the content jumped there).
        if let Some(offset) = self.detail_bar.press(p) {
            self.detail_scroll.scroll_to(offset);
            return None;
        }
        // The commits' and the branches' bars (when shown).
        if let Some(offset) = self.list_bar.press(p) {
            self.scroll = offset;
            return None;
        }
        if let Some(offset) = self.branches_bar.press(p) {
            self.branches_scroll = offset;
            return None;
        }
        let Some(hit) = hit else {
            // On a separator: grabbed.
            if let Some(s) = self.splitters.iter_mut().find(|s| s.contains(p)) {
                s.begin_drag(p);
            }
            return None;
        };
        match hit {
            // A folder opens / closes; a branch is chosen, its log shown with a double click.
            Hit::Branch(i) => {
                let group = matches!(
                    self.branch_rows.get(i).map(|r| &r.item),
                    Some(BranchItem::Group { .. })
                );
                if group || clicks >= 2 {
                    self.choose_branch(i);
                }
                self.branch_selected = Some(i);
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
        if let Some(tip) = self.tree.tooltip_at(p) {
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
        if let Some(picker) = &self.bar.picker {
            return picker.rect().contains(p).then_some(if picker.over_field(p) {
                ViewerCursor::Text
            } else {
                ViewerCursor::Pointer
            });
        }
        if let Some(split) = &self.files_split
            && !self.chosen.is_empty()
            && (split.is_dragging() || split.contains(p))
        {
            return Some(ViewerCursor::ResizeVertical);
        }
        let dragging = self.splitters.iter().any(Splitter::is_dragging);
        let hit = self.hit(p);
        if dragging || (hit.is_none() && self.splitters.iter().any(|s| s.contains(p))) {
            return Some(ViewerCursor::ResizeHorizontal);
        }
        hit.map(|hit| match hit {
            Hit::TextField | Hit::PopupQuery | Hit::BranchSearch => ViewerCursor::Text,
            _ => ViewerCursor::Pointer,
        })
    }

    fn hover(&mut self, p: Point) -> bool {
        self.pointer = p;
        if let Some(picker) = &mut self.bar.picker {
            return picker.hover(p);
        }
        let hover = self.hit(p);
        let mut changed = (hover != self.hover)
            | self.detail_bar.hover(p)
            | self.tree.hover(p)
            | self.list_bar.hover(p)
            | self.branches_bar.hover(p);
        self.hover = hover;
        // A separator lights up only where nothing else is.
        let at = if hover.is_some() { Point::new(f32::MIN, f32::MIN) } else { p };
        for s in &mut self.splitters {
            changed |= s.hover(at);
        }
        if let Some(split) = &mut self.files_split {
            changed |= split.hover(if self.chosen.is_empty() {
                Point::new(f32::MIN, f32::MIN)
            } else {
                p
            });
        }
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
            let under = Rect::new(b.min_x(), head.max_y(), end - b.min_x(), ds.line());
            layer.quad(Quad::filled(under, p.card_border));
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
    }
}
