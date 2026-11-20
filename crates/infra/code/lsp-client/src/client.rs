//! A language server's client: one driver thread per server owning everything about it —
//! its process, the documents as it has them, the requests waiting for its answers — fed
//! by a channel. The editor never waits: what it tells (documents opened, changed, closed;
//! files changed on disk; questions) is queued, and each answer comes back as a
//! [`Reply`] when it's ready, the app woken to look at it.
//!
//! - Changes are sent grouped (`debounce`), always before a question about them, each as the
//!   one span that differs ([`Mirror`]).
//! - A question replacing an older one of its kind (a completion at the next key) cancels
//!   it (`$/cancelRequest`); so does a [`Reply`] dropped unanswered, and its timeout.
//! - A crash starts the server again after a pause growing each time (1 s, 2 s, 4 s…); after
//!   [`RestartPolicy::crashes`] crashes within [`RestartPolicy::window`] it stays stopped
//!   until restarted by hand. Its documents are opened again in it.
//! - Without documents for [`Config::idle`], it stops by itself (its thread ends).
//! - Waiting for nothing, the driver sleeps (no polling): only a deadline wakes it.
//!
//! The threads: `lsp-<server>` (the driver), `lsp-<server>-in` (writing to the server),
//! `lsp-<server>-out` (reading it) and `lsp-<server>-err` (its stderr, into its log).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use code_intelligence::servers::{
    CodeAction, Reply, ServedCompletion, ServedLocation, WorkspaceChange,
};
use code_intelligence::{CompletionItem, HoverInfo, SignatureHelp, Symbol};
use document::TextSnapshot;
use kernel::DocumentId;
use serde_json::{Value, json};
use workbench_model::Diagnostic;

use crate::convert::{self, DiskTexts, Texts};
use crate::jsonrpc::{self, Id, Message, ResponseError, codes};
use crate::launch::{Connection, Launcher};
use crate::status::{Capabilities, Feature, State, Status};
use crate::text::{Encoding, Lines, Mirror, Position, range_value};
use crate::uri;

mod incoming;
mod lifecycle;

mod requests;

/// How long things may take.
#[derive(Debug, Clone, Copy)]
pub struct Timeouts {
    /// The server's `initialize` answered.
    pub start: Duration,
    /// A question answered.
    pub request: Duration,
    /// `shutdown` answered and the process gone, before it's killed.
    pub shutdown: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            start: Duration::from_secs(60),
            request: Duration::from_secs(10),
            shutdown: Duration::from_secs(2),
        }
    }
}

/// When a crashed server is started again.
#[derive(Debug, Clone, Copy)]
pub struct RestartPolicy {
    /// This many crashes within `window` and it rests for `cooldown` before it's tried
    /// again (never given up for good: the built-in engine answers meanwhile).
    pub crashes: usize,
    pub window: Duration,
    /// The pause before the first restart (doubled for each crash after).
    pub pause: Duration,
    /// The rest after crashing too often (doubled each time it happens again, up to
    /// [`MAX_COOLDOWN`]).
    pub cooldown: Duration,
}

/// The longest rest after crashing too often.
pub const MAX_COOLDOWN: Duration = Duration::from_secs(600);

impl Default for RestartPolicy {
    fn default() -> Self {
        Self {
            crashes: 3,
            window: Duration::from_secs(180),
            pause: Duration::from_secs(1),
            cooldown: Duration::from_secs(60),
        }
    }
}

