//! An agent run as a process for a graphical conversation (the Agent Client Protocol): its
//! standard input and output carry JSON-RPC, one message a line. The editor starts it, opens a
//! session in a folder — a new one, or one taken up again by its id, its history replayed —
//! sends what's typed, cancels a turn, answers its permissions; everything the agent says
//! comes back as [`Event`]s, taken on the UI thread by a poll after a wake.
//!
//! Its output is read on a thread of its own (`acp-read`), its errors' last lines on
//! another (`acp-errors`), what's sent written on a third (`acp-write`, fed by a channel):
//! sending never waits for the agent, and no lock is held while reading, writing or waking.
//! Lines are read bounded and as lossy UTF-8 (one too long, or not text, skipped — never the
//! end of the conversation). Dropped, the process and what it started are stopped on a
//! worker (stopping a process tree may wait: never on the UI thread).

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};

use acp::{Inbound, wire};
use serde_json::Value;

/// What the agent's process said, in order.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The session is open: its id (the agent's, which a later `load` names).
    Ready { session: String },
    /// A `session/update`'s `update` (a message's chunk, a tool, the plan…).
    Update(Value),
    /// A permission asked (`session/request_permission`): its request's id and parameters.
    Ask { id: Value, params: Value },
    /// The turn ended (`end_turn`, `cancelled`, `refusal`…).
    TurnEnded(String),
    /// Something failed (the agent refused a request, it couldn't start): said to the user.
    Failed(String),
    /// The session asked for couldn't be taken up again (the agent can't): a new one opened.
    NotResumed,
    /// The process ended.
    Exited,
}

/// The longest message read (a session's history replayed can be large), and the longest
/// error line kept.
const LINE_MOST: usize = 64 * 1024 * 1024;
const ERROR_LINE_MOST: usize = 4 * 1024;

/// What a request of ours was for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Asked {
    Initialize,
    Session,
    Prompt,
}

/// How the session is opened once the agent is initialized.
#[derive(Debug, Clone)]
struct Open {
    cwd: String,
    resume: Option<String>,
}

/// What the reading thread and the editor share.
struct Shared {
    /// What's to be written (by `acp-write`).
    out: Sender<String>,
    next: u64,
    pending: HashMap<u64, Asked>,
    events: VecDeque<Event>,
    open: Option<Open>,
    session: Option<String>,
    /// Events came since the last wake (the wake itself called once the lock is let go).
    told: bool,
}

impl Shared {
    /// Sends request `method` (what it's for kept to read its answer).
    fn ask(&mut self, method: &str, params: Value, asked: Asked) {
        self.next += 1;
        let id = self.next;
        self.pending.insert(id, asked);
        self.send(wire::request(id, method, params));
    }

    /// `line` handed to the writer (never waiting).
    fn send(&mut self, line: String) {
        if self.out.send(line).is_err() {
            self.tell(Event::Failed("the agent stopped listening".into()));
        }
    }

    fn tell(&mut self, event: Event) {
        self.events.push_back(event);
        self.told = true;
    }

    /// One message from the agent, handled.
    fn received(&mut self, message: Inbound) {
        match message {
            Inbound::Response { id, result } => {
                let Some(asked) = self.pending.remove(&id) else { return };
                match (asked, result) {
                    (_, Err(why)) => {
                        if asked == Asked::Prompt {
                            self.tell(Event::TurnEnded("failed".into()));
                        }
                        self.tell(Event::Failed(why));
                    }
                    (Asked::Initialize, Ok(answer)) => {
                        let Some(open) = self.open.take() else { return };
                        match open.resume {
                            Some(id) if wire::loads_sessions(&answer) => {
                                self.session = Some(id.clone());
                                self.ask(
                                    "session/load",
                                    wire::load_session(&id, &open.cwd),
                                    Asked::Session,
                                );
                            }
                            resume => {
                                if resume.is_some() {
                                    self.tell(Event::NotResumed);
                                }
                                self.ask(
                                    "session/new",
                                    wire::new_session(&open.cwd),
                                    Asked::Session,
                                );
                            }
                        }
                    }
                    (Asked::Session, Ok(answer)) => {
                        let id = answer.get("sessionId").and_then(Value::as_str).map(str::to_owned);
                        match id.or_else(|| self.session.clone()) {
                            Some(id) => {
                                self.session = Some(id.clone());
                                self.tell(Event::Ready { session: id });
                            }
                            None => self.tell(Event::Failed("the agent opened no session".into())),
                        }
                    }
                    (Asked::Prompt, Ok(answer)) => {
                        let reason = answer.get("stopReason").and_then(Value::as_str);
                        self.tell(Event::TurnEnded(reason.unwrap_or("end_turn").to_owned()));
                    }
                }
            }
            Inbound::Notification { method, params } if method == "session/update" => {
                if let Value::Object(mut params) = params
                    && let Some(update) = params.remove("update")
                {
                    self.tell(Event::Update(update));
                }
            }
            Inbound::Notification { .. } => {}
            Inbound::Request { id, method, params } if method == "session/request_permission" => {
                self.tell(Event::Ask { id, params });
            }
            // The editor reads and writes no files and runs no terminals for the agent: it
            // said so at `initialize`.
            Inbound::Request { id, .. } => {
                self.send(wire::error(&id, -32601, "not offered by this editor"));
            }
        }
    }
}

