//! The view's answers to the app: its shell started at the first paint, its grid drawn,
//! the output polled, the mouse (selection, links, the scrollbar), the keys and the text
//! typed, renaming, its commands.

use super::*;

impl Viewer for TerminalView {
    fn memory(&self) -> ui_components::ViewMemory {
        // Its screen and history (what the scrollback setting allows, as filled); asleep,
        // the last screen it keeps.
        let ram = self.session.as_ref().map_or(0, |s| s.memory());
        ui_components::ViewMemory { ram, gpu: 0 }
    }

    /// An order to sleep or to wake waits (given from elsewhere while it's idle): polled.
    fn wants_poll(&self) -> bool {
        self.renamed.as_ref().is_some_and(|slot| {
            let asked = asked(slot);
            asked.sleep.is_some() || asked.wake
        })
    }

    /// Its tab chosen or left: chosen, it's touched (its time untouched starts again). Asleep,
    /// it stays so (moving through the tabs onto it by mistake wakes nothing): only a key, a
    /// click in it or "Wake Up" does.
    fn set_active(&mut self, active: bool) {
        self.active = active;
        if active {
            self.touch();
        }
    }

    fn set_data(&mut self, data: &dyn std::any::Any) {
        // Where it starts (only before it has): as asked, or — reopened from the session —
        // in its project. Asked again by its key (brought forward from the sessions' panel),
        // it keeps what it is: asleep stays asleep, its slot and its state the same.
        if self.session.is_some() || self.renamed.is_some() || self.asleep {
            return;
        }
        if let Some(start) = data.downcast_ref::<TerminalStart>() {
            self.cwd.clone_from(&start.cwd);
            self.launch.clone_from(&start.launch);
            self.icon = start.icon;
            self.title.clone_from(&start.title);
            self.key.clone_from(&start.key);
            self.root.clone_from(&start.root);
            if let Some(key) = &start.key {
                let title = start.title.clone();
                let in_editor = start.in_editor;
                let asked = Asked { project: start.project, title, in_editor, ..Asked::default() };
                let slot = Arc::new(Mutex::new(asked));
                NAMED
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push((key.clone(), Arc::downgrade(&slot)));
                self.renamed = Some(slot);
                self._gone = Some(Gone { key: key.clone(), project: start.project });
                // Watched while it lives (what it holds; asleep when untouched and idle).
                crate::sleep::ensure_running();
            }
            self.project = start.project;
            // A program another module runs isn't one of the shells (they're numbered apart).
            if start.launch.is_none() {
                self._live = Some(Live::new(start.project));
            }
        } else if let Some(restored) = data.downcast_ref::<ui_components::Restored>() {
            self.cwd.clone_from(&restored.project);
            self.root.clone_from(&restored.project);
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
        // What wakes its window, for orders from elsewhere (once: the same every frame).
        if let (Some(slot), Some(waker)) = (&self.renamed, &ctx.waker) {
            let mut asked = asked(slot);
            if asked.waker.is_none() {
                asked.waker = Some(Arc::clone(waker));
            }
        }
        // With the keyboard: what "Put Session to Sleep" acts on (a plain shell with it: none);
        // drawn without it, no longer.
        let renamed = self.renamed.as_ref();
        with_focused(|focused| {
            let mine =
                renamed.is_some_and(|slot| std::ptr::eq(focused.as_ptr(), Arc::as_ptr(slot)));
            if ctx.focused {
                *focused = renamed.map(Arc::downgrade).unwrap_or_default();
            } else if mine {
                *focused = std::sync::Weak::new();
            }
        });
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
        // Started again (or failed to): said at once.
        self.announce();
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
        let resuming = self.resuming && self.settling.is_some();
        // Asleep: its last screen stays, nothing runs until the user wakes it.
        let look = ScreenLook {
            font: &font,
            colors: &colors,
            palette,
            cell: (cell_w, cell_h),
            focused: ctx.focused,
            scale: s,
            blink: config.blink,
            bold_bright: config.bold_bright,
        };
        let Some(session) = self.session.as_mut() else {
            if self.held {
                self.paint_screen(layer, ctx.text, &look);
                // Over the screen's text in a layer of its own (a layer draws its quads
                // before its glyphs: in the grid's, the text would show through).
                let over = ctx.scene.push_layer(Some(self.grid_area));
                self.paint_scrim(over, &colors);
                self.paint_asleep(over, (ctx.text, ctx.icons), (palette, s), false);
                self.paint_bar(ctx.scene, palette, s);
            }
            return;
        };
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
        // Its first output still coming: not drawn (a loader instead), the reader let run —
        // woken from a sleep, its last screen stays meanwhile (with a band saying so).
        if self.settling.is_some() && !resuming {
            let ds = ui_components::ds::Ctx::new(palette, s);
            let label = i18n::t("terminal.loading");
            let loader = ui_components::ds::Loader::with_text(&label);
            let width = loader.width(ctx.text, &ds);
            let x = self.grid_area.min_x() + (self.grid_area.size.width - width) / 2.0;
            let mid = self.grid_area.min_y() + self.grid_area.size.height / 2.0;
            ui_components::ds::loader(layer, ctx.text, &ds, x, mid, &loader);
            return;
        }
        if !resuming {
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
            let taken = if behind {
                session.snapshot(&mut self.grid);
                self.snapshot_waited();
                true
            } else {
                self.try_snapshot()
            };
            if taken {
                self.stale_since = None;
            } else {
                self.stale_since.get_or_insert_with(std::time::Instant::now);
            }
        }
        // Ended, its tab kept — or woken from a sleep, its last screen shown while it's
        // resumed: said on a band at the grid's foot.
        let notice = self.ended().map(|code| i18n::tr!("settings.terminal_ended", code = code));
        self.paint_screen(layer, ctx.text, &look);
        // What's said over the screen: in a layer of its own, over the grid's text.
        if notice.is_some() || resuming {
            let over = ctx.scene.push_layer(Some(self.grid_area));
            if let Some(notice) = notice {
                let band = Band { text: &notice, font: &font, background: colors.background };
                self.paint_band(over, ctx.text, (band, palette.text_muted));
            }
            if resuming {
                self.paint_scrim(over, &colors);
                self.paint_asleep(over, (ctx.text, ctx.icons), (palette, s), true);
            }
        }
        if !resuming {
            self.wake_button = None;
        }
        self.paint_bar(ctx.scene, palette, s);
    }

    fn poll(&mut self) -> bool {
        // A frame that found the screen busy asks for the next one.
        let dirty = self.session.as_ref().is_some_and(|s| s.take_dirty());
        // Orders first, a session still settling too. Woken from elsewhere (the sessions'
        // panel): started again at the next paint.
        let woken = self.renamed.as_ref().is_some_and(|slot| std::mem::take(&mut asked(slot).wake));
        if woken && self.wake_by_user() {
            return true;
        }
        // Back (or never asleep): no longer waking, whatever ended the coming back (a key
        // pressed while it settled, its program failing to start).
        if !self.asleep
            && !(self.resuming && self.settling.is_some())
            && let Some(slot) = &self.renamed
        {
            asked(slot).waking = false;
        }
        // Put to sleep: by hand now, shown or not (it stays asleep until the user acts on
        // it); when idle, unless it was touched since it was asked (used again: awake).
        let order = self.renamed.as_ref().and_then(|slot| {
            let mut asked = asked(slot);
            let (launch, at) = asked.sleep.take()?;
            let by_hand = std::mem::take(&mut asked.by_hand);
            // Too old (not taken in time): let go, never applied late.
            if at.elapsed() >= SLEEP_ORDER_FRESH {
                return None;
            }
            // Touched since it was asked: awake.
            let untouched = asked.touched.is_none_or(|touched| touched <= at);
            (by_hand || untouched).then_some((launch, by_hand))
        });
        if let Some((launch, by_hand)) = order {
            self.fall_asleep(launch, by_hand);
            return true;
        }
        // What changed since (typed, measured, back, an order let go): said to whoever
        // shows it.
        self.announce();
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
            self.resuming = false;
            self.slept = None;
            if let Some(slot) = &self.renamed {
                asked(slot).waking = false;
            }
            self.announce();
            return true;
        }
        // The screen changed: what the program calls itself may have too.
        let mut renamed = dirty && self.refresh_title();
        // Known by a key: renamed by it (its session renamed), and says whether it runs.
        let name = self.renamed.as_ref().and_then(|slot| {
            let running = self.session.as_ref().is_some_and(|s| s.exit_status().is_none());
            let mut asked = asked(slot);
            // Asleep, it isn't running (its owner may write over its files), yet resumes.
            asked.running = running;
            asked.name.take()
        });
        if let Some(title) = name {
            renamed |= self.title.as_deref() != Some(title.as_str());
            if let Some(slot) = &self.renamed {
                asked(slot).title = Some(title.clone());
            }
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

    fn take_notifications(&mut self) -> Vec<workbench_model::Notification> {
        std::mem::take(&mut self.notices)
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }

    /// The wheel: the history scrolls by pixels (a trackpad's steps and its momentum
    /// continuous; a wheel's notch its lines): the emulator by whole lines, the grid drawn
    /// shifted by the rest. A program that asked for the mouse, or a full-screen one, gets it
    /// instead, at once, in lines.
    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        self.touch();
        // Asleep (or coming back): its last screen stays still, nothing sent anywhere.
        if self.frozen() {
            return false;
        }
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
        if self.frozen() {
            return false;
        }
        self.with_controls(|c, parts| c.hover(parts, Some(p)))
    }

    fn press(&mut self, p: Point, clicks: u32) -> Option<ViewerAction> {
        self.touch();
        // Asleep: a click resumes it; coming back: nothing to press yet.
        if self.wake_by_user() || self.frozen() {
            return None;
        }
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
        if self.frozen() {
            return false;
        }
        let shown = self.grid.display_offset;
        if self.with_controls(|c, parts| c.drag(parts, p)).is_some() {
            return self.grid.display_offset != shown;
        }
        self.drag_selection(p)
    }

    fn release(&mut self) -> bool {
        if self.frozen() {
            self.selecting = false;
            return false;
        }
        let part = self.with_controls(|c, parts| c.release(parts)).is_some_and(|(_, c)| c);
        part | self.end_selection()
    }

    fn link_hover(&mut self, p: Option<Point>) -> bool {
        let link = p
            .filter(|_| !self.frozen())
            .and_then(|p| self.link_under(p))
            .map(|(line, cols, _)| (line, cols));
        let changed = link != self.link;
        self.link = link;
        changed
    }

    fn link_at(&self, p: Point) -> Option<ViewerAction> {
        if self.frozen() {
            return None;
        }
        Some(match self.link_under(p)?.2 {
            TerminalLink::Url(url) => ViewerAction::OpenLink(url),
            TerminalLink::File { path, line, column } => {
                ViewerAction::OpenFile { path, line, column }
            }
        })
    }

    /// The history's bar (over it, or held): the bar's; a hand on a link; the text's on the
    /// grid.
    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        // Asleep: the arrow, a hand over the card's button.
        if self.frozen() {
            let on_button = self.wake_button.is_some_and(|b| b.contains(p));
            return Some(if on_button { ViewerCursor::Pointer } else { ViewerCursor::Arrow });
        }
        self.controls.bar_or(p, || match self.link {
            Some(_) => Some(ViewerCursor::Pointer),
            None => self.grid_area.contains(p).then_some(ViewerCursor::Text),
        })
    }

