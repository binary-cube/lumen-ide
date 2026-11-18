//! The comparison as a view: what it reacts to (clicks, the separators dragged, keys, scrolling)
//! and saving the composed file.

use super::*;

impl Viewer for Compare {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        if let Some(sizes) = data.downcast_ref::<workbench_model::LayoutSizes>() {
            self.take_sizes(sizes);
            return;
        }
        if let Some(input) = data.downcast_ref::<MergeInput>() {
            self.take_merge(input);
            return;
        }
        let Some(input) = data.downcast_ref::<CompareInput>() else { return };
        self.resolving = None;
        let (left, lcrlf, ltrail) = split_lines(&input.left.1);
        let (right, rcrlf, rtrail) = split_lines(&input.right.1);
        self.left_path = input.left.0.clone();
        self.right_path = input.right.0.clone();
        self.labels = input.labels.clone();
        self.read_only = input.read_only;
        self.left = left;
        self.right = right;
        self.crlf = (lcrlf, rcrlf);
        self.trailing_newline = (ltrail, rtrail);
        self.scroll = 0.0;
        self.loaded = false;
        self.diff_in_background();
    }

    fn working(&self) -> bool {
        self.diffing.is_some()
    }

    fn poll(&mut self) -> bool {
        let Some(found) = self.diffing.as_ref().and_then(|rx| rx.try_recv().ok()) else {
            return false;
        };
        self.diffing = None;
        match found {
            (Some((side, lines)), found) => self.saved_diffed(side, lines, found),
            (None, found) => self.diffed(found),
        }
        true
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        self.bounds = ctx.bounds;
        self.scale = ctx.scale;
        // The thread wakes the app from now on; found already: taken now.
        if let Ok(mut slot) = self.waker.0.lock()
            && slot.is_none()
        {
            slot.clone_from(&ctx.waker);
        }
        self.poll();
        self.hits.clear();
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let p = ctx.palette;
        let b = ctx.bounds;
        let px = |v: f32| ds.px(v);
        let layer = ctx.scene.push_layer(Some(b));
        // Rounded like the editor's card it fills (its bottom corners show).
        layer.quad(Quad::filled(b, p.editor_background).rounded(px(ds::radius::CARD)));
        let toolbar_h = px(48.0);
        self.paint_toolbar(
            layer,
            ctx.text,
            &ds,
            Rect::new(b.min_x(), b.min_y(), b.size.width, toolbar_h),
        );
        let font = ds.code_font(-0.5);
        self.row_h = (font.size * 1.6).round();
        let overview_w = px(12.0);
        self.list = Rect::new(
            b.min_x(),
            b.min_y() + toolbar_h + ds.line(),
            b.size.width - overview_w,
            (b.size.height - toolbar_h - ds.line()).max(0.0),
        );
        self.overview =
            Rect::new(self.list.max_x(), self.list.min_y(), overview_w, self.list.size.height);
        // Opened: straight to the first difference (once the view's size is known).
        if std::mem::take(&mut self.reveal_first) {
            self.reveal_change(self.current);
        }
        self.clamp_scroll();
        if !self.loaded {
            if self.diffing.is_some() {
                let font = ds.ui_font(-0.5);
                let shaped = ctx.text.shape_line(&t("compare.comparing"), &font);
                let list = self.list;
                draw_text(
                    layer,
                    &shaped,
                    list.min_x() + px(20.0),
                    list.min_y() + px(32.0),
                    p.text_muted,
                );
            }
            return;
        }
        let columns = self.columns(&ds);
        let number_font = ds.code_font(-2.0);
        let number_w = ctx
            .text
            .shape_line(&format!("{}", self.left.len().max(self.right.len()) + 1), &number_font)
            .width
            + px(16.0);
        let grid = Grid {
            font,
            number_font,
            number_w,
            first: (self.scroll / self.row_h).floor() as usize,
            visible: (self.list.size.height / self.row_h).ceil() as usize + 1,
        };
        let rows = std::mem::take(&mut self.rows);
        // Columns, each in its own layer (clipped to its edges: long text does not spill into the
        // gutter), then the choices, then the separators over them.
        let writing = self.resolving.as_ref().is_some_and(|r| r.by_hand.is_some());
        let result = columns.mid.filter(|_| !writing);
        let each = [(0, Some(columns.left)), (1, result), (2, Some(columns.right))];
        // How far each column's code scrolls across: its longest line past its room.
        let advance = ctx.text.metrics(&grid.font).advance;
        self.advance = advance;
        let number_w = |which: usize| if which == 1 { px(12.0) } else { grid.number_w };
        for (which, col) in each {
            let across = &mut self.across[which];
            let Some(col) = col else {
                *across = Across::default();
                continue;
            };
            across.area = col;
            across.room = (col.size.width - number_w(which) - px(6.0)).max(0.0);
            let content = self.widest[which] as f32 * advance + px(24.0);
            across.max = (content - across.room).max(0.0);
            across.clamp();
        }
        for (which, col) in each {
            let Some(col) = col else { continue };
            // The tints and the numbers (fixed), then the code clipped after the numbers.
            let layer = ctx.scene.push_layer(Some(col));
            let pen = Pen { layer, text: ctx.text, ds: &ds };
            self.paint_column(pen, &grid, &rows, which, col, Pass::Back);
            let code = Rect::new(
                col.min_x() + number_w(which),
                col.min_y(),
                (col.size.width - number_w(which)).max(0.0),
                col.size.height,
            );
            self.code_x[which] = code.min_x();
            let layer = ctx.scene.push_layer(Some(code));
            // What's selected, under the code.
            self.paint_selection(layer, &ds, which, &grid, &rows);
            let pen = Pen { layer, text: ctx.text, ds: &ds };
            self.paint_column(pen, &grid, &rows, which, col, Pass::Code);
        }
        // A click on a difference's rows (any column) makes it the current one.
        for (ri, row) in rows.iter().enumerate().skip(grid.first).take(grid.visible) {
            if let Some(change) = row.change {
                let y = (self.list.min_y() + ri as f32 * self.row_h - self.scroll).round();
                let r = Rect::new(self.list.min_x(), y, self.list.size.width, self.row_h);
                // Only the visible part: a row under the toolbar must not take its clicks.
                if let Some(r) = r.intersect(&self.list) {
                    self.hits.push((r, Hit::Select(change)));
                }
            }
        }
        // "Compose": the current difference and the one under the mouse get their choices —
        // one pill in the result column (the separators stay plain splitters).
        if self.merge
            && !writing
            && let Some(mid) = columns.mid
        {
            let layer = ctx.scene.push_layer(Some(mid));
            self.paint_compose(Pen { layer, text: ctx.text, ds: &ds }, mid);
        }
        if let (Some(mid), focused) = (columns.mid, ctx.focused)
            && let Some(field) = self.by_hand()
        {
            let spec =
                ui_components::MultilineSpec { placeholder: "", focused, subject_limit: None };
            field.paint(ctx.scene, ctx.text, &ds, mid, spec);
        }
        // Over the columns and their choices: the handles reach into both sides.
        self.paint_splitters(ctx.scene, &columns, p, ctx.scale);
        self.rows = rows;
        let overview = ctx.scene.push_layer(Some(b));
        self.paint_overview(overview, &ds);
        // A column with lines longer than its room: its own bar across, along its bottom.
        // The whole track: the thumb widened on hover fits it (a thinner one cut its
        // rounded ends); at rest it sits by the edge as before.
        let thickness = ui_components::Scrollbar::track_thickness(ctx.scale);
        let style = ui_components::ScrollbarStyle::from_palette(p);
        for (which, across) in self.across.iter_mut().enumerate() {
            if across.max <= 0.0 {
                across.bar = None;
                continue;
            }
            let c = across.area;
            let x = c.min_x() + number_w(which);
            let track = Rect::new(x, c.max_y() - thickness, (c.max_x() - x).max(0.0), thickness);
            let metrics = across.metrics();
            let bar = across.bar.get_or_insert_with(|| {
                ui_components::Scrollbar::new(style.clone(), ui_components::Orientation::Horizontal)
            });
            bar.set_style(style.clone());
            bar.set_track(track, ctx.scale);
            // A thin line; hovered or held, only its thumb thickens (no wide rail).
            bar.set_expansion(0.0);
            bar.paint(ctx.scene, metrics);
        }
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        let pointer = self.pointer;
        if let Some(field) = self.by_hand().filter(|f| f.contains(pointer)) {
            return field.scroll_by(dy);
        }
        let before = self.scroll;
        self.scroll += dy;
        self.clamp_scroll();
        // Across: only the column under the mouse.
        let across = match self.column_at(pointer) {
            Some(i) => self.across[i].go(self.across[i].offset + dx),
            None => false,
        };
        self.scroll != before || across
    }

    fn drag(&mut self, p: Point) -> bool {
        if let Some(field) = self.by_hand()
            && field.drag(p)
        {
            return true;
        }
        if self.selecting {
            return self.drag_select(p);
        }
        if let Some(grab) = self.overview_grab {
            self.drag_overview(p.y, grab);
            return true;
        }
        if let Some(across) =
            self.across.iter_mut().find(|a| a.bar.as_ref().is_some_and(|b| b.is_dragging()))
        {
            let metrics = across.metrics();
            if let Some(offset) = across.bar.as_ref().and_then(|b| b.drag_to(p, metrics)) {
                across.go(offset);
            }
            return true;
        }
        let Some(i) = self.splitters.iter().position(Splitter::is_dragging) else { return false };
        if let Some(x) = self.splitters[i].drag_to(p) {
            self.drag_separator(i, x);
        }
        true
    }

    fn release(&mut self) -> bool {
        if let Some(field) = self.by_hand()
            && field.release()
        {
            return true;
        }
        if std::mem::take(&mut self.selecting) {
            return true;
        }
        if self.overview_grab.take().is_some() {
            return true;
        }
        let ended = self.across.iter_mut().fold(false, |any, a| {
            a.bar.as_mut().is_some_and(ui_components::Scrollbar::end_drag) | any
        });
        if ended {
            return true;
        }
        let released = self.splitters.iter_mut().fold(false, |any, s| s.end_drag() | any);
        if released {
            self.keep_splits();
        }
        released
    }

    fn is_animating(&self) -> bool {
        self.splitters.iter().any(Splitter::is_animating)
    }

    /// A press: `clicks` in a row select a word (two) or a line (three) in the code.
    fn pointer_down(
        &mut self,
        p: Point,
        clicks: u32,
        _mods: ui_components::PressMods,
    ) -> Option<ViewerAction> {
        self.press_clicks = clicks;
        let action = self.click(p);
        self.press_clicks = 1;
        action
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        if let Some(field) = self.by_hand().filter(|f| f.contains(p)) {
            field.press(p, 1, false);
            return None;
        }
        // A column's bar across: its code scrolled there (and dragged from there).
        if let Some(across) =
            self.across.iter_mut().find(|a| a.bar.as_ref().is_some_and(|b| b.contains(p)))
        {
            let metrics = across.metrics();
            if let Some(offset) = across.bar.as_mut().and_then(|b| b.begin_drag(p, metrics)) {
                across.go(offset);
            }
            return None;
        }
        // The buttons on a separator win; elsewhere on it, it's grabbed (a difference's rows
        // reach under it too: the separator is above them).
        let hit = self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h);
        if matches!(hit, None | Some(Hit::Select(_)))
            && let Some(s) = self.splitters.iter_mut().find(|s| s.contains(p))
        {
            s.begin_drag(p);
            return None;
        }
        let Some(hit) = hit else {
            // On the code: the selection starts (elsewhere, what was selected goes).
            match self.code_at(p) {
                Some(which) => self.begin_select(which, p, self.press_clicks.max(1)),
                None => self.selection = None,
            }
            return None;
        };
        match hit {
            Hit::Mode(merge) => {
                self.merge = merge || self.resolving.is_some();
                self.rebuild_rows();
            }
            Hit::Target(side) => {
                // The unmodified result follows the file being written to.
                let untouched = self.picks.iter().all(|&p| p == self.default_pick());
                self.target = Some(side);
                if untouched {
                    self.picks = vec![self.default_pick(); self.changes()];
                }
                self.rebuild_rows();
            }
            Hit::ByHand => self.toggle_by_hand(),
            Hit::AllLeft => self.pick_all(Pick::Left),
            Hit::AllRight => self.pick_all(Pick::Right),
            Hit::Prev => self.reveal_change(self.current.saturating_sub(1)),
            Hit::Next => {
                self.reveal_change((self.current + 1).min(self.changes().saturating_sub(1)))
            }
            Hit::Pick(change, pick) => {
                // Both / neither can toggle back to the file being written to.
                let current = self.picks.get(change).copied();
                let pick = if current == Some(pick) && matches!(pick, Pick::Both | Pick::None) {
                    self.initial_pick(change)
                } else {
                    pick
                };
                self.set_pick(change, pick);
            }
            // A difference's rows: it becomes the current one, and the selection starts.
            Hit::Select(change) => {
                self.current = change;
                match self.code_at(p) {
                    Some(which) => self.begin_select(which, p, self.press_clicks.max(1)),
                    None => self.selection = None,
                }
            }
            Hit::Save => {
                let _ = self.save();
                return self.pending.take();
            }
            // On the thumb: taken where it was pressed; elsewhere: the thumb centred there,
            // and held by its middle (dragged on without letting go).
            Hit::Overview => {
                let thumb = self.overview_thumb();
                let grab = if thumb.contains(p) {
                    p.y - thumb.min_y()
                } else {
                    let grab = thumb.size.height / 2.0;
                    self.drag_overview(p.y, grab);
                    grab
                };
                self.overview_grab = Some(grab);
            }
        }
        None
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        let on_button = self.hits.iter().any(|(r, _)| r.contains(p));
        let dragging = self.splitters.iter().any(Splitter::is_dragging);
        let selectable = matches!(
            self.hits.iter().rev().find(|(r, _)| r.contains(p)),
            None | Some((_, Hit::Select(_)))
        );
        if dragging || (!on_button && self.splitters.iter().any(|s| s.contains(p))) {
            Some(ViewerCursor::ResizeHorizontal)
        } else if selectable && self.code_at(p).is_some() {
            Some(ViewerCursor::Text)
        } else {
            on_button.then_some(ViewerCursor::Pointer)
        }
    }

    fn hover(&mut self, p: Point) -> bool {
        self.pointer = p;
        let bar = self
            .across
            .iter_mut()
            .fold(false, |any, a| a.bar.as_mut().is_some_and(|b| b.hover(p)) | any);
        let before = self.hover;
        self.hover = self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h);
        // A separator lights up only where no button is.
        let on_button = self.hover.is_some();
        let mut changed = bar || before != self.hover;
        for s in &mut self.splitters {
            let at = if on_button { Point::new(f32::MIN, f32::MIN) } else { p };
            changed |= s.hover(at);
        }
        changed
    }

    fn wants_text(&self) -> bool {
        self.resolving.as_ref().is_some_and(|r| r.by_hand.is_some())
    }

    fn text_input(&mut self, text: &str) -> bool {
        self.by_hand().map(|f| f.insert(text)).is_some()
    }

    fn paste(&mut self, text: &str) -> bool {
        self.text_input(text)
    }

    /// What's selected: in the field written by hand, else in a column.
    fn selected_text(&self) -> Option<String> {
        if let Some(field) = self.resolving.as_ref().and_then(|r| r.by_hand.as_ref()) {
            return Some(field.selected_text().to_owned()).filter(|s| !s.is_empty());
        }
        self.selection_text()
    }

    /// ⌘A: the column selected last (else the one under the mouse, else the right one).
    fn select_all(&mut self) -> bool {
        if !self.loaded || self.by_hand().is_some() {
            return false;
        }
        let which =
            self.selection.map(|s| s.which).or_else(|| self.code_at(self.pointer)).unwrap_or(2);
        self.select_column(which);
        true
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: ui_components::KeyReach) -> bool {
        let (word, extend) = (reach.word, reach.extend);
        let Some(field) = self.by_hand() else { return self.edit_key(key) };
        match key {
            ViewerKey::Backspace => field.backspace(word),
            ViewerKey::Delete => field.delete(),
            ViewerKey::Left => field.left(word, extend),
            ViewerKey::Right => field.right(word, extend),
            ViewerKey::Up => field.vertical(false, extend),
            ViewerKey::Down => field.vertical(true, extend),
            ViewerKey::Home => field.home(word, extend),
            ViewerKey::End => field.end(word, extend),
            ViewerKey::Enter => field.newline(),
            _ => return false,
        }
        true
    }

    /// Its actions ([`ACTIONS`]).
    fn command(&mut self, id: &str) -> bool {
        if let Some(field) = self.by_hand() {
            match id {
                "select_all" => field.select_all(),
                "cut" => field.insert(""),
                "undo" => {
                    field.undo();
                }
                "redo" => {
                    field.redo();
                }
                _ => return false,
            }
            return true;
        }
        match id {
            "select_all" => return self.select_all(),
            "next" => self.reveal_change((self.current + 1).min(self.changes().saturating_sub(1))),
            "previous" => self.reveal_change(self.current.saturating_sub(1)),
            "take_left" if self.merge => self.set_pick(self.current, Pick::Left),
            "take_right" if self.merge => self.set_pick(self.current, Pick::Right),
            "merge" if self.writable() && self.resolving.is_none() => {
                self.merge = !self.merge;
                self.rebuild_rows();
            }
            _ => return false,
        }
        true
    }

    /// The arrows go from change to change (navigation, not remapped).
    fn edit_key(&mut self, key: ViewerKey) -> bool {
        match key {
            // Esc: what's selected goes first.
            ViewerKey::Escape if self.selection.is_some() => {
                self.selection = None;
                true
            }
            ViewerKey::Down => self.command("next"),
            ViewerKey::Up => self.command("previous"),
            _ => false,
        }
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.pending.take()
    }

    fn is_dirty(&self) -> bool {
        match &self.resolving {
            // Not written yet: closing it leaves the conflict as it was.
            Some(resolving) => {
                self.loaded && !resolving.written && resolving.chosen.contains(&true)
            }
            None => {
                !self.saved
                    && self.writable()
                    && self.merge
                    && self.picks.iter().any(|&p| p != self.default_pick())
            }
        }
    }

    fn save(&mut self) -> std::io::Result<()> {
        if !self.can_save() {
            return Ok(());
        }
        if let Some(resolving) = &mut self.resolving {
            // Written as ours was (its line endings); the comparison stays as it is.
            let (crlf, trailing) = (self.crlf.0, self.trailing_newline.0);
            let newline = if crlf { "\r\n" } else { "\n" };
            let mut text = match &resolving.by_hand {
                Some(field) => field.text().replace('\n', newline),
                None => compose(&self.left, &self.right, &self.chunks, &self.picks).join(newline),
            };
            if trailing && !text.ends_with('\n') {
                text.push_str(newline);
            }
            resolving.written = true;
            self.pending = Some(ViewerAction::SaveText { path: resolving.result.clone(), text });
            return Ok(());
        }
        let (path, crlf, trailing) = match self.target() {
            Side::Left => (&self.left_path, self.crlf.0, self.trailing_newline.0),
            Side::Right => (&self.right_path, self.crlf.1, self.trailing_newline.1),
        };
        let mut text = self.result().join(if crlf { "\r\n" } else { "\n" });
        if trailing {
            text.push_str(if crlf { "\r\n" } else { "\n" });
        }
        // Saved by the app from the file's tab (it shows the result; the save is announced:
        // the local history, a remote project's upload).
        self.pending = Some(ViewerAction::SaveText { path: path.clone(), text });
        // The written file becomes its new side: the comparison continues from there (its
        // differences found on a thread: a big file's take a while).
        let lines: Vec<String> = self.result();
        self.diff_saved(self.target(), lines);
        Ok(())
    }

    fn status(&self) -> Option<String> {
        if let Some(resolving) = &self.resolving {
            return Some(resolving_status(resolving));
        }
        Some(tr!("compare.changes", count = self.changes() as i64))
    }
}