/// An agent talked to with the protocol.
pub struct AgentProcess {
    shared: Arc<Mutex<Shared>>,
    wake: Arc<dyn Fn() + Send + Sync>,
    /// The process (`None`: tests' own streams).
    process: Option<process::Started>,
    /// Its errors' last lines (said when it fails).
    errors: Arc<Mutex<VecDeque<String>>>,
}

impl std::fmt::Debug for AgentProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentProcess").finish_non_exhaustive()
    }
}

/// The last lines of its errors kept.
const ERROR_LINES: usize = 20;

/// One line of `reader` into `buf` (cleared), at most `most` bytes kept: `None` at its end;
/// `Some(false)` for a line too long (the rest of it skipped).
fn read_line(reader: &mut impl BufRead, buf: &mut Vec<u8>, most: usize) -> Option<bool> {
    buf.clear();
    let limit = u64::try_from(most).unwrap_or(u64::MAX).saturating_add(1);
    let n = reader.by_ref().take(limit).read_until(b'\n', buf).ok()?;
    if n == 0 {
        return None;
    }
    if buf.last() == Some(&b'\n') || n <= most {
        return Some(true);
    }
    // Too long: the rest of the line skipped, in pieces.
    let mut rest = Vec::new();
    loop {
        rest.clear();
        let n = reader.by_ref().take(64 * 1024).read_until(b'\n', &mut rest).ok()?;
        if n == 0 || rest.last() == Some(&b'\n') {
            return Some(false);
        }
    }
}