    fn takes_keyboard(&self) -> bool {
        true
    }

    fn key_press(&mut self, key: KeyPress) -> bool {
        self.touch();
        // Asleep: a key resumes it (the key itself goes to nothing yet).
        if self.wake_by_user() {
            return true;
        }
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
        // ↵ sends what was typed (⇧↵ / ⌥↵ only add a line to it); Ctrl+C, Ctrl+U and Esc
        // drop it; ⌫ takes a character back.
        let sent = match key.key {
            PressedKey::Enter => !key.shift && !key.alt,
            PressedKey::Escape => true,
            PressedKey::Char('c' | 'C' | 'u' | 'U') => key.ctrl,
            _ => false,
        };
        if sent {
            self.note_typed(Typing::Sent);
        } else if matches!(key.key, PressedKey::Backspace) {
            self.note_typed(Typing::Erased);
        }
        self.write(&bytes)
    }

    fn wants_text(&self) -> bool {
        true
    }

    fn text_input(&mut self, text: &str) -> bool {
        self.touch();
        if self.wake_by_user() {
            return true;
        }
        self.settling = None;
        self.selection = None;
        ui_kit::caret::reset();
        self.note_typed(Typing::Typed(text.chars().count()));
        self.write(text.as_bytes())
    }

    fn selected_text(&self) -> Option<String> {
        let selection = self.selection.filter(|s| !s.is_empty())?;
        // All of it, what scrolled off the screen while it was made too. The emulator busy:
        // the whole text made ready at its last free moment, else the screen's part (the
        // whole selection is on it then).
        let whole = self
            .session
            .as_ref()
            .and_then(|s| s.selection_text(&selection, self.grid.history))
            .or_else(|| self.ready_copy_of(&selection));
        whole.or_else(|| Some(selection.text(&self.grid))).filter(|t| !t.is_empty())
    }

