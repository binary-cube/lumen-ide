//! A project's request runs, shared by what starts them (the `.http` file's bar, its ▶ in
//! the gutter, ⌘↵) and what shows them (the Services window): each run's request, its
//! state, its response; the environment chosen for each file; the project's cookies. One
//! per project (a file opened alone: one of its own), held by whoever uses it, gone with
//! the last one. Each change bumps its generation and wakes whoever follows it.

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Instant;

use http_requests::cookies::CookieJar;
use http_requests::transport::Incoming;
use http_requests::vars::ResolvedRequest;
use kernel::ProjectId;
use kernel::work::Cancel;
use ui_components::editor_toolbar::{MarkState, StatementMark, text_hash};

/// The most runs kept (the oldest go first).
const KEPT: usize = 50;

/// A run's identity (a UUID: its node in the Services window).
pub type RunId = u128;

/// How a run went.
#[derive(Debug, Clone)]
pub enum RunState {
    Running,
    Done(Arc<Outcome>),
    /// Never sent (a variable missing, a file not found) or no response (no connection).
    Failed(String),
}

/// What a run asks of the app.
#[derive(Debug)]
enum Asked {
    Page(String),
    Tell(String),
}

/// What a run got.
#[derive(Debug)]
pub struct Outcome {
    /// What was sent (a secret in it: its values hidden when shown).
    pub request: ResolvedRequest,
    pub response: Incoming,
    /// The response's body as saved for the editor (`<project dir>/http/responses/…`).
    pub saved: Option<PathBuf>,
    /// A copy of a large body not saved (`# @no-log`: not in the history), in the module's
    /// private folder, so the memory budget can leave it there too.
    pub kept: Option<PathBuf>,
    /// Its body read as JSON once, on the run's worker (one up to [`JSON_KEPT`] held in
    /// memory): what completion and the requests chained after it read, never parsed again.
    pub json: Option<Arc<serde_json::Value>>,
    /// What its scripts did (`None`: it has none): their tests, their logs, a failure.
    pub script: Option<http_requests::script::ScriptRun>,
}

/// The largest body kept read as JSON (a larger one is read when a chained request needs it,
/// on its worker; completion doesn't offer its paths).
pub const JSON_KEPT: usize = 1024 * 1024;

/// The most bytes of bodies a project's runs hold in memory: past it, the oldest saved ones
/// are read from their files when they're shown again (each is saved as it comes).
pub const IN_MEMORY: usize = 64 * 1024 * 1024;

/// The most of a run's response kept to be shown while it comes (its last part).
pub const LIVE_KEPT: usize = 512 * 1024;
/// How often a run's views are told of what came.
const LIVE_EVERY: std::time::Duration = std::time::Duration::from_millis(100);

impl Outcome {
    /// What it holds in memory: its body, and its JSON read (roughly four times its bytes).
    fn held(&self) -> usize {
        match &self.response.body {
            http_requests::transport::IncomingBody::Memory(bytes) => {
                bytes.len() * if self.json.is_some() { 5 } else { 1 }
            }
            http_requests::transport::IncomingBody::File { .. } => 0,
        }
    }

    /// The same with its body left in the file it was saved to (`None`: not saved, or not
    /// in memory).
    fn lightened(&self) -> Option<Self> {
        let path = self.saved.clone().or_else(|| self.kept.clone())?;
        let http_requests::transport::IncomingBody::Memory(bytes) = &self.response.body else {
            return None;
        };
        let r = &self.response;
        Some(Self {
            request: self.request.clone(),
            response: http_requests::transport::Incoming {
                status: r.status,
                reason: r.reason.clone(),
                version: r.version.clone(),
                headers: r.headers.clone(),
                body: http_requests::transport::IncomingBody::File {
                    path,
                    len: bytes.len() as u64,
                },
                url: r.url.clone(),
                hops: r.hops.clone(),
                timings: r.timings,
            },
            saved: self.saved.clone(),
            kept: self.kept.clone(),
            json: None,
            script: self.script.clone(),
        })
    }

    /// `response`'s body read as JSON, if it's JSON held in memory and small enough to keep.
    pub fn json_of(
        response: &http_requests::transport::Incoming,
    ) -> Option<Arc<serde_json::Value>> {
        let http_requests::transport::IncomingBody::Memory(bytes) = &response.body else {
            return None;
        };
        let json =
            crate::format::kind(response.media_type().as_deref()) == crate::format::Kind::Json;
        (json && bytes.len() <= JSON_KEPT)
            .then(|| serde_json::from_slice(bytes).ok().map(Arc::new))
            .flatten()
    }
}