/// A server's client, as it's started.
pub struct Config {
    /// The server's name (its threads are named after it).
    pub name: String,
    /// Its root (`rootUri`, the workspace folder).
    pub root: PathBuf,
    /// The project's root: diagnostics are said relative to it.
    pub project_root: PathBuf,
    pub initialization_options: Value,
    /// `workspace/configuration`'s answers, by section (`typescript.preferences` is
    /// `settings["typescript"]["preferences"]`).
    pub settings: Value,
    /// What it may ask for by URL (`vscode/content`): a URL prefix and the folder it's read
    /// from (nothing else is answered: nothing is downloaded).
    pub contents: Vec<(String, PathBuf)>,
    /// How its completion kinds are shown (its description's `completion-kinds`).
    pub completion_kinds: convert::CompletionKinds,
    /// Notifications (without parameters) sent once it's initialized (its description's
    /// `on-initialized`).
    pub on_initialized: Vec<String>,
    /// How long changes are gathered before they're sent.
    pub debounce: Duration,
    pub timeouts: Timeouts,
    pub restart: RestartPolicy,
    /// Stopped after this long without a document (`None`: never).
    pub idle: Option<Duration>,
    /// The protocol's traffic written to its log too (while it's on).
    pub trace: Arc<AtomicBool>,
    /// Initialization options given from `secrets` (the option's key, the secret's id): read
    /// when it starts, on its thread; never written in its log.
    pub secret_options: Vec<(String, String)>,
    pub secrets: Option<Arc<dyn kernel::SecretStore>>,
    /// Its `window/showMessage` told as [`Event::Message`] (to be shown), not only logged.
    pub show_messages: bool,
    /// Wakes the app (an answer came, the state changed).
    pub wake: Arc<dyn Fn() + Send + Sync>,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("name", &self.name)
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

/// What the driver tells its owner, from its thread.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// A file's diagnostics (relative to the project), replacing that file's earlier ones.
    Diagnostics { path: PathBuf, diagnostics: Vec<Diagnostic> },
    /// Every diagnostic it published is gone (it stopped or crashed).
    DiagnosticsCleared,
    /// The server asked for these changes to be made (`workspace/applyEdit`).
    ApplyEdit { label: Option<String>, change: WorkspaceChange },
    /// Its state, figures or log changed.
    Changed,
    /// A message it wants shown (`window/showMessage`, when its description asks for them).
    Message { kind: MessageKind, text: String },
    /// It stopped for good (asked to, unused for long, or after crashing too often): its
    /// thread is gone.
    Ended,
}

/// How a server's message reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    Error,
    Warning,
    Info,
}

/// What happened to a file on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileEvent {
    Created,
    Changed,
    Deleted,
}

impl FileEvent {
    fn code(self) -> u8 {
        match self {
            Self::Created => 1,
            Self::Changed => 2,
            Self::Deleted => 3,
        }
    }
}

/// How the server lays a text out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatOptions {
    pub tab_size: u32,
    pub insert_spaces: bool,
}

/// The kinds of questions a newer one replaces (for the same document).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Slot {
    Completion,
    Hover,
    Signature,
    Highlights,
}

/// A question's request, made on the driver's thread (with the documents as the server has
/// them): its method, its parameters, and the text its answer's positions are in.
struct Prepared {
    method: &'static str,
    params: Value,
    lines: Option<Arc<Lines>>,
}

type Build = Box<dyn FnOnce(&Driver) -> Result<Prepared, String> + Send>;
type Answer = Box<dyn FnOnce(Result<Value, String>, Option<Arc<Lines>>, &mut Driver) + Send>;

/// A question for the server.
struct Ask {
    token: u64,
    feature: Option<Feature>,
    slot: Option<(Slot, DocumentId)>,
    build: Build,
    answer: Answer,
    timeout: Duration,
}

/// A document opened.
struct Opened {
    doc: DocumentId,
    path: PathBuf,
    language_id: String,
    version: u64,
    text: TextSnapshot,
}

enum Input {
    Open(Box<Opened>),
    Change {
        doc: DocumentId,
        version: u64,
        text: TextSnapshot,
    },
    Close(DocumentId),
    Saved(PathBuf),
    Watched(Vec<(PathBuf, FileEvent)>),
    Configure(Value),
    Ask(Box<Ask>),
    Cancel(u64),
    Restart,
    Stop,
    /// From an instance's threads (its generation: an old instance's are ignored).
    Incoming(u64, Message),
    Exited(u64),
    Log(u64, String),
}

/// The handle on a server's client (cheap to clone the questions through). Dropped, the
/// server is stopped.
pub struct Client {
    name: String,
    tx: Sender<Input>,
    status: Arc<Status>,
    tokens: Arc<AtomicU64>,
    wake: Arc<dyn Fn() + Send + Sync>,
    /// Its driver ended (the server stopped for good).
    ended: Arc<AtomicBool>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client").field("name", &self.name).finish_non_exhaustive()
    }
}

