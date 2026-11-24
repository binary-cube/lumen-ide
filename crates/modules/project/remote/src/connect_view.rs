//! "Open Remote Project…", a wizard (a tab) in three steps:
//!
//! 1. **The connection**: protocol, server, sign-in (the password masked, an eye to show
//!    it). "Continue" connects — an unknown server's key / certificate shows its
//!    fingerprint for the user to trust first.
//! 2. **The folder on the server**: browsed from where the user lands (up, into a folder),
//!    the files shown dimmed to recognize the project; "Choose this folder".
//! 3. **What comes down**: the project's tree with checkboxes (unchecked: never transferred;
//!    folders open on demand), the exclusions listed with their full paths; the name, the
//!    local folder, where the connection and its secret are kept. "Connect and download":
//!    the project comes down in parallel (its progress shown), the connection is saved and
//!    the local copy opens.
//!
//! Browsing keeps one session open on its own thread (`remote-browse`).
//!
//! **Editing** a remote project's connection ([`EditConnection`]) is the same form,
//! prefilled with what's saved: every field editable but the project's folder (its local
//! copy, open as the project: shown with why it stays); the password never shown — left
//! empty, the kept one stays (the keychain, the vault). "Test Connection" signs in and says
//! how it went, "Continue" browses from the saved folder, "Save" keeps it all (from any
//! step) and reconnects the project if it's open, what still transfers asked about first.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender};

use i18n::{t, tr};
use kernel::{Point, Rect, Rgba};
use remote::application::{Connector, Direction, Run, apply, list_remote, plan};
use remote::manifest::Manifest;
use remote::{
    Auth, Connection, Mode, Protocol, RemoteEntry, RemoteError, SavedIn, Secret, SecretStore,
    SecretsIn, WriteAs,
};
use render_api::Quad;
use text_layout::TextSystem;
use ui_components::control::{Control, Controls, Thumb};
use ui_components::ds::{self, Check, Ctx, Tone, radius};
use ui_components::{
    AppRequest, Viewer, ViewerAction, ViewerCursor, ViewerKey, ViewerKind, ViewerPaint,
    ViewerProvider,
};
use ui_kit::{TextInput, TextInputStyle, draw_text};

use crate::config::{self, Saved};
use crate::paint::{Column, Pen};
use crate::session;

mod browser;
mod fields;
mod owner;
mod paint;
mod steps;
mod suggestions;
mod transfer;
mod view;
use owner::{Account, ServerAccounts};
use transfer::*;

/// The extension of the wizard's tab.
pub const EXTENSION: &str = "view-remote-connect";

#[derive(Debug)]
pub(crate) struct ConnectProvider;

impl ViewerProvider for ConnectProvider {
    fn id(&self) -> &'static str {
        "remote.connect"
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(ConnectView::new()))
    }
}

/// What the form edits: a saved connection (its project's), prefilled; the view's data.
#[derive(Debug, Clone)]
pub struct EditConnection {
    pub saved: Saved,
}

/// How the form reaches a server (the real connectors; a test's fake).
pub(crate) type Connectors = fn(&Saved, Option<Secret>, Option<String>) -> Arc<dyn Connector>;
/// Where a connection's secret is kept (the keychain, the vault; a test's memory).
pub(crate) type Stores = fn(&Saved) -> Arc<dyn SecretStore>;

/// What keeping an edited connection reports: it, with its new secret — or why not.
type Kept = Result<(Saved, Option<Secret>), config::Unsaved>;

/// Why the form can't be used as typed: the field to fix (marked), and what to tell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Invalid {
    field: Option<Field>,
    why: String,
}