/// A run of a request.
#[derive(Debug, Clone)]
pub struct Run {
    pub id: RunId,
    /// Counted from 1 in the project, shown as `#3`.
    pub number: u32,
    pub file: PathBuf,
    /// The request's name, else its method and URL as written.
    pub label: String,
    pub method: String,
    /// Its request line's bytes in the file, and their text's hash (its mark shows while
    /// they're unchanged).
    pub line: Range<usize>,
    /// Its request line's number (0-based): where its node leads.
    pub line_number: u32,
    pub hash: u64,
    pub state: RunState,
    pub started: Instant,
    pub cancel: Cancel,
}

#[derive(Debug, Default)]
struct State {
    runs: Vec<Run>,
    /// The environment chosen per file (`None`: none, `$shared` only).
    environments: HashMap<PathBuf, Option<String>>,
    counter: u32,
    /// The run just started (to show), taken once.
    reveal: Option<RunId>,
}

/// The oldest runs' bodies left in their files until `runs` hold at most `budget` bytes in
/// memory (the newest ones stay: what's looked at, chained, completed from).
fn lighten(runs: &mut [Run], budget: usize) {
    let mut held: usize = runs
        .iter()
        .map(|r| match &r.state {
            RunState::Done(o) => o.held(),
            _ => 0,
        })
        .sum();
    for run in runs.iter_mut() {
        if held <= budget {
            break;
        }
        let RunState::Done(outcome) = &run.state else { continue };
        if let Some(light) = outcome.lightened() {
            held -= outcome.held();
            run.state = RunState::Done(Arc::new(light));
        }
    }
    // Still over it: the bodies that have no file to be left in (a `# @no-log` one under its
    // private copy's size) let go of their JSON read (four times their bytes; read again
    // from the body when asked), oldest first.
    for run in runs.iter_mut() {
        if held <= budget {
            break;
        }
        let RunState::Done(outcome) = &run.state else { continue };
        if outcome.json.is_none() {
            continue;
        }
        let light = Outcome {
            request: outcome.request.clone(),
            response: outcome.response.clone(),
            saved: outcome.saved.clone(),
            kept: outcome.kept.clone(),
            json: None,
            script: outcome.script.clone(),
        };
        held -= outcome.held() - light.held();
        run.state = RunState::Done(Arc::new(light));
    }
}

/// A project's runs.
#[derive(Debug)]
pub struct Runs {
    project: Option<ProjectId>,
    /// Told by the first who knows it (a bar; the Services window doesn't).
    root: Mutex<Option<PathBuf>>,
    state: Mutex<State>,
    generation: AtomicU64,
    wakes: Mutex<Vec<Weak<dyn Fn() + Send + Sync>>>,
    /// The project's cookies (read from its file at the first run).
    pub(crate) cookies: Mutex<Option<CookieJar>>,
    /// `client.global`: what the project's scripts keep between requests (until it closes).
    globals: Mutex<std::collections::BTreeMap<String, String>>,
    /// `client.global.headers`: sent with every request (its own headers first).
    headers: Mutex<std::collections::BTreeMap<String, String>>,
    /// The project's OAuth tokens.
    pub(crate) tokens: crate::auth::Tokens,
    /// What its requests sent and received (bytes: how much only), for its row in the
    /// Task Manager and a profile recording.
    pub(crate) traffic: workbench_model::NetIo,
    /// What a run asks of the app (a page opened in the system's browser for an OAuth
    /// sign-in, a message for the user), taken by the first of the module's views to poll.
    asked: Mutex<Vec<Asked>>,
    /// What a run going has received so far (its last part), and when its views were last
    /// told (they're told at most every [`LIVE_EVERY`]).
    live: Mutex<HashMap<RunId, (Vec<u8>, Instant)>>,
    /// Counts what came to the runs going (apart from [`Self::generation`]: only a response's
    /// own view reads it; the bars, the editors, Services don't redo their work for it).
    live_generation: AtomicU64,
}

type Registry = Mutex<Vec<(Option<ProjectId>, Weak<Runs>)>>;

fn registry() -> &'static Registry {
    static RUNS: Registry = Mutex::new(Vec::new());
    &RUNS
}

