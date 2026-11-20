//! "Properties…" of files and folders on the server (the module's modal, closed only by ×,
//! Esc or its button), as WinSCP shows them: the icon and the name (several: how many), the
//! location on the server, the size (a folder's: "Unknown" and **Calculate** — measured in
//! the background, a Task Manager operation, stopped by its button, from there or when the
//! dialog closes), the date; the **owner** and the **group** (lists of the server's users and
//! groups, `name [id]`); the permissions as a grid (owner / group / others × read / write /
//! execute, set-UID, set-GID, sticky) and as **octal** (`0755`), each following the other.
//!
//! Several chosen: what they share is shown, what they don't is mixed (a box with a dash, no
//! octal) and stays as each has it unless set.
//!
//! What the server says is read in the background when the dialog opens (one session from
//! the project's pool: one request per file, one listing per folder for the names).

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

use i18n::{t, tr};
use kernel::{Point, Rect};
use remote::application::{Applied, Measured, apply_properties, measure};
use remote::properties::{
    Access, Accounts, Algo, Bit, Capabilities, Class, ModeEdit, Principal, Properties,
    PropertiesChange, Special, Tri, shared,
};
use text_layout::Font;
use ui_components::control::{Control, Controls};
use ui_components::ds::{self, Check, Ctx};
use ui_components::picker::{Picker, PickerColumn, PickerEvent, PickerItem, TableRows};
use ui_components::{
    KeyReach, ModalButton, ModalStatus, Viewer, ViewerAction, ViewerCursor, ViewerKey, ViewerKind,
    ViewerPaint, ViewerProvider,
};
use ui_kit::{TextInput, draw_text};

use crate::session::{self, State};

mod octal;
mod viewer;

pub(crate) mod checksum;

/// The extension of the dialog's view.
pub const EXTENSION: &str = "view-remote-properties";

/// What the dialog is opened with: the remote project, what's chosen (relative to its root,
/// `""`: the root), its queries (the files' icons).
#[derive(Clone)]
pub struct PropertiesData {
    pub project: kernel::ProjectId,
    pub paths: Vec<String>,
    pub queries: Option<Arc<dyn workbench_model::ProjectQueries>>,
}

/// The window opened on `paths` of `project`.
pub(crate) fn open(
    host: &mut dyn workbench_model::WorkbenchHost,
    project: kernel::ProjectId,
    paths: Vec<String>,
) -> bool {
    let data = PropertiesData { project, paths, queries: host.queries() };
    host.open_modal(&i18n::t("remote.props_title"), EXTENSION, &data)
}

/// What a view of the module does for "Properties…" on `paths` (its rows).
pub(crate) fn action(project: kernel::ProjectId, paths: Vec<String>) -> ViewerAction {
    ViewerAction::WithHost(ui_components::WithHost::once(move |host| {
        open(host, project, paths);
    }))
}

#[derive(Debug)]
pub(crate) struct PropertiesProvider;

impl ViewerProvider for PropertiesProvider {
    fn id(&self) -> &'static str {
        "remote.properties"
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(PropertiesView::default()))
    }
}

/// What the server said of the chosen.
#[derive(Debug, Clone, Default)]
pub(crate) struct Loaded {
    /// Those on the server (one only here is left out).
    pub props: Vec<Properties>,
    pub accounts: Accounts,
    pub caps: Capabilities,
}

/// Reads `paths`' properties, the server's accounts and what the session can do.
pub(crate) fn load(
    connector: &dyn remote::application::Connector,
    paths: &[String],
) -> Result<Loaded, String> {
    let mut fs = connector.connect().map_err(|e| e.to_string())?;
    let props: Vec<Properties> =
        fs.properties(paths).map_err(|e| e.to_string())?.into_iter().flatten().collect();
    // The accounts are a help (the lists): without them, those the files have.
    let accounts = fs.accounts().unwrap_or_default().with_seen(
        props.iter().filter_map(|p| p.owner.as_ref()),
        props.iter().filter_map(|p| p.group.as_ref()),
    );
    let props = props.into_iter().map(|p| accounts.naming(p)).collect();
    Ok(Loaded { caps: fs.capabilities(), props, accounts })
}

/// The chosen folders being measured.
struct Measuring {
    stop: Arc<AtomicBool>,
    so_far: Arc<Mutex<Measured>>,
    done: Receiver<Result<Measured, String>>,
}

/// The two lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Choice {
    Owner,
    Group,
}

/// What a list opened from a field chooses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Pick {
    Account(Choice),
    Algo,
}

