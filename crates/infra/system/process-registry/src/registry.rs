//! This instance's file, kept by a thread of its own (`process-registry`): the news of
//! programs started and ended arrive on a channel, are joined for a moment
//! ([`COALESCE`]), then written at once (a temporary file renamed over it). A program whose
//! end isn't told (a terminal's shell, one taken over from an earlier run) is looked at
//! every [`CHECK_EVERY`] while there is one. Nothing written while nothing runs (the file
//! removed); at a normal exit the file goes, unless it holds programs still to ask about.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use process::launched::{Launch, Recorder};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

use crate::record::{InstanceFile, MAX_RECORDS, Record};

/// How long news are joined before writing.
pub const COALESCE: Duration = Duration::from_millis(300);

/// The longest news are joined (a stream of them still gets written).
const COALESCE_MAX: Duration = Duration::from_secs(2);

/// How often the programs whose end isn't told are looked at.
pub const CHECK_EVERY: Duration = Duration::from_secs(10);

/// What the thread is told.
enum News {
    Started(Launch),
    Ended(u32),
    /// Programs of an earlier run, kept until answered (written at once, then told).
    Adopt(Vec<Record>, Sender<()>),
    /// Programs answered (pid and start time): no longer kept.
    Forget(Vec<(u32, u64)>),
    /// A normal exit: the file goes (or keeps only what's still to ask about), then told.
    Finish(Sender<()>),
    /// Written now, then told (tests).
    Flush(Sender<()>),
}

/// This instance's registry.
pub struct Registry {
    file: PathBuf,
    news: Mutex<Option<Sender<News>>>,
}

impl std::fmt::Debug for Registry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Registry").field("file", &self.file).finish_non_exhaustive()
    }
}

impl Registry {
    /// This instance's registry in `dir` (its file named by a fresh UUID), its thread
    /// started; `None` if the thread couldn't start.
    pub fn open(dir: &Path) -> Option<Self> {
        let name = format!("{}.toml", kernel::id::uuid_text(kernel::id::new_uuid()));
        let file = dir.join(name);
        let (tx, rx) = mpsc::channel();
        let path = file.clone();
        std::thread::Builder::new()
            .name("process-registry".into())
            .spawn(move || Keeper::new(path).run(&rx))
            .ok()?;
        Some(Self { file, news: Mutex::new(Some(tx)) })
    }

    /// This instance's file.
    pub fn file(&self) -> &Path {
        &self.file
    }

    fn tell(&self, news: News) -> bool {
        self.news.lock().ok().and_then(|tx| tx.as_ref().map(|tx| tx.send(news).is_ok()))
            == Some(true)
    }

    /// Programs of an earlier run kept here until answered: written before this returns
    /// (at most a second; call it off the UI thread).
    pub fn adopt(&self, records: Vec<Record>) {
        let (tx, rx) = mpsc::channel();
        if self.tell(News::Adopt(records, tx)) {
            let _ = rx.recv_timeout(Duration::from_secs(1));
        }
    }

    /// Programs answered (pid, start time): no longer kept.
    pub fn forget(&self, programs: Vec<(u32, u64)>) {
        self.tell(News::Forget(programs));
    }

    /// Everything told so far written (tests; at most a second).
    pub fn flush(&self) {
        let (tx, rx) = mpsc::channel();
        if self.tell(News::Flush(tx)) {
            let _ = rx.recv_timeout(Duration::from_secs(1));
        }
    }

    /// A normal exit: the file removed (kept with only the programs still to ask about),
    /// the thread ended. At most a second.
    pub fn finish(&self) {
        let (tx, rx) = mpsc::channel();
        if self.tell(News::Finish(tx)) {
            let _ = rx.recv_timeout(Duration::from_secs(1));
        }
        if let Ok(mut news) = self.news.lock() {
            *news = None;
        }
    }
}

impl Recorder for Registry {
    fn started(&self, launch: Launch) {
        self.tell(News::Started(launch));
    }

    fn ended(&self, pid: u32) {
        self.tell(News::Ended(pid));
    }
}

/// A program kept: its record, and whether its end is told.
struct Kept {
    record: Record,
    told_end: bool,
    /// From an earlier run, not answered yet.
    adopted: bool,
}

/// The registry's thread: what's kept, the file.
struct Keeper {
    file: PathBuf,
    header: InstanceFile,
    kept: Vec<Kept>,
    system: System,
    /// What's on disk is what's kept.
    written: bool,
}