impl Runs {
    /// `project`'s runs (made if none is held), its root `root`.
    pub fn of(project: Option<ProjectId>, root: Option<&Path>) -> Arc<Self> {
        let mut all = registry().lock().unwrap_or_else(PoisonError::into_inner);
        all.retain(|(_, w)| w.strong_count() > 0);
        // A file opened alone shares nothing: its own.
        if project.is_some()
            && let Some(runs) =
                all.iter().find(|(p, _)| *p == project).and_then(|(_, w)| w.upgrade())
        {
            if let Some(root) = root {
                runs.root
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get_or_insert_with(|| root.to_path_buf());
            }
            return runs;
        }
        let runs = Arc::new(Self {
            traffic: workbench_model::NetIo::new(),
            project,
            root: Mutex::new(root.map(Path::to_path_buf)),
            state: Mutex::default(),
            generation: AtomicU64::new(0),
            live_generation: AtomicU64::new(0),
            wakes: Mutex::default(),
            cookies: Mutex::new(None),
            globals: Mutex::default(),
            headers: Mutex::default(),
            tokens: crate::auth::Tokens::default(),
            asked: Mutex::default(),
            live: Mutex::default(),
        });
        all.push((project, Arc::downgrade(&runs)));
        runs
    }

    /// Every project's runs held now.
    pub fn all() -> Vec<Arc<Self>> {
        let all = registry().lock().unwrap_or_else(PoisonError::into_inner);
        all.iter().filter_map(|(_, w)| w.upgrade()).collect()
    }

    /// What its responses hold in memory (their bodies and their JSON read; one in a file:
    /// nothing).
    pub fn memory(&self) -> u64 {
        self.state()
            .runs
            .iter()
            .map(|r| match &r.state {
                RunState::Done(o) => o.held() as u64,
                _ => 0,
            })
            .sum()
    }

    /// `project`'s runs if anyone holds them.
    pub fn held(project: ProjectId) -> Option<Arc<Self>> {
        let all = registry().lock().unwrap_or_else(PoisonError::into_inner);
        all.iter().find(|(p, _)| *p == Some(project)).and_then(|(_, w)| w.upgrade())
    }

    pub fn project(&self) -> Option<ProjectId> {
        self.project
    }