/// The dialog's tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Tab {
    #[default]
    General,
    Checksum,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Tab(Tab),
    Choice(Choice),
    /// The Checksum tab's algorithm field, its button, a row's copy.
    Algo,
    Calculate,
    Copy(usize),
    Bit(Bit),
    Octal,
    Measure,
    Recursive,
    XOnFolders,
    /// Cancel (Close once it's done).
    Close,
    Ok,
    /// From the confirmation back to the edits.
    Back,
    /// Applying stopped.
    Stop,
}

/// The footer's buttons' ids (their clicks come back as commands) and what each does.
const FOOTER: [(&str, Hit); 4] = [
    ("remote.props.ok", Hit::Ok),
    ("remote.props.back", Hit::Back),
    ("remote.props.stop", Hit::Stop),
    ("remote.props.close", Hit::Close),
];

/// Where applying is.
#[derive(Debug, Default)]
enum Phase {
    #[default]
    Editing,
    /// Recursively: what's in the folders counted first (the measure), then asked.
    Counting,
    /// "N items will change": asked before it runs.
    Confirm(u64),
    Applying(Applying),
    /// Done, some refused: each and why.
    Failed(Vec<(String, String)>),
}

/// The change running in the background.
#[derive(Debug)]
struct Applying {
    stop: Arc<AtomicBool>,
    seen: Arc<std::sync::atomic::AtomicU64>,
    total: u64,
    done: Receiver<Result<Applied, String>>,
}

/// The places of its parts (see [`PropertiesView::with_controls`]).
const LIST: usize = 0;
const OCTAL: usize = 1;

#[derive(Default)]
pub struct PropertiesView {
    data: Option<PropertiesData>,
    state: Option<Arc<State>>,
    /// The project's folder on the server and the connection's name (its root's name).
    root: String,
    name: String,
    /// Where the server is (`user@host/root`), under the title.
    address: String,
    loading: Option<Receiver<Result<Loaded, String>>>,
    loaded: Option<Loaded>,
    failed: Option<String>,
    /// The permissions as edited (all of those whose mode the server says).
    edit: Option<ModeEdit>,
    octal: TextInput,
    /// Where the octal field was drawn (while it can be edited), from the last paint.
    octal_area: Option<Rect>,
    /// The owner and the group chosen (`None`: as they are).
    owner: Option<Principal>,
    group: Option<Principal>,
    /// The list open (which), and where its fields are.
    picking: Option<Pick>,
    picker: Picker,
    /// Its controls' state of use (see [`PropertiesView::with_controls`]).
    controls: Controls,
    anchors: Vec<(Pick, Rect)>,
    tab: Tab,
    /// Where the footer starts (the last paint): the tabs' content ends above it.
    footer_top: f32,
    /// The Checksum tab (the chosen files').
    sums: checksum::Checksums,
    measuring: Option<Measuring>,
    measured: Option<Result<Measured, String>>,
    /// Into the chosen folders, and X on folders where readable.
    recursive: bool,
    x_on_folders: bool,
    phase: Phase,
    notes: Vec<workbench_model::Notification>,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
    action: Option<ViewerAction>,
}

impl std::fmt::Debug for PropertiesView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PropertiesView")
            .field("paths", &self.data.as_ref().map(|d| d.paths.len()))
            .finish_non_exhaustive()
    }
}

impl Drop for PropertiesView {
    fn drop(&mut self) {
        // Closed: a measure still going stops.
        if let Some(m) = &self.measuring {
            m.stop.store(true, Ordering::Relaxed);
        }
    }
}