impl Invalid {
    fn at(field: Field, key: &str) -> Self {
        Self { field: Some(field), why: t(key) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Connection,
    Folder,
    Contents,
}

/// The wizard's text fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Host,
    Port,
    User,
    Secret,
    Key,
    /// The project's name (its local folder follows it).
    Name,
    /// The connection's name (the project's, unless typed).
    Label,
    Local,
    /// Step 2: the server's folder, typed to go there.
    Path,
}

const FIELDS: [Field; 9] = [
    Field::Host,
    Field::Port,
    Field::User,
    Field::Secret,
    Field::Key,
    Field::Name,
    Field::Label,
    Field::Local,
    Field::Path,
];

/// The controls before the text fields (see [`ConnectView::with_controls`]): the open
/// list, the step's scrollbar.
const FIELD_PARTS: usize = 2;

const PROTOCOLS: [Protocol; 4] = [
    Protocol::Sftp,
    Protocol::Ftp,
    Protocol::Ftps { implicit: false },
    Protocol::Ftps { implicit: true },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthKind {
    Password,
    Key,
    Agent,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Hit {
    Field(Field),
    Protocol(usize),
    Auth(AuthKind),
    SavedIn(SavedIn),
    /// How the project is worked on: a local copy kept in sync, or on the server.
    Mode(Mode),
    /// Uploads / downloads at once for this connection (`None`: as in the settings).
    Uploads(Option<usize>),
    Downloads(Option<usize>),
    /// Moves and deletions made here go to the server too (on / off).
    Moves,
    /// A save uploads at once (on / off).
    AutoUpload,
    SecretsIn(SecretsIn),
    Eye,
    /// Stops connecting / downloading (the wizard stays).
    Cancel,
    /// The local folder: the system's picker.
    Browse,
    /// A folder suggested for the field being typed (the local copy's, the server's).
    Suggestion(usize),
    Next,
    Back,
    /// A folder of the server's listing (step 2): go into it.
    Enter(usize),
    Up,
    Choose,
    /// Editing: signs in with what's typed (the result said in the footer).
    Test,
    /// Editing: keeps the connection (from any step).
    Save,
    /// The project tree (step 3): its checkbox, its folder opened / closed.
    Check(usize),
    Expand(usize),
    /// An exclusion's × in the list.
    Unexclude(usize),
    /// Who files written belong to: the server's way, the project folder's, chosen.
    WriteAs(usize),
    /// The chosen owner's / group's list.
    Account(Account),
    Connect,
}

/// A row of the project's tree (step 3).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Node {
    /// Relative to the chosen folder, with `/`.
    path: String,
    is_dir: bool,
    depth: usize,
    expanded: bool,
    loaded: bool,
}

/// What the browsing session is asked.
enum Request {
    Home,
    List(String),
    /// A folder's folders, to complete the typed path (never waited for: no spinner).
    Folders(String),
    /// The server's accounts, and those of the project's folder (absolute).
    Accounts(String),
}

/// What it answers.
enum Reply {
    Home(Result<String, RemoteError>),
    Listed(String, Result<Vec<RemoteEntry>, RemoteError>),
    /// A folder's folders (their names); one that can't be listed has none.
    Folders(String, Vec<String>),
    Accounts(ServerAccounts),
}

/// What the download reports.
type Downloaded = Result<(PathBuf, usize, usize), Failure>;

/// Why the download stopped.
#[derive(Debug)]
enum Failure {
    Remote(RemoteError),
    /// The secret couldn't be kept (the keychain, the vault).
    Secret(String),
    /// The connection couldn't be saved.
    Save(String),
}

impl From<RemoteError> for Failure {
    fn from(e: RemoteError) -> Self {
        Self::Remote(e)
    }
}

/// What went wrong, for the user (the server's own words after it).
pub(crate) fn explain(e: &RemoteError) -> String {
    match e {
        RemoteError::Connect(why) => tr!("remote.error_connect", reason = why.as_str()),
        RemoteError::Auth(why) => tr!("remote.error_auth", reason = why.as_str()),
        RemoteError::HostKey(why) => tr!("remote.error_host_key", reason = why.as_str()),
        RemoteError::NotFound(path) => tr!("remote.error_not_found", path = path.as_str()),
        RemoteError::Server(why) => tr!("remote.error_server", reason = why.as_str()),
        RemoteError::TimedOut => t("remote.error_timeout"),
        RemoteError::Cancelled => t("remote.error_cancelled"),
    }
}

pub struct ConnectView {
    step: Step,
    /// Its text fields, in [`FIELDS`]' order (their places among its controls follow it).
    inputs: [(Field, TextInput); FIELDS.len()],
    protocol: Protocol,
    auth: AuthKind,
    mode: Mode,
    /// Uploads / downloads at once (`None`: the global settings).
    uploads: Option<usize>,
    downloads: Option<usize>,
    /// Moves and deletions made here go to the server too.
    moves: bool,
    /// A save uploads at once.
    auto_upload: bool,
    saved_in: SavedIn,
    secrets_in: SecretsIn,
    /// The local folder was typed (else it follows the name), the name too (else the folder).
    local_typed: bool,
    name_typed: bool,
    label_typed: bool,
    status: Option<(String, Tone)>,
    /// An unknown server's fingerprint, waiting to be trusted; the one trusted.
    unknown: Option<String>,
    accepted: Option<String>,
    /// The browsing session (steps 2 and 3).
    browser: Option<(Sender<Request>, Receiver<Reply>)>,
    waiting: usize,
    /// Step 2: where we are on the server, and what's there.
    current: String,
    listing: Vec<RemoteEntry>,
    /// Step 3: the chosen folder, its tree, what's left out.
    root: String,
    tree: Vec<Node>,
    excluded: BTreeSet<String>,
    /// The keyboard is on the listing / tree (steps 2, 3), its highlighted row.
    on_list: bool,
    highlight: Option<usize>,
    downloading: Option<Receiver<Downloaded>>,
    progress: Arc<(AtomicUsize, AtomicUsize)>,
    cancel: Arc<AtomicBool>,
    /// Folders completing the focused field's typed path (the local copy's, the server's:
    /// full paths), the one chosen with ↑ ↓, and where the field is (the list opens under it).
    suggestions: Vec<String>,
    suggested: Option<usize>,
    /// The local folders completing a typed path, read in the background: for that text
    /// (shown only if it's still what's typed).
    local_folders: Option<(String, Receiver<Vec<String>>)>,
    completed_field: Rect,
    /// Step 2: the server's folders already read, by folder (`None`: asked, not answered),
    /// so typing a path asks for each folder once. Kept for one browsing session.
    server_folders: HashMap<String, Option<Vec<String>>>,
    /// Who files written on the server belong to; the server's accounts to choose from
    /// (`None`: being read), their list open, where their fields are.
    write_as: WriteAs,
    server_accounts: Option<ServerAccounts>,
    picker: ui_components::picker::Picker,
    picking: Option<Account>,
    account_fields: Vec<(Account, Rect)>,
    scroll: f32,
    /// How far the step scrolls (measured when painted), and its scrollbar.
    max_scroll: f32,
    bar: ui_components::EdgeScrollbar,
    /// Its controls' state of use (see [`ConnectView::with_controls`]).
    controls: Controls,
    /// The new connection's identity (UUID v4, made once: its secret is kept under it, so
    /// two connections named alike never share or overwrite one).
    connection_id: String,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
    action: Option<ViewerAction>,
    /// Where the folder picker starts, worked out in the background.
    browsing: ui_components::deferred::Deferred,
    waker: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Editing this saved connection (else making a new one).
    editing: Option<Saved>,
    /// "Test Connection" waits for the server (it stays on the step).
    testing: bool,
    /// The edited connection being kept (in the background: the keychain may take a while).
    saving: Option<Receiver<Kept>>,
    /// Editing: the kept password, read in the background (the keychain may take a while)
    /// and put in its field, masked; while it's the field's text, it's the one kept (not
    /// written again on save).
    kept_secret: Option<Secret>,
    reading_secret: Option<Receiver<Option<Secret>>>,
    /// The field the last check found wrong (outlined until it's typed in).
    invalid: Option<Field>,
    connectors: Connectors,
    stores: Stores,
}

impl std::fmt::Debug for ConnectView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the secret.
        f.debug_struct("ConnectView").field("step", &self.step).finish_non_exhaustive()
    }
}

