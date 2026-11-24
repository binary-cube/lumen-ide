//! A folder's size measured in the background (a Task Manager operation, stopped by its
//! button), the change made of the dialog, and OK applying it (recursively when asked).

use super::*;

impl PropertiesView {
    /// The chosen folders, measured together in the background.
    pub(crate) fn measure(&mut self) {
        let (Some(state), None) = (self.state.clone(), &self.measuring) else { return };
        let folders: Vec<String> =
            self.props().iter().filter(|p| p.is_dir).map(|p| p.path.clone()).collect();
        if folders.is_empty() {
            return;
        }
        use workbench_model::{ActivityKind, ActivityUnit, Outcome};
        let subject = match folders.as_slice() {
            [one] => format!("/{one}"),
            many => tr!("remote.props_items", count = many.len()),
        };
        let id = state.activities.start(
            Some(state.project),
            ActivityKind::Measure,
            subject,
            ActivityUnit::Files,
            None,
        );
        let stop = Arc::new(AtomicBool::new(false));
        state.activities.stoppable(id, Arc::clone(&stop));
        let so_far = Arc::new(Mutex::new(Measured::default()));
        let (tx, done) = std::sync::mpsc::channel();
        let (flag, told) = (Arc::clone(&stop), Arc::clone(&so_far));
        kernel::work::run("remote-measure", move || {
            let wake = state.wake();
            let result = state.connector.connect().and_then(|mut fs| {
                let mut total = Measured::default();
                let mut last_wake = std::time::Instant::now();
                for folder in &folders {
                    let base = total;
                    let counted =
                        measure(fs.as_mut(), folder, &|| flag.load(Ordering::Relaxed), &mut |m| {
                            let now = Measured {
                                bytes: base.bytes + m.bytes,
                                files: base.files + m.files,
                                folders: base.folders + m.folders,
                            };
                            let before = told.lock().map(|t| t.files).unwrap_or(now.files);
                            state.activities.advance(id, now.files.saturating_sub(before));
                            if let Ok(mut t) = told.lock() {
                                *t = now;
                            }
                            // A redraw now and then, not per folder.
                            if last_wake.elapsed() > std::time::Duration::from_millis(150) {
                                last_wake = std::time::Instant::now();
                                wake();
                            }
                        })?;
                    total = Measured {
                        bytes: base.bytes + counted.bytes,
                        files: base.files + counted.files,
                        folders: base.folders + counted.folders,
                    };
                }
                Ok(total)
            });
            let outcome = match &result {
                Ok(_) => Outcome::Done,
                Err(remote::RemoteError::Cancelled) => Outcome::Stopped,
                Err(e) => Outcome::Failed(e.to_string()),
            };
            state.activities.end(id, outcome);
            let _ = tx.send(result.map_err(|e| e.to_string()));
            wake();
        });
        self.measured = None;
        self.measuring = Some(Measuring { stop, so_far, done });
    }

    /// What OK applies (`None`: nothing changed). Recursively, the permissions shown go
    /// down the folders even when they weren't changed here.
    pub(crate) fn change(&self) -> Option<PropertiesChange> {
        if self.octal_invalid() {
            return None;
        }
        let recursive = self.recursive && self.has_folders();
        let mode = self.edit.filter(|e| e.is_changed() || recursive);
        let change = PropertiesChange {
            mode,
            owner: self.chosen_id(Choice::Owner),
            group: self.chosen_id(Choice::Group),
            recursive,
            x_on_folders: recursive && self.x_on_folders,
        };
        let any = change.mode.is_some() || change.owner.is_some() || change.group.is_some();
        any.then_some(change)
    }

    pub(super) fn has_folders(&self) -> bool {
        self.props().iter().any(|p| p.is_dir)
    }

    /// OK: applied at once, or (recursively) what's in the folders counted and asked first.
    pub(crate) fn ok(&mut self) {
        let Some(change) = self.change() else { return };
        if !matches!(self.phase, Phase::Editing) {
            return;
        }
        if !change.recursive {
            self.apply(self.props().len() as u64);
            return;
        }
        match &self.measured {
            Some(Ok(m)) => self.phase = Phase::Confirm(self.items_with(*m)),
            _ => {
                self.phase = Phase::Counting;
                if self.measuring.is_none() {
                    self.measure();
                }
            }
        }
    }

