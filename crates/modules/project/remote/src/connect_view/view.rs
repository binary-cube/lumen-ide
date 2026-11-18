//! The wizard as a view: what it reacts to (clicks, keys, typing, scrolling) and the requests it
//! makes.

use super::*;

impl Viewer for ConnectView {
    /// Editing a saved connection: the form prefilled with it.
    fn set_data(&mut self, data: &dyn std::any::Any) {
        if let Some(edit) = data.downcast_ref::<EditConnection>() {
            self.edit(edit.saved.clone());
        }
    }

    fn poll(&mut self) -> bool {
        if let Some(action) = self.browsing.poll() {
            self.action = Some(action);
            return true;
        }
        let mut changed = false;
        let replies: Vec<Reply> =
            self.browser.as_ref().map(|(_, rx)| rx.try_iter().collect()).unwrap_or_default();
        for reply in replies {
            changed = true;
            // Completions are never waited for.
            if !matches!(reply, Reply::Folders(..) | Reply::Accounts(_)) {
                self.waiting = self.waiting.saturating_sub(1);
            }
            match reply {
                // Testing: signed in, said so (the step stays; the session kept to browse).
                Reply::Home(Ok(_)) if self.testing => {
                    self.testing = false;
                    self.status = Some((t("remote.test_ok"), Tone::Success));
                }
                Reply::Home(Ok(home)) => {
                    self.status = None;
                    self.step = Step::Folder;
                    // Editing: from the saved folder.
                    let start = if self.editing.is_some() { self.root.clone() } else { home };
                    self.go(start);
                }
                Reply::Listed(dir, Ok(entries))
                    if self.step == Step::Folder && dir == self.current =>
                {
                    let mut entries = entries;
                    entries.sort_by(|a, b| {
                        b.is_dir
                            .cmp(&a.is_dir)
                            .then_with(|| a.path.to_lowercase().cmp(&b.path.to_lowercase()))
                    });
                    // Its folders complete a path typed in it, without asking again.
                    let folders = names(&entries);
                    self.server_folders.insert(folder_key(&dir), Some(folders));
                    self.listing = entries;
                }
                Reply::Folders(dir, folders) => self.server_folders_read(dir, folders),
                Reply::Accounts(read) => self.server_accounts = Some(read),
                Reply::Listed(dir, Ok(entries)) if self.step == Step::Contents => {
                    self.place_listing(&dir, entries);
                }
                Reply::Listed(_, Ok(_)) => {}
                Reply::Home(Err(e)) | Reply::Listed(_, Err(e)) => self.failed(e),
            }
        }
        changed |= self.saved_now();
        changed |= self.kept_secret_read();
        if let Some(rx) = &self.downloading
            && let Ok(outcome) = rx.try_recv()
        {
            changed = true;
            self.downloading = None;
            match outcome {
                Ok((local, done, failed)) => {
                    let message = if failed == 0 {
                        tr!("remote.downloaded", count = done)
                    } else {
                        tr!("remote.downloaded_failed", count = done, failed = failed)
                    };
                    self.status = Some((message, Tone::Success));
                    // The local copy opens as the project.
                    self.action = Some(ViewerAction::Request(AppRequest::OpenProject(local)));
                }
                Err(Failure::Remote(e)) => self.failed(e),
                Err(Failure::Secret(why)) => {
                    self.status =
                        Some((tr!("remote.error_secret", reason = why.as_str()), Tone::Danger));
                }
                Err(Failure::Save(why)) => {
                    self.status =
                        Some((tr!("remote.error_save", reason = why.as_str()), Tone::Danger));
                }
            }
        }
        changed
    }

    /// Connecting or downloading: redrawn at the loading pace (its progress), never every
    /// frame. Also while the kept password is read (it may come before the form had a
    /// waker to wake it).
    fn is_loading(&self) -> bool {
        self.busy() || self.reading_secret.is_some()
    }

    fn wants_text(&self) -> bool {
        true
    }