    fn clear_selection(&mut self) -> bool {
        self.selection.take().is_some()
    }

    fn paste(&mut self, text: &str) -> bool {
        self.touch();
        if self.wake_by_user() {
            return true;
        }
        let Some(session) = self.session.as_ref() else { return false };
        let modes = session.try_modes().unwrap_or_else(|| InputModes::of(&self.grid));
        let bytes = encode_paste(text, modes.bracketed_paste);
        // Ending in a new line, it's sent as it lands.
        if text.ends_with(['\n', '\r']) {
            self.note_typed(Typing::Sent);
        } else {
            self.note_typed(Typing::Typed(text.chars().count()));
        }
        self.write(&bytes)
    }

    /// A shell is opened again at the next start; a program a module asked for isn't (that
    /// module takes it up again its own way: an agent's session resumed).
    fn restorable(&self) -> bool {
        self.launch.is_none()
    }

    /// Asleep: a moon in its icon's place (resumed once woken).
    fn tab_icon(&self) -> Option<&'static str> {
        if self.asleep { Some(ASLEEP_ICON) } else { self.icon }
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
        if let Some(slot) = &self.renamed {
            asked(slot).title.clone_from(&self.title);
        }
        let asked = contracts::agents::RenameSession { key, title: title.to_owned() };
        self.action = Some(ViewerAction::Ask(ui_components::Asked::new(asked)));
        None
    }

    /// A keyed session: "Put to Sleep", greyed out with why while it's starting (how it
    /// resumes not known yet), something typed isn't sent or it's on its way to sleep;
    /// asleep, "Wake Up", greyed out while it wakes; nothing while it can't sleep at all.
    /// Read from memory.
    fn tab_menu(&self) -> Vec<ui_components::ViewMenuEntry> {
        use contracts::terminal::CanSleep;
        let Some(slot) = &self.renamed else { return Vec::new() };
        let can = asked(slot).can_sleep();
        // Asleep, or waking: waking it instead.
        if self.asleep || can == CanSleep::Waking {
            let waking = can == CanSleep::Waking;
            let wake = i18n::t(if waking { "terminal.waking_up" } else { "terminal.wake_up" });
            return vec![
                ui_components::ViewMenuEntry::item(crate::WAKE_ACTION, wake)
                    .with_icon("sun")
                    .disabled_if(waking),
            ];
        }
        let label = match can {
            CanSleep::No | CanSleep::Waking => return Vec::new(),
            CanSleep::Yes => i18n::t("terminal.sleep_now"),
            CanSleep::Typed => i18n::t("terminal.sleep_now_typed"),
            CanSleep::Sleeping => i18n::t("terminal.going_to_sleep"),
            CanSleep::Starting => i18n::t("terminal.sleep_now_starting"),
        };
        vec![
            ui_components::ViewMenuEntry::item(crate::SLEEP_ACTION, label)
                .with_icon(ASLEEP_ICON)
                .disabled_if(can != CanSleep::Yes),
        ]
    }

    /// "Wake Up" brings its tab to the front first: shown, it starts again.
    fn tab_row_activates(&self, id: &str) -> bool {
        id == crate::WAKE_ACTION
    }

    /// Clear Terminal (⌘K / Ctrl+Shift+K): the history emptied, whatever programs may do;
    /// Put to Sleep (by hand).
    fn command(&mut self, id: &str) -> bool {
        #[cfg(feature = "snapshot")]
        if id == "snapshot:asleep" {
            return self.asleep_for_snapshot();
        }
        // Acted on (Clear, Wake Up): its time untouched starts again (put to sleep by hand,
        // it sleeps whatever).
        self.touch();
        if id == crate::WAKE_ACTION {
            return self.wake_by_user();
        }
        if id == crate::SLEEP_ACTION {
            let Some(slot) = &self.renamed else { return false };
            let can = asked(slot).can_sleep();
            if can != contracts::terminal::CanSleep::Yes {
                // Never silently nothing: why it can't, told.
                self.notices.push(cant_sleep(can));
                return true;
            }
            let asked_now = asked(slot).sleep_by_hand();
            // Asleep at once (this frame draws it), not at a later poll.
            return asked_now && self.poll();
        }
        if id != crate::CLEAR_ACTION {
            return false;
        }
        let Some(session) = self.session.as_mut() else { return false };
        session.clear_history();
        self.selection = None;
        session.snapshot(&mut self.grid);
        self.snapshot_waited();
        true
    }
}