impl Default for ConnectView {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ConnectView {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// `My Shop!` → `my-shop`.
fn slug(name: &str) -> String {
    let s: String =
        name.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect();
    let s = s.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    if s.is_empty() { "remote".into() } else { s }
}

/// Where a connection's local copy goes by default.
fn default_local(name: &str) -> String {
    dirs::home_dir()
        .map(|h| h.join("remote-projects").join(slug(name)))
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// `/var/www` + `shop` → `/var/www/shop`.
fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') { format!("{dir}{name}") } else { format!("{dir}/{name}") }
}

/// The folder above `dir` (`/` stays).
fn parent(dir: &str) -> String {
    let trimmed = dir.trim_end_matches('/');
    match trimmed.rsplit_once('/') {
        Some(("", _)) | None => "/".into(),
        Some((up, _)) => up.into(),
    }
}

/// A server folder as a key: no trailing `/` (the root stays `/`).
fn folder_key(dir: &str) -> String {
    match dir.trim_end_matches('/') {
        "" => "/".into(),
        trimmed => trimmed.into(),
    }
}

/// The names of a listing's folders.
fn names(listing: &[RemoteEntry]) -> Vec<String> {
    listing
        .iter()
        .filter(|e| e.is_dir)
        .filter_map(|e| e.path.rsplit('/').next().map(str::to_owned))
        .collect()
}

impl ConnectView {
    /// Its controls, the topmost first, with their state of use: an open list over
    /// everything, the step's scrollbar, the text fields ([`fields::FormField`], one of them
    /// always focused: a press beside them leaves it the keys). The wizard moves the focus
    /// itself (Tab and the arrows walk the step's fields, a step given focuses one), and asks
    /// the open list and the path's suggestions first; what a field typed is followed after.
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let mut shown = self.inputs.each_ref().map(|(field, _)| self.in_step(*field)).into_iter();
        let edited = std::cell::Cell::new(None);
        let answer = {
            let hits = &self.hits;
            let mut fields = self.inputs.each_mut().map(|(field, input)| fields::FormField {
                field: *field,
                input,
                hits,
                in_step: shown.next().unwrap_or(false),
                edited: &edited,
            });
            let [f0, f1, f2, f3, f4, f5, f6, f7, f8] = &mut fields;
            let mut step = Thumb { bar: &mut self.bar, offset: &mut self.scroll };
            let mut parts: [&mut dyn Control; FIELD_PARTS + FIELDS.len()] =
                [&mut self.picker, &mut step, f0, f1, f2, f3, f4, f5, f6, f7, f8];
            f(&mut self.controls, &mut parts)
        };
        if let Some(field) = edited.get() {
            self.edited(field);
        }
        answer
    }

