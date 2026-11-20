//! The thread writing the lines (`log-writer`): they come on a bounded channel; nobody waits
//! for the disk. Written through a buffer emptied at most [`EVERY`] apart, at once for a
//! warning or an error, and when asked ([`Msg::Flush`]: the app ending, a panic). A new day
//! or a full file turns to the next; at start the oldest files are let go. A disk that
//! refuses (full, no rights) is tried again a minute later, what was lost said once.

use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender};
use std::time::{Duration, Instant};

use crate::files::{self, Current, Limits};
use crate::line::day_name;

/// The buffer emptied at most this far apart.
pub const EVERY: Duration = Duration::from_millis(200);

/// A refused disk tried again after this long.
const RETRY: Duration = Duration::from_secs(60);

/// What the writer is told.
#[derive(Debug)]
pub enum Msg {
    /// A line (without its newline), at milliseconds since the epoch; `urgent`: written at
    /// once (a warning, an error).
    Line { text: String, at_ms: i64, urgent: bool },
    /// Everything written so far on disk, then told.
    Flush(SyncSender<()>),
    /// The lines go into this folder from now (`None`: none on disk); the open file closed.
    Dir(Option<PathBuf>),
}

/// Where the lines go: a folder (`None`: none on disk), the console too.
#[derive(Debug, Clone)]
pub struct Sinks {
    pub dir: Option<PathBuf>,
    pub console: bool,
    pub limits: Limits,
    pub run: String,
}

/// Runs the writer until every sender is gone.
pub fn run(rx: &Receiver<Msg>, sinks: &Sinks) {
    let mut out = Out::new(sinks.clone());
    let mut last = Instant::now();
    loop {
        match rx.recv_timeout(EVERY) {
            Ok(Msg::Line { text, at_ms, urgent }) => {
                out.write(&text, at_ms);
                if urgent || last.elapsed() >= EVERY {
                    out.flush();
                    last = Instant::now();
                }
            }
            Ok(Msg::Dir(dir)) => {
                out.flush();
                out.move_to(dir);
            }
            Ok(Msg::Flush(done)) => {
                // What came before it, written too.
                while let Ok(Msg::Line { text, at_ms, .. }) = rx.try_recv() {
                    out.write(&text, at_ms);
                }
                out.flush();
                let _ = done.send(());
            }
            Err(RecvTimeoutError::Timeout) => {
                out.flush();
                last = Instant::now();
            }
            Err(RecvTimeoutError::Disconnected) => {
                out.flush();
                return;
            }
        }
    }
}

/// The open file (if any) and the console.
struct Out {
    sinks: Sinks,
    file: Option<(Current, std::io::BufWriter<std::fs::File>)>,
    /// A refused disk: not tried again before this.
    refused_until: Option<Instant>,
    /// Lines not written since the disk refused.
    lost: u64,
    told_lost: bool,
}

impl Out {
    fn new(sinks: Sinks) -> Self {
        Self { sinks, file: None, refused_until: None, lost: 0, told_lost: false }
    }

    fn write(&mut self, text: &str, at_ms: i64) {
        if self.sinks.console {
            let mut err = std::io::stderr().lock();
            let _ = writeln!(err, "{text}");
        }
        if self.sinks.dir.is_none() {
            return;
        }
        let Some((current, buffer)) = self.file_for(at_ms, text.len()) else {
            self.lost += 1;
            return;
        };
        let written = writeln!(buffer, "{text}");
        match written {
            Ok(()) => current.size += text.len() as u64 + 1,
            Err(e) => self.refused(&e),
        }
    }

    /// The file for a line of `len` bytes at `at_ms`: the open one, or the day's next one
    /// (a new day, the open one full); `None` while the disk refuses.
    fn file_for(
        &mut self,
        at_ms: i64,
        len: usize,
    ) -> Option<&mut (Current, std::io::BufWriter<std::fs::File>)> {
        if self.refused_until.is_some_and(|until| Instant::now() < until) {
            return None;
        }
        let dir = self.sinks.dir.clone()?;
        let day = day_name(at_ms);
        let limits = self.sinks.limits;
        let turn = match &self.file {
            None => true,
            Some((current, _)) => current.day != day || current.size + len as u64 + 1 > limits.file,
        };
        if turn {
            let next = match self.file.take() {
                Some((current, mut buffer)) if current.day == day => {
                    let _ = buffer.flush();
                    files::next(&dir, &current, &self.sinks.run, limits)
                }
                old => {
                    if let Some((_, mut buffer)) = old {
                        let _ = buffer.flush();
                    }
                    let opened = files::open(&dir, &day, &self.sinks.run, limits);
                    // A new file of a new day: the oldest let go (not the one just opened).
                    if let Ok(current) = &opened {
                        files::clean(&dir, &day, limits, &current.path);
                    }
                    opened
                }
            };
            match next {
                Ok(current) => {
                    let clone = current.file.try_clone();
                    match clone {
                        Ok(handle) => {
                            self.file = Some((current, std::io::BufWriter::new(handle)));
                        }
                        Err(e) => {
                            self.refused(&e);
                            return None;
                        }
                    }
                    self.recovered();
                }
                Err(e) => {
                    self.refused(&e);
                    return None;
                }
            }
        }
        self.file.as_mut()
    }

    /// The disk refused: tried again later, said once on the console.
    fn refused(&mut self, why: &std::io::Error) {
        self.file = None;
        self.refused_until = Some(Instant::now() + RETRY);
        if !self.told_lost {
            self.told_lost = true;
            let _ = writeln!(std::io::stderr(), "the log can't be written: {why}");
        }
    }

    /// Writing again after a refusal: what was lost said in the file.
    fn recovered(&mut self) {
        self.refused_until = None;
        if self.lost > 0 {
            let lost = std::mem::take(&mut self.lost);
            if let Some((current, buffer)) = &mut self.file {
                let text = serde_json::json!({
                    "ts": crate::line::timestamp(crate::layer::now_ms()),
                    "level": "WARN",
                    "target": "logging",
                    "thread": "log-writer",
                    "run": self.sinks.run,
                    "msg": "lines lost while the disk refused them",
                    "fields": { "lost": lost }
                })
                .to_string();
                if writeln!(buffer, "{text}").is_ok() {
                    current.size += text.len() as u64 + 1;
                }
            }
        }
        self.told_lost = false;
    }

    /// From now into `dir` (`None`: none on disk): the open file let go, a refusal forgotten.
    fn move_to(&mut self, dir: Option<PathBuf>) {
        self.file = None;
        self.refused_until = None;
        self.told_lost = false;
        self.sinks.dir = dir;
    }

    fn flush(&mut self) {
        if let Some((_, buffer)) = &mut self.file
            && let Err(e) = buffer.flush()
        {
            self.refused(&e);
        }
        if self.sinks.console {
            let _ = std::io::stderr().flush();
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/writer.rs"]
mod tests;