/// The text of `snapshot`.
fn text_of(snapshot: &TextSnapshot) -> String {
    snapshot.slice(0..snapshot.len())
}

impl Client {
    /// Starts the server (on its own thread: this returns at once); `on_event` hears what
    /// it does, from that thread.
    pub fn start(
        config: Config,
        launcher: Box<dyn Launcher>,
        on_event: Arc<dyn Fn(Event) + Send + Sync>,
    ) -> std::io::Result<Self> {
        let (tx, rx) = mpsc::channel();
        let status = Arc::new(Status::default());
        let ended = Arc::new(AtomicBool::new(false));
        let name = config.name.clone();
        let wake = Arc::clone(&config.wake);
        let driver = Driver::new(
            config,
            launcher,
            rx,
            tx.clone(),
            Arc::clone(&status),
            on_event,
            Arc::clone(&ended),
        );
        std::thread::Builder::new().name(format!("lsp-{name}")).spawn(move || driver.run())?;
        Ok(Self { name, tx, status, tokens: Arc::new(AtomicU64::new(1)), wake, ended })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn status(&self) -> &Arc<Status> {
        &self.status
    }

    /// Its driver ended (stopped for good): a new client is needed.
    pub fn ended(&self) -> bool {
        self.ended.load(Ordering::Acquire)
    }

    pub fn open(
        &self,
        doc: DocumentId,
        path: PathBuf,
        language_id: String,
        version: u64,
        text: TextSnapshot,
    ) {
        let _ =
            self.tx.send(Input::Open(Box::new(Opened { doc, path, language_id, version, text })));
    }

    pub fn change(&self, doc: DocumentId, version: u64, text: TextSnapshot) {
        let _ = self.tx.send(Input::Change { doc, version, text });
    }

    pub fn close(&self, doc: DocumentId) {
        let _ = self.tx.send(Input::Close(doc));
    }

    pub fn saved(&self, path: PathBuf) {
        let _ = self.tx.send(Input::Saved(path));
    }

    pub fn watched(&self, changes: Vec<(PathBuf, FileEvent)>) {
        if !changes.is_empty() {
            let _ = self.tx.send(Input::Watched(changes));
        }
    }

    pub fn configure(&self, settings: Value) {
        let _ = self.tx.send(Input::Configure(settings));
    }

    /// Stops the server and starts it again (its crashes forgotten).
    pub fn restart(&self) {
        let _ = self.tx.send(Input::Restart);
    }

    pub fn stop(&self) {
        let _ = self.tx.send(Input::Stop);
    }

    /// Asks the server; `convert` turns its answer into the reply's value (`None`: no
    /// answer, the reply is gone).
    fn ask<T: Send + 'static>(
        &self,
        feature: Option<Feature>,
        slot: Option<(Slot, DocumentId)>,
        build: impl FnOnce(&Driver) -> Result<Prepared, String> + Send + 'static,
        convert: impl FnOnce(Result<Value, String>, Option<Arc<Lines>>, &mut Driver) -> Option<T>
        + Send
        + 'static,
    ) -> Reply<T> {
        let (tx, reply) = Reply::pair();
        let token = self.tokens.fetch_add(1, Ordering::Relaxed);
        let cancel = self.tx.clone();
        let reply = reply.on_cancel(move || {
            let _ = cancel.send(Input::Cancel(token));
        });
        let wake = Arc::clone(&self.wake);
        let answer: Answer = Box::new(move |result, lines, driver| {
            if let Some(value) = convert(result, lines, driver) {
                tx.send(value);
            }
            wake();
        });
        let ask =
            Ask { token, feature, slot, build: Box::new(build), answer, timeout: Duration::ZERO };
        let _ = self.tx.send(Input::Ask(Box::new(ask)));
        reply
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.tx.send(Input::Stop);
    }
}