    /// The field with the keys (the form's first before any is given them).
    fn focused(&self) -> Field {
        let at = self.controls.focused().and_then(|i| i.checked_sub(FIELD_PARTS));
        at.and_then(|i| self.inputs.get(i)).map_or(Field::Host, |(field, _)| *field)
    }

    /// The keys given to `field`.
    fn focus(&mut self, field: Field) {
        let Some(at) = self.inputs.iter().position(|(f, _)| *f == field) else { return };
        self.with_controls(|c, parts| c.focus(parts, Some(FIELD_PARTS + at)));
    }

    /// The part that was pressed is a field (its place among the controls).
    fn is_field(part: usize) -> bool {
        part >= FIELD_PARTS
    }

    pub fn new() -> Self {
        let mut inputs = FIELDS.map(|f| (f, TextInput::default()));
        for (field, input) in &mut inputs {
            match field {
                Field::Port => input.set_text("22"),
                Field::Secret => input.set_masked(true),
                _ => {}
            }
        }
        let mut view = Self {
            step: Step::Connection,
            inputs,
            protocol: Protocol::Sftp,
            auth: AuthKind::Password,
            mode: Mode::Mirror,
            uploads: None,
            downloads: None,
            moves: false,
            auto_upload: true,
            saved_in: SavedIn::Project,
            secrets_in: SecretsIn::Keychain,
            local_typed: false,
            name_typed: false,
            label_typed: false,
            status: None,
            unknown: None,
            accepted: None,
            browser: None,
            waiting: 0,
            current: String::new(),
            listing: Vec::new(),
            root: String::new(),
            tree: Vec::new(),
            excluded: BTreeSet::new(),
            on_list: false,
            highlight: None,
            downloading: None,
            progress: Arc::default(),
            cancel: Arc::default(),
            suggestions: Vec::new(),
            local_folders: None,
            suggested: None,
            completed_field: Rect::default(),
            server_folders: HashMap::new(),
            write_as: WriteAs::Server,
            server_accounts: None,
            picker: Default::default(),
            picking: None,
            account_fields: Vec::new(),
            scroll: 0.0,
            max_scroll: 0.0,
            bar: Default::default(),
            controls: Controls::sticky(),
            connection_id: uuid::Uuid::new_v4().to_string(),
            hits: Vec::new(),
            hover: None,
            action: None,
            browsing: Default::default(),
            waker: None,
            editing: None,
            testing: false,
            saving: None,
            kept_secret: None,
            reading_secret: None,
            invalid: None,
            connectors: session::connector,
            stores: config::secret_store,
        };
        // The keys in the first field from the start.
        view.focus(Field::Host);
        view
    }