    /// How many a recursive change looks at: the chosen and all in their folders.
    pub(super) fn items_with(&self, m: Measured) -> u64 {
        self.props().len() as u64 + m.files + m.folders
    }

    /// Runs the change in the background (a Task Manager operation, stopped from there or
    /// by its button); `total`: how many it looks at.
    pub(super) fn apply(&mut self, total: u64) {
        let (Some(state), Some(change)) = (self.state.clone(), self.change()) else { return };
        use workbench_model::{ActivityKind, ActivityUnit, Outcome};
        let targets = self.props().to_vec();
        let subject = match targets.as_slice() {
            [one] => format!("/{}", one.path),
            many => tr!("remote.props_items", count = many.len()),
        };
        let id = state.activities.start(
            Some(state.project),
            ActivityKind::Permissions,
            subject,
            ActivityUnit::Files,
            Some(total),
        );
        let stop = Arc::new(AtomicBool::new(false));
        state.activities.stoppable(id, Arc::clone(&stop));
        let seen = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let (tx, done) = std::sync::mpsc::channel();
        let (flag, counted) = (Arc::clone(&stop), Arc::clone(&seen));
        kernel::work::run("remote-permissions", move || {
            let wake = state.wake();
            let mut last_wake = std::time::Instant::now();
            let result = state.connector.connect().and_then(|mut fs| {
                apply_properties(
                    fs.as_mut(),
                    &targets,
                    &change,
                    &|| flag.load(Ordering::Relaxed),
                    &mut |n| {
                        let before = counted.swap(n, Ordering::Relaxed);
                        state.activities.advance(id, n.saturating_sub(before));
                        if last_wake.elapsed() > std::time::Duration::from_millis(150) {
                            last_wake = std::time::Instant::now();
                            wake();
                        }
                    },
                )
            });
            let outcome = match &result {
                Ok(a) if a.stopped => Outcome::Stopped,
                Ok(a) if !a.failed.is_empty() => {
                    Outcome::Failed(tr!("remote.props_refused", count = a.failed.len()))
                }
                Ok(_) => Outcome::Done,
                Err(e) => Outcome::Failed(e.to_string()),
            };
            state.activities.end(id, outcome);
            let _ = tx.send(result.map_err(|e| e.to_string()));
            wake();
        });
        self.phase = Phase::Applying(Applying { stop, seen, total, done });
    }

    /// What applying did: done (it closes, said), stopped, or the refusals listed.
    pub(super) fn applied(&mut self, result: Result<Applied, String>) {
        use workbench_model::Notification;
        // What the tree shows of the server's permissions read again.
        if let Some(state) = &self.state {
            state.forget_all_permissions();
        }
        match result {
            Ok(a) if a.failed.is_empty() => {
                let note = if a.stopped {
                    Notification::info(tr!("remote.props_stopped", count = a.changed))
                } else {
                    Notification::success(tr!("remote.props_applied", count = a.changed))
                };
                self.notes.push(note);
                self.action = Some(ViewerAction::Close);
                self.phase = Phase::Editing;
            }
            Ok(a) => self.phase = Phase::Failed(a.failed),
            Err(why) => self.phase = Phase::Failed(vec![(String::new(), why)]),
        }
    }

    /// A footer's button: OK (Apply once asked), Back, Stop, Cancel / Close.
    pub(super) fn footer(&mut self, hit: Hit) {
        match hit {
            Hit::Ok => match self.phase {
                Phase::Confirm(total) => self.apply(total),
                _ => self.ok(),
            },
            Hit::Back => {
                self.stop_measure();
                self.phase = Phase::Editing;
            }
            Hit::Stop => {
                if let Phase::Applying(a) = &self.phase {
                    a.stop.store(true, Ordering::Relaxed);
                }
            }
            Hit::Close => self.action = Some(ViewerAction::Close),
            _ => {}
        }
    }

    /// The measure stopped (by its button).
    pub(super) fn stop_measure(&mut self) {
        if let Some(m) = &self.measuring {
            m.stop.store(true, Ordering::Relaxed);
        }
    }

    /// Each chosen file's digest computed (in the background).
    pub(crate) fn calculate(&mut self) {
        let Some(state) = self.state.clone() else { return };
        self.sums.start(Arc::clone(&state.connector), state.saved.local.clone(), state.wake());
    }
}
