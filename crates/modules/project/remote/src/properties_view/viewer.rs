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
        self.typing() || (self.picker.is_open() && self.picker.searches())
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        self.with_controls(|c, parts| c.scroll_hovered(parts, dx, dy)).is_some()
    }

    fn frame_deadline(&self) -> Option<std::time::Instant> {
        self.controls.next_frame()
    }

    fn text_input(&mut self, typed: &str) -> bool {
        self.with_controls(|c, parts| c.text(parts, typed)).is_some()
    }

    fn paste(&mut self, text: &str) -> bool {
        self.with_controls(|c, parts| c.paste(parts, text.trim())).is_some()
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
        self.octal_area = None;
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
        // What the parts want of the frames, as they were just drawn.
        self.with_controls(|c, parts| c.settle(parts));
    }

    fn press(&mut self, p: Point, clicks: u32) -> Option<ViewerAction> {
        // Its controls first. An open list takes the press (outside it, it closes) as one
        // click (its search selects no word); the octal field takes the caret (while it can
        // be edited); a press elsewhere takes the keys from it.
        let clicks = if self.picker.is_open() { 1 } else { clicks };
        match self.with_controls(|c, parts| c.press(parts, p, clicks)) {
            Some((LIST, _)) => {
                self.list_answered();
                return None;
            }
            Some(_) => return None,
            None => {}
        }
        let mut hit = self.hit(p);
        // Asked or running: only the footer's buttons (nothing changes meanwhile).
        let footer = matches!(hit, Some(Hit::Ok | Hit::Back | Hit::Stop | Hit::Close));
        if !matches!(self.phase, Phase::Editing | Phase::Failed(_)) && !footer {
            hit = None;
        }
        match hit {
            Some(Hit::Choice(choice)) => self.open_list(choice),
            Some(Hit::Tab(tab)) => self.tab = tab,
            Some(Hit::Algo) => self.open_algos(),
            Some(Hit::Calculate) if self.sums.is_running() => self.sums.stop(),
            Some(Hit::Calculate) => self.calculate(),
            Some(Hit::Copy(i)) => self.copy(i),
            Some(Hit::Bit(bit)) => self.toggle(bit),
            Some(Hit::Octal) => {}
            Some(Hit::Measure) if self.measuring.is_some() => self.stop_measure(),
            Some(Hit::Measure) => self.measure(),
            Some(Hit::Recursive) => self.recursive = !self.recursive,
            Some(Hit::XOnFolders) => self.x_on_folders = !self.x_on_folders,
            Some(hit @ (Hit::Ok | Hit::Back | Hit::Stop | Hit::Close)) => self.footer(hit),
            None => {}
        }
        // What the mouse is over may have changed (a list opened under it).
        self.with_controls(|c, parts| c.hover(parts, Some(p)));
        self.action.take()
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        self.press(p, 1)
    }

    fn drag(&mut self, p: Point) -> bool {
        // A control holding the pointer (the list's thumb, its search's selection).
        self.with_controls(|c, parts| c.drag(parts, p)).is_some_and(|(_, moved)| moved)
    }

    fn release(&mut self) -> bool {
        self.with_controls(|c, parts| c.release(parts)).is_some_and(|(_, changed)| changed)
    }

    fn hover(&mut self, p: Point) -> bool {
        // Its controls: an open list takes the mouse over everything.
        let list = self.with_controls(|c, parts| c.hover(parts, Some(p)));
        if self.picker.is_open() {
            return list;
        }
        let hover = self.hit(p);
        let changed = hover != self.hover;
        self.hover = hover;
        changed
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        // A part's own (an open list's: a hand on its rows; the octal field's), and over a
        // part saying none, the arrow: never what's under it.
        if let Some(cursor) = self.controls.cursor_at(p) {
            return Some(cursor);
        }
        if self.controls.hovered().is_some() {
            return Some(ViewerCursor::Arrow);
        }
        Some(match self.hit(p) {
            Some(_) => ViewerCursor::Pointer,
            // Nothing to click or select there.
            None => ViewerCursor::Arrow,
        })
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        self.edit_key_with(key, KeyReach::default())
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        // The part with the keys first (an open list takes them all; the octal field its
        // own, Tab giving it the keys); the dialog's after.
        match self.with_controls(|c, parts| c.key(parts, key, reach)) {
            Some(LIST) => {
                self.list_answered();
                return true;
            }
            Some(_) => return true,
            None => {}
        }
        match key {
            ViewerKey::Escape => self.action = Some(ViewerAction::Close),
            ViewerKey::Enter => match self.phase {
                Phase::Confirm(total) => self.apply(total),
                _ => self.ok(),
            },
            // The field had the keys (or nothing takes them): they're the dialog's again.
            ViewerKey::Tab => self.with_controls(|c, parts| c.focus(parts, None)),
            _ => return false,
        }
        true
    }

    /// Read from `&self`: the focused part's own, lent by its place (an open list has the
    /// focus while it's open).
    fn selected_text(&self) -> Option<String> {
        self.controls.selected_text_with(|i| match i {
            LIST => Some(&self.picker as &dyn Control),
            OCTAL => Some(&self.octal as &dyn Control),
            _ => None,
        })
    }

    fn command(&mut self, id: &str) -> bool {
        // The footer's buttons (the window draws them).
        if let Some(hit) = FOOTER.iter().find(|(i, _)| *i == id).map(|(_, h)| *h) {
            self.footer(hit);
            return true;
        }
        // The part with the keys: an open list's search (⌘A…), the octal field.
        self.with_controls(|c, parts| c.command(parts, id)).is_some()
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }
}