/// A type of a server's type hierarchy: where it is, and the server's own item (asked
/// about again for its supertypes / subtypes).
#[derive(Debug, Clone)]
pub struct HierarchyItem {
    pub place: code_intelligence::hierarchy::Place,
    raw: Value,
}

/// A lens of a document: its title, or (`None`) the server's lens still to be resolved.
#[derive(Debug, Clone)]
pub struct LensItem {
    pub lens: Option<code_intelligence::servers::ServedLens>,
    raw: Option<Value>,
    doc: DocumentId,
}

/// A document as the server has it.
struct Doc {
    path: PathBuf,
    uri: String,
    language_id: String,
    /// The text the server was last sent.
    mirror: Mirror,
    /// A newer version not sent yet.
    pending: Option<(u64, TextSnapshot)>,
}

/// A running instance of the server.
struct Instance {
    writer: Sender<Vec<u8>>,
    _process: Option<Box<dyn std::any::Any + Send>>,
    /// Its `initialize` request, until it's answered (then it's ready).
    initializing: Option<i64>,
    started: Instant,
}

/// A request waiting for its answer.
struct Pending {
    method: &'static str,
    sent: Instant,
    deadline: Option<Instant>,
    slot: Option<(Slot, DocumentId)>,
    token: Option<u64>,
    lines: Option<Arc<Lines>>,
    answer: Option<Answer>,
}

/// How the server syncs documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyncKind {
    None,
    Full,
    Incremental,
}

pub(crate) struct Driver {
    config: Config,
    launcher: Box<dyn Launcher>,
    rx: Receiver<Input>,
    tx: Sender<Input>,
    status: Arc<Status>,
    on_event: Arc<dyn Fn(Event) + Send + Sync>,
    ended: Arc<AtomicBool>,
    docs: HashMap<DocumentId, Doc>,
    instance: Option<Instance>,
    generation: u64,
    next_id: i64,
    pending: HashMap<i64, Pending>,
    flush_at: Option<Instant>,
    restart_at: Option<Instant>,
    idle_since: Option<Instant>,
    crashes: Vec<Instant>,
    /// How many times in a row it crashed too often (its rest doubles each time).
    rests: u32,
    last_crash: Option<Instant>,
    capabilities: Capabilities,
    encoding: Encoding,
    sync: SyncKind,
    save: bool,
    /// The last completion's raw items (by its sequence), for resolving one.
    completions: (u64, Vec<Value>),
    completion_seq: u64,
    /// The last code actions offered (raw), for running one.
    actions: (u64, Vec<Value>),
    action_seq: u64,
    /// The diagnostics last published, by URI (sent back with a code action request).
    published: HashMap<String, Vec<Value>>,
    /// The documents whose diagnostics are asked for (a server that doesn't push them).
    pulls: HashMap<DocumentId, pull::Pull>,
    /// The titles of the work in progress, by token.
    progress_titles: HashMap<String, String>,
    finished: bool,
}

impl Driver {
    fn new(
        config: Config,
        launcher: Box<dyn Launcher>,
        rx: Receiver<Input>,
        tx: Sender<Input>,
        status: Arc<Status>,
        on_event: Arc<dyn Fn(Event) + Send + Sync>,
        ended: Arc<AtomicBool>,
    ) -> Self {
        Self {
            config,
            launcher,
            rx,
            tx,
            status,
            on_event,
            ended,
            docs: HashMap::new(),
            instance: None,
            generation: 0,
            next_id: 1,
            pending: HashMap::new(),
            flush_at: None,
            restart_at: None,
            idle_since: None,
            crashes: Vec::new(),
            rests: 0,
            last_crash: None,
            capabilities: Capabilities::default(),
            encoding: Encoding::Utf16,
            sync: SyncKind::Incremental,
            save: false,
            completions: (0, Vec::new()),
            completion_seq: 0,
            actions: (0, Vec::new()),
            action_seq: 0,
            published: HashMap::new(),
            pulls: HashMap::new(),
            progress_titles: HashMap::new(),
            finished: false,
        }
    }

    fn event(&self, event: Event) {
        (self.on_event)(event);
        (self.config.wake)();
    }