    fn text_input(&mut self, text: &str) -> bool {
        if self.picker.is_open() {
            return self.picker.text_input(text);
        }
        self.type_text(text);
        true
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        if self.picker.is_open() {
            let event = self.picker.key(key, ui_components::KeyReach::default());
            self.picked(event);
            return true;
        }
        if self.on_list && self.step != Step::Connection {
            return self.list_key(key);
        }
        let field = self.focused;
        let editing = self.fields().contains(&field);
        // The path's suggestions (the local copy's, the server's): ↑ ↓ choose, Tab / ↵
        // complete, Esc closes them.
        if matches!(field, Field::Local | Field::Path) && !self.suggestions.is_empty() {
            let len = self.suggestions.len();
            match key {
                ViewerKey::Down => {
                    self.suggested = Some(self.suggested.map_or(0, |s| (s + 1) % len));
                    return true;
                }
                ViewerKey::Up => {
                    self.suggested = Some(self.suggested.map_or(len - 1, |s| (s + len - 1) % len));
                    return true;
                }
                ViewerKey::Tab => {
                    self.complete(self.suggested.unwrap_or(0));
                    return true;
                }
                ViewerKey::Enter if self.suggested.is_some() => {
                    self.complete(self.suggested.unwrap_or(0));
                    return true;
                }
                ViewerKey::Escape => {
                    self.suggestions.clear();
                    return true;
                }
                _ => {}
            }
        }
        match key {
            ViewerKey::Tab | ViewerKey::Down => self.focus_next(1),
            ViewerKey::Up => self.focus_next(-1),
            ViewerKey::Enter => self.next(),
            // Working: Esc stops it (the wizard stays); else it closes.
            ViewerKey::Escape if self.busy() => self.stop(),
            ViewerKey::Escape => {
                self.action = Some(ViewerAction::Close);
                return true;
            }
            ViewerKey::Backspace if editing => {
                self.input_mut(field).backspace(false);
                self.edited(field);
            }
            ViewerKey::Delete if editing => {
                self.input_mut(field).delete();
                self.edited(field);
            }
            ViewerKey::Left if editing => self.input_mut(field).move_left(false, false),
            ViewerKey::Right if editing => self.input_mut(field).move_right(false, false),
            ViewerKey::Home if editing => self.input_mut(field).home(false),
            ViewerKey::End if editing => self.input_mut(field).end(false),
            _ => return false,
        }
        true
    }

    fn command(&mut self, id: &str) -> bool {
        // An open list's search gets the edit commands (⌘A…).
        if self.picker.is_open() {
            return self.picker.command(id);
        }
        // The folder chosen in the system's picker.
        if let Some(path) = id.strip_prefix("picked-folder:") {
            self.input_mut(Field::Local).set_text(path);
            self.local_typed = true;
            self.suggestions.clear();
            return true;
        }
        if id == "select_all" && self.fields().contains(&self.focused) {
            let field = self.focused;
            self.input_mut(field).select_all();
            return true;
        }
        false
    }