impl AgentProcess {
    /// `launch` started, initialized, its session opened in `cwd` — a new one, or `resume`
    /// taken up again (when the agent can: else a new one). `wake` is called when events
    /// come.
    pub fn start(
        launch: &agents::Launch,
        cwd: &str,
        resume: Option<&str>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self, String> {
        let mut run = process::Run::new(&launch.program, &launch.args).owner("acp");
        if let Some(dir) = &launch.cwd {
            run = run.dir(dir);
        }
        for (key, value) in &launch.env {
            run = run.env(key, value);
        }
        let mut started = process::start(&run).map_err(|e| e.to_string())?;
        let (Some(stdin), Some(stdout)) = (started.stdin.take(), started.stdout.take()) else {
            return Err("the agent's pipes didn't open".into());
        };
        let errors = Arc::new(Mutex::new(VecDeque::new()));
        if let Some(stderr) = started.stderr.take() {
            let kept = Arc::clone(&errors);
            let _ = std::thread::Builder::new().name("acp-errors".into()).spawn(move || {
                let (mut reader, mut buf) = (BufReader::new(stderr), Vec::new());
                // Every line read (the pipe never fills), the last ones kept, each bounded.
                while let Some(whole) = read_line(&mut reader, &mut buf, ERROR_LINE_MOST) {
                    let mut line = String::from_utf8_lossy(&buf).trim_end().to_owned();
                    if !whole {
                        line.push('…');
                    }
                    let mut kept = kept.lock().unwrap_or_else(PoisonError::into_inner);
                    if kept.len() == ERROR_LINES {
                        kept.pop_front();
                    }
                    kept.push_back(line);
                }
            });
        }
        let mut agent = Self::over(stdout, stdin, cwd, resume, wake);
        agent.process = Some(started);
        agent.errors = errors;
        Ok(agent)
    }

    /// The same over given streams (tests: an agent of their own).
    pub fn over(
        reader: impl Read + Send + 'static,
        writer: impl Write + Send + 'static,
        cwd: &str,
        resume: Option<&str>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let (out, lines) = channel::<String>();
        let shared = Arc::new(Mutex::new(Shared {
            out,
            next: 0,
            pending: HashMap::new(),
            events: VecDeque::new(),
            open: Some(Open { cwd: cwd.to_owned(), resume: resume.map(str::to_owned) }),
            session: None,
            told: false,
        }));
        // What's sent, written here (the agent slow to read: only this thread waits).
        let (writing, woken) = (Arc::clone(&shared), Arc::clone(&wake));
        let _ = std::thread::Builder::new().name("acp-write".into()).spawn(move || {
            let mut writer = writer;
            for line in lines {
                if writer.write_all(line.as_bytes()).and_then(|()| writer.flush()).is_err() {
                    let failed = Event::Failed("the agent stopped listening".into());
                    writing.lock().unwrap_or_else(PoisonError::into_inner).tell(failed);
                    woken();
                    return;
                }
            }
        });
        let (reading, woken) = (Arc::clone(&shared), Arc::clone(&wake));
        let _ = std::thread::Builder::new().name("acp-read".into()).spawn(move || {
            let (mut reader, mut buf) = (BufReader::new(reader), Vec::new());
            // A line too long or not text is skipped: never the conversation's end.
            while let Some(whole) = read_line(&mut reader, &mut buf, LINE_MOST) {
                let message = whole.then(|| wire::read(&String::from_utf8_lossy(&buf))).flatten();
                // A huge line (a replayed history) leaves no 64 MB buffer behind.
                buf.shrink_to(64 * 1024);
                let Some(message) = message else { continue };
                let told = {
                    let mut shared = reading.lock().unwrap_or_else(PoisonError::into_inner);
                    shared.received(message);
                    std::mem::take(&mut shared.told)
                };
                if told {
                    woken();
                }
            }
            reading.lock().unwrap_or_else(PoisonError::into_inner).tell(Event::Exited);
            woken();
        });
        let version = kernel::brand::version();
        shared.lock().unwrap_or_else(PoisonError::into_inner).ask(
            "initialize",
            wire::initialize("editor", version),
            Asked::Initialize,
        );
        Self { shared, wake, process: None, errors: Arc::default() }
    }

    /// `f` with what's shared, the lock let go before any wake it asks for.
    fn with<R>(&self, f: impl FnOnce(&mut Shared) -> R) -> R {
        let (result, told) = {
            let mut shared = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
            let result = f(&mut shared);
            (result, std::mem::take(&mut shared.told))
        };
        if told {
            (self.wake)();
        }
        result
    }

    /// What came since the last call, in order.
    pub fn events(&self) -> Vec<Event> {
        self.with(|s| s.events.drain(..).collect())
    }

    /// The session's id, once it's open.
    pub fn session(&self) -> Option<String> {
        self.with(|s| s.session.clone())
    }

    /// `text` sent as the next turn. `false`: the session isn't open yet.
    pub fn prompt(&self, text: &str) -> bool {
        self.with(|s| {
            let Some(session) = s.session.clone() else { return false };
            s.ask("session/prompt", wire::prompt(&session, text), Asked::Prompt);
            true
        })
    }

    /// The turn going on, cancelled (its answer still comes: `cancelled`).
    pub fn cancel(&self) {
        self.with(|s| {
            if let Some(session) = s.session.clone() {
                s.send(wire::notification("session/cancel", wire::cancel(&session)));
            }
        });
    }

    /// Permission request `id` answered with option `chosen` (`None`: cancelled).
    pub fn answer(&self, id: &Value, chosen: Option<&str>) {
        self.with(|s| s.send(wire::response(id, wire::permission(chosen))));
    }

    /// Its errors' last lines (why it failed, when it says).
    pub fn errors(&self) -> Vec<String> {
        self.errors.lock().unwrap_or_else(PoisonError::into_inner).iter().cloned().collect()
    }

    /// The process' id, when it's one.
    pub fn pid(&self) -> Option<u32> {
        self.process.as_ref().map(process::Started::id)
    }
}

impl Drop for AgentProcess {
    /// Its process stopped on a worker (stopping a tree may wait: `taskkill` on Windows).
    fn drop(&mut self) {
        if let Some(process) = self.process.take() {
            kernel::work::run("acp-stop", move || drop(process));
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
