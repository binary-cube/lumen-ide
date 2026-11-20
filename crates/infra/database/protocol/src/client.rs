//! The editor's end of a driver's pipe: requests sent with their ids, the answers matched
//! to them by a reader thread, a request cancelled (`cancel`, then the driver stopped if
//! it doesn't answer soon) or timed out (the driver stopped), the driver's error output
//! kept as its last lines with the session's password taken out, and the driver told to
//! `exit` and stopped when the connection goes. Once the driver stops or breaks the
//! protocol, every request fails with why ([`DriverFailure::Stopped`]): its session is
//! gone, the next one starts it again.

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead as _, BufReader, Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use database::{Cancel, DbError, DriverFailure, Flow, Secret};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::value::RawValue;
use zeroize::Zeroize;

use crate::dto;
use crate::wire::{self, ErrorObject, method};

/// The driver's pipes (and its program, stopped when the connection goes; none: a driver
/// in this process, the tests').
pub struct Transport {
    /// What it writes (its stdout).
    pub input: Box<dyn Read + Send>,
    /// What it reads (its stdin).
    pub output: Box<dyn Write + Send>,
    /// Its error output (its stderr).
    pub errors: Option<Box<dyn Read + Send>>,
    pub process: Option<process::Started>,
}

impl std::fmt::Debug for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transport").field("process", &self.process).finish_non_exhaustive()
    }
}

/// The lines of its error output kept (the newest).
pub const LOG_LINES: usize = 64;
/// The longest line kept (the rest of it dropped).
const LOG_LINE: usize = 2048;
/// How often a waiting request looks at its cancel.
const TICK: Duration = Duration::from_millis(20);
/// How long a cancelled request may take to answer before the driver is stopped.
pub const CANCEL_GRACE: Duration = Duration::from_secs(5);
/// How long the driver has to end by itself once told to `exit`.
const EXIT_GRACE: Duration = Duration::from_secs(1);
/// What a password is replaced with in the driver's error output.
const HIDDEN: &str = "••••";

type Answer = Result<Box<RawValue>, ErrorObject>;

/// What a waiting request gets: rows sent before its answer, then its answer.
enum Message {
    Rows(Box<RawValue>),
    Answer(Answer),
}

struct Shared {
    /// The driver's name (as listed), for the errors.
    driver: String,
    pending: Mutex<HashMap<u64, mpsc::Sender<Message>>>,
    /// Why it ended: every request fails then.
    ended: Mutex<Option<String>>,
    /// Its error output's last lines, scrubbed.
    log: Mutex<VecDeque<String>>,
    /// Its error output ended (read to the end).
    log_done: AtomicBool,
    /// What's taken out of its error output (the session's password).
    secrets: Mutex<Vec<Secret>>,
    process: Mutex<Option<process::Started>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Shared {
    /// It ended (`why`): what waits is told (its channel closed).
    fn end(&self, why: String) {
        lock(&self.ended).get_or_insert(why);
        lock(&self.pending).clear();
    }

    fn scrub(&self, line: &str) -> String {
        let mut line = line.to_owned();
        for secret in lock(&self.secrets).iter() {
            if !secret.is_empty() {
                line = line.replace(secret.expose(), HIDDEN);
            }
        }
        line
    }

    /// Why it stopped, said for the user: how it ended, its last words.
    fn why(&self) -> String {
        // Its last lines may still be on their way.
        let waited = Instant::now();
        while !self.log_done.load(Ordering::Acquire)
            && waited.elapsed() < Duration::from_millis(300)
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut why = lock(&self.ended).clone().unwrap_or_else(|| "it ended".to_owned());
        if let Some(process) = lock(&self.process).as_mut() {
            let waited = Instant::now();
            let mut exited = process.exited();
            while exited.is_none() && waited.elapsed() < Duration::from_millis(300) {
                std::thread::sleep(Duration::from_millis(10));
                exited = process.exited();
            }
            match exited {
                Some(Some(code)) => why.push_str(&format!(" (exit code {code})")),
                Some(None) => why.push_str(" (killed)"),
                None => {}
            }
        }
        if let Some(last) = lock(&self.log).iter().rev().find(|l| !l.trim().is_empty()) {
            why.push_str(": ");
            why.push_str(last.trim());
        }
        why
    }

    fn kill(&self) {
        if let Some(process) = lock(&self.process).as_mut() {
            process.stop();
        }
    }
}

/// A connection to a driver (see the module's doc). Used from one thread at a time (a
/// session's), its reader and its error output read on threads of their own.
pub struct Connection {
    shared: Arc<Shared>,
    output: Mutex<Option<Box<dyn Write + Send>>>,
    next: AtomicU64,
}

impl std::fmt::Debug for Connection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Connection").field("driver", &self.shared.driver).finish_non_exhaustive()
    }
}

