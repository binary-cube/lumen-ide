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

mod fields;
mod owner;
mod paint;
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

/// Folders completing `typed` (a path being typed; `~` is the home folder): those in its
/// folder whose names start with what follows the last separator (hidden ones only when
/// asked for), sorted, at most 8 — none once it names one exactly.
fn folder_suggestions(typed: &str) -> Vec<String> {
    const MAX: usize = 8;
    let sep = std::path::MAIN_SEPARATOR;
    let expanded = match typed.strip_prefix('~') {
        Some(rest) => match dirs::home_dir() {
            Some(home) => format!("{}{rest}", home.display()),
            None => return Vec::new(),
        },
        None => typed.to_owned(),
    };
    if expanded.is_empty() {
        return Vec::new();
    }
    let (dir, prefix) = match expanded.rfind(['/', sep]) {
        Some(at) => (&expanded[..=at], &expanded[at + 1..]),
        None => return Vec::new(),
    };
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let lower = prefix.to_lowercase();
    let mut found: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.to_lowercase().starts_with(&lower))
        .filter(|name| !name.starts_with('.') || prefix.starts_with('.'))
        .filter(|name| name != prefix)
        .map(|name| format!("{dir}{name}"))
        .collect();
    found.sort_by_key(|p| p.to_lowercase());
    found.truncate(MAX);
    found
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

/// A server path being typed (relative ones from `current`) split into the folder it's in
/// and the start of the name typed after it; nothing for an empty one.
fn typed_server_path(typed: &str, current: &str) -> Option<(String, String)> {
    if typed.is_empty() {
        return None;
    }
    let full = if typed.starts_with('/') { typed.to_owned() } else { join(current, typed) };
    let (dir, prefix) = full.rsplit_once('/')?;
    Some((folder_key(dir), prefix.to_owned()))
}