    fn ready(&self) -> bool {
        self.instance.as_ref().is_some_and(|i| i.initializing.is_none())
    }

    fn run(mut self) {
        self.start_instance();
        while !self.finished {
            let deadline = self.next_deadline();
            let input = match deadline {
                Some(at) => {
                    match self.rx.recv_timeout(at.saturating_duration_since(Instant::now())) {
                        Ok(input) => Some(input),
                        Err(RecvTimeoutError::Timeout) => None,
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
                None => match self.rx.recv() {
                    Ok(input) => Some(input),
                    Err(_) => break,
                },
            };
            if let Some(input) = input {
                self.handle(input);
            }
            self.on_time(Instant::now());
        }
        self.shutdown();
        self.ended.store(true, Ordering::Release);
        self.event(Event::Ended);
    }

    /// The next time something is due.
    fn next_deadline(&self) -> Option<Instant> {
        let starting = self
            .instance
            .as_ref()
            .filter(|i| i.initializing.is_some())
            .map(|i| i.started + self.config.timeouts.start);
        let idle = self
            .idle_since
            .filter(|_| self.ready() && self.docs.is_empty())
            .zip(self.config.idle)
            .map(|(s, i)| s + i);
        [self.flush_at, self.restart_at, starting, idle]
            .into_iter()
            .chain(self.pending.values().map(|p| p.deadline))
            .flatten()
            .min()
    }

    fn handle(&mut self, input: Input) {
        match input {
            Input::Open(opened) => self.open(*opened),
            Input::Change { doc, version, text } => {
                if let Some(d) = self.docs.get_mut(&doc) {
                    d.pending = Some((version, text));
                    self.flush_at.get_or_insert(Instant::now() + self.config.debounce);
                }
            }
            Input::Close(doc) => self.close(doc),
            Input::Saved(path) => {
                if self.ready()
                    && self.save
                    && let Some(d) = self.docs.values().find(|d| d.path == path)
                {
                    let uri = d.uri.clone();
                    self.flush();
                    self.notify("textDocument/didSave", json!({"textDocument": {"uri": uri}}));
                }
            }
            Input::Watched(changes) => {
                if self.ready() {
                    let changes: Vec<Value> = changes
                        .iter()
                        .map(|(path, kind)| json!({"uri": uri::from_path(path), "type": kind.code()}))
                        .collect();
                    self.notify("workspace/didChangeWatchedFiles", json!({"changes": changes}));
                }
            }
            Input::Configure(settings) => {
                self.config.settings = settings;
                if self.ready() {
                    let settings = self.config.settings.clone();
                    self.notify("workspace/didChangeConfiguration", json!({"settings": settings}));
                }
            }
            Input::Ask(ask) => self.ask(*ask),
            Input::Cancel(token) => {
                if let Some(id) =
                    self.pending.iter().find(|(_, p)| p.token == Some(token)).map(|(id, _)| *id)
                {
                    self.cancel(id);
                }
            }
            Input::Restart => {
                self.crashes.clear();
                self.rests = 0;
                self.restart_at = None;
                self.stop_instance();
                self.start_instance();
            }
            Input::Stop => {
                self.finished = true;
            }
            Input::Incoming(generation, message) if generation == self.generation => {
                self.incoming(message)
            }
            Input::Exited(generation) if generation == self.generation => self.exited(),
            Input::Log(generation, line) if generation == self.generation => {
                self.status.log(line);
            }
            Input::Incoming(..) | Input::Exited(_) | Input::Log(..) => {}
        }
    }

    fn on_time(&mut self, now: Instant) {
        if self.flush_at.is_some_and(|at| at <= now) {
            self.flush();
        }
        let expired: Vec<i64> = self
            .pending
            .iter()
            .filter(|(_, p)| p.deadline.is_some_and(|d| d <= now))
            .map(|(id, _)| *id)
            .collect();
        for id in expired {
            if let Some(p) = self.pending.get(&id) {
                self.status.error(format!("{} timed out", p.method));
            }
            self.cancel(id);
            self.event(Event::Changed);
        }
        if self.instance.as_ref().is_some_and(|i| {
            i.initializing.is_some() && i.started + self.config.timeouts.start <= now
        }) {
            self.status.error("it didn't start in time");
            self.exited();
        }
        if self.restart_at.is_some_and(|at| at <= now) {
            self.restart_at = None;
            self.start_instance();
        }
        let idle = self.config.idle.zip(self.idle_since).is_some_and(|(i, s)| s + i <= now);
        if idle && self.ready() && self.docs.is_empty() {
            self.status.log("stopped: no documents open for a while");
            self.finished = true;
        }
    }

    // ─── The instance ───

    fn drop_pending(&mut self) {
        // Their answers go: the replies say they're gone.
        self.pending.clear();
        self.pulls.clear();
    }

    fn clear_diagnostics(&mut self) {
        if !self.published.is_empty() {
            self.published.clear();
            self.event(Event::DiagnosticsCleared);
        }
    }

    // ─── Documents ───

    fn open(&mut self, opened: Opened) {
        let Opened { doc, path, language_id, version, text } = opened;
        let uri = uri::from_path(&path);
        self.docs.insert(
            doc,
            Doc {
                path,
                uri,
                language_id,
                mirror: Mirror::new(text_of(&text), version),
                pending: None,
            },
        );
        if self.ready() {
            self.did_open(doc);
        }
        self.set_ready_state();
    }

    fn did_open(&mut self, doc: DocumentId) {
        let Some(d) = self.docs.get_mut(&doc) else { return };
        // What wasn't sent yet is in what's opened.
        if let Some((version, text)) = d.pending.take() {
            d.mirror = Mirror::new(text_of(&text), version);
        }
        let params = json!({"textDocument": {
            "uri": d.uri,
            "languageId": d.language_id,
            "version": lsp_version(d.mirror.version),
            "text": d.mirror.text(),
        }});
        self.notify("textDocument/didOpen", params);
        self.pull_diagnostics(doc);
    }

    fn close(&mut self, doc: DocumentId) {
        let Some(d) = self.docs.remove(&doc) else { return };
        self.pulls.remove(&doc);
        if self.ready() {
            self.notify("textDocument/didClose", json!({"textDocument": {"uri": d.uri}}));
        }
        self.set_ready_state();
    }

    /// Sends the changes gathered.
    fn flush(&mut self) {
        self.flush_at = None;
        if !self.ready() {
            return;
        }
        let mut messages = Vec::new();
        let mut changed = Vec::new();
        for (doc, d) in &mut self.docs {
            let Some((version, text)) = d.pending.take() else { continue };
            let new = text_of(&text);
            let changes = match self.sync {
                SyncKind::None => {
                    d.mirror = Mirror::new(new, version);
                    continue;
                }
                SyncKind::Full => {
                    let changes = vec![json!({"text": new})];
                    d.mirror = Mirror::new(new, version);
                    changes
                }
                SyncKind::Incremental => match d.mirror.update(&new, version, self.encoding) {
                    Some(change) => vec![change.to_value()],
                    None => continue,
                },
            };
            changed.push(*doc);
            messages.push(json!({
                "textDocument": {"uri": d.uri, "version": lsp_version(version)},
                "contentChanges": changes,
            }));
        }
        for params in messages {
            self.notify("textDocument/didChange", params);
        }
        for doc in changed {
            self.pull_diagnostics(doc);
        }
    }

    // ─── Questions ───

    /// Where byte `offset` of `doc` is for the server (its URI, the position, its text).
    fn at(&self, doc: DocumentId, offset: usize) -> Result<(String, Position, Arc<Lines>), String> {
        let d = self.docs.get(&doc).ok_or("not open")?;
        let lines = d.mirror.shared();
        Ok((d.uri.clone(), lines.position(offset, self.encoding), lines))
    }

    fn position_request(
        &self,
        doc: DocumentId,
        offset: usize,
        method: &'static str,
        extra: Value,
    ) -> Result<Prepared, String> {
        let (uri, position, lines) = self.at(doc, offset)?;
        let mut params = json!({"textDocument": {"uri": uri}, "position": position.to_value()});
        if let (Value::Object(params), Value::Object(extra)) = (&mut params, extra) {
            params.extend(extra);
        }
        Ok(Prepared { method, params, lines: Some(lines) })
    }

    fn ask(&mut self, ask: Ask) {
        let Ask { token, feature, slot, build, answer, timeout } = ask;
        if !self.ready() || feature.is_some_and(|f| !self.capabilities.supports(f)) {
            return; // The reply is gone.
        }
        // The answer is about what the editor has: everything sent first.
        self.flush();
        // A newer question of its kind: the older one is stale.
        if let Some(slot) = slot
            && let Some(old) =
                self.pending.iter().find(|(_, p)| p.slot == Some(slot)).map(|(id, _)| *id)
        {
            self.cancel(old);
        }
        let prepared = match build(self) {
            Ok(prepared) => prepared,
            Err(why) => {
                answer(Err(why), None, self);
                return;
            }
        };
        // Nothing to ask (an action carrying its edit): answered with what it has.
        if prepared.method.is_empty() {
            answer(Ok(prepared.params), prepared.lines, self);
            return;
        }
        let id = self.request(prepared, timeout, answer);
        if let Some(p) = self.pending.get_mut(&id) {
            p.slot = slot;
            p.token = Some(token);
        }
    }

    /// Sends a request; `answer` gets its answer (dropped: never, the reply gone).
    fn request(&mut self, prepared: Prepared, timeout: Duration, answer: Answer) -> i64 {
        let id = self.next();
        let now = Instant::now();
        let timeout = if timeout.is_zero() { self.config.timeouts.request } else { timeout };
        self.write(&Message::Request {
            id: Id::Number(id),
            method: prepared.method.into(),
            params: prepared.params,
        });
        self.status.requested(now);
        self.pending.insert(
            id,
            Pending {
                method: prepared.method,
                sent: now,
                deadline: Some(now + timeout),
                slot: None,
                token: None,
                lines: prepared.lines,
                answer: Some(answer),
            },
        );
        id
    }

    /// Request `id` isn't wanted any more: the server is told, its answer dropped.
    fn cancel(&mut self, id: i64) {
        if self.pending.remove(&id).is_some() {
            self.notify("$/cancelRequest", json!({"id": id}));
        }
    }

    fn locations(&mut self, value: &Value) -> Vec<ServedLocation> {
        convert::locations(value, &mut DriverTexts::new(&self.docs), self.encoding)
    }

    /// A server's hierarchy type, as kept between its requests.
    fn hierarchy_item(&mut self, raw: Value) -> Option<HierarchyItem> {
        let place =
            convert::hierarchy_place(&raw, &mut DriverTexts::new(&self.docs), self.encoding)?;
        Some(HierarchyItem { raw, place })
    }

    fn workspace_edit(&mut self, value: &Value) -> WorkspaceChange {
        convert::workspace_edit(value, &mut DriverTexts::new(&self.docs), self.encoding)
    }

    /// A code action (resolved) run: its edit; its command, the server runs.
    fn run_code_action(&mut self, action: &Value) -> WorkspaceChange {
        let change = action.get("edit").map(|e| self.workspace_edit(e)).unwrap_or_default();
        // A command alone (`{title, command: "x", arguments}`), or an action's.
        let command = match action.get("command") {
            Some(Value::String(name)) => Some(
                json!({"command": name, "arguments": action.get("arguments").cloned().unwrap_or(json!([]))}),
            ),
            Some(Value::Object(c)) => Some(
                json!({"command": c.get("command"), "arguments": c.get("arguments").cloned().unwrap_or(json!([]))}),
            ),
            _ => None,
        };
        if let Some(params) = command {
            let prepared = Prepared { method: "workspace/executeCommand", params, lines: None };
            self.request(prepared, Duration::ZERO, Box::new(|_, _, _| {}));
        }
        change
    }

    // ─── Messages ───

    fn next(&mut self) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn write(&mut self, message: &Message) {
        let Some(instance) = &self.instance else { return };
        let value = message.to_value();
        let mut bytes = Vec::new();
        if jsonrpc::write_message(&mut bytes, &value).is_ok() {
            if self.config.trace.load(Ordering::Relaxed) {
                self.status.log(format!("→ {}", traced(&self.status.redacted(&value.to_string()))));
            }
            let _ = instance.writer.send(bytes);
        }
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.write(&Message::Notification { method: method.into(), params });
    }

    fn respond(&mut self, id: Id, result: Result<Value, ResponseError>) {
        self.write(&Message::Response { id, result });
    }
}

/// An editor version as LSP's (an integer).
fn lsp_version(version: u64) -> i64 {
    i64::try_from(version).unwrap_or(i64::MAX)
}

/// The settings' section `section` (`a.b`: `settings.a.b`; none: all of them).
fn section(settings: &Value, section: Option<&str>) -> Value {
    let Some(section) = section.filter(|s| !s.is_empty()) else { return settings.clone() };
    section.split('.').try_fold(settings, |v, key| v.get(key)).cloned().unwrap_or(Value::Null)
}

/// A message as the log shows it: short (cut after its secrets are taken out: never half of
/// one left).
fn traced(text: &str) -> String {
    let mut text = text.to_owned();
    if text.len() > 600 {
        let mut cut = 600;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
        text.push('…');
    }
    text
}

/// The texts of an answer's positions: the open documents as the server has them, the
/// others from disk.
struct DriverTexts<'a> {
    by_path: HashMap<&'a Path, &'a Doc>,
    disk: DiskTexts,
}

impl<'a> DriverTexts<'a> {
    fn new(docs: &'a HashMap<DocumentId, Doc>) -> Self {
        Self {
            by_path: docs.values().map(|d| (d.path.as_path(), d)).collect(),
            disk: DiskTexts::default(),
        }
    }
}

impl Texts for DriverTexts<'_> {
    fn lines(&mut self, path: &Path) -> Option<std::borrow::Cow<'_, Lines>> {
        if let Some(doc) = self.by_path.get(path) {
            return Some(std::borrow::Cow::Borrowed(doc.mirror.lines()));
        }
        self.disk.read(path).map(std::borrow::Cow::Borrowed)
    }
}

