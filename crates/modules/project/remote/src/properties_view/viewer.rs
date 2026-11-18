//! The properties window as a viewer.

use super::*;

impl Viewer for PropertiesView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        let Some(data) = data.downcast_ref::<PropertiesData>() else { return };
        self.data = Some(data.clone());
        #[cfg(feature = "snapshot")]
        if let Some(sample) = data.paths.first().and_then(|p| p.strip_prefix(SAMPLE)) {
            self.root = "/var/www/shop".into();
            self.name = "Shop".into();
            self.address = "deploy@shop.example.com/var/www/shop".into();
            if let Some(d) = &mut self.data {
                d.paths = sample::paths(sample);
            }
            self.take_loaded(sample::loaded(sample));
            if sample == "list" {
                self.open_list(Choice::Group);
            }
            if matches!(sample, "folder" | "confirm" | "failed") {
                self.measured = Some(Ok(Measured { bytes: 52_428_800, files: 1240, folders: 86 }));
            }
            if sample == "confirm" {
                self.recursive = true;
                self.x_on_folders = true;
                self.toggle(Bit::Access(Class::Others, Access::Read));
                self.ok();
            }
            if sample == "failed" {
                let refused = |p: &str| {
                    (p.to_owned(), "the server refused: operation not permitted".to_owned())
                };
                self.phase =
                    Phase::Failed(vec![refused("storage/logs/app.log"), refused("storage/cache")]);
            }
            if sample == "algos" {
                self.tab = Tab::Checksum;
                self.open_algos();
            }
            if sample == "checksum" {
                use checksum::{Local, Sum};
                self.tab = Tab::Checksum;
                self.sums.choose(Algo::Sha256);
                let done = |hex: &str, on_server, local| Sum::Done {
                    sum: remote::properties::Checksum {
                        algo: Algo::Sha256,
                        hex: hex.into(),
                        on_server,
                    },
                    local,
                };
                self.sums.rows[0].1 = done(
                    "3908c567feda72bc0dbdb2dff040fe0d3470dcd51b942374378a476930dbf6b3",
                    true,
                    Local::Same,
                );
                self.sums.rows[1].1 = done(
                    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
                    false,
                    Local::Different,
                );
            }
            return;
        }
        self.state = session::session(data.project);
        let Some(state) = self.state.clone() else {
            self.failed = Some(t("remote.not_remote"));
            return;
        };
        self.root.clone_from(&state.saved.connection.root);
        self.name.clone_from(&state.saved.connection.name);
        self.address = state.saved.connection.address();
        let paths = data.paths.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        kernel::work::run("remote-properties", move || {
            let _ = tx.send(load(state.connector.as_ref(), &paths));
            (state.wake())();
        });
        self.loading = Some(rx);
    }

    fn subtitle(&self) -> Option<String> {
        (!self.address.is_empty()).then(|| self.address.clone())
    }

    fn modal_width(&self) -> Option<f32> {
        Some(WIDTH)
    }

    fn modal_height(&self) -> Option<f32> {
        Some(HEIGHT)
    }

    fn modal_icon(&self) -> Option<&'static str> {
        Some("info")
    }

    /// The footer's buttons for where applying is: Cancel / OK, Back / Apply, Stop, Close.
    fn modal_buttons(&self) -> Vec<ModalButton> {
        let button = |hit: Hit| FOOTER.iter().find(|(_, h)| *h == hit).map_or("", |(id, _)| id);
        let ready = self.loaded.is_some() && self.failed.is_none();
        let other = |hit, key: &str| ModalButton::other(button(hit), t(key));
        match &self.phase {
            _ if !ready => vec![other(Hit::Close, "remote.props_close")],
            Phase::Editing => vec![
                ModalButton::main(button(Hit::Ok), t("remote.props_ok"), self.change().is_some()),
                other(Hit::Close, "remote.props_cancel"),
            ],
            Phase::Counting => vec![other(Hit::Back, "remote.props_back")],
            Phase::Confirm(_) => vec![
                ModalButton::main(button(Hit::Ok), t("remote.props_apply"), true),
                other(Hit::Back, "remote.props_back"),
            ],
            Phase::Applying(_) => vec![other(Hit::Stop, "remote.props_measure_stop")],
            Phase::Failed(_) => vec![other(Hit::Close, "remote.props_close")],
        }
    }

    /// What's going on, in the footer: counted, asked, applied.
    fn modal_status(&self) -> Option<ModalStatus> {
        match &self.phase {
            Phase::Counting => Some(ModalStatus::Busy(t("remote.props_counting"))),
            Phase::Applying(a) => {
                let done = a.seen.load(Ordering::Relaxed);
                Some(ModalStatus::Busy(tr!("remote.props_applying", done = done, total = a.total)))
            }
            Phase::Confirm(count) => {
                Some(ModalStatus::Warning(tr!("remote.props_confirm", count = *count)))
            }
            _ => None,
        }
    }

    fn closed(&self) -> bool {
        false
    }

    fn wants_text(&self) -> bool {
        self.typing || (self.picker.is_open() && self.picker.searches())
    }

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        self.picker.is_open() && self.picker.scroll(dy)
    }

    fn frame_deadline(&self) -> Option<std::time::Instant> {
        self.picker.next_frame()
    }

    fn text_input(&mut self, typed: &str) -> bool {
        if self.picker.is_open() {
            return self.picker.text_input(typed);
        }
        if !self.typing {
            return false;
        }
        // Only octal digits, four at most.
        let digits: String = typed.chars().filter(|c| ('0'..='7').contains(c)).collect();
        if digits.is_empty() {
            return true;
        }
        let room =
            4usize.saturating_sub(self.octal.text().len() - self.octal.selected_text().len());
        self.octal.insert(&digits[..digits.len().min(room)]);
        self.octal_typed();
        true
    }

    fn paste(&mut self, text: &str) -> bool {
        self.text_input(text.trim())
    }

    fn poll(&mut self) -> bool {
        let mut changed = false;
        if let Some(rx) = &self.loading
            && let Ok(result) = rx.try_recv()
        {
            self.loading = None;
            match result {
                Ok(loaded) if loaded.props.is_empty() => {
                    self.failed = Some(t("remote.props_not_there"));
                }
                Ok(loaded) => self.take_loaded(loaded),
                Err(why) => self.failed = Some(why),
            }
            changed = true;
        }
        if let Some(m) = &self.measuring {
            if let Ok(result) = m.done.try_recv() {
                self.measuring = None;
                // Stopped: nothing said (it can be asked again).
                self.measured = match result {
                    Err(why) if why == remote::RemoteError::Cancelled.to_string() => None,
                    other => Some(other),
                };
                // Counted for a recursive change: asked now (or back to the edits).
                if matches!(self.phase, Phase::Counting) {
                    self.phase = match &self.measured {
                        Some(Ok(m)) => Phase::Confirm(self.items_with(*m)),
                        _ => Phase::Editing,
                    };
                }
            }
            changed = true;
        }
        if let Phase::Applying(a) = &self.phase {
            if let Ok(result) = a.done.try_recv() {
                self.applied(result);
            }
            changed = true;
        }
        changed | self.sums.poll()
    }

    /// Reading, measuring, applying or summing: the loader's pace.
    fn is_loading(&self) -> bool {
        self.loading.is_some()
            || self.measuring.is_some()
            || matches!(self.phase, Phase::Applying(_))
            || self.sums.is_running()
    }

    fn take_notifications(&mut self) -> Vec<workbench_model::Notification> {
        std::mem::take(&mut self.notes)
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        let ViewerPaint { bounds, scale, text, scene, palette, icons, focused, .. } = ctx;
        let ds = Ctx::new(palette, scale);
        self.hits.clear();
        self.anchors.clear();
        // (The footer — its keys, its buttons — is the window's: `modal_buttons`.)
        let layer = scene.push_layer(Some(bounds));
        self.paint_body(layer, text, icons, &ds, bounds, focused);
        // The open list, over everything.
        if let Some(anchor) =
            self.picking.and_then(|c| self.anchors.iter().find(|(a, _)| *a == c)).map(|(_, r)| *r)
        {
            let search = t("remote.props_find_account");
            // (The algorithms are few: no search.)
            self.picker.paint(scene, text, icons, &ds, (anchor, bounds), &search);
        }
    }

    fn press(&mut self, p: Point, clicks: u32) -> Option<ViewerAction> {
        // An open list takes the click (outside it, it closes).
        if self.picker.is_open() {
            let event = self.picker.press(p);
            self.picked(event);
            return None;
        }
        let mut hit = self.hit(p);
        // Asked or running: only the footer's buttons (nothing changes meanwhile).
        let footer = matches!(hit, Some(Hit::Ok | Hit::Back | Hit::Stop | Hit::Close));
        if !matches!(self.phase, Phase::Editing | Phase::Failed(_)) && !footer {
            hit = None;
        }
        self.typing = matches!(hit, Some(Hit::Octal));
        match hit {
            Some(Hit::Choice(choice)) => self.open_list(choice),
            Some(Hit::Tab(tab)) => self.tab = tab,
            Some(Hit::Algo) => self.open_algos(),
            Some(Hit::Calculate) if self.sums.is_running() => self.sums.stop(),
            Some(Hit::Calculate) => self.calculate(),
            Some(Hit::Copy(i)) => self.copy(i),
            Some(Hit::Bit(bit)) => self.toggle(bit),
            Some(Hit::Octal) => self.octal.press(p.x, clicks),
            Some(Hit::Measure) if self.measuring.is_some() => self.stop_measure(),
            Some(Hit::Measure) => self.measure(),
            Some(Hit::Recursive) => self.recursive = !self.recursive,
            Some(Hit::XOnFolders) => self.x_on_folders = !self.x_on_folders,
            Some(hit @ (Hit::Ok | Hit::Back | Hit::Stop | Hit::Close)) => self.footer(hit),
            None => {}
        }
        self.action.take()
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        self.press(p, 1)
    }

    fn drag(&mut self, p: Point) -> bool {
        if self.typing {
            self.octal.extend_to(p.x);
        }
        self.typing
    }

    fn hover(&mut self, p: Point) -> bool {
        if self.picker.is_open() {
            return self.picker.hover(p);
        }
        let hover = self.hit(p);
        let changed = hover != self.hover;
        self.hover = hover;
        changed
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        // An open list: its own (a hand on its rows); never what's under it.
        if self.picker.is_open() {
            return Some(self.picker.cursor_at(p).unwrap_or(ViewerCursor::Arrow));
        }
        Some(match self.hit(p) {
            Some(Hit::Octal) => ViewerCursor::Text,
            Some(_) => ViewerCursor::Pointer,
            // Nothing to click or select there.
            None => ViewerCursor::Arrow,
        })
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        self.edit_key_with(key, KeyReach::default())
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        if self.picker.is_open() {
            let event = self.picker.key(key, reach);
            self.picked(event);
            return true;
        }
        match key {
            ViewerKey::Escape => self.action = Some(ViewerAction::Close),
            ViewerKey::Enter => match self.phase {
                Phase::Confirm(total) => self.apply(total),
                _ => self.ok(),
            },
            ViewerKey::Tab => self.typing = self.edit.is_some() && !self.typing,
            _ if !self.typing => return false,
            ViewerKey::Left => self.octal.move_left(reach.word, reach.extend),
            ViewerKey::Right => self.octal.move_right(reach.word, reach.extend),
            ViewerKey::Home => self.octal.home(reach.extend),
            ViewerKey::End => self.octal.end(reach.extend),
            ViewerKey::Backspace => {
                self.octal.backspace(reach.word);
                self.octal_typed();
            }
            ViewerKey::Delete => {
                self.octal.delete();
                self.octal_typed();
            }
            _ => return false,
        }
        true
    }

    fn selected_text(&self) -> Option<String> {
        if self.picker.is_open() {
            return self.picker.selected_text();
        }
        let selected = self.octal.selected_text();
        (self.typing && !selected.is_empty()).then(|| selected.to_owned())
    }

    fn command(&mut self, id: &str) -> bool {
        // The footer's buttons (the window draws them).
        if let Some(hit) = FOOTER.iter().find(|(i, _)| *i == id).map(|(_, h)| *h) {
            self.footer(hit);
            return true;
        }
        // An open list's search gets the edit commands (⌘A…).
        if self.picker.is_open() {
            return self.picker.command(id);
        }
        if !self.typing {
            return false;
        }
        match id {
            "select_all" => {
                self.octal.select_all();
                true
            }
            _ => false,
        }
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }
}
