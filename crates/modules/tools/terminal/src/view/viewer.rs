//! The view's answers to the app: its shell started at the first paint, its grid drawn,
//! the output polled, the mouse (selection, links, the scrollbar), the keys and the text
//! typed, renaming, its commands.

use super::*;

impl Viewer for TerminalView {
    fn memory(&self) -> ui_components::ViewMemory {
        // Its screen and history (what the scrollback setting allows, as filled).
        let ram = self.session.as_ref().map_or(0, |s| s.memory());
        ui_components::ViewMemory { ram, gpu: 0 }
    }

    fn set_data(&mut self, data: &dyn std::any::Any) {
        // Where it starts (only before it has): as asked, or — reopened from the session —
        // in its project.
        if self.session.is_some() {
            return;
        }
        if let Some(start) = data.downcast_ref::<TerminalStart>() {
            self.cwd.clone_from(&start.cwd);
            self.launch.clone_from(&start.launch);
            self.icon = start.icon;
            self.title.clone_from(&start.title);
            self.key.clone_from(&start.key);
            if let Some(key) = &start.key {
                let slot = Arc::new(Mutex::new(Asked::default()));
                NAMED
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push((key.clone(), Arc::downgrade(&slot)));
                self.renamed = Some(slot);
            }
            self.project = start.project;
            // A program another module runs isn't one of the shells (they're numbered apart).
            if start.launch.is_none() {
                self._live = Some(Live::new(start.project));
            }
        } else if let Some(restored) = data.downcast_ref::<ui_components::Restored>() {
            self.cwd.clone_from(&restored.project);
            self._live = Some(Live::new(restored.id));
            self.project = restored.id;
        }
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        // Selecting with the mouse held past an edge: the history goes on scrolling.
        self.auto_scroll(std::time::Instant::now());
        let b = ctx.bounds;
        if b.size.width <= 0.0 || b.size.height <= 0.0 {
            return;
        }
        let palette = ctx.palette;
        let s = ctx.scale;
        let px = |v: f32| (v * s).round();
        let config = self.config();
        let colors = self.term_colors(palette, config.colors);
        ctx.scene.push_layer(Some(b)).quad(Quad::filled(b, colors.background));
        let pad = px(PADDING);
        let lane = px(LANE);
        // On whole pixels: the rows (whole pixels tall) never straddle one, nor overlap.
        let top = (b.min_y() + px(6.0)).round();
        self.grid_area = Rect::new(
            (b.min_x() + pad).round(),
            top,
            (b.size.width - 2.0 * pad - lane).max(0.0),
            (b.max_y() - pad - top).max(0.0),
        );
        // The bar's lane: right of the grid, its whole height (always kept: the columns
        // don't change when history appears).
        self.bar_area = Rect::new(
            b.max_x() - pad / 2.0 - lane,
            b.min_y() + px(6.0),
            lane,
            (b.size.height - px(6.0) - pad / 2.0).max(0.0),
        );
        // Cell size from the code font, at the size and line height chosen.
        let size = config.font_size.unwrap_or(palette.code_font_size);
        let font = Font::new(palette.code_font.clone(), px(size));
        let cell_w = ctx.text.shape_line("M", &font).width;
        let cell_h = (px(size) * config.line_height).round();
        self.cell = (cell_w, cell_h);
        self.start(ctx.waker.clone());
        // Programs asking its colors (a light or a dark background) are answered these.
        let answered = colors.palette();
        if let Some(session) = &self.session
            && self.answered != Some(answered)
        {
            session.set_palette(answered);
            self.answered = Some(answered);
        }
        let layer = ctx.scene.push_layer(Some(self.grid_area));
        if let Some(why) = &self.failed {
            let message = i18n::tr!("terminal.failed", reason = why.as_str());
            let shaped = ctx.text.shape_line(&message, &font);
            let metrics = ctx.text.metrics(&font);
            let baseline = centered_baseline(self.grid_area.min_y(), cell_h, &metrics);
            draw_text(layer, &shaped, self.grid_area.min_x(), baseline, palette.danger);
            return;
        }
        let size = self.term_size();
        // A part of a line at the bottom (where the output is followed) once the gesture is
        // over: gone — the screen exactly at its bottom, its first row whole at the top.
        let gesture_over = self.last_wheel.is_none_or(|at| at.elapsed() >= SNAP_AFTER);
        if self.grid.display_offset == 0 && gesture_over {
            self.smooth.reset();
        }
        let Some(session) = self.session.as_mut() else { return };
        if size != self.size {
            session.resize(size);
            self.size = size;
            // Resized: the screen is the program's again, at a whole line.
            self.smooth.reset();
        }
        let options = TermOptions {
            scrollback: config.scrollback,
            history_clear: config.history_clear,
            cursor: match config.cursor {
                Cursor::Block => CursorShape::Block,
                Cursor::Bar => CursorShape::Beam,
                Cursor::Underline => CursorShape::Underline,
            },
        };
        if self.options != Some(options) {
            session.set_options(options);
            self.options = Some(options);
        }
        // Its first output still coming: not drawn (a loader instead), the reader let run.
        if self.settling.is_some() {
            let ds = ui_components::ds::Ctx::new(palette, s);
            let label = i18n::t("terminal.loading");
            let loader = ui_components::ds::Loader::with_text(&label);
            let width = loader.width(ctx.text, &ds);
            let x = self.grid_area.min_x() + (self.grid_area.size.width - width) / 2.0;
            let mid = self.grid_area.min_y() + self.grid_area.size.height / 2.0;
            ui_components::ds::loader(layer, ctx.text, &ds, x, mid, &loader);
            return;
        }
        self.grid.want_above = self.smooth.partial();
        // Never waiting for the program's output being parsed (keys and frames go on): the
        // screen it had stays, and the next frame tries again — waiting only once it's been
        // behind for a while, so the screen is never further behind than that.
        let behind = self.stale_since.is_some_and(|t| t.elapsed() >= MAX_BEHIND);
        if self.pending_scroll != 0 {
            if behind {
                session.scroll(self.pending_scroll);
                self.pending_scroll = 0;
            } else if session.try_scroll(self.pending_scroll) {
                self.pending_scroll = 0;
            }
        }
        if behind {
            session.snapshot(&mut self.grid);
            self.stale_since = None;
        } else if session.try_snapshot(&mut self.grid) {
            self.stale_since = None;
        } else {
            self.stale_since.get_or_insert_with(std::time::Instant::now);
        }
        // Moved by itself (the output followed, the history cleared, a full-screen program):
        // nothing partial left where there's nothing to show.
        if self.grid.alt_screen {
            self.smooth.reset();
        }
        self.smooth.settle(self.grid.display_offset, self.grid.history);
        // The selection under the text, and the ⌘-hovered link underlined; all of it shifted
        // down by the part of a line scrolled (the line above in the gap).
        let shift = self.smooth.shift(cell_h);
        let area = Rect::new(
            self.grid_area.min_x(),
            self.grid_area.min_y() + shift,
            self.grid_area.size.width,
            self.grid_area.size.height,
        );
        let x_of = |col: u16| (area.min_x() + f32::from(col) * cell_w).round();
        if let Some(selection) = self.selection.filter(|s| !s.is_empty()) {
            for row in 0..self.grid.rows {
                let line = self.grid.point(row, 0).line;
                if let Some(cols) = selection.columns(line, self.grid.cols) {
                    let top = area.min_y() + f32::from(row) * cell_h;
                    layer.quad(Quad::filled(
                        Rect::new(x_of(cols.start), top, x_of(cols.end) - x_of(cols.start), cell_h),
                        palette.selection,
                    ));
                }
            }
        }
        if let Some((line, cols)) = &self.link
            && let Some(row) = self.grid.visible_row(*line)
        {
            let thin = s.round().max(1.0);
            let y = area.min_y() + f32::from(row + 1) * cell_h - 3.0 * thin;
            layer.quad(Quad::filled(
                Rect::new(x_of(cols.start), y.round(), x_of(cols.end) - x_of(cols.start), thin),
                palette.accent,
            ));
        }
        let style = GridStyle {
            cell: (cell_w, cell_h),
            font: &font,
            focused: ctx.focused,
            blink: config.blink,
            caret_id: std::ptr::from_ref(self) as u64,
            colors: &colors,
            bold_bright: config.bold_bright,
            scale: s,
        };
        paint_grid(layer, &self.grid, area, ctx.text, &style);
        // Ended, its tab kept: said on a band at the grid's foot.
        if let Some(code) = self.ended() {
            let notice = i18n::tr!("settings.terminal_ended", code = code);
            let band = Rect::new(
                self.grid_area.min_x(),
                (self.grid_area.max_y() - cell_h).round(),
                self.grid_area.size.width,
                cell_h,
            );
            layer.quad(Quad::filled(band, colors.background));
            let shaped = ctx.text.shape_line(&notice, &font);
            let baseline = centered_baseline(band.min_y(), cell_h, &ctx.text.metrics(&font));
            draw_text(layer, &shaped, band.min_x(), baseline, palette.text_muted);
        }
        // The history's place: where the screen is in it (none without history, nor on a
        // full-screen program's own screen).
        if let Some((content, offset)) = self.bar_metrics() {
            self.bar.paint(ctx.scene, palette, s, self.bar_area, content, offset);
        }
    }