impl Connection {
    /// A connection over `transport` to the driver named `driver` (its threads named
    /// `driver-output` and `driver-errors`).
    pub fn new(driver: &str, transport: Transport) -> Result<Self, DbError> {
        let Transport { input, output, errors, process } = transport;
        let shared = Arc::new(Shared {
            driver: driver.to_owned(),
            pending: Mutex::default(),
            ended: Mutex::default(),
            log: Mutex::default(),
            log_done: AtomicBool::new(errors.is_none()),
            secrets: Mutex::default(),
            process: Mutex::new(process),
        });
        let failed = |e: std::io::Error| {
            DbError::Driver(DriverFailure::NotStarted {
                driver: driver.to_owned(),
                why: e.to_string(),
            })
        };
        let reading = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("driver-output".to_owned())
            .spawn(move || read_answers(&reading, input))
            .map_err(failed)?;
        if let Some(errors) = errors {
            let logging = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("driver-errors".to_owned())
                .spawn(move || read_errors(&logging, errors))
                .map_err(failed)?;
        }
        Ok(Self { shared, output: Mutex::new(Some(output)), next: AtomicU64::new(1) })
    }

    pub fn driver(&self) -> &str {
        &self.shared.driver
    }

    /// `secret` is taken out of the driver's error output from now on (before it's sent).
    pub fn hide(&self, secret: &Secret) {
        if !secret.is_empty() {
            lock(&self.shared.secrets).push(secret.clone());
        }
    }

    /// The last lines of the driver's error output (its secrets taken out).
    pub fn log(&self) -> Vec<String> {
        lock(&self.shared.log).iter().cloned().collect()
    }

    /// It ended (stopped, crashed, broke the protocol).
    pub fn ended(&self) -> bool {
        lock(&self.shared.ended).is_some()
    }

    fn stopped(&self) -> DbError {
        DbError::Driver(DriverFailure::Stopped {
            driver: self.shared.driver.clone(),
            why: self.shared.why(),
        })
    }

    /// The driver broke the protocol (`why`): stopped.
    fn broken(&self, why: String) -> DbError {
        self.shared.end(why);
        self.shared.kill();
        self.stopped()
    }

    /// Writes one frame (its bytes wiped afterwards: a password may be in them).
    fn write(&self, mut body: Vec<u8>) -> Result<(), String> {
        let mut output = lock(&self.output);
        let result = match output.as_mut() {
            Some(out) => wire::write_frame(out.as_mut(), &body).map_err(|e| e.to_string()),
            None => Err("closed".to_owned()),
        };
        body.zeroize();
        result
    }

    /// A notification (no answer).
    pub fn notify<P: Serialize>(&self, method: &str, params: &P) {
        let _ = self.write(wire::request(None, method, params));
    }