    pub fn step(&self) -> Step {
        self.step
    }

    /// The form shows `saved` to edit it: every field as kept (the password read in the
    /// background and put in, masked; left empty, the kept one stays), its identity kept.
    pub fn edit(&mut self, saved: Saved) {
        self.read_kept_secret(&saved);
        let c = &saved.connection;
        self.protocol = c.protocol;
        self.auth = match &c.auth {
            Auth::Password => AuthKind::Password,
            Auth::Key { .. } => AuthKind::Key,
            Auth::Agent => AuthKind::Agent,
        };
        let key = match &c.auth {
            Auth::Key { path } => path.display().to_string(),
            _ => String::new(),
        };
        let name = saved.local.file_name().map(|n| n.to_string_lossy().into_owned());
        let texts = [
            (Field::Host, c.host.clone()),
            (Field::Port, c.port.to_string()),
            (Field::User, c.user.clone()),
            (Field::Secret, String::new()),
            (Field::Key, key),
            (Field::Name, name.unwrap_or_default()),
            (Field::Label, c.name.clone()),
            (Field::Local, saved.local.display().to_string()),
            (Field::Path, c.root.clone()),
        ];
        for (field, text) in texts {
            self.input_mut(field).set_text(&text);
        }
        self.mode = c.mode;
        (self.uploads, self.downloads, self.moves) = (c.uploads, c.downloads, c.moves);
        self.auto_upload = c.auto_upload;
        self.write_as = c.write_as.clone();
        (self.saved_in, self.secrets_in) = (c.saved_in, c.secrets_in);
        self.root.clone_from(&c.root);
        self.current.clone_from(&c.root);
        // Git's folder and the project's data always stay out: not listed as choices.
        self.excluded = c
            .excluded
            .iter()
            .filter(|e| *e != ".git" && *e != kernel::brand::PROJECT_DIR)
            .cloned()
            .collect();
        self.connection_id.clone_from(&c.id);
        (self.local_typed, self.name_typed, self.label_typed) = (true, true, true);
        (self.step, self.status, self.invalid) = (Step::Connection, None, None);
        self.focus(Field::Host);
        self.editing = Some(saved);
    }

    /// Reads `saved`'s kept secret on a worker (never with the agent: it has none).
    fn read_kept_secret(&mut self, saved: &Saved) {
        self.kept_secret = None;
        self.reading_secret = None;
        if saved.connection.auth == Auth::Agent {
            return;
        }
        let store = (self.stores)(saved);
        let id = saved.connection.id.clone();
        let waker = self.waker.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        kernel::work::run("remote-secret", move || {
            let _ = tx.send(store.get(&id).filter(|s| !s.is_empty()));
            if let Some(wake) = &waker {
                wake();
            }
        });
        self.reading_secret = Some(rx);
    }

    /// The kept secret read: in its field unless something was typed there meanwhile.
    /// `true` if it arrived.
    fn kept_secret_read(&mut self) -> bool {
        let Some(read) = self.reading_secret.as_ref().and_then(|rx| rx.try_recv().ok()) else {
            return false;
        };
        self.reading_secret = None;
        let Some(secret) = read else { return true };
        if self.input(Field::Secret).text().is_empty() {
            self.input_mut(Field::Secret).set_text(secret.expose());
        }
        self.kept_secret = Some(secret);
        true
    }

    /// The form edits a saved connection.
    pub fn editing(&self) -> bool {
        self.editing.is_some()
    }

    fn input(&self, field: Field) -> &TextInput {
        self.inputs.iter().find(|(f, _)| *f == field).map_or(&self.inputs[0].1, |(_, i)| i)
    }

    fn input_mut(&mut self, field: Field) -> &mut TextInput {
        let i = self.inputs.iter().position(|(f, _)| *f == field).unwrap_or(0);
        &mut self.inputs[i].1
    }

    fn value(&self, field: Field) -> String {
        self.input(field).text().trim().to_owned()
    }