    fn poll(&mut self) -> bool {
        // A frame that found the screen busy asks for the next one.
        let dirty = self.session.as_ref().is_some_and(|s| s.take_dirty());
        // Its first output: the screen shows once it's quiet (else nothing to draw yet).
        if let Some(settling) = &mut self.settling {
            let now = std::time::Instant::now();
            if dirty {
                settling.last = now;
            }
            if now < settling.ends() {
                return false;
            }
            self.settling = None;
            return true;
        }
        // The screen changed: what the program calls itself may have too.
        let mut renamed = dirty && self.refresh_title();
        // Known by a key: renamed by it (its session renamed), and says whether it runs.
        let name = self.renamed.as_ref().and_then(|slot| {
            let running = self.session.as_ref().is_some_and(|s| s.exit_status().is_none());
            let mut asked = slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            asked.running = running;
            asked.name.take()
        });
        if let Some(title) = name {
            renamed |= self.title.as_deref() != Some(title.as_str());
            self.title = Some(title);
        }
        self.stale_since.is_some() | dirty | renamed
    }

    /// What the program calls itself, else its name — but a terminal known by a key (an
    /// agent's session) keeps the name it's given (followed, or the user's) over the program's.
    fn live_title(&self) -> Option<&str> {
        if self.key.is_some() {
            return self.title.as_deref().or(self.program_title.as_deref());
        }
        self.program_title.as_deref().or(self.title.as_deref())
    }