impl Keeper {
    fn new(file: PathBuf) -> Self {
        let mut system = System::new();
        let app_pid = std::process::id();
        let app_started = start_time(&mut system, app_pid).unwrap_or(0);
        let header = InstanceFile { app_pid, app_started, programs: Vec::new() };
        Self { file, header, kept: Vec::new(), system, written: true }
    }

    fn run(mut self, news: &Receiver<News>) {
        loop {
            let watching = self.kept.iter().any(|k| !k.told_end);
            let first = if watching {
                news.recv_timeout(CHECK_EVERY)
            } else {
                news.recv().map_err(|_| RecvTimeoutError::Disconnected)
            };
            match first {
                Ok(item) => {
                    if self.take(item) {
                        return;
                    }
                    // What follows soon joins this write.
                    let since = Instant::now();
                    while since.elapsed() < COALESCE_MAX {
                        match news.recv_timeout(COALESCE) {
                            Ok(item) => {
                                if self.take(item) {
                                    return;
                                }
                            }
                            Err(RecvTimeoutError::Timeout) => break,
                            Err(RecvTimeoutError::Disconnected) => {
                                self.write();
                                return;
                            }
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => self.check_watched(),
                Err(RecvTimeoutError::Disconnected) => return,
            }
            self.write();
        }
    }

    /// One piece of news taken in; `true`: the thread ends.
    fn take(&mut self, news: News) -> bool {
        match news {
            News::Started(launch) => {
                // Ended already (or never seen): nothing to keep.
                let Some(started) = start_time(&mut self.system, launch.pid) else {
                    return false;
                };
                self.kept.retain(|k| k.record.pid != launch.pid);
                if self.kept.len() >= MAX_RECORDS {
                    return false;
                }
                let record = Record {
                    pid: launch.pid,
                    started,
                    kind: launch.kind.id().to_owned(),
                    program: launch.program,
                    project: launch.project.map(|p| p.to_string_lossy().into_owned()),
                };
                self.kept.push(Kept { record, told_end: launch.told_end, adopted: false });
                self.written = false;
            }
            News::Ended(pid) => {
                let before = self.kept.len();
                self.kept.retain(|k| !(k.told_end && k.record.pid == pid));
                self.written &= before == self.kept.len();
            }
            News::Adopt(records, done) => {
                for record in records {
                    if self.kept.len() >= MAX_RECORDS {
                        break;
                    }
                    self.kept.retain(|k| k.record.pid != record.pid);
                    self.kept.push(Kept { record, told_end: false, adopted: true });
                }
                self.written = false;
                self.write();
                let _ = done.send(());
            }
            News::Forget(programs) => {
                let before = self.kept.len();
                self.kept.retain(|k| !programs.contains(&(k.record.pid, k.record.started)));
                self.written &= before == self.kept.len();
            }
            News::Flush(done) => {
                self.write();
                let _ = done.send(());
            }
            News::Finish(done) => {
                self.kept.retain(|k| k.adopted);
                self.written = false;
                self.write();
                let _ = done.send(());
                return true;
            }
        }
        false
    }

    /// The programs whose end isn't told: those gone (or their pid now another's) let go.
    fn check_watched(&mut self) {
        let pids: Vec<Pid> =
            self.kept.iter().filter(|k| !k.told_end).map(|k| Pid::from_u32(k.record.pid)).collect();
        if pids.is_empty() {
            return;
        }
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&pids),
            true,
            ProcessRefreshKind::nothing(),
        );
        let system = &self.system;
        let before = self.kept.len();
        self.kept.retain(|k| {
            k.told_end
                || system
                    .process(Pid::from_u32(k.record.pid))
                    .is_some_and(|p| p.start_time() == k.record.started)
        });
        self.written &= before == self.kept.len();
    }

    /// What's kept, on disk (nothing kept: the file removed).
    fn write(&mut self) {
        if self.written {
            return;
        }
        self.written = true;
        if self.kept.is_empty() {
            match std::fs::remove_file(&self.file) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => tracing::debug!("the processes' file wasn't removed: {e}"),
            }
            return;
        }
        let mut file = self.header.clone();
        file.programs = self.kept.iter().map(|k| k.record.clone()).collect();
        if let Err(e) = files::write_atomic(&self.file, file.to_text().as_bytes()) {
            tracing::debug!("the processes' file wasn't written: {e}");
        }
    }
}

/// When `pid` started (seconds since the epoch), if it runs.
pub(crate) fn start_time(system: &mut System, pid: u32) -> Option<u64> {
    let pid = Pid::from_u32(pid);
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing(),
    );
    system.process(pid).map(sysinfo::Process::start_time)
}

#[cfg(test)]
#[path = "../tests/unit/registry.rs"]
mod tests;
