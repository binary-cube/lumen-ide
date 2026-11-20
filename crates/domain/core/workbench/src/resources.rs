//! What the parts of the app cost, for the Task Manager: each part — the app's own (open
//! files, rendering, the index) and the modules' (a remote connection…) — reports what it
//! holds, the project it works for (by its identity), and names the threads doing its
//! work, so their CPU counts for it. What only the module knows — the disk it reads and
//! writes, the CPU its work took when threads are shared by projects, the operations
//! running — it counts itself, with cheap atomic counters ([`DiskIo`], [`CpuMeter`],
//! [`Activities`]) read by the Task Manager only while it's open.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kernel::ProjectId;

/// A part of the app and what it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceUse {
    /// What it is, in the user's language ("Open files", "Remote: shop").
    pub name: String,
    /// Whose it is: a module's id, or `app`.
    pub owner: String,
    /// Memory it holds (bytes), when known.
    pub memory: Option<u64>,
    /// GPU memory it holds (bytes), when known.
    pub gpu: Option<u64>,
    /// A short detail ("12 files", "3 transfers running").
    pub detail: String,
    /// The threads doing its work, by name; one ending with `-` is a prefix (`git-`).
    pub threads: Vec<&'static str>,
    /// Where it's listed: [`APP`], [`INDEXING`], [`TERMINALS`], [`PROGRAMS`],
    /// [`LEFT_BEHIND`], [`SERVICES`] (connections and the like), [`LANGUAGE_SERVERS`] or
    /// [`MODULES`].
    pub group: &'static str,
    /// The data it sent and received over the network so far (bytes, counting up), when
    /// it uses it: the Task Manager shows the rate.
    pub sent: Option<u64>,
    pub received: Option<u64>,
    /// The project it works for (`None`: the app's, every project's).
    pub project: Option<ProjectId>,
    /// What it read from and wrote to the disk so far (bytes, counting up), when it counts
    /// it ([`DiskIo`]): the Task Manager shows the rate.
    pub read: Option<u64>,
    pub written: Option<u64>,
    /// The CPU time its work took so far, measured by itself ([`CpuMeter`]): used instead of
    /// its threads' when they're shared (every project's indexer has the same name).
    pub cpu: Option<Duration>,
    /// Its operations running, and the last ones finished.
    pub activities: Vec<Activity>,
    /// The programs it started and are running (process ids: a terminal's shell, `git`, a
    /// formatter): their CPU, memory and disk — their descendants' too — count for it.
    pub processes: Vec<u32>,
    /// The project each of its programs works for, when the part is every project's (the
    /// terminals: a shell or an agent of a project's): said on the program's row.
    pub process_projects: Vec<(u32, ProjectId)>,
    /// What can be done to it from the Task Manager (stop a program, close a session,
    /// release its data): done by whoever reported it ([`ResourceReporter::act`]), never by
    /// the Task Manager itself.
    pub actions: Vec<PartAction>,
    /// What only its reporter knows of it, each a label and a value in the user's language
    /// (a language server's state, its latency, its last error), shown in its detail.
    pub facts: Vec<(String, String)>,
}

/// What an action does (its label, in the Task Manager's language, comes from this).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionKind {
    /// Stops a program or an operation.
    Stop,
    /// Closes a session (a terminal's shell).
    Close,
    /// Starts it again (a language server).
    Restart,
    /// Lets its data go (read again when needed).
    Release,
    /// Closes its idle connections.
    Disconnect,
    /// Shows what it wrote (a language server's log).
    ShowLog,
    /// Opens its settings.
    Settings,
}

/// What an action is done to: the part itself, one of its programs, one of its operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionTarget {
    Part,
    Process(u32),
    Activity(u128),
}

/// An action a part offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PartAction {
    /// What its reporter is asked to do ([`ResourceReporter::act`]).
    pub id: &'static str,
    pub kind: ActionKind,
    pub target: ActionTarget,
    /// Work may be lost (a program stopped, a shell closed): asked first.
    pub confirm: bool,
}