    /// The fields of the current step, in their order.
    fn fields(&self) -> Vec<Field> {
        FIELDS.into_iter().filter(|f| self.in_step(*f)).collect()
    }

    /// `field` is one of the current step's (a key's path only with a key, no secret with
    /// the agent).
    fn in_step(&self, field: Field) -> bool {
        match self.step {
            Step::Connection => match field {
                Field::Host | Field::Port | Field::User => true,
                Field::Key => self.auth == AuthKind::Key,
                Field::Secret => self.auth != AuthKind::Agent,
                _ => false,
            },
            Step::Folder => field == Field::Path,
            // Editing: the project's folder (its name and local copy) stays.
            Step::Contents if self.editing.is_some() => field == Field::Label,
            Step::Contents => matches!(field, Field::Name | Field::Label | Field::Local),
        }
    }

    /// Types `text` into the focused field.
    pub fn type_text(&mut self, text: &str) {
        if self.on_list && self.step != Step::Connection {
            if text == " "
                && self.step == Step::Contents
                && let Some(i) = self.highlight
            {
                self.toggle(i);
            }
            return;
        }
        // The focused field takes it, when it's the step's.
        self.with_controls(|c, parts| c.text(parts, text));
    }

    fn edited(&mut self, field: Field) {
        if self.invalid == Some(field) {
            self.invalid = None;
        }
        match field {
            Field::Name => {
                self.name_typed = true;
                let name = self.value(Field::Name);
                if !self.local_typed {
                    self.input_mut(Field::Local).set_text(&default_local(&name));
                }
                if !self.label_typed {
                    self.input_mut(Field::Label).set_text(&name);
                }
            }
            Field::Label => self.label_typed = true,
            Field::Local => {
                self.local_typed = true;
                self.suggest();
            }
            Field::Path => self.suggest_server(),
            _ => self.unknown = None,
        }
    }

    /// Connecting, listing, downloading or saving.
    fn busy(&self) -> bool {
        self.waiting > 0 || self.downloading.is_some() || self.saving.is_some()
    }

    /// Stops what's going on: a download is cancelled (what came stays, the connection
    /// isn't saved), a connection or a listing is no longer waited for. The wizard stays,
    /// to change something and try again.
    fn stop(&mut self) {
        if self.downloading.take().is_some() {
            self.cancel.store(true, Ordering::Relaxed);
        }
        if self.waiting > 0 {
            self.waiting = 0;
            self.testing = false;
            // Still connecting: that session is dropped (a new one on Continue).
            if self.step == Step::Connection {
                self.browser = None;
            }
        }
        self.status = Some((t("remote.stopped"), Tone::Neutral));
    }

    fn focus_next(&mut self, step: isize) {
        let fields = self.fields();
        if fields.is_empty() {
            return;
        }
        let focused = self.focused();
        let at = fields.iter().position(|f| *f == focused).unwrap_or(0) as isize;
        let to = at + step;
        // Steps 2, 3: before the first field / past the last one, the listing / tree.
        if self.step != Step::Connection && (to < 0 || to >= fields.len() as isize) {
            self.on_list = true;
            return;
        }
        self.focus(fields[to.rem_euclid(fields.len() as isize) as usize]);
    }

    fn select_protocol(&mut self, i: usize) {
        let Some(protocol) = PROTOCOLS.get(i).copied() else { return };
        // The port follows, unless one was typed.
        let typed = self.value(Field::Port);
        if typed.is_empty() || typed == self.protocol.default_port().to_string() {
            self.input_mut(Field::Port).set_text(&protocol.default_port().to_string());
        }
        self.protocol = protocol;
        if protocol != Protocol::Sftp {
            self.auth = AuthKind::Password;
        }
    }

    fn hit(&self, p: Point) -> Option<Hit> {
        self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)
    }
}

/// The fonts the last step draws with: its labels, the small ones, the paths.
struct Fonts {
    ui: text_layout::Font,
    small: text_layout::Font,
    mono: text_layout::Font,
}

/// The footer's room left of its buttons: where it starts and ends, the footer, its middle.
#[derive(Clone, Copy)]
struct FooterBar {
    x0: f32,
    right: f32,
    rect: Rect,
    mid: f32,
}

#[cfg(test)]
#[path = "../tests/unit/connect_view.rs"]
mod tests;
