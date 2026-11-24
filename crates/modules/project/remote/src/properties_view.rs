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

mod applying;
mod lists;
mod octal;
mod paint;
mod viewer;

pub(crate) mod checksum;

use paint::{HEIGHT, LABEL_W, ROW_H, WIDTH};

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

    /// A row's digest on the clipboard.
    fn copy(&mut self, i: usize) {
        let Some((_, checksum::Sum::Done { sum, .. })) = self.sums.rows.get(i) else { return };
        let request = ui_components::AppRequest::CopyText(sum.hex.clone());
        self.action = Some(ViewerAction::Request(request));
        let note = tr!("remote.props_copied", algo = sum.algo.label());
        self.notes.push(workbench_model::Notification::info(note));
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