impl PartAction {
    pub const fn new(id: &'static str, kind: ActionKind, target: ActionTarget) -> Self {
        Self { id, kind, target, confirm: false }
    }

    /// The same, asked first.
    pub const fn confirmed(mut self) -> Self {
        self.confirm = true;
        self
    }
}

/// The part an action is for, as its reporter named it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartRef {
    pub owner: String,
    pub name: String,
    pub project: Option<ProjectId>,
}

impl PartRef {
    /// Whether `part` is the one named.
    pub fn is(&self, part: &ResourceUse) -> bool {
        part.owner == self.owner && part.name == self.name && part.project == self.project
    }
}

/// How an action went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActOutcome {
    /// Done (or started: what's long goes on in the background).
    Done,
    /// It couldn't be done: why.
    Failed(String),
    /// Not one it does (the part is gone, or nobody offers it).
    Unsupported,
    /// Done by running command `id` (a view opened: a log, the settings), as from its menu.
    Command(String),
}

/// What a part read from and wrote to the disk, counted as it goes (from any thread), for
/// its [`ResourceUse::read`] / [`ResourceUse::written`].
#[derive(Debug, Default)]
pub struct DiskIo {
    read: AtomicU64,
    written: AtomicU64,
}

impl DiskIo {
    pub const fn new() -> Self {
        Self { read: AtomicU64::new(0), written: AtomicU64::new(0) }
    }

    pub fn read(&self, bytes: u64) {
        self.read.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn written(&self, bytes: u64) {
        self.written.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Its totals on `part`.
    pub fn report(&self, part: &mut ResourceUse) {
        part.read = Some(self.read.load(Ordering::Relaxed));
        part.written = Some(self.written.load(Ordering::Relaxed));
    }
}

/// What a part sent and received over the network (bytes), counted as it goes (from any
/// thread), for its [`ResourceUse::sent`] / [`ResourceUse::received`]: only how much,
/// never to where or what.
#[derive(Debug, Default)]
pub struct NetIo {
    sent: AtomicU64,
    received: AtomicU64,
}

impl NetIo {
    pub const fn new() -> Self {
        Self { sent: AtomicU64::new(0), received: AtomicU64::new(0) }
    }

    pub fn sent(&self, bytes: u64) {
        self.sent.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn received(&self, bytes: u64) {
        self.received.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Its totals on `part`.
    pub fn report(&self, part: &mut ResourceUse) {
        part.sent = Some(self.sent.load(Ordering::Relaxed));
        part.received = Some(self.received.load(Ordering::Relaxed));
    }
}

/// The CPU time a part's work took, measured on the thread doing it (two clock reads per
/// piece of work, not per byte), for its [`ResourceUse::cpu`].
#[derive(Debug, Default)]
pub struct CpuMeter {
    nanos: AtomicU64,
}

impl CpuMeter {
    pub const fn new() -> Self {
        Self { nanos: AtomicU64::new(0) }
    }

    /// Runs `work`, counting the CPU time it took on this thread.
    pub fn measure<T>(&self, work: impl FnOnce() -> T) -> T {
        let start = kernel::cpu::thread_time();
        let result = work();
        if let (Some(start), Some(end)) = (start, kernel::cpu::thread_time()) {
            self.add(end.saturating_sub(start));
        }
        result
    }

    pub fn add(&self, time: Duration) {
        let nanos = u64::try_from(time.as_nanos()).unwrap_or(u64::MAX);
        self.nanos.fetch_add(nanos, Ordering::Relaxed);
    }

    /// Its total on `part`.
    pub fn report(&self, part: &mut ResourceUse) {
        part.cpu = Some(Duration::from_nanos(self.nanos.load(Ordering::Relaxed)));
    }
}

/// What an operation is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActivityKind {
    Upload,
    Download,
    /// Both ways (a remote project's sync).
    Sync,
    Copy,
    Move,
    Delete,
    Index,
    History,
    Save,
    /// A table's rows written to a file.
    Export,
    /// A query (a statement, a script) running on a database.
    Query,
    /// What a folder holds, counted (its size on a server).
    Measure,
    /// Files' permissions, owners or groups changed (on a server).
    Permissions,
}

impl ActivityKind {
    /// Its name's key (`tasks.op_<key>`), the same in every list of operations.
    pub fn key(self) -> &'static str {
        match self {
            Self::Upload => "upload",
            Self::Download => "download",
            Self::Sync => "sync",
            Self::Copy => "copy",
            Self::Move => "move",
            Self::Delete => "delete",
            Self::Index => "index",
            Self::History => "history",
            Self::Save => "save",
            Self::Export => "export",
            Self::Query => "query",
            Self::Measure => "measure",
            Self::Permissions => "permissions",
        }
    }

    /// Its icon (in the app's set).
    pub fn icon(self) -> &'static str {
        match self {
            Self::Upload => "upload",
            Self::Download | Self::Export => "download",
            Self::Sync => "refresh",
            Self::Copy => "copy",
            Self::Move => "folder-input",
            Self::Delete => "trash",
            Self::Index => "search",
            Self::History => "clock",
            Self::Save => "file-check",
            Self::Query => "database",
            Self::Measure => "hard-drive",
            Self::Permissions => "lock",
        }
    }
}

/// What an operation counts its progress in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityUnit {
    Bytes,
    Files,
    /// A table's rows.
    Rows,
}