    pub fn root(&self) -> Option<PathBuf> {
        self.root.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// `wake` is woken at each change (while it lives).
    pub fn follow(&self, wake: &Arc<dyn Fn() + Send + Sync>) {
        let mut wakes = self.wakes.lock().unwrap_or_else(PoisonError::into_inner);
        wakes.retain(|w| w.strong_count() > 0);
        if !wakes.iter().any(|w| w.upgrade().is_some_and(|w| Arc::ptr_eq(&w, wake))) {
            wakes.push(Arc::downgrade(wake));
        }
    }

    /// Something changed: the generation bumped, the followers woken.
    pub(crate) fn changed(&self) {
        self.generation.fetch_add(1, Ordering::Release);
        self.wake();
    }

    /// Something came to a run going: its view reads it at its next poll.
    fn live_changed(&self) {
        self.live_generation.fetch_add(1, Ordering::Release);
        self.wake();
    }

    /// What a run going received, counted (see [`Self::live`]).
    pub fn live_generation(&self) -> u64 {
        self.live_generation.load(Ordering::Acquire)
    }

    fn wake(&self) {
        let wakes: Vec<_> = self
            .wakes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter_map(Weak::upgrade)
            .collect();
        for wake in wakes {
            wake();
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// The runs, oldest first.
    /// How many runs it keeps and how many are going (counted where they are, nothing
    /// copied).
    pub fn counts(&self) -> (usize, usize) {
        let state = self.state();
        let going = state.runs.iter().filter(|r| matches!(r.state, RunState::Running)).count();
        (state.runs.len(), going)
    }

    pub fn runs(&self) -> Vec<Run> {
        self.state().runs.clone()
    }

    pub fn run(&self, id: RunId) -> Option<Run> {
        self.state().runs.iter().find(|r| r.id == id).cloned()
    }

    /// A run begun: its number given, the oldest past [`KEPT`] let go.
    pub(crate) fn begin(&self, mut run: Run) -> RunId {
        let id = run.id;
        {
            let mut state = self.state();
            state.counter += 1;
            run.number = state.counter;
            state.runs.push(run);
            let extra = state.runs.len().saturating_sub(KEPT);
            state.runs.drain(..extra);
            state.reveal = Some(id);
        }
        self.changed();
        id
    }

    /// Run `id` ended.
    pub(crate) fn end(&self, id: RunId, outcome: RunState) {
        {
            let mut state = self.state();
            if let Some(run) = state.runs.iter_mut().find(|r| r.id == id) {
                run.state = outcome;
            }
            self.live.lock().unwrap_or_else(PoisonError::into_inner).remove(&id);
            lighten(&mut state.runs, IN_MEMORY);
        }
        self.changed();
    }

    /// The run just begun, taken once (the Services window shows it).
    pub fn take_reveal(&self) -> Option<RunId> {
        self.state().reveal.take()
    }

    /// The runs of `file` still going, stopped.
    pub fn stop(&self, file: &Path) -> bool {
        let state = self.state();
        let mut any = false;
        for run in
            state.runs.iter().filter(|r| r.file == file && matches!(r.state, RunState::Running))
        {
            run.cancel.cancel();
            any = true;
        }
        any
    }

    /// Run `id` stopped, if it's going (its request answers nothing more).
    pub fn cancel(&self, id: RunId) {
        if let Some(run) = self.state().runs.iter().find(|r| r.id == id) {
            run.cancel.cancel();
        }
    }

    /// Run `id` let go (one going: stopped first). `true` if it was there.
    pub fn remove(&self, id: RunId) -> bool {
        let removed = {
            let mut state = self.state();
            let before = state.runs.len();
            state.runs.retain(|r| {
                if r.id == id {
                    r.cancel.cancel();
                }
                r.id != id
            });
            state.runs.len() != before
        };
        if removed {
            self.changed();
        }
        removed
    }

    /// Every finished run let go (the ones going stay). How many went.
    pub fn clear_finished(&self) -> usize {
        let removed = {
            let mut state = self.state();
            let before = state.runs.len();
            state.runs.retain(|r| matches!(r.state, RunState::Running));
            before - state.runs.len()
        };
        if removed > 0 {
            self.changed();
        }
        removed
    }

    /// Whether a run of `file` is going.
    pub fn running(&self, file: &Path) -> bool {
        self.state().runs.iter().any(|r| r.file == file && matches!(r.state, RunState::Running))
    }

    /// The newest response of the request named `name` in `file`.
    pub fn latest(&self, file: &Path, name: &str) -> Option<Arc<Outcome>> {
        self.state().runs.iter().rev().find_map(|r| match &r.state {
            RunState::Done(outcome) if r.file == file && r.label == name => {
                Some(Arc::clone(outcome))
            }
            _ => None,
        })
    }

    /// The run going for request `label` of `file`, if one is (nothing cloned: asked each
    /// frame while it goes).
    pub fn running_for(&self, file: &Path, label: &str) -> Option<RunId> {
        let state = self.state();
        let latest = state.runs.iter().rev().find(|r| r.file == file && r.label == label)?;
        matches!(latest.state, RunState::Running).then_some(latest.id)
    }

    /// The latest run's state of each of `labels` (requests of `file`), read at once.
    pub fn states(&self, file: &Path, labels: &[String]) -> Vec<Option<RunState>> {
        let state = self.state();
        labels
            .iter()
            .map(|label| {
                state
                    .runs
                    .iter()
                    .rev()
                    .find(|r| r.file == file && r.label == *label)
                    .map(|r| r.state.clone())
            })
            .collect()
    }

    /// What the scripts keep (`client.global`).
    pub fn globals(&self) -> std::collections::BTreeMap<String, String> {
        self.globals.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// A piece of run `id`'s response come (on its worker): kept (its last [`LIVE_KEPT`]
    /// bytes), its view told now and then (only it: see [`Self::live_generation`]).
    pub(crate) fn append_live(&self, id: RunId, piece: &[u8]) {
        let tell = {
            let mut live = self.live.lock().unwrap_or_else(PoisonError::into_inner);
            let (bytes, told) =
                live.entry(id).or_insert_with(|| (Vec::new(), Instant::now() - LIVE_EVERY));
            bytes.extend_from_slice(piece);
            // Cut only at twice what's kept: each byte moved at most once, not per piece.
            if bytes.len() > 2 * LIVE_KEPT {
                let cut = bytes.len() - LIVE_KEPT;
                bytes.drain(..cut);
            }
            let due = told.elapsed() >= LIVE_EVERY;
            if due {
                *told = Instant::now();
            }
            due
        };
        if tell {
            self.live_changed();
        }
    }

    /// What run `id` has received so far, while it goes (`None`: nothing yet, or it ended).
    pub fn live(&self, id: RunId) -> Option<String> {
        let live = self.live.lock().unwrap_or_else(PoisonError::into_inner);
        live.get(&id).map(|(bytes, _)| {
            let tail = &bytes[bytes.len().saturating_sub(LIVE_KEPT)..];
            String::from_utf8_lossy(tail).into_owned()
        })
    }

    /// A page to be opened in the system's browser (from a run's worker): a web page only.
    pub(crate) fn ask_browser(&self, url: &str) {
        if crate::auth::web_page(url) {
            self.ask(Asked::Page(url.to_owned()));
        }
    }

    /// A message for the user (from a run's worker).
    pub(crate) fn tell(&self, text: &str) {
        self.ask(Asked::Tell(text.to_owned()));
    }

    fn ask(&self, asked: Asked) {
        self.asked.lock().unwrap_or_else(PoisonError::into_inner).push(asked);
        self.changed();
    }

    /// What a run asked of the app, as a view's action, taken once.
    pub fn take_asked(&self) -> Option<ui_components::ViewerAction> {
        let mut asked = self.asked.lock().unwrap_or_else(PoisonError::into_inner);
        if asked.is_empty() {
            return None;
        }
        Some(match asked.remove(0) {
            Asked::Page(url) => ui_components::ViewerAction::OpenLink(url),
            Asked::Tell(text) => {
                ui_components::ViewerAction::WithHost(ui_components::WithHost::once(move |host| {
                    host.notify(workbench_model::Notification::info(text));
                }))
            }
        })
    }

    /// The headers the scripts set for every request.
    pub fn global_headers(&self) -> std::collections::BTreeMap<String, String> {
        self.headers.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// What a script set (`Some`) or cleared (`None`) of them.
    pub(crate) fn apply_headers(
        &self,
        changes: &std::collections::BTreeMap<String, Option<String>>,
    ) {
        let mut kept = self.headers.lock().unwrap_or_else(PoisonError::into_inner);
        for (name, value) in changes {
            match value {
                Some(value) => {
                    kept.insert(name.clone(), value.clone());
                }
                None => {
                    kept.remove(name);
                }
            }
        }
    }

    /// What a script changed in the globals (`before` → `after`: set, cleared) applied to
    /// the ones kept, the rest as they are: two scripts at once each keep theirs.
    pub(crate) fn apply_globals(
        &self,
        before: &std::collections::BTreeMap<String, String>,
        after: &std::collections::BTreeMap<String, String>,
    ) {
        let mut kept = self.globals.lock().unwrap_or_else(PoisonError::into_inner);
        for name in before.keys().filter(|k| !after.contains_key(*k)) {
            kept.remove(name);
        }
        for (name, value) in after.iter().filter(|(k, v)| before.get(*k) != Some(*v)) {
            kept.insert(name.clone(), value.clone());
        }
    }

    /// `file`'s environment chosen (`None`: none).
    pub fn environment(&self, file: &Path) -> Option<String> {
        self.state().environments.get(file).cloned().flatten()
    }

    pub fn choose_environment(&self, file: &Path, environment: Option<String>) {
        self.state().environments.insert(file.to_path_buf(), environment);
        self.changed();
    }

    /// `file`'s gutter marks: each request's latest run (its request line, unchanged since):
    /// running, done, failed; `parsed`: the file now (`text` read), its other requests ready
    /// to run.
    pub fn marks(
        &self,
        file: &Path,
        parsed: &http_requests::HttpFile,
        text: &str,
    ) -> Vec<StatementMark> {
        let state = self.state();
        parsed
            .requests
            .iter()
            .filter_map(|request| {
                let line = request.line_range.clone();
                let hash = text_hash(text.get(line.clone())?);
                let ran = state
                    .runs
                    .iter()
                    .rev()
                    .find(|r| r.file == file && r.line == line && r.hash == hash);
                let state = match ran.map(|r| &r.state) {
                    Some(RunState::Running) => MarkState::Running,
                    Some(RunState::Done(outcome)) if outcome.response.status < 400 => {
                        MarkState::Done
                    }
                    Some(RunState::Done(_) | RunState::Failed(_)) => MarkState::Failed,
                    None => MarkState::Runnable,
                };
                Some(StatementMark { range: line, hash, state })
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "../tests/unit/runs.rs"]
mod tests;

/// A body's bytes (one kept in a file read whole: it's asked for once, to take a value out).
pub fn body_bytes(body: &http_requests::transport::IncomingBody) -> std::borrow::Cow<'_, [u8]> {
    match body {
        http_requests::transport::IncomingBody::Memory(bytes) => std::borrow::Cow::Borrowed(bytes),
        http_requests::transport::IncomingBody::File { path, .. } => {
            std::borrow::Cow::Owned(std::fs::read(path).unwrap_or_default())
        }
    }
}