/// The folders of `dir` (`names`) completing `prefix`, like the local copy's: those starting
/// with it (any case; hidden ones only when asked for), sorted, at most 8, as full paths —
/// none once it names one exactly.
fn server_suggestions(dir: &str, prefix: &str, names: &[String]) -> Vec<String> {
    const MAX: usize = 8;
    let lower = prefix.to_lowercase();
    let mut found: Vec<String> = names
        .iter()
        .filter(|name| name.to_lowercase().starts_with(&lower))
        .filter(|name| !name.starts_with('.') || prefix.starts_with('.'))
        .filter(|name| name.as_str() != prefix)
        .map(|name| join(dir, name))
        .collect();
    found.sort_by_key(|p| p.to_lowercase());
    found.truncate(MAX);
    found
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

    /// The folders that complete the local copy's typed path (read from the one it's in:
    /// a single folder, on a key).
    fn suggest(&mut self) {
        self.suggestions = folder_suggestions(&self.value(Field::Local));
        self.suggested = None;
    }

    /// The server's folders that complete step 2's typed path: from the folders already
    /// read (the one shown counts), else asked of the browsing session once (in the
    /// background: they show when it answers, if the path still wants them).
    fn suggest_server(&mut self) {
        self.suggested = None;
        let Some((dir, prefix)) = typed_server_path(&self.value(Field::Path), &self.current) else {
            self.suggestions.clear();
            return;
        };
        match self.server_folders.get(&dir) {
            Some(Some(names)) => self.suggestions = server_suggestions(&dir, &prefix, names),
            Some(None) => self.suggestions.clear(),
            None => {
                self.suggestions.clear();
                if let Some((tx, _)) = &self.browser
                    && tx.send(Request::Folders(dir.clone())).is_ok()
                {
                    self.server_folders.insert(dir, None);
                }
            }
        }
    }

    /// The server's folders of `dir`, kept; suggested at once if the path is still in it.
    fn server_folders_read(&mut self, dir: String, names: Vec<String>) {
        self.server_folders.insert(folder_key(&dir), Some(names));
        if self.step == Step::Folder && self.focused() == Field::Path && !self.on_list {
            self.suggest_server();
        }
    }

    /// The focused path completed with suggestion `i` (its folders listed next).
    fn complete(&mut self, i: usize) {
        let Some(path) = self.suggestions.get(i).cloned() else { return };
        if self.focused() == Field::Path {
            self.input_mut(Field::Path).set_text(&format!("{path}/"));
            self.input_mut(Field::Path).end(false);
            self.suggest_server();
            return;
        }
        let path = format!("{path}{}", std::path::MAIN_SEPARATOR);
        self.input_mut(Field::Local).set_text(&path);
        self.input_mut(Field::Local).end(false);
        self.local_typed = true;
        self.suggest();
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

    /// The keys of the listing (step 2) and of the tree (step 3).
    fn list_key(&mut self, key: ViewerKey) -> bool {
        let len = if self.step == Step::Folder { self.listing.len() } else { self.tree.len() };
        match key {
            ViewerKey::Down if len > 0 => {
                self.highlight = Some(self.highlight.map_or(0, |h| (h + 1).min(len - 1)));
            }
            ViewerKey::Up => self.highlight = self.highlight.map(|h| h.saturating_sub(1)),
            ViewerKey::Tab => {
                self.on_list = false;
                self.focus(if self.step == Step::Folder { Field::Path } else { Field::Name });
            }
            ViewerKey::Right | ViewerKey::Enter if self.step == Step::Folder => {
                match self.highlight.and_then(|h| self.listing.get(h)).filter(|e| e.is_dir) {
                    Some(entry) => {
                        let name = entry.path.rsplit('/').next().unwrap_or("").to_owned();
                        let dir = join(&self.current, &name);
                        self.go(dir);
                    }
                    None if key == ViewerKey::Enter => self.choose(),
                    None => {}
                }
            }
            ViewerKey::Left | ViewerKey::Backspace if self.step == Step::Folder => {
                let up = parent(&self.current);
                self.go(up);
            }
            ViewerKey::Right | ViewerKey::Left => {
                let Some(h) = self.highlight.filter(|h| *h < len) else { return true };
                let node = &self.tree[h];
                if node.is_dir && node.expanded != (key == ViewerKey::Right) {
                    self.expand(h);
                } else if key == ViewerKey::Left {
                    // On its folder.
                    let depth = node.depth;
                    self.highlight =
                        self.tree[..h].iter().rposition(|n| n.depth < depth).or(Some(h));
                }
            }
            ViewerKey::Enter if self.editing.is_some() => self.save(),
            ViewerKey::Enter => self.connect(),
            ViewerKey::Escape if self.busy() => self.stop(),
            ViewerKey::Escape => self.action = Some(ViewerAction::Close),
            _ => return false,
        }
        true
    }

    /// The connection as typed (the server's folder: `root`), with its secret — checked
    /// first: the server, the port, the user, the key.
    pub(crate) fn connection(&self, root: &str) -> Result<(Saved, Option<Secret>), Invalid> {
        let host = self.value(Field::Host);
        let user = self.value(Field::User);
        if host.is_empty() {
            return Err(Invalid::at(Field::Host, "remote.form_incomplete"));
        }
        if host.contains(char::is_whitespace) || host.contains('/') {
            return Err(Invalid::at(Field::Host, "remote.invalid_host"));
        }
        let port = match self.value(Field::Port) {
            typed if typed.is_empty() => self.protocol.default_port(),
            typed => match typed.parse::<u16>() {
                Ok(port) if port > 0 => port,
                _ => return Err(Invalid::at(Field::Port, "remote.invalid_port")),
            },
        };
        if user.is_empty() {
            return Err(Invalid::at(Field::User, "remote.form_incomplete"));
        }
        let auth = match (self.auth, self.protocol != Protocol::Sftp) {
            (_, true) | (AuthKind::Password, _) => Auth::Password,
            (AuthKind::Key, _) if self.value(Field::Key).is_empty() => {
                return Err(Invalid::at(Field::Key, "remote.invalid_key"));
            }
            (AuthKind::Key, _) => Auth::Key { path: PathBuf::from(self.value(Field::Key)) },
            (AuthKind::Agent, _) => Auth::Agent,
        };
        let name = match self.value(Field::Label) {
            label if label.is_empty() => self.value(Field::Name),
            label => label,
        };
        let mut excluded = vec![".git".to_owned(), kernel::brand::PROJECT_DIR.to_owned()];
        excluded.extend(self.excluded.iter().cloned());
        // Editing: what isn't on the form stays as it was (its retries, the certificates
        // trusted, its local copy).
        let original = self.editing.as_ref();
        let mut trusted: Vec<String> = original.map(|o| o.trusted.clone()).unwrap_or_default();
        trusted.extend(
            self.accepted.iter().filter(|a| !trusted.contains(a)).cloned().collect::<Vec<_>>(),
        );
        let connection = Connection {
            id: self.connection_id.clone(),
            name: if name.is_empty() { host.clone() } else { name },
            protocol: self.protocol,
            host,
            port,
            user,
            auth,
            root: root.to_owned(),
            mode: self.mode,
            excluded,
            saved_in: self.saved_in,
            secrets_in: self.secrets_in,
            retry: original.and_then(|o| o.connection.retry),
            uploads: self.uploads,
            downloads: self.downloads,
            moves: self.moves,
            auto_upload: self.auto_upload,
            write_as: if self.owners_changeable() {
                self.write_as.clone()
            } else {
                WriteAs::Server
            },
        };
        let secret = Secret::new(self.input(Field::Secret).text());
        // The kept one, as read: not a new one (it stays where it is, read when connecting).
        let secret =
            if self.kept_secret.as_ref() == Some(&secret) { Secret::default() } else { secret };
        let local = match original {
            Some(o) => o.local.clone(),
            None => PathBuf::from(self.value(Field::Local)),
        };
        let saved = Saved { connection, local, trusted };
        Ok((saved, (!secret.is_empty()).then_some(secret)))
    }

    /// The whole connection, ready to download (step 3) or to keep (editing).
    pub(crate) fn saved(&self) -> Result<(Saved, Option<Secret>), Invalid> {
        let (saved, secret) = self.connection(&self.root)?;
        if !self.root.starts_with('/') {
            return Err(Invalid { field: None, why: t("remote.invalid_path") });
        }
        if self.value(Field::Name).is_empty() {
            return Err(Invalid::at(Field::Name, "remote.form_incomplete"));
        }
        if saved.local.as_os_str().is_empty() {
            return Err(Invalid::at(Field::Local, "remote.form_incomplete"));
        }
        Ok((saved, secret))
    }

    /// What's wrong said in the footer, its field outlined.
    fn reject(&mut self, invalid: Invalid) {
        self.status = Some((invalid.why, Tone::Danger));
        self.invalid = invalid.field;
        if let Some(field) = invalid.field.filter(|f| self.fields().contains(f)) {
            self.focus(field);
            self.on_list = false;
        }
    }

    /// A connector for browsing (paths from the server's root). Editing with the password
    /// left empty: the kept one, read when it connects (on the browsing thread).
    fn browsing_connector(&self) -> Result<Arc<dyn Connector>, Invalid> {
        let (saved, secret) = self.connection("/")?;
        let accepted = self.accepted.clone();
        Ok(match (&self.editing, secret) {
            (Some(original), None) if saved.connection.auth != Auth::Agent => {
                Arc::new(KeptSecret {
                    saved,
                    original: original.clone(),
                    accepted,
                    connectors: self.connectors,
                    stores: self.stores,
                })
            }
            (_, secret) => (self.connectors)(&saved, secret, accepted),
        })
    }

    /// Enter: the step's main action (editing, on the form and the options: Save).
    fn next(&mut self) {
        match self.step {
            Step::Connection | Step::Contents if self.editing.is_some() => self.save(),
            Step::Connection => self.browse(),
            // The typed path: gone there.
            Step::Folder if !self.on_list => {
                let dir = self.value(Field::Path);
                let dir = if dir.starts_with('/') { dir } else { join(&self.current, &dir) };
                self.go(dir);
            }
            Step::Folder => self.choose(),
            Step::Contents => self.connect(),
        }
    }

    /// Step 1 → 2: connects (a session kept for browsing) and lands where the user does
    /// (editing: in the saved folder).
    fn browse(&mut self) {
        self.sign_in();
        self.testing = false;
    }

    /// Editing: signs in with what's typed, and says how it went (it stays on the step).
    fn test(&mut self) {
        self.sign_in();
        self.testing = self.waiting > 0;
    }

    /// Connects with what's typed (a session kept for browsing), asking where the user lands.
    fn sign_in(&mut self) {
        // The fingerprint shown: going on trusts it.
        if let Some(fingerprint) = self.unknown.take() {
            self.accepted = Some(fingerprint);
        }
        let connector = match self.browsing_connector() {
            Ok(c) => c,
            Err(invalid) => {
                self.reject(invalid);
                return;
            }
        };
        self.invalid = None;
        self.open_browser(connector);
        self.ask(Request::Home);
        self.status = Some((t("remote.testing"), Tone::Neutral));
    }

    /// Editing: keeps the connection as typed (its secret with it, a new one replacing the
    /// kept), in the background; then the project reconnects with it if it's open.
    fn save(&mut self) {
        let Some(original) = self.editing.clone() else { return };
        if self.busy() {
            return;
        }
        let (saved, secret) = match self.saved() {
            Ok(s) => s,
            Err(invalid) => {
                self.reject(invalid);
                return;
            }
        };
        self.invalid = None;
        let stores = self.stores;
        let waker = self.waker.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        kernel::work::run("remote-save", move || {
            let outcome = config::update(&original, &saved, secret.as_ref(), &|s| stores(s));
            let _ = tx.send(outcome.map(|()| (saved, secret)));
            if let Some(wake) = &waker {
                wake();
            }
        });
        self.saving = Some(rx);
        self.status = Some((t("remote.saving"), Tone::Neutral));
    }

    /// The edited connection kept (or why not): the project reconnects with it, the form
    /// closes. `true` if it ended.
    fn saved_now(&mut self) -> bool {
        let Some(outcome) = self.saving.as_ref().and_then(|rx| rx.try_recv().ok()) else {
            return false;
        };
        self.saving = None;
        match outcome {
            Ok((saved, secret)) => {
                let key = if session::reconnect_later(saved, secret) {
                    "remote.saved_reconnecting"
                } else {
                    "remote.saved"
                };
                self.status = Some((t(key), Tone::Success));
                self.action = Some(ViewerAction::Close);
            }
            Err(config::Unsaved::Secret(why)) => {
                self.status =
                    Some((tr!("remote.error_secret", reason = why.as_str()), Tone::Danger));
            }
            Err(config::Unsaved::Save(why)) => {
                self.status = Some((tr!("remote.error_save", reason = why.as_str()), Tone::Danger));
            }
        }
        true
    }

    fn back(&mut self) {
        self.step = match self.step {
            Step::Connection | Step::Folder => Step::Connection,
            Step::Contents => Step::Folder,
        };
        self.status = None;
        self.scroll = 0.0;
        self.on_list = self.step != Step::Connection;
        self.highlight = None;
        self.suggestions.clear();
        if self.step == Step::Connection {
            self.browser = None;
            self.server_folders.clear();
            self.focus(Field::Host);
        }
    }

    /// A session on its own thread, answering listings (reconnecting if it dropped): it
    /// holds the connection while the wizard browses.
    #[allow(clippy::disallowed_methods)]
    fn open_browser(&mut self, connector: Arc<dyn Connector>) {
        let (tx, requests) = std::sync::mpsc::channel::<Request>();
        let (replies, rx) = std::sync::mpsc::channel::<Reply>();
        let waker = self.waker.clone();
        let spawned = std::thread::Builder::new().name("remote-browse".into()).spawn(move || {
            let mut fs = None;
            for request in requests {
                let mut attempt = || {
                    if fs.is_none() {
                        fs = Some(connector.connect()?);
                    }
                    let Some(session) = fs.as_mut() else { return Err(RemoteError::TimedOut) };
                    match &request {
                        Request::Home => session.home().map(|h| Reply::Home(Ok(h))),
                        Request::List(dir) => {
                            let relative = dir.trim_start_matches('/').to_owned();
                            session.list(&relative).map(|l| Reply::Listed(dir.clone(), Ok(l)))
                        }
                        Request::Folders(dir) => {
                            let relative = dir.trim_start_matches('/').to_owned();
                            session.list(&relative).map(|l| Reply::Folders(dir.clone(), names(&l)))
                        }
                        Request::Accounts(dir) => {
                            Ok(Reply::Accounts(owner::read_accounts(session.as_mut(), dir)))
                        }
                    }
                };
                let reply = match attempt() {
                    Ok(reply) => reply,
                    Err(e) => {
                        if e.is_transient() {
                            fs = None;
                        }
                        match &request {
                            Request::Home => Reply::Home(Err(e)),
                            Request::List(dir) => Reply::Listed(dir.clone(), Err(e)),
                            Request::Folders(dir) => Reply::Folders(dir.clone(), Vec::new()),
                            Request::Accounts(_) => Reply::Accounts(ServerAccounts::default()),
                        }
                    }
                };
                if replies.send(reply).is_err() {
                    break;
                }
                if let Some(wake) = &waker {
                    wake();
                }
            }
        });
        if spawned.is_ok() {
            self.browser = Some((tx, rx));
        }
    }

    fn ask(&mut self, request: Request) {
        if let Some((tx, _)) = &self.browser
            && tx.send(request).is_ok()
        {
            self.waiting += 1;
        }
    }

    /// Step 2: goes into `dir` (absolute).
    fn go(&mut self, dir: String) {
        self.suggestions.clear();
        self.input_mut(Field::Path).set_text(&dir);
        self.current = dir.clone();
        self.listing.clear();
        self.on_list = true;
        self.highlight = None;
        self.scroll = 0.0;
        self.ask(Request::List(dir));
    }

    /// Step 2 → 3: the folder chosen; its tree listed.
    fn choose(&mut self) {
        // Editing, the same folder again: its exclusions stay.
        let same = self.editing.is_some() && self.root == self.current;
        self.root = self.current.clone();
        self.step = Step::Contents;
        self.tree.clear();
        if !same {
            self.excluded.clear();
        }
        self.scroll = 0.0;
        self.focus(Field::Name);
        self.on_list = true;
        self.highlight = None;
        if !self.name_typed {
            let name = self.root.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_owned();
            self.input_mut(Field::Name).set_text(&name);
            if !self.label_typed {
                self.input_mut(Field::Label).set_text(&name);
            }
            if !self.local_typed {
                let local = default_local(&name);
                self.input_mut(Field::Local).set_text(&local);
            }
        }
        let root = self.root.clone();
        self.ask(Request::List(root));
        self.ask_accounts();
    }

    /// A tree path's absolute place on the server.
    fn absolute(&self, relative: &str) -> String {
        if relative.is_empty() { self.root.clone() } else { join(&self.root, relative) }
    }

    /// `path` (relative) is left out: it or a folder above it is.
    fn is_excluded(&self, path: &str) -> bool {
        self.excluded
            .iter()
            .any(|e| path == e || path.strip_prefix(e.as_str()).is_some_and(|r| r.starts_with('/')))
    }

    fn toggle(&mut self, i: usize) {
        let Some(node) = self.tree.get(i) else { return };
        let path = node.path.clone();
        if !self.excluded.remove(&path) {
            // Left out by a folder above: that one decides.
            if self.is_excluded(&path) {
                return;
            }
            // What was excluded inside goes with it.
            self.excluded.retain(|e| !e.starts_with(&format!("{path}/")));
            self.excluded.insert(path);
        }
    }

    fn expand(&mut self, i: usize) {
        let Some(node) = self.tree.get_mut(i) else { return };
        if !node.is_dir {
            return;
        }
        node.expanded = !node.expanded;
        let (path, expanded, loaded, depth) =
            (node.path.clone(), node.expanded, node.loaded, node.depth);
        if !expanded {
            // Its rows go (they come back when it opens again).
            let end = self.tree[i + 1..]
                .iter()
                .position(|n| n.depth <= depth)
                .map_or(self.tree.len(), |p| i + 1 + p);
            self.tree.drain(i + 1..end);
            self.tree[i].loaded = false;
        } else if !loaded {
            let dir = self.absolute(&path);
            self.ask(Request::List(dir));
        }
    }

    /// A listing for the tree: its rows under their folder.
    fn place_listing(&mut self, dir: &str, entries: Vec<RemoteEntry>) {
        let relative =
            dir.strip_prefix(self.root.as_str()).unwrap_or("").trim_start_matches('/').to_owned();
        let (at, depth) = if relative.is_empty() {
            self.tree.clear();
            (0, 0)
        } else {
            let Some(i) = self.tree.iter().position(|n| n.path == relative) else { return };
            self.tree[i].loaded = true;
            (i + 1, self.tree[i].depth + 1)
        };
        let mut rows: Vec<Node> = entries
            .into_iter()
            .filter_map(|e| {
                let name = e.path.rsplit('/').next()?.to_owned();
                // The project's data and git's folder never travel: not shown.
                (name != ".git" && name != kernel::brand::PROJECT_DIR).then(|| Node {
                    path: if relative.is_empty() { name } else { format!("{relative}/{name}") },
                    is_dir: e.is_dir,
                    depth,
                    expanded: false,
                    loaded: false,
                })
            })
            .collect();
        // Folders first, then by name.
        rows.sort_by(|a, b| {
            b.is_dir.cmp(&a.is_dir).then_with(|| a.path.to_lowercase().cmp(&b.path.to_lowercase()))
        });
        self.tree.splice(at..at, rows);
    }

    /// Downloads everything (but the exclusions) in parallel, saves the connection.
    fn connect(&mut self) {
        let (saved, secret) = match self.saved() {
            Ok(s) => s,
            Err(invalid) => {
                self.reject(invalid);
                return;
            }
        };
        if self.downloading.is_some() {
            return;
        }
        let connector = (self.connectors)(&saved, secret.clone(), self.accepted.clone());
        // The browsing session closed first: one connection less for the server to count.
        self.browser = None;
        self.waiting = 0;
        // Its downloads share a pool: cautious for FTP (the server may take only a few).
        let limit = crate::settings::defaults().limits.downloads;
        let connector: Arc<dyn Connector> =
            Arc::new(crate::pool::Pooled(session::pool_for(&saved, connector, limit)));
        let progress = Arc::clone(&self.progress);
        progress.0.store(0, Ordering::Relaxed);
        progress.1.store(0, Ordering::Relaxed);
        self.cancel = Arc::default();
        let cancel = Arc::clone(&self.cancel);
        let waker = self.waker.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        kernel::work::run("remote-download", move || {
            let wake = || {
                if let Some(w) = &waker {
                    w();
                }
            };
            // The Task Manager shows it while it runs.
            session::OPENING.0.fetch_add(1, Ordering::Relaxed);
            let outcome = download(&saved, secret, connector.as_ref(), &progress, &cancel, &wake);
            session::OPENING.0.fetch_sub(1, Ordering::Relaxed);
            let _ = tx.send(outcome);
            wake();
        });
        self.downloading = Some(rx);
        self.status = Some((t("remote.downloading"), Tone::Neutral));
    }

    fn failed(&mut self, e: RemoteError) {
        self.testing = false;
        match e {
            // An unknown server: its fingerprint, to trust.
            RemoteError::HostKey(f) if !f.starts_with("changed") => {
                self.status = Some((t("remote.unknown_server"), Tone::Neutral));
                self.unknown = Some(f);
                self.browser = None;
            }
            other => self.status = Some((explain(&other), Tone::Danger)),
        }
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

    /// Exclusions with their full paths on the server.
    fn exclusions(&self) -> Vec<String> {
        self.excluded.iter().map(|e| self.absolute(e)).collect()
    }
}

/// Editing with the password left empty: the kept secret, read from where it's kept each
/// time it connects (never held by the form).
struct KeptSecret {
    saved: Saved,
    /// Where it was kept (its identity, its store).
    original: Saved,
    accepted: Option<String>,
    connectors: Connectors,
    stores: Stores,
}

impl Connector for KeptSecret {
    fn connect(&self) -> Result<Box<dyn remote::application::RemoteFs>, RemoteError> {
        let secret = (self.stores)(&self.original).get(&self.original.connection.id);
        (self.connectors)(&self.saved, secret, self.accepted.clone()).connect()
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