/// How an operation ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Done,
    Failed(String),
    Stopped,
}

/// An operation (running, or one of the last finished): what it is, on what, how far.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Activity {
    /// Its identity (a UUID v4: never a name or a position).
    pub id: u128,
    pub project: Option<ProjectId>,
    pub kind: ActivityKind,
    /// What it works on (a file, a folder, "12 files", a query's first line).
    pub subject: String,
    /// Where, in the user's language (a query's source, database and console); may be
    /// empty.
    pub detail: String,
    /// The whole of what it works on, when the subject is only its start (a query's text).
    pub text: Option<String>,
    pub unit: ActivityUnit,
    pub done: u64,
    /// How much in all, when known.
    pub total: Option<u64>,
    pub started: Instant,
    /// When and how it ended (`None`: still running).
    pub ended: Option<(Instant, Outcome)>,
    /// It can be stopped ([`Activities::stoppable`]): it then ends [`Outcome::Stopped`].
    pub stoppable: bool,
}

impl Activity {
    pub fn running(&self) -> bool {
        self.ended.is_none()
    }
}

/// The operations a part runs: started, advanced and ended from any thread; the last
/// [`Activities::KEPT`] ended ones of each project are kept (for the Task Manager), older
/// ones forgotten.
#[derive(Debug)]
pub struct Activities {
    list: Mutex<VecDeque<Activity>>,
    /// What stops each stoppable one, while it runs.
    stops: Mutex<Vec<(u128, Stopper)>>,
    /// What tells the time an operation started and ended (the domain doesn't read the
    /// clock itself).
    clock: fn() -> Instant,
}

impl Activities {
    /// The ended operations kept per project.
    pub const KEPT: usize = 50;

    /// Operations timed by `clock` (`std::time::Instant::now`, given by whoever makes them).
    pub const fn new(clock: fn() -> Instant) -> Self {
        Self { list: Mutex::new(VecDeque::new()), stops: Mutex::new(Vec::new()), clock }
    }

    /// Operation `id` checks `flag` as it goes and ends [`Outcome::Stopped`] once it's set:
    /// the Task Manager can stop it ([`Self::stop`]).
    pub fn stoppable(&self, id: u128, flag: Arc<std::sync::atomic::AtomicBool>) {
        self.stoppable_with(id, move || flag.store(true, Ordering::Relaxed));
    }

    /// Operation `id` is stopped by `stop` (which tells it to end, without waiting).
    pub fn stoppable_with(&self, id: u128, stop: impl Fn() + Send + Sync + 'static) {
        let Ok(mut list) = self.list.lock() else { return };
        let Some(a) = list.iter_mut().find(|a| a.id == id && a.running()) else { return };
        a.stoppable = true;
        if let Ok(mut stops) = self.stops.lock() {
            stops.retain(|(i, _)| *i != id);
            stops.push((id, Stopper(Arc::new(stop))));
        }
    }