    fn modal_icon(&self) -> Option<&'static str> {
        Some("server")
    }

    fn draws_footer(&self) -> bool {
        true
    }

    fn subtitle(&self) -> Option<String> {
        Some(t(if self.editing.is_some() {
            "remote.edit_subtitle"
        } else {
            "remote.connect_subtitle"
        }))
    }

    fn key_hints(&self) -> Vec<(Vec<&'static str>, String)> {
        let next = match self.step {
            Step::Connection | Step::Contents if self.editing.is_some() => t("remote.save"),
            Step::Connection => t("remote.continue"),
            Step::Folder => t("remote.choose_folder"),
            Step::Contents if self.mode == Mode::OnDemand => t("remote.connect_on_demand"),
            Step::Contents => t("remote.connect"),
        };
        vec![(vec!["tab"], t("remote.hint_next_field")), (vec!["↵"], next.to_lowercase())]
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        if self.picker.is_open() {
            return self.picker.scroll(dy);
        }
        let before = self.scroll;
        self.scroll = (self.scroll + dy).clamp(0.0, self.max_scroll);
        self.scroll != before
    }

    fn frame_deadline(&self) -> Option<std::time::Instant> {
        self.picker.next_frame()
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        // An open list takes the click (outside it, it closes).
        if self.picker.is_open() {
            let event = self.picker.press(p);
            self.picked(event);
            return None;
        }
        if let Some(offset) = self.bar.press(p) {
            self.scroll = offset;
            return None;
        }
        match self.hit(p)? {
            Hit::Field(field) => {
                if field != self.focused {
                    self.suggestions.clear();
                }
                self.focused = field;
                self.on_list = false;
                self.input_mut(field).click(p.x);
            }
            Hit::Protocol(i) => self.select_protocol(i),
            Hit::Auth(kind) => self.auth = kind,
            Hit::SavedIn(at) => self.saved_in = at,
            Hit::Mode(mode) => self.mode = mode,
            Hit::Uploads(n) => self.uploads = n,
            Hit::Downloads(n) => self.downloads = n,
            Hit::Moves => self.moves = !self.moves,
            Hit::AutoUpload => self.auto_upload = !self.auto_upload,
            Hit::SecretsIn(at) => self.secrets_in = at,
            Hit::Cancel => self.stop(),
            Hit::Browse => {
                let typed = PathBuf::from(self.value(Field::Local));
                // Opened where the typed folder is (or the closest folder above it).
                self.browsing.start("remote-browse", move || {
                    let start = typed.ancestors().find(|a| a.is_dir()).map(Path::to_path_buf);
                    Some(ViewerAction::Request(AppRequest::PickFolder(start)))
                });
                return None;
            }
            Hit::Suggestion(i) => self.complete(i),
            Hit::Eye => {
                let secret = self.input_mut(Field::Secret);
                let masked = secret.is_masked();
                secret.set_masked(!masked);
            }
            Hit::Next if self.step == Step::Connection => self.browse(),
            Hit::Next => self.next(),
            Hit::Test => self.test(),
            Hit::Save => self.save(),
            Hit::Back => self.back(),
            Hit::Enter(i) => {
                if let Some(entry) = self.listing.get(i).filter(|e| e.is_dir) {
                    let name = entry.path.rsplit('/').next().unwrap_or("").to_owned();
                    let dir = join(&self.current, &name);
                    self.go(dir);
                }
            }
            Hit::Up => {
                let up = parent(&self.current);
                self.go(up);
            }
            Hit::Choose => self.choose(),
            Hit::Check(i) => {
                (self.on_list, self.highlight) = (true, Some(i));
                self.toggle(i);
            }
            Hit::Expand(i) => {
                (self.on_list, self.highlight) = (true, Some(i));
                self.expand(i);
            }
            Hit::Unexclude(i) => {
                if let Some(path) = self.excluded.iter().nth(i).cloned() {
                    self.excluded.remove(&path);
                }
            }
            Hit::Connect => self.connect(),
            Hit::WriteAs(i) => self.write_as_kind(i),
            Hit::Account(account) => self.open_accounts(account),
        }
        None
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        if self.picker.is_open() {
            return Some(self.picker.cursor_at(p).unwrap_or(ViewerCursor::Arrow));
        }
        match self.hit(p)? {
            Hit::Field(_) => None,
            _ => Some(ViewerCursor::Pointer),
        }
    }

    fn drag(&mut self, p: Point) -> bool {
        let Some(offset) = self.bar.drag(p) else { return false };
        let changed = offset != self.scroll;
        self.scroll = offset;
        changed
    }

    fn release(&mut self) -> bool {
        self.bar.release()
    }

    fn hover(&mut self, p: Point) -> bool {
        if self.picker.is_open() {
            return self.picker.hover(p);
        }
        let bar = self.bar.hover(p);
        let hover = self.hit(p);
        let changed = hover != self.hover;
        self.hover = hover;
        changed || bar
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        self.browsing.set_waker(&ctx.waker);
        if ctx.waker.is_some() {
            self.waker.clone_from(&ctx.waker);
        }
        self.hits.clear();
        self.account_fields.clear();
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let p = ctx.palette;
        let px = |v: f32| ds.px(v);
        let b = ctx.bounds;
        // In the app's modal frame (its title, `esc`, the keys): the steps, the step, the
        // buttons.
        let layer = ctx.scene.push_layer(Some(b));
        let width = (b.size.width - px(56.0)).max(0.0);
        let x0 = b.min_x() + px(28.0);
        let mut y = b.min_y() + px(18.0);
        // The steps: done ones checked, the current one ringed, joined by lines.
        let steps = [
            (Step::Connection, "remote.step_connection"),
            (Step::Folder, "remote.step_folder"),
            (
                Step::Contents,
                if self.editing.is_some() { "remote.step_options" } else { "remote.step_contents" },
            ),
        ];
        let current = steps.iter().position(|(s, _)| *s == self.step).unwrap_or(0);
        let labels: Vec<String> = steps.iter().map(|(_, key)| t(key)).collect();
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        ds::stepper(
            layer,
            ctx.text,
            &ds,
            ds::StepperSpec { x: x0, width, mid: y + px(12.0), labels: &labels, current },
        );
        layer.quad(Quad::filled(
            Rect::new(b.min_x(), y + px(38.0), b.size.width, ds.line()),
            p.card_border,
        ));
        y += px(20.0);
        y += px(36.0);
        let footer_h = px(60.0);
        let body = Rect::new(b.min_x(), y, b.size.width, (b.max_y() - footer_h - y).max(0.0));
        let body_hits = self.hits.len();
        {
            let clip = ctx.scene.push_layer(Some(body));
            // Whole pixels: a fractional scroll (trackpad momentum) would move text and
            // boxes out of step by a pixel (jitter).
            let mut yy = y - self.scroll.round();
            let pen = Pen { layer: clip, text: ctx.text, icons: ctx.icons, ds: &ds };
            let column = Column { x0, width };
            match self.step {
                Step::Connection => self.paint_connection(pen, column, &mut yy),
                Step::Folder => self.paint_folder(pen, column, &mut yy, body),
                Step::Contents => self.paint_contents(pen, column, &mut yy),
            }
            // Measured here (it doesn't depend on the scroll): the wheel never goes past it,
            // so no frame is drawn past the end and then snapped back.
            self.max_scroll = (yy + self.scroll.round() - y + px(16.0) - body.size.height).max(0.0);
            self.scroll = self.scroll.min(self.max_scroll);
        }
        // The step's hits keep only their visible part: one scrolled under the steps or the
        // footer must not take their clicks (hidden ones: empty, never hit).
        for (r, _) in &mut self.hits[body_hits..] {
            *r = r.intersect(&body).unwrap_or_default();
        }
        // More above / below: faded edges with a chevron.
        let hints = ctx.scene.push_layer(Some(body));
        ds::scroll_hints(
            hints,
            ctx.icons,
            &ds,
            ds::ScrollHintsSpec {
                area: body,
                axis: ds::Axis::Vertical,
                before: self.scroll > 0.5,
                after: self.scroll < self.max_scroll - 0.5,
                background: p.popup_background,
            },
        );
        let content = self.max_scroll + body.size.height;
        self.bar.paint(ctx.scene, p, ctx.scale, body, content, self.scroll.round());
        let completing = match self.step {
            Step::Folder => self.focused == Field::Path,
            Step::Contents => self.focused == Field::Local,
            Step::Connection => false,
        };
        if completing && !self.on_list {
            self.paint_suggestions(ctx.scene, ctx.text, ctx.icons, &ds);
        }
        // The open list, over everything (its field still in the step: else it closes).
        let anchor = self
            .picking
            .and_then(|a| self.account_fields.iter().find(|(f, _)| *f == a))
            .map(|(_, r)| *r);
        match anchor {
            Some(anchor) => {
                let search = t("remote.props_find_account");
                self.picker.paint(ctx.scene, ctx.text, ctx.icons, &ds, (anchor, b), &search);
            }
            None if self.picker.is_open() => {
                self.picker.close();
                self.picking = None;
            }
            None => {}
        }
        self.paint_footer(
            ctx.scene,
            ctx.text,
            &ds,
            Column { x0, width },
            Rect::new(b.min_x(), b.max_y() - footer_h, b.size.width, footer_h),
        );
    }
}