    /// Its first output let run: woken when it should be quiet by then.
    fn frame_deadline(&self) -> Option<std::time::Instant> {
        let settling = self.settling.map(Settling::ends);
        // The next step of a selection scrolling past an edge.
        match (settling, self.auto_next) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// A part of a line at the bottom waiting for its gesture to end: frames until it snaps.
    fn is_animating(&self) -> bool {
        self.grid.display_offset == 0
            && self.smooth.partial()
            && self.last_wheel.is_some_and(|at| at.elapsed() < SNAP_AFTER)
    }

    /// Its loader turning while its first output comes (at a loader's pace, not every frame
    /// the display could draw; its end wakes it: `frame_deadline`).
    fn is_loading(&self) -> bool {
        self.settling.is_some()
    }

    /// The shell exited (`exit`; its tab stays when the settings say so), or was asked to
    /// close from the Task Manager: its tab closes (its session with it).
    fn closed(&self) -> bool {
        let asked = self
            .shell
            .as_ref()
            .is_some_and(|ShellPid(pid)| CLOSING.lock().is_ok_and(|c| c.contains(pid)));
        let exited = self.session.as_ref().is_some_and(|s| s.exit_status().is_some());
        asked || (exited && self.ended().is_none())
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }

    /// The wheel: the history scrolls by pixels (a trackpad's steps and its momentum
    /// continuous; a wheel's notch its lines): the emulator by whole lines, the grid drawn
    /// shifted by the rest. A program that asked for the mouse, or a full-screen one, gets it
    /// instead, at once, in lines.
    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        self.settling = None;
        let line = self.cell.1.max(1.0);
        let at = self.pointer.and_then(|p| self.cell_at(p, true)).unwrap_or((0, 0));
        // A program that asked for the mouse gets the wheel, unless the settings say not.
        let reported = self.config().mouse_reporting
            && terminal_session::encode_wheel(1, at, &self.grid).is_some();
        if !reported {
            let (offset, history) = (self.grid.display_offset, self.grid.history);
            let before = self.smooth;
            let lines = self.smooth.wheel(dy, line, offset, history);
            self.last_wheel = Some(std::time::Instant::now());
            if self.session.is_none() {
                return false;
            }
            if lines != 0 {
                self.scroll_lines(lines);
            }
            if lines == 0 && self.smooth == before {
                return false;
            }
            // Shown now (not at the next output), the line above with it when partial.
            self.grid.want_above = self.smooth.partial();
            self.show_scrolled();
            return true;
        }
        self.wheel_rest += -dy / line;
        let lines = self.wheel_rest.trunc();
        self.wheel_rest -= lines;
        let lines = lines as i32;
        if lines == 0 {
            return false;
        }
        let at = self.pointer.and_then(|p| self.cell_at(p, true)).unwrap_or((0, 0));
        let report = encode_wheel(lines, at, &self.grid);
        let Some(session) = self.session.as_mut() else { return false };
        match report {
            Some(bytes) => {
                session.write(&bytes);
                false
            }
            None => {
                self.scroll_lines(lines);
                // Shown now (not at the next output).
                self.show_scrolled();
                true
            }
        }
    }

    fn hover(&mut self, p: Point) -> bool {
        self.pointer = Some(p);
        self.with_controls(|c, parts| c.hover(parts, Some(p)))
    }