    /// Operation `id` asked to stop: `false` if it can't be (ended, never stoppable).
    pub fn stop(&self, id: u128) -> bool {
        let stop =
            self.stops.lock().ok().and_then(|stops| {
                stops.iter().find(|(i, _)| *i == id).map(|(_, s)| Arc::clone(&s.0))
            });
        match stop {
            Some(stop) => {
                stop();
                true
            }
            None => false,
        }
    }

    /// An action on one of its operations ([`ActionTarget::Activity`]) done: stopped.
    pub fn act(&self, target: ActionTarget) -> ActOutcome {
        match target {
            ActionTarget::Activity(id) if self.stop(id) => ActOutcome::Done,
            _ => ActOutcome::Unsupported,
        }
    }

    /// An operation starts: its identity (to advance and end it).
    pub fn start(
        &self,
        project: Option<ProjectId>,
        kind: ActivityKind,
        subject: impl Into<String>,
        unit: ActivityUnit,
        total: Option<u64>,
    ) -> u128 {
        let id = kernel::id::new_uuid();
        let activity = Activity {
            id,
            project,
            kind,
            subject: subject.into(),
            detail: String::new(),
            text: None,
            unit,
            done: 0,
            total,
            started: (self.clock)(),
            ended: None,
            stoppable: false,
        };
        if let Ok(mut list) = self.list.lock() {
            list.push_back(activity);
        }
        id
    }

    /// Operation `id` described: where it runs (`detail`) and the whole of its subject.
    pub fn describe(&self, id: u128, detail: impl Into<String>, text: Option<String>) {
        if let Ok(mut list) = self.list.lock()
            && let Some(a) = list.iter_mut().find(|a| a.id == id)
        {
            a.detail = detail.into();
            a.text = text;
        }
    }

    /// Operation `id` forgotten, as if it never ran (one too quick to be worth keeping).
    pub fn discard(&self, id: u128) {
        if let Ok(mut stops) = self.stops.lock() {
            stops.retain(|(i, _)| *i != id);
        }
        if let Ok(mut list) = self.list.lock() {
            list.retain(|a| a.id != id);
        }
    }

    /// Operation `id`'s total, known once it's counted.
    pub fn set_total(&self, id: u128, total: u64) {
        if let Ok(mut list) = self.list.lock()
            && let Some(a) = list.iter_mut().find(|a| a.id == id)
        {
            a.total = Some(total);
        }
    }

    /// Operation `id` got `amount` further.
    pub fn advance(&self, id: u128, amount: u64) {
        if let Ok(mut list) = self.list.lock()
            && let Some(a) = list.iter_mut().find(|a| a.id == id)
        {
            a.done = a.done.saturating_add(amount);
        }
    }

    /// Operation `id` ended: kept among its project's last ended ones.
    pub fn end(&self, id: u128, outcome: Outcome) {
        if let Ok(mut stops) = self.stops.lock() {
            stops.retain(|(i, _)| *i != id);
        }
        let Ok(mut list) = self.list.lock() else { return };
        let now = (self.clock)();
        let Some(project) = list.iter_mut().find(|a| a.id == id).map(|a| {
            a.ended = Some((now, outcome));
            a.project
        }) else {
            return;
        };
        // Its project's ended ones beyond the last KEPT go (the oldest first).
        let ended = list.iter().filter(|a| a.project == project && !a.running()).count();
        let mut extra = ended.saturating_sub(Self::KEPT);
        list.retain(|a| {
            let forget = extra > 0 && a.project == project && !a.running();
            if forget {
                extra -= 1;
            }
            !forget
        });
    }

    /// Project `project` closed: its operations are forgotten.
    pub fn forget(&self, project: ProjectId) {
        if let Ok(mut list) = self.list.lock() {
            list.retain(|a| a.project != Some(project));
        }
    }