    /// Request `method` with `params`, its answer read as `R`; `cancel` stops it (the
    /// driver told, then stopped if it doesn't answer within [`CANCEL_GRACE`]), and past
    /// `timeout` the driver is stopped.
    pub fn call<P: Serialize, R: DeserializeOwned>(
        &self,
        method: &str,
        params: &P,
        cancel: Option<&Cancel>,
        timeout: Duration,
        ours: (u32, u32),
    ) -> Result<R, DbError> {
        match self.call_streaming(method, params, cancel, timeout, ours, &mut |_| Flow::Continue)? {
            (Some(answer), _) => Ok(answer),
            (None, _) => Err(DbError::Cancelled),
        }
    }

    /// As [`Self::call`], each `$/rows` chunk sent before the answer given to `rows` in
    /// order (`timeout` counted from the last of them). `rows` saying stop: the driver is
    /// told to cancel and the chunks after it dropped; its answer comes with `true`, and its
    /// `cancelled` is no error but no answer (`None`).
    pub fn call_streaming<P: Serialize, R: DeserializeOwned>(
        &self,
        method: &str,
        params: &P,
        cancel: Option<&Cancel>,
        timeout: Duration,
        ours: (u32, u32),
        rows: &mut dyn FnMut(dto::RowsChunk) -> Flow,
    ) -> Result<(Option<R>, bool), DbError> {
        if self.ended() {
            return Err(self.stopped());
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        lock(&self.shared.pending).insert(id, tx);
        // Ended meanwhile (its waiting ones dropped before this one was added).
        if self.ended() {
            lock(&self.shared.pending).remove(&id);
            return Err(self.stopped());
        }
        if let Err(why) = self.write(wire::request(Some(id), method, params)) {
            lock(&self.shared.pending).remove(&id);
            self.shared.end(format!("its input closed ({why})"));
            return Err(self.stopped());
        }
        let mut heard = Instant::now();
        let mut cancelled: Option<Instant> = None;
        // The rows' consumer had enough: the driver was told to cancel.
        let mut enough = false;
        let forget = || {
            lock(&self.shared.pending).remove(&id);
        };
        loop {
            match rx.recv_timeout(TICK) {
                Ok(Message::Rows(raw)) => {
                    heard = Instant::now();
                    if enough {
                        continue;
                    }
                    let chunk = serde_json::from_str::<dto::RowsChunk>(raw.get()).map_err(|e| {
                        forget();
                        self.broken(format!("rows of `{method}` not in the protocol: {e}"))
                    })?;
                    if rows(chunk) == Flow::Stop {
                        enough = true;
                        if cancelled.is_none() {
                            self.notify(method::CANCEL, &dto::CancelParams { id });
                            cancelled = Some(Instant::now());
                        }
                    }
                }
                Ok(Message::Answer(Ok(raw))) => {
                    return serde_json::from_str::<R>(raw.get())
                        .map(|answer| (Some(answer), enough))
                        .map_err(|e| {
                            self.broken(format!("an answer to `{method}` not in the protocol: {e}"))
                        });
                }
                Ok(Message::Answer(Err(error))) => {
                    let error = error.to_db(&self.shared.driver, ours);
                    return if enough && error == DbError::Cancelled {
                        Ok((None, true))
                    } else {
                        Err(error)
                    };
                }
                Err(RecvTimeoutError::Disconnected) => return Err(self.stopped()),
                Err(RecvTimeoutError::Timeout) => {}
            }
            if cancelled.is_none() && cancel.is_some_and(Cancel::is_cancelled) {
                self.notify(method::CANCEL, &dto::CancelParams { id });
                cancelled = Some(Instant::now());
            }
            let silent = |what: String| {
                forget();
                self.shared.end(format!("it didn't answer {what} in time"));
                self.shared.kill();
                DbError::Driver(DriverFailure::TimedOut {
                    driver: self.shared.driver.clone(),
                    what,
                })
            };
            if cancelled.is_some_and(|at| at.elapsed() > CANCEL_GRACE) {
                return Err(silent(format!("`{method}`, cancelled")));
            }
            if heard.elapsed() > timeout {
                return Err(silent(format!("`{method}`")));
            }
        }
    }
}

impl Drop for Connection {
    /// The driver told to `exit` and its input closed; stopped if it hasn't ended a moment
    /// later (without waiting here: on a thread of its own).
    fn drop(&mut self) {
        self.notify(method::EXIT, &serde_json::json!({}));
        lock(&self.output).take();
        self.shared.end("its session closed".to_owned());
        let process = lock(&self.shared.process).take();
        if let Some(mut process) = process {
            let reaped =
                std::thread::Builder::new().name("driver-exit".to_owned()).spawn(move || {
                    if !process.wait_for(EXIT_GRACE) {
                        process.stop();
                    }
                });
            // No thread to wait on: stopped now (the process goes with its value).
            if reaped.is_err() {
                tracing::warn!("a driver's program was stopped without its grace");
            }
        }
    }
}

/// Whose rows a `$/rows` brings (only its id read: the rest left to the request).
#[derive(serde::Deserialize)]
struct ChunkOf {
    id: u64,
}

/// The reader: each answer to its request; the end (or a message not in the protocol)
/// ends the connection.
fn read_answers(shared: &Shared, input: Box<dyn Read + Send>) {
    let mut input = BufReader::new(input);
    let why = loop {
        let body = match wire::read_frame(&mut input) {
            Ok(Some(body)) => body,
            Ok(None) => break "it closed its output".to_owned(),
            Err(e) => break format!("its output broke the protocol: {e}"),
        };
        let message: wire::Incoming<'_> = match serde_json::from_slice(&body) {
            Ok(message) => message,
            Err(e) => break format!("a message not in the protocol: {e}"),
        };
        // Rows for a request still waiting (protocol 2); its other notifications ignored.
        if message.id.is_none() && message.method == Some(method::ROWS) {
            let Some(params) = message.params else { continue };
            let Ok(chunk) = serde_json::from_str::<ChunkOf>(params.get()) else {
                break "rows not in the protocol".to_owned();
            };
            let waiting = lock(&shared.pending).get(&chunk.id).cloned();
            if let Some(tx) = waiting {
                let _ = tx.send(Message::Rows(params.to_owned()));
            }
            continue;
        }
        let (Some(id), None) = (message.id, message.method) else { continue };
        let answer = match (message.error, message.result) {
            (Some(error), _) => Err(error),
            (None, Some(result)) => Ok(result.to_owned()),
            (None, None) => Ok(RawValue::from_string("null".to_owned()).unwrap_or_default()),
        };
        let waiting = lock(&shared.pending).remove(&id);
        if let Some(tx) = waiting {
            let _ = tx.send(Message::Answer(answer));
        }
    };
    shared.end(why);
}