/// The thread writing to the server.
fn spawn_writer(name: &str, mut input: Box<dyn Write + Send>) -> Sender<Vec<u8>> {
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let spawned = std::thread::Builder::new().name(format!("lsp-{name}-in")).spawn(move || {
        while let Ok(bytes) = rx.recv() {
            if input.write_all(&bytes).and_then(|()| input.flush()).is_err() {
                break;
            }
        }
    });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "a language server's writer didn't start");
    }
    tx
}

/// The thread reading the server's messages.
fn spawn_reader(name: &str, output: Box<dyn Read + Send>, tx: Sender<Input>, generation: u64) {
    let spawned = std::thread::Builder::new().name(format!("lsp-{name}-out")).spawn(move || {
        let mut reader = BufReader::with_capacity(64 * 1024, output);
        loop {
            match jsonrpc::read_message(&mut reader) {
                Ok(Some(message)) => {
                    if tx.send(Input::Incoming(generation, message)).is_err() {
                        return;
                    }
                }
                // A frame that isn't a message: skipped, the stream goes on.
                Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
                    let _ = tx.send(Input::Log(generation, format!("a bad message: {e}")));
                }
                Ok(None) | Err(_) => break,
            }
        }
        let _ = tx.send(Input::Exited(generation));
    });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "a language server's reader didn't start");
    }
}

/// The thread reading the server's stderr into its log.
fn spawn_errors(name: &str, errors: Box<dyn Read + Send>, tx: Sender<Input>, generation: u64) {
    let spawned = std::thread::Builder::new().name(format!("lsp-{name}-err")).spawn(move || {
        let reader = BufReader::new(errors);
        for line in reader.lines() {
            let Ok(line) = line else { break };
            if tx.send(Input::Log(generation, line)).is_err() {
                break;
            }
        }
    });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "a language server's stderr reader didn't start");
    }
}

mod pull;

#[cfg(test)]
#[path = "../tests/unit/client.rs"]
mod tests;