    /// Its operations (of `project`, or all) on `part`, each running stoppable one with
    /// its "Stop" ([`Self::act`] does it).
    pub fn report(&self, part: &mut ResourceUse) {
        if let Ok(list) = self.list.lock() {
            part.activities = list
                .iter()
                .filter(|a| part.project.is_none() || a.project == part.project)
                .cloned()
                .collect();
        }
        part.actions.extend(part.activities.iter().filter(|a| a.stoppable && a.running()).map(
            |a| PartAction::new(STOP_OPERATION, ActionKind::Stop, ActionTarget::Activity(a.id)),
        ));
    }
}

/// The action stopping an operation ([`Activities::report`] offers it).
pub const STOP_OPERATION: &str = "stop-operation";

/// What stops an operation.
struct Stopper(Arc<dyn Fn() + Send + Sync>);

impl std::fmt::Debug for Stopper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Stopper")
    }
}

/// The bytes a part sent and received over the network, counted as they pass (from any
/// thread, mid-transfer — not when a file is done), for its [`ResourceUse::sent`] /
/// [`ResourceUse::received`].
#[derive(Debug, Default)]
pub struct Traffic {
    sent: std::sync::atomic::AtomicU64,
    received: std::sync::atomic::AtomicU64,
}

impl Traffic {
    pub const fn new() -> Self {
        Self {
            sent: std::sync::atomic::AtomicU64::new(0),
            received: std::sync::atomic::AtomicU64::new(0),
        }
    }

    pub fn sent(&self, bytes: u64) {
        self.sent.fetch_add(bytes, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn received(&self, bytes: u64) {
        self.received.fetch_add(bytes, std::sync::atomic::Ordering::Relaxed);
    }

    /// Its totals on `part`.
    pub fn report(&self, part: &mut ResourceUse) {
        part.sent = Some(self.sent.load(std::sync::atomic::Ordering::Relaxed));
        part.received = Some(self.received.load(std::sync::atomic::Ordering::Relaxed));
    }
}

/// The groups of the Task Manager's list: the app's core (the interface, open files,
/// rendering, syntax)…
pub const APP: &str = "app";
/// … indexing (the project's index, the app's symbols)…
pub const INDEXING: &str = "indexing";
/// … the terminal's sessions…
pub const TERMINALS: &str = "terminals";
/// … the programs started outside the app (git, formatters and checkers)…
pub const PROGRAMS: &str = "programs";
/// … what an earlier run that ended without stopping them left running (a terminal's
/// shell, an agent), while it runs…
pub const LEFT_BEHIND: &str = "left_behind";
/// … connections (data sources, remote projects)…
pub const SERVICES: &str = "services";
/// … language servers (the language servers module's: one part per server running for a
/// project)…
pub const LANGUAGE_SERVERS: &str = "language_servers";
/// … and the modules themselves.
pub const MODULES: &str = "modules";

impl ResourceUse {
    /// A part named `name`, owned by `owner`, holding nothing known yet.
    pub fn new(name: impl Into<String>, owner: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            owner: owner.into(),
            memory: None,
            gpu: None,
            detail: String::new(),
            threads: Vec::new(),
            group: APP,
            sent: None,
            received: None,
            project: None,
            read: None,
            written: None,
            cpu: None,
            activities: Vec::new(),
            processes: Vec::new(),
            process_projects: Vec::new(),
            actions: Vec::new(),
            facts: Vec::new(),
        }
    }

    /// Whether thread `name` works for it.
    pub fn runs_on(&self, name: &str) -> bool {
        self.threads.iter().any(|t| {
            if t.ends_with('-') { name.starts_with(t) && name.len() > t.len() } else { name == *t }
        })
    }
}

/// The threads a module's work runs on (contributed; one ending with `-` is a prefix): their
/// CPU counts on the module's row, after the parts its reporter names (a project's).
#[derive(Debug, Clone, Copy)]
pub struct ModuleThreads(pub &'static [&'static str]);

/// What a module counts itself, every project's (contributed): its row in the Task Manager
/// shows their rates.
#[derive(Debug, Clone, Copy, Default)]
pub struct ModuleCounters {
    /// What it reads and writes on the disk (files it opens itself: images, sources).
    pub disk: Option<&'static DiskIo>,
    /// What it sends and receives over the network.
    pub traffic: Option<&'static Traffic>,
}