    fn press(&mut self, p: Point, clicks: u32) -> Option<ViewerAction> {
        // Its parts: the scrollbar's thumb taken, or the history jumped there.
        if self.with_controls(|c, parts| c.press(parts, p, clicks)).is_some() {
            return None;
        }
        self.begin_selection(p, clicks);
        None
    }

    /// In an editor's tab (an agent's session): a press as at the bottom — the scrollbar's
    /// thumb taken, a selection started.
    fn pointer_down(
        &mut self,
        p: Point,
        clicks: u32,
        _mods: ui_components::PressMods,
    ) -> Option<ViewerAction> {
        self.press(p, clicks)
    }

    /// The part holding the pointer follows it (`true` if the history moved), else the
    /// selection does.
    fn drag(&mut self, p: Point) -> bool {
        let shown = self.grid.display_offset;
        if self.with_controls(|c, parts| c.drag(parts, p)).is_some() {
            return self.grid.display_offset != shown;
        }
        self.drag_selection(p)
    }

    fn release(&mut self) -> bool {
        let part = self.with_controls(|c, parts| c.release(parts)).is_some_and(|(_, c)| c);
        part | self.end_selection()
    }

    fn link_hover(&mut self, p: Option<Point>) -> bool {
        let link = p.and_then(|p| self.link_under(p)).map(|(line, cols, _)| (line, cols));
        let changed = link != self.link;
        self.link = link;
        changed
    }

    fn link_at(&self, p: Point) -> Option<ViewerAction> {
        Some(match self.link_under(p)?.2 {
            TerminalLink::Url(url) => ViewerAction::OpenLink(url),
            TerminalLink::File { path, line, column } => {
                ViewerAction::OpenFile { path, line, column }
            }
        })
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        if self.link.is_some() {
            return Some(ViewerCursor::Pointer);
        }
        self.grid_area.contains(p).then_some(ViewerCursor::Text)
    }

    fn takes_keyboard(&self) -> bool {
        true
    }

    fn key_press(&mut self, key: KeyPress) -> bool {
        self.settling = None;
        self.selection = None;
        // Typing: the cursor shows solid, its blinking starts again.
        ui_kit::caret::reset();
        let Some(session) = self.session.as_ref() else { return false };
        let modes = session.try_modes().unwrap_or_else(|| InputModes::of(&self.grid));
        let mods = TermModifiers { shift: key.shift, ctrl: key.ctrl, alt: key.alt };
        let Some(bytes) = encode_key(term_key(key.key), mods, modes.app_cursor) else {
            return false;
        };
        self.write(&bytes)
    }

    fn wants_text(&self) -> bool {
        true
    }

    fn text_input(&mut self, text: &str) -> bool {
        self.settling = None;
        self.selection = None;
        ui_kit::caret::reset();
        self.write(text.as_bytes())
    }

    fn selected_text(&self) -> Option<String> {
        let selection = self.selection.filter(|s| !s.is_empty())?;
        Some(selection.text(&self.grid)).filter(|t| !t.is_empty())
    }

    fn clear_selection(&mut self) -> bool {
        self.selection.take().is_some()
    }

    fn paste(&mut self, text: &str) -> bool {
        let Some(session) = self.session.as_ref() else { return false };
        let modes = session.try_modes().unwrap_or_else(|| InputModes::of(&self.grid));
        let bytes = encode_paste(text, modes.bracketed_paste);
        self.write(&bytes)
    }

    /// A shell is opened again at the next start; a program a module asked for isn't (that
    /// module takes it up again its own way: an agent's session resumed).
    fn restorable(&self) -> bool {
        self.launch.is_none()
    }

    fn tab_icon(&self) -> Option<&'static str> {
        self.icon
    }

    /// A program another module runs by a key (an agent's session) can be renamed.
    fn renamable(&self) -> bool {
        self.key.is_some()
    }

    /// Its tab renamed, and its session asked to be so too (whoever follows it, its agent's
    /// own way).
    fn rename(&mut self, title: &str) -> Option<ViewerAction> {
        let key = self.key.clone()?;
        self.title = Some(title.chars().take(TITLE_MOST).collect());
        let asked = contracts::agents::RenameSession { key, title: title.to_owned() };
        self.action = Some(ViewerAction::Ask(ui_components::Asked::new(asked)));
        None
    }

    /// Clear Terminal (⌘K / Ctrl+Shift+K): the history emptied, whatever programs may do.
    fn command(&mut self, id: &str) -> bool {
        if id != crate::CLEAR_ACTION {
            return false;
        }
        let Some(session) = self.session.as_mut() else { return false };
        session.clear_history();
        self.selection = None;
        session.snapshot(&mut self.grid);
        true
    }
}