/// Its error output, line by line: the passwords taken out, the newest lines kept, each
/// one also in the app's log (debug).
fn read_errors(shared: &Shared, errors: Box<dyn Read + Send>) {
    let mut errors = BufReader::new(errors);
    let mut line = Vec::new();
    loop {
        line.clear();
        match (&mut errors).take(LOG_LINE as u64).read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        // A line longer than kept: the rest of it dropped.
        // In bounded pieces (a driver writing without newlines never grows it).
        if !line.ends_with(b"\n") && line.len() >= LOG_LINE {
            let mut rest = Vec::new();
            loop {
                rest.clear();
                match (&mut errors).take(LOG_LINE as u64).read_until(b'\n', &mut rest) {
                    Ok(0) | Err(_) => break,
                    Ok(_) if rest.ends_with(b"\n") => break,
                    Ok(_) => {}
                }
            }
            rest.zeroize();
        }
        let text = shared.scrub(String::from_utf8_lossy(&line).trim_end());
        line.zeroize();
        tracing::debug!(target: "driver", driver = %shared.driver, line = %text, "the driver said");
        let mut log = lock(&shared.log);
        if log.len() == LOG_LINES {
            log.pop_front();
        }
        log.push_back(text);
    }
    shared.log_done.store(true, Ordering::Release);
}

#[cfg(test)]
#[path = "../tests/unit/client.rs"]
mod tests;