/// A module's part in the Task Manager (contributed): what it holds now. Called from the
/// Task Manager's thread while it's open, so it answers from what it already knows.
pub trait ResourceReporter: Send + Sync + std::fmt::Debug {
    fn report(&self) -> Vec<ResourceUse>;
    /// What it runs now that a closing would stop (asked only when a window, a project or
    /// the app closes — on the UI thread: quick, from what it already knows). By default its
    /// parts' operations still running ([`crate::running::of_parts`]); a module adds what
    /// isn't an operation (a program in a terminal, a query).
    fn running(&self) -> Vec<crate::running::RunningWork> {
        crate::running::of_parts(&self.report())
    }
    /// One of the actions its part `part` offers ([`ResourceUse::actions`]): `action` on
    /// `target`. Called on the UI thread: quick, never waiting — what takes long starts on
    /// the module's own thread (its result then told as a notification).
    fn act(&self, part: &PartRef, action: &str, target: ActionTarget) -> ActOutcome {
        let _ = (part, action, target);
        ActOutcome::Unsupported
    }
}

/// An open project, as the Task Manager names it: its identity, its name and its root (only
/// shown — it's known by its identity).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenProject {
    pub id: ProjectId,
    pub name: String,
    pub root: std::path::PathBuf,
}

/// The whole picture: the process's memory and GPU memory, and every part.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResourceSnapshot {
    /// The process's memory (bytes), as the system counts it.
    pub memory: u64,
    /// What the process read from and wrote to the disk so far, as the system counts it
    /// (to compare with the parts' sum), when it tells.
    pub disk: Option<(u64, u64)>,
    /// The open projects (every window's).
    pub projects: Vec<OpenProject>,
    /// This window's picture: the other windows' projects, only to name the programs of
    /// theirs it lists (a terminal's are every project's); empty in every window's.
    pub elsewhere: Vec<OpenProject>,
    /// The GPU memory the app's rendering holds (bytes), when known.
    pub gpu: Option<u64>,
    /// Of `gpu`, what every window draws with (the glyph atlas, the images): a window's
    /// picture counts it, every window's once.
    pub gpu_shared: Option<u64>,
    /// The heap's live bytes (the app's data, apart from the code, the GPU's memory and the
    /// system's libraries), when the allocator counts them.
    pub heap: Option<u64>,
    pub parts: Vec<ResourceUse>,
    /// What draws it: the driver, its backend and the graphics adapter, as the system names
    /// them (`wgpu / Metal / Apple M3`), when known; the window's scale (percent: 200 on a
    /// Retina screen).
    pub renderer: Option<String>,
    pub scale_percent: Option<u32>,
}

/// Where the Task Manager reads the picture from (the app gives it; it gathers the app's
/// parts and the modules' reporters).
pub trait ResourceSource: Send + Sync {
    /// This window's: its project's parts and the app's (what's every project's).
    fn snapshot(&self) -> ResourceSnapshot;
    /// Every window's: each open project's parts, the app's once.
    fn snapshot_all(&self) -> ResourceSnapshot {
        self.snapshot()
    }
    /// Both at once ([`Self::snapshot`], [`Self::snapshot_all`]), for two views looking at
    /// different scopes: a source that can reads what they share (the modules' reports)
    /// once.
    fn snapshot_both(&self) -> (ResourceSnapshot, ResourceSnapshot) {
        (self.snapshot(), self.snapshot_all())
    }
    /// An action of part `part` done by whoever reported it (the app for its own parts, a
    /// module's reporter for its): see [`ResourceReporter::act`].
    fn act(&self, part: &PartRef, action: &str, target: ActionTarget) -> ActOutcome {
        let _ = (part, action, target);
        ActOutcome::Unsupported
    }
}

/// What the Task Manager is opened with.
#[derive(Clone)]
pub struct ResourceData(pub Arc<dyn ResourceSource>);

impl std::fmt::Debug for ResourceData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ResourceData")
    }
}

#[cfg(test)]
#[path = "../tests/unit/resources.rs"]
mod tests;