impl PropertiesView {
    /// Its controls, the topmost first, with their state of use: an open list, the octal
    /// field (its text typed: the boxes follow it).
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let mut typed = false;
        let answer = {
            let mut octal = octal::OctalField {
                input: &mut self.octal,
                area: self.octal_area,
                editable: matches!(self.phase, Phase::Editing | Phase::Failed(_)),
                has_mode: self.edit.is_some(),
                typed: &mut typed,
            };
            let mut parts: [&mut dyn Control; 2] = [&mut self.picker, &mut octal];
            f(&mut self.controls, &mut parts)
        };
        if typed {
            self.octal_typed();
        }
        answer
    }

    /// The octal field has the keys.
    fn typing(&self) -> bool {
        self.controls.focused() == Some(OCTAL)
    }

    /// A list opened: it has the keys (its search, its rows).
    fn focus_list(&mut self) {
        self.with_controls(|c, parts| c.focus(parts, Some(LIST)));
    }

    /// What the open list said: done, and the keys go nowhere once it closed.
    fn list_answered(&mut self) {
        let event = self.picker.take_event();
        self.picked(event);
        if !self.picker.is_open() && self.controls.focused() == Some(LIST) {
            self.with_controls(|c, parts| c.focus(parts, None));
        }
    }

    /// What the server said (once read), with the edits made from it.
    pub(crate) fn take_loaded(&mut self, loaded: Loaded) {
        let modes: Vec<_> = loaded.props.iter().filter_map(|p| p.mode).collect();
        self.edit = (!modes.is_empty()).then(|| ModeEdit::of(modes));
        let files = loaded.props.iter().filter(|p| !p.is_dir).map(|p| p.path.clone());
        self.sums = checksum::Checksums::for_files(files);
        self.loaded = Some(loaded);
        self.sync_octal();
    }

    #[cfg(test)]
    pub(crate) fn edit(&self) -> Option<&ModeEdit> {
        self.edit.as_ref()
    }

    /// The octal field shows the boxes (empty while one is mixed).
    fn sync_octal(&mut self) {
        let text = self.edit.as_ref().and_then(ModeEdit::octal).unwrap_or_default();
        if self.octal.text() != text {
            self.octal = TextInput::new(&text);
        }
    }

    /// A box clicked.
    pub(crate) fn toggle(&mut self, bit: Bit) {
        if let Some(edit) = &mut self.edit {
            edit.toggle(bit);
            self.sync_octal();
        }
    }

    /// Octal typed: the boxes follow once it's a mode.
    pub(crate) fn octal_typed(&mut self) {
        let text = self.octal.text().to_owned();
        if let Some(edit) = &mut self.edit {
            edit.set_octal(&text);
        }
    }

    /// The typed octal isn't a mode (the field marked).
    fn octal_invalid(&self) -> bool {
        let text = self.octal.text().trim();
        !text.is_empty() && remote::properties::Mode::parse_octal(text).is_none()
    }

    fn props(&self) -> &[Properties] {
        self.loaded.as_ref().map_or(&[], |l| l.props.as_slice())
    }

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

    /// The owner or the group as all the chosen have it (none chosen in the dialog).
    fn shared_of(&self, choice: Choice) -> Option<Principal> {
        let of = |p: &Properties| match choice {
            Choice::Owner => p.owner.clone(),
            Choice::Group => p.group.clone(),
        };
        shared(self.props().iter().map(of)).flatten()
    }

    /// The id chosen for the owner or the group, when it differs from what they have.
    fn chosen_id(&self, choice: Choice) -> Option<u32> {
        let chosen = match choice {
            Choice::Owner => self.owner.as_ref(),
            Choice::Group => self.group.as_ref(),
        }?;
        let unchanged = self.shared_of(choice).is_some_and(|s| s.same(chosen));
        if unchanged { None } else { chosen.id }
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

    fn has_folders(&self) -> bool {
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
    fn items_with(&self, m: Measured) -> u64 {
        self.props().len() as u64 + m.files + m.folders
    }

    /// Runs the change in the background (a Task Manager operation, stopped from there or
    /// by its button); `total`: how many it looks at.
    fn apply(&mut self, total: u64) {
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
    fn applied(&mut self, result: Result<Applied, String>) {
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
    fn footer(&mut self, hit: Hit) {
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
    fn stop_measure(&mut self) {
        if let Some(m) = &self.measuring {
            m.stop.store(true, Ordering::Relaxed);
        }
    }

    /// The accounts a list offers.
    fn accounts(&self, choice: Choice) -> &[Principal] {
        let Some(loaded) = &self.loaded else { return &[] };
        match choice {
            Choice::Owner => &loaded.accounts.users,
            Choice::Group => &loaded.accounts.groups,
        }
    }

    /// A list's rows (each account: its name, its id) and the one shown.
    fn options(&self, choice: Choice) -> (Vec<PickerItem>, Option<usize>) {
        let shown = self.value_of(choice);
        let accounts = self.accounts(choice);
        let items = accounts
            .iter()
            .map(|a| {
                let id = a.id.map(|n| n.to_string()).unwrap_or_default();
                let name = a.name.clone().unwrap_or_else(|| a.to_string());
                PickerItem::person(name, "").with_cells(vec![id])
            })
            .collect();
        let chosen = shown.and_then(|s| accounts.iter().position(|a| a.same(&s)));
        (items, chosen)
    }

    /// A list opened from its field.
    fn open_list(&mut self, choice: Choice) {
        let (items, chosen) = self.options(choice);
        // A table: the name, the id in a column of its own (titled, fixed while scrolling).
        let id = match choice {
            Choice::Owner => "UID",
            Choice::Group => "GID",
        };
        let columns = vec![
            PickerColumn { title: t("remote.props_account"), width: 0.0, right: false },
            PickerColumn { title: id.into(), width: 64.0, right: true },
        ];
        self.picker.open_with(items, chosen, Box::new(TableRows::new(columns)));
        self.picking = Some(Pick::Account(choice));
        self.focus_list();
    }

    /// The algorithms' list opened.
    fn open_algos(&mut self) {
        let items = Algo::ALL
            .iter()
            .map(|a| PickerItem { label: a.label().into(), ..PickerItem::default() })
            .collect();
        let chosen = Algo::ALL.iter().position(|a| *a == self.sums.algo());
        self.picker.open(items, chosen);
        self.picking = Some(Pick::Algo);
        self.focus_list();
    }

    /// Each chosen file's digest computed (in the background).
    pub(crate) fn calculate(&mut self) {
        let Some(state) = self.state.clone() else { return };
        self.sums.start(Arc::clone(&state.connector), state.saved.local.clone(), state.wake());
    }

    /// A row's digest on the clipboard.
    fn copy(&mut self, i: usize) {
        let Some((_, checksum::Sum::Done { sum, .. })) = self.sums.rows.get(i) else { return };
        let request = ui_components::AppRequest::CopyText(sum.hex.clone());
        self.action = Some(ViewerAction::Request(request));
        let note = tr!("remote.props_copied", algo = sum.algo.label());
        self.notes.push(workbench_model::Notification::info(note));
    }

    /// What the open list said.
    fn picked(&mut self, event: Option<PickerEvent>) {
        match (event, self.picking) {
            (Some(PickerEvent::Chosen(at)), Some(Pick::Account(choice))) => self.choose(choice, at),
            (Some(PickerEvent::Chosen(at)), Some(Pick::Algo)) => {
                if let Some(algo) = Algo::ALL.get(at) {
                    self.sums.choose(*algo);
                }
                self.picking = None;
            }
            (Some(_), _) => self.picking = None,
            (None, _) => {}
        }
    }

    /// The owner or the group shown: the one chosen, else the one all have (`None`: they
    /// differ, or the server doesn't say).
    fn value_of(&self, choice: Choice) -> Option<Principal> {
        let chosen = match choice {
            Choice::Owner => &self.owner,
            Choice::Group => &self.group,
        };
        chosen.clone().or_else(|| {
            let of = |p: &Properties| match choice {
                Choice::Owner => p.owner.clone(),
                Choice::Group => p.group.clone(),
            };
            shared(self.props().iter().map(of)).flatten()
        })
    }

    /// What a list's field says.
    fn label_of(&self, choice: Choice) -> String {
        if let Some(value) = self.value_of(choice) {
            return value.to_string();
        }
        let known = self.props().iter().any(|p| match choice {
            Choice::Owner => p.owner.is_some(),
            Choice::Group => p.group.is_some(),
        });
        if known { t("remote.props_mixed") } else { "—".into() }
    }

    /// The server lets it change (this protocol, this session).
    fn allowed(&self, choice: Choice) -> bool {
        self.loaded.as_ref().is_some_and(|l| match choice {
            Choice::Owner => l.caps.chown,
            Choice::Group => l.caps.chgrp,
        })
    }

    fn may_change(&self, choice: Choice) -> bool {
        self.allowed(choice) && !self.accounts(choice).is_empty()
    }

    fn choose(&mut self, choice: Choice, at: usize) {
        let picked = self.accounts(choice).get(at).cloned();
        match choice {
            Choice::Owner => self.owner = picked,
            Choice::Group => self.group = picked,
        }
        self.picking = None;
        self.picker.close();
    }

    fn hit(&self, p: Point) -> Option<Hit> {
        self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)
    }

    /// The title's subject: the one chosen's name (the project's for its root), else how many.
    fn subject(&self) -> String {
        let Some(data) = &self.data else { return String::new() };
        match data.paths.as_slice() {
            [one] if one.is_empty() => self.name.clone(),
            [one] => one.rsplit('/').next().unwrap_or(one).to_owned(),
            many => tr!("remote.props_items", count = many.len()),
        }
    }

    /// Where the chosen are on the server: their folder (several: theirs, if they share it).
    fn location(&self) -> String {
        let root = self.root.trim_end_matches('/');
        let parents =
            self.data.iter().flat_map(|d| d.paths.iter()).map(|p| match p.rsplit_once('/') {
                Some((dir, _)) => format!("{root}/{dir}"),
                None if p.is_empty() => root.rsplit_once('/').map_or("/".into(), |(d, _)| {
                    if d.is_empty() { "/".into() } else { d.to_owned() }
                }),
                None => {
                    if root.is_empty() {
                        "/".into()
                    } else {
                        root.to_owned()
                    }
                }
            });
        shared(parents).unwrap_or_else(|| t("remote.props_mixed"))
    }

    /// The size said: a file's, the files' together, the folders' once measured.
    fn size_text(&self) -> String {
        let props = self.props();
        let files: u64 = props.iter().filter(|p| !p.is_dir).map(|p| p.size).sum();
        let has_files = props.iter().any(|p| !p.is_dir);
        let has_folders = props.iter().any(|p| p.is_dir);
        if let Some(m) = &self.measuring {
            let so_far = m.so_far.lock().map(|m| *m).unwrap_or_default();
            return tr!(
                "remote.props_measuring",
                size = i18n::bytes(files + so_far.bytes),
                files = so_far.files
            );
        }
        match &self.measured {
            Some(Ok(m)) => tr!(
                "remote.props_measured",
                size = i18n::bytes(files + m.bytes),
                files = m.files + props.iter().filter(|p| !p.is_dir).count() as u64,
                folders = m.folders
            ),
            Some(Err(why)) => tr!("remote.props_measure_failed", why = why.as_str()),
            None if has_folders && has_files => {
                tr!("remote.props_size_and_folders", size = i18n::bytes(files))
            }
            None if has_folders => t("remote.props_size_unknown"),
            None => i18n::bytes(files),
        }
    }

    /// The date said: the one all have, else the newest.
    fn modified_text(&self) -> String {
        let props = self.props();
        let Some(newest) = props.iter().map(|p| p.modified).max() else { return "—".into() };
        if newest <= 0 {
            return "—".into();
        }
        let when = crate::local_time::stamp(newest);
        match shared(props.iter().map(|p| p.modified)) {
            Some(_) => when,
            None => tr!("remote.props_newest", when = when.as_str()),
        }
    }

    /// The header's icon: a folder's, a file's by its language, several's.
    fn icon(&self) -> &'static str {
        match self.props() {
            [one] if one.is_dir => "folder",
            [one] => self.data.as_ref().and_then(|d| d.queries.as_ref()).map_or("file", |q| {
                q.file_icon(one.path.rsplit('/').next().unwrap_or(&one.path)).0
            }),
            _ => "layers",
        }
    }
}

/// The dialog's size (logical pixels).
const WIDTH: f32 = 560.0;
const HEIGHT: f32 = 600.0;
/// A label's column.
const LABEL_W: f32 = 112.0;
const ROW_H: f32 = 30.0;

impl PropertiesView {
    fn paint_body(
        &mut self,
        layer: &mut render_api::Layer,
        text: &mut text_layout::TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        b: Rect,
        focused: bool,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let pad = px(20.0);
        let x0 = b.min_x() + pad;
        let x1 = b.max_x() - pad;
        self.footer_top = b.max_y();
        let body = b;
        if let Phase::Failed(failed) = &self.phase {
            paint_failures(layer, text, ds, body, failed);
            return;
        }
        if let Some(why) = &self.failed {
            ds::empty_state(layer, text, ds, body, &t("remote.props_failed"), Some(why));
            return;
        }
        if self.loaded.is_none() {
            let reading = t("remote.props_reading");
            let loader = ds::Loader::with_text(&reading);
            let w = loader.width(text, ds);
            let mid = body.min_y() + body.size.height / 2.0;
            ds::loader(layer, text, ds, body.min_x() + (body.size.width - w) / 2.0, mid, &loader);
            return;
        }
        let font = ds.ui_font(-0.5);
        let strong = ds.ui_font(0.5).with_weight(Font::SEMIBOLD);
        let muted = p.text_muted;
        // The header: the icon, the name, how many.
        let mut y = b.min_y() + px(12.0);
        let s = px(28.0);
        if let Some(id) = icons.id(self.icon()) {
            ui_kit::draw_icon(layer, icons, id, Rect::new(x0, y, s, s), p.accent);
        }
        // On the right: the tabs (the Checksum one only for files).
        let mut name_room = x1 - x0 - s - px(12.0);
        if !self.sums.rows.is_empty() {
            let labels = [t("remote.props_tab_general"), t("remote.props_tab_checksum")];
            let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
            let hovered = match self.hover {
                Some(Hit::Tab(Tab::General)) => Some(0),
                Some(Hit::Tab(Tab::Checksum)) => Some(1),
                _ => None,
            };
            let selected = usize::from(self.tab == Tab::Checksum);
            let mid = y + s / 2.0;
            let spec = ds::SegmentedSpec { right: x1, mid, labels: &labels, selected, hovered };
            let tabs = ds::segmented(layer, text, ds, spec);
            for (r, tab) in tabs.iter().zip([Tab::General, Tab::Checksum]) {
                self.hits.push((*r, Hit::Tab(tab)));
            }
            name_room =
                tabs.first().map_or(name_room, |r| r.min_x() - px(16.0) - x0 - s - px(12.0));
        }
        let name = ds::ellipsize_file(text, &self.subject(), &strong, name_room.max(0.0));
        let shaped = text.shape_line(&name, &strong);
        draw_text(layer, &shaped, x0 + s + px(12.0), ds::optical_baseline(y, s, &strong), p.text);
        y += s + px(14.0);
        ds::divider(layer, ds, x0, x1, y);
        y += px(8.0);
        if self.tab == Tab::Checksum {
            self.paint_checksums(
                &mut ui_components::LayerPainter::new(layer, text, icons, *ds),
                (x0, x1),
                y,
            );
            return;
        }
        let vx = x0 + px(LABEL_W);
        let row = |layer: &mut render_api::Layer,
                   text: &mut text_layout::TextSystem,
                   y: f32,
                   label: &str| {
            let shaped = text.shape_line(label, &font);
            draw_text(layer, &shaped, x0, ds::optical_baseline(y, px(ROW_H), &font), muted);
        };
        let value = |layer: &mut render_api::Layer,
                     text: &mut text_layout::TextSystem,
                     y: f32,
                     v: &str,
                     right: f32| {
            let shown = ds::ellipsize_middle(text, v, &font, right - vx);
            let shaped = text.shape_line(&shown, &font);
            draw_text(layer, &shaped, vx, ds::optical_baseline(y, px(ROW_H), &font), p.text);
        };
        // Where, how big, when.
        row(layer, text, y, &t("remote.props_location"));
        value(layer, text, y, &self.location(), x1);
        y += px(ROW_H);
        row(layer, text, y, &t("remote.props_size"));
        let has_folders = self.props().iter().any(|p| p.is_dir);
        let mut right = x1;
        if has_folders {
            let label = if self.measuring.is_some() {
                t("remote.props_measure_stop")
            } else {
                t("remote.props_measure")
            };
            let w = ds::primary_width(text, ds, &[&label]);
            let h = px(26.0);
            let r = Rect::new(x1 - w, y + (px(ROW_H) - h) / 2.0, w, h);
            ds::secondary_button(
                layer,
                text,
                ds,
                r,
                &label,
                true,
                self.hover == Some(Hit::Measure),
            );
            self.hits.push((r, Hit::Measure));
            right = r.min_x() - px(8.0);
            if self.measuring.is_some() {
                let loader = ds::Loader { size: 12.0, ..ds::Loader::default() };
                ds::loader(layer, text, ds, right - px(12.0), y + px(ROW_H) / 2.0, &loader);
                right -= px(20.0);
            }
        }
        value(layer, text, y, &self.size_text(), right);
        y += px(ROW_H);
        row(layer, text, y, &t("remote.props_modified"));
        value(layer, text, y, &self.modified_text(), x1);
        y += px(ROW_H) + px(6.0);
        ds::divider(layer, ds, x0, x1, y);
        y += px(10.0);
        // The owner and the group.
        for choice in [Choice::Owner, Choice::Group] {
            let key = match choice {
                Choice::Owner => "remote.props_owner",
                Choice::Group => "remote.props_group",
            };
            row(layer, text, y, &t(key));
            // As wide as the row: its list opens as wide.
            let r = Rect::new(vx, y, x1 - vx, px(ROW_H));
            let enabled = self.may_change(choice);
            let not_allowed = t("remote.props_not_allowed");
            let spec = ds::DropdownSpec {
                value: &self.label_of(choice),
                open: self.picking == Some(Pick::Account(choice)),
                hovered: self.hover == Some(Hit::Choice(choice)),
                enabled,
                // Why it can't be chosen.
                note: (!self.allowed(choice)).then_some(not_allowed.as_str()),
                placeholder: None,
            };
            ds::dropdown(layer, text, icons, ds, r, spec);
            if enabled {
                self.hits.push((r, Hit::Choice(choice)));
            }
            self.anchors.push((Pick::Account(choice), r));
            y += px(ROW_H) + px(8.0);
        }
        y += px(4.0);
        ds::divider(layer, ds, x0, x1, y);
        y += px(12.0);
        // The permissions: the grid, the special bits, the octal.
        let Some(edit) = self.edit else {
            let note = t("remote.props_no_mode");
            let note = ds::ellipsize(text, &note, &font, x1 - x0);
            let shaped = text.shape_line(&note, &font);
            draw_text(layer, &shaped, x0, ds::optical_baseline(y, px(ROW_H), &font), muted);
            self.paint_recursive(layer, text, ds, x0, y + px(ROW_H) + px(10.0));
            return;
        };
        let col = px(72.0);
        let heads = ["remote.props_read", "remote.props_write", "remote.props_execute"];
        for (i, key) in heads.iter().enumerate() {
            let shaped = text.shape_line(&t(key), &font);
            let cx = vx + col * i as f32;
            draw_text(layer, &shaped, cx, ds::optical_baseline(y, px(22.0), &font), muted);
        }
        y += px(24.0);
        let classes = [
            (Class::Owner, "remote.props_class_owner"),
            (Class::Group, "remote.props_class_group"),
            (Class::Others, "remote.props_class_others"),
        ];
        let check = |state: Tri| match state {
            Tri::On => Check::On,
            Tri::Off => Check::Off,
            Tri::Mixed => Check::Mixed,
        };
        let enabled = self.loaded.as_ref().is_some_and(|l| l.caps.chmod);
        // Each class's row, its special bit on its right (as WinSCP lays them).
        let specials = [
            (Special::SetUid, "remote.props_setuid"),
            (Special::SetGid, "remote.props_setgid"),
            (Special::Sticky, "remote.props_sticky"),
        ];
        let sx = vx + col * 3.0 + px(16.0);
        for ((class, key), (special, special_key)) in classes.into_iter().zip(specials) {
            row(layer, text, y, &t(key));
            let mid = y + px(ROW_H) / 2.0;
            for (i, access) in Access::ALL.into_iter().enumerate() {
                let bit = Bit::Access(class, access);
                let x = vx + col * i as f32;
                let r = ds::checkbox(layer, text, ds, x, mid, check(edit.state(bit)), enabled);
                let area = Rect::new(r.min_x() - px(6.0), y, px(36.0), px(ROW_H));
                if enabled {
                    self.hits.push((area, Hit::Bit(bit)));
                }
            }
            let bit = Bit::Special(special);
            let r = ds::checkbox(layer, text, ds, sx, mid, check(edit.state(bit)), enabled);
            let shaped = text.shape_line(&t(special_key), &font);
            let lx = r.max_x() + px(8.0);
            draw_text(layer, &shaped, lx, ds::optical_baseline(y, px(ROW_H), &font), p.text);
            if enabled {
                self.hits
                    .push((Rect::new(sx, y, lx + shaped.width - sx, px(ROW_H)), Hit::Bit(bit)));
            }
            y += px(ROW_H);
        }
        y += px(8.0);
        row(layer, text, y, &t("remote.props_octal"));
        let field = Rect::new(vx, y, px(90.0), px(ROW_H));
        let style = ds::field_style(ds, ds.code_font(-0.5), self.octal_invalid());
        let placeholder =
            if edit.octal().is_none() { t("remote.props_mixed") } else { String::new() };
        self.octal.paint(layer, text, field, self.typing() && focused, &placeholder, &style);
        if enabled {
            self.hits.push((field, Hit::Octal));
            self.octal_area = Some(field);
        }
        // Beside it: as `ls -l` writes it, or why the octal isn't taken.
        let (note, color) = if self.octal_invalid() {
            (t("remote.props_octal_bad"), p.danger)
        } else {
            (edit.whole().map(|m| m.symbolic()).unwrap_or_default(), muted)
        };
        let small = ds.code_font(-0.5);
        let nx = field.max_x() + px(12.0);
        let note = ds::ellipsize(text, &note, &small, x1 - nx);
        let shaped = text.shape_line(&note, &small);
        draw_text(layer, &shaped, nx, ds::optical_baseline(y, px(ROW_H), &small), color);
        self.paint_recursive(layer, text, ds, x0, y + px(ROW_H) + px(12.0));
    }

    /// With a folder chosen: "Apply recursively" and "Add X to folders" (only then).
    fn paint_recursive(
        &mut self,
        layer: &mut render_api::Layer,
        text: &mut text_layout::TextSystem,
        ds: &Ctx<'_>,
        x0: f32,
        y: f32,
    ) {
        if !self.has_folders() {
            return;
        }
        let px = |v: f32| ds.px(v);
        let font = ds.ui_font(-0.5);
        let mut x = x0;
        let boxes = [
            (Hit::Recursive, self.recursive, true, "remote.props_recursive"),
            (Hit::XOnFolders, self.x_on_folders, self.recursive, "remote.props_x_on_folders"),
        ];
        for (hit, on, enabled, key) in boxes {
            let mid = y + px(ROW_H) / 2.0;
            let state = if on && enabled { Check::On } else { Check::Off };
            let r = ds::checkbox(layer, text, ds, x, mid, state, enabled);
            let shaped = text.shape_line(&t(key), &font);
            let lx = r.max_x() + px(8.0);
            let color = if enabled { ds.palette.text } else { ds.palette.text_muted };
            draw_text(layer, &shaped, lx, ds::optical_baseline(y, px(ROW_H), &font), color);
            let area = Rect::new(x, y, lx + shaped.width - x, px(ROW_H));
            if enabled {
                self.hits.push((area, hit));
            }
            x = area.max_x() + px(24.0);
        }
    }
}

/// What the server refused, each and why (a lost connection: why only).
fn paint_failures(
    layer: &mut render_api::Layer,
    text: &mut text_layout::TextSystem,
    ds: &Ctx<'_>,
    body: Rect,
    failed: &[(String, String)],
) {
    let px = |v: f32| ds.px(v);
    let p = ds.palette;
    let x0 = body.min_x() + px(20.0);
    let width = body.size.width - px(40.0);
    let title = tr!("remote.props_refused", count = failed.len());
    let mut y = ds::section_title(layer, text, ds, x0, body.min_y() + px(16.0), &title, None);
    y += px(10.0);
    let mono = ds.code_font(-1.0);
    let small = ds.ui_font(-1.0);
    let line_h = px(40.0);
    let fits = ((body.max_y() - y) / line_h).floor().max(1.0) as usize;
    let shown = if failed.len() > fits { fits - 1 } else { failed.len() };
    for (path, why) in &failed[..shown] {
        if !path.is_empty() {
            let path = ds::ellipsize_path(text, &format!("/{path}"), &mono, width);
            let shaped = text.shape_line(&path, &mono);
            draw_text(layer, &shaped, x0, ds::optical_baseline(y, px(20.0), &mono), p.text);
        }
        let why = ds::ellipsize(text, why, &small, width);
        let shaped = text.shape_line(&why, &small);
        draw_text(
            layer,
            &shaped,
            x0,
            ds::optical_baseline(y + px(18.0), px(20.0), &small),
            p.danger,
        );
        y += line_h;
    }
    if shown < failed.len() {
        let more = tr!("remote.props_more", count = failed.len() - shown);
        let shaped = text.shape_line(&more, &small);
        draw_text(layer, &shaped, x0, ds::optical_baseline(y, px(20.0), &small), p.text_muted);
    }
}

/// The app's `--snapshot` (`REMOTE_PROPS`): the dialog over made-up files (no server).
#[cfg(feature = "snapshot")]
const SAMPLE: &str = "\u{0}sample:";

/// Opens the dialog over a sample: `file` (one), `several` (mixed), `folder` (measured),
/// `list` (the groups' list open), `confirm` (a recursive change asked), `failed` (what the
/// server refused), `checksum` (the Checksum tab, two files computed), `algos` (its
/// algorithms' list open).
#[cfg(feature = "snapshot")]
pub fn snapshot(host: &mut dyn workbench_model::WorkbenchHost, which: &str) -> bool {
    let data = PropertiesData {
        project: kernel::ProjectId::new(),
        paths: vec![format!("{SAMPLE}{which}")],
        queries: host.queries(),
    };
    host.open_modal(&t("remote.props_title"), EXTENSION, &data)
}

#[cfg(feature = "snapshot")]
mod sample {
    use super::*;
    use remote::properties::Mode;

    pub fn paths(which: &str) -> Vec<String> {
        match which {
            "several" | "checksum" | "algos" => {
                vec!["public/index.php".into(), "public/app.js".into(), "public/img".into()]
            }
            "folder" | "confirm" | "failed" => vec!["storage".into()],
            _ => vec!["public/index.php".into()],
        }
    }

    pub fn loaded(which: &str) -> Loaded {
        let www = Principal::named("www-data", Some(33));
        let deploy = Principal::named("deploy", Some(1000));
        let props = paths(which)
            .into_iter()
            .enumerate()
            .map(|(i, path)| {
                let is_dir = !path.contains('.');
                Properties {
                    is_dir,
                    size: if is_dir { 4096 } else { 18_432 * (i as u64 + 1) },
                    modified: 1_762_425_000 - i as i64 * 3600,
                    mode: Some(Mode::new(match (is_dir, i) {
                        (true, _) => 0o2775,
                        (false, 0) => 0o644,
                        _ => 0o664,
                    })),
                    owner: Some(if i == 2 { www.clone() } else { deploy.clone() }),
                    group: Some(www.clone()),
                    path,
                }
            })
            .collect();
        Loaded {
            props,
            accounts: Accounts {
                users: vec![deploy, Principal::named("root", Some(0)), www.clone()],
                groups: ["adm", "audio", "backup", "cdrom", "crontab", "deploy", "staff", "sudo"]
                    .iter()
                    .zip([4, 29, 34, 24, 112, 1000, 50, 27])
                    .map(|(n, id)| Principal::named(*n, Some(id)))
                    .chain([www])
                    .collect(),
            },
            caps: Capabilities {
                chmod: true,
                chown: false,
                chgrp: true,
                server_hashes: Vec::new(),
            },
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/properties_view.rs"]
mod tests;
