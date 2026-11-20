//! Running external programs (git, a PHP interpreter, a formatter, a checker) and the
//! project's own scripts: in a folder, with what to write on stdin, stopped after a timeout
//! or when asked to, both outputs read while it runs (a full pipe would block it), never
//! waiting for input nobody types, no console window flashing up on Windows.
//!
//! Whoever runs programs can follow them ([`Running`]): the ids of those running now, for
//! the Task Manager (their CPU, memory and disk count for the part that started them).
//!
//! A program that keeps running beside the app (a driver talking on its stdin and stdout)
//! is [`start`]ed instead: the same rules, its pipes given to the caller, stopped with what
//! it started when it's dropped.
//!
//! A program that exits with an error isn't an error here: its [`Output`] says how it ended
//! (callers decide what an exit code means — a checker reports problems that way).

pub mod descendants;
pub mod launched;

use std::ffi::OsString;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// The most of each output kept (a runaway tool printing forever fills no memory); what
/// follows is read and dropped, so the program never blocks on a full pipe.
pub const MAX_OUTPUT: usize = 16 * 1024 * 1024;

/// What to run.
#[derive(Debug, Clone)]
pub struct Run {
    pub program: OsString,
    pub args: Vec<OsString>,
    /// The folder it runs in (the project's); `None`: the app's.
    pub dir: Option<PathBuf>,
    /// Added to its environment.
    pub env: Vec<(OsString, OsString)>,
    /// Written to its stdin, then closed; `None`: an empty stdin (nothing to wait for).
    pub stdin: Option<Vec<u8>>,
    /// Stopped after this long.
    pub timeout: Duration,
    /// Stopped when this turns true (a newer run replacing it).
    pub stop: Option<Arc<AtomicBool>>,
    /// Listed there while it runs.
    pub running: Option<Arc<Running>>,
    /// What runs it (`git`, `quality`…): its helper threads are named after it
    /// (`<owner>-input`, `<owner>-output`), so the Task Manager puts their CPU on its row.
    pub owner: &'static str,
}

/// The programs a part of the app is running now (their process ids), listed by
/// [`run`] while they run.
#[derive(Debug, Default)]
pub struct Running(std::sync::Mutex<Vec<(u32, bool)>>);

impl Running {
    pub const fn new() -> Self {
        Self(std::sync::Mutex::new(Vec::new()))
    }

    /// The process ids running now.
    pub fn pids(&self) -> Vec<u32> {
        self.0.lock().map(|p| p.iter().map(|(pid, _)| *pid).collect()).unwrap_or_default()
    }

    /// Program `pid` (one of these) asked to stop: its run stops it — and what it started —
    /// as a timeout would, and ends [`RunError::Stopped`]. `false`: not one of these.
    pub fn stop(&self, pid: u32) -> bool {
        let Ok(mut pids) = self.0.lock() else { return false };
        match pids.iter_mut().find(|(p, _)| *p == pid) {
            Some((_, stop)) => {
                *stop = true;
                true
            }
            None => false,
        }
    }

    /// Whether program `pid` was asked to stop.
    fn stop_asked(&self, pid: u32) -> bool {
        self.0.lock().is_ok_and(|p| p.iter().any(|(p, stop)| *p == pid && *stop))
    }

    fn add(&self, pid: u32) {
        if let Ok(mut pids) = self.0.lock() {
            pids.push((pid, false));
        }
    }

    fn remove(&self, pid: u32) {
        if let Ok(mut pids) = self.0.lock() {
            pids.retain(|(p, _)| *p != pid);
        }
    }
}

/// Takes a program off its list when its run ends, however it ends.
struct Listed<'a>(Option<&'a Running>, u32);

impl Drop for Listed<'_> {
    fn drop(&mut self) {
        if let Some(running) = self.0 {
            running.remove(self.1);
        }
    }
}

impl Run {
    /// `program` with `args`, a minute at most.
    pub fn new(
        program: impl Into<OsString>,
        args: impl IntoIterator<Item = impl Into<OsString>>,
    ) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            dir: None,
            env: Vec::new(),
            stdin: None,
            timeout: Duration::from_secs(60),
            stop: None,
            running: None,
            owner: "process",
        }
    }

    /// What runs it (its helper threads are named after it).
    pub fn owner(mut self, owner: &'static str) -> Self {
        self.owner = owner;
        self
    }

    /// A command line as the user writes it (`./scripts/linux/shell.sh composer cs:fix`),
    /// run by the system's shell: `sh -c` on Unix, `cmd /C` on Windows.
    pub fn shell(command: &str) -> Self {
        if cfg!(windows) {
            Self::new("cmd", ["/C", command])
        } else {
            Self::new("sh", ["-c", command])
        }
    }

    pub fn dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.dir = Some(dir.into());
        self
    }

    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    pub fn stdin(mut self, input: impl Into<Vec<u8>>) -> Self {
        self.stdin = Some(input.into());
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn stop_when(mut self, stop: Arc<AtomicBool>) -> Self {
        self.stop = Some(stop);
        self
    }

    /// Listed in `running` while it runs.
    pub fn listed_in(mut self, running: Arc<Running>) -> Self {
        self.running = Some(running);
        self
    }
}

/// How a run ended and what it printed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Output {
    /// Its exit code (`None`: ended by a signal).
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }

    /// Its last meaningful line, for the user (programs report on stderr too).
    pub fn summary(&self) -> String {
        let last = |s: &str| s.lines().map(str::trim).rfind(|l| !l.is_empty()).map(str::to_owned);
        last(&self.stderr).or_else(|| last(&self.stdout)).unwrap_or_default()
    }
}

/// Why it didn't run to its end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunError {
    /// The program isn't installed (or not where it was said to be).
    NotFound(String),
    /// Stopped after its timeout.
    TimedOut,
    /// Stopped because a newer run replaced it.
    Stopped,
    Failed(String),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(program) => write!(f, "{program} not found"),
            Self::TimedOut => f.write_str("timed out"),
            Self::Stopped => f.write_str("stopped"),
            Self::Failed(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for RunError {}

/// Runs it to its end (or its timeout, or until stopped). Blocks: call it off the UI thread.
pub fn run(spec: &Run) -> Result<Output, RunError> {
    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .stdin(if spec.stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = &spec.dir {
        command.current_dir(dir);
    }
    for (key, value) in &spec.env {
        command.env(key, value);
    }
    // Windows: no console window flashing up.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    // Unix: a process group of its own, stopped whole (`sh -c` and what it started).
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = match command.spawn() {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(RunError::NotFound(spec.program.to_string_lossy().into_owned()));
        }
        Err(e) => return Err(RunError::Failed(e.to_string())),
        Ok(child) => child,
    };
    let running = spec.running.as_deref();
    if let Some(running) = running {
        running.add(child.id());
    }
    let _listed = Listed(running, child.id());
    // Its input written on a thread of its own (it may read slowly), then closed.
    let writer = spec.stdin.clone().zip(child.stdin.take()).map(|(input, mut pipe)| {
        std::thread::Builder::new().name(format!("{}-input", spec.owner)).spawn(move || {
            let _ = pipe.write_all(&input);
        })
    });
    // Both outputs read while it runs (a full pipe would block it).
    let output = format!("{}-output", spec.owner);
    let drain = |pipe: Option<Box<dyn std::io::Read + Send>>| {
        std::thread::Builder::new().name(output.clone()).spawn(move || {
            let mut text = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = (&mut pipe).take(MAX_OUTPUT as u64).read_to_end(&mut text);
                // The rest read and dropped (the program never blocks on a full pipe).
                let _ = std::io::copy(&mut pipe, &mut std::io::sink());
            }
            String::from_utf8_lossy(&text).into_owned()
        })
    };
    let stdout = drain(child.stdout.take().map(|p| Box::new(p) as Box<dyn std::io::Read + Send>));
    let stderr = drain(child.stderr.take().map(|p| Box::new(p) as Box<dyn std::io::Read + Send>));
    let started = Instant::now();
    let pid = child.id();
    // Recorded once it has run long enough to matter if the app crashed (`launched`).
    let mut recorded: Option<launched::Recorded> = None;
    let mut told = false;
    let stopped = || {
        spec.stop.as_ref().is_some_and(|s| s.load(Ordering::Relaxed))
            || running.is_some_and(|r| r.stop_asked(pid))
    };
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > spec.timeout || stopped() => {
                kill_tree(&mut child);
                let _ = child.wait();
                return Err(if stopped() { RunError::Stopped } else { RunError::TimedOut });
            }
            Ok(None) => {
                if !told && started.elapsed() >= launched::LONG_LIVED {
                    told = true;
                    recorded = recorded_launch(spec, pid);
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(RunError::Failed(e.to_string())),
        }
    };
    drop(recorded);
    if let Some(Ok(writer)) = writer {
        let _ = writer.join();
    }
    Ok(Output {
        code: status.code(),
        stdout: stdout.ok().and_then(|t| t.join().ok()).unwrap_or_default(),
        stderr: stderr.ok().and_then(|t| t.join().ok()).unwrap_or_default(),
    })
}

/// A program started to keep running beside the app (a driver speaking a protocol on its
/// stdin and stdout): its three pipes for the caller, stopped with what it started when
/// [`Started::stop`] is called or when it's dropped.
#[derive(Debug)]
pub struct Started {
    child: std::process::Child,
    pub stdin: Option<std::process::ChildStdin>,
    pub stdout: Option<std::process::ChildStdout>,
    pub stderr: Option<std::process::ChildStderr>,
    _listed: Option<ListedOwned>,
    /// Told to the app's recorder while it runs (its end told when this goes, after it's
    /// stopped).
    _recorded: Option<launched::Recorded>,
}

/// Takes a program off its list when it's dropped (owned: it lives as long as the program).
#[derive(Debug)]
struct ListedOwned(Arc<Running>, u32);

impl Drop for ListedOwned {
    fn drop(&mut self) {
        self.0.remove(self.1);
    }
}

impl Started {
    /// Its process id.
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    /// How it ended, if it has: its exit code (`Some(None)`: ended by a signal).
    pub fn exited(&mut self) -> Option<Option<i32>> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(status.code()),
            Ok(None) => None,
            Err(_) => Some(None),
        }
    }

    /// Waits up to `wait` for it to end by itself (it was asked to): whether it did.
    pub fn wait_for(&mut self, wait: Duration) -> bool {
        let started = Instant::now();
        loop {
            if self.exited().is_some() {
                return true;
            }
            if started.elapsed() >= wait {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Stops it and what it started, now (its pipes closed).
    pub fn stop(&mut self) {
        self.stdin = None;
        if self.exited().is_none() {
            kill_tree(&mut self.child);
        }
        let _ = self.child.wait();
    }
}

impl Drop for Started {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Starts `spec`'s program to keep running (its `timeout`, `stop` and `stdin` aren't
/// used: the caller talks to it through [`Started`]'s pipes and stops it), in its own
/// process group, no console window on Windows. Its arguments are passed as they are,
/// never through a shell.
pub fn start(spec: &Run) -> Result<Started, RunError> {
    let mut command = Command::new(&spec.program);
    command.args(&spec.args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(dir) = &spec.dir {
        command.current_dir(dir);
    }
    for (key, value) in &spec.env {
        command.env(key, value);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = match command.spawn() {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(RunError::NotFound(spec.program.to_string_lossy().into_owned()));
        }
        Err(e) => return Err(RunError::Failed(e.to_string())),
        Ok(child) => child,
    };
    let listed = spec.running.as_ref().map(|running| {
        running.add(child.id());
        ListedOwned(Arc::clone(running), child.id())
    });
    let recorded = recorded_launch(spec, child.id());
    Ok(Started {
        _recorded: recorded,
        stdin: child.stdin.take(),
        stdout: child.stdout.take(),
        stderr: child.stderr.take(),
        child,
        _listed: listed,
    })
}

/// `spec`'s program (`pid`) told to the app's recorder: its kind from its owner, the
/// project from its folder.
fn recorded_launch(spec: &Run, pid: u32) -> Option<launched::Recorded> {
    let kind = launched::Kind::of_owner(spec.owner);
    launched::record(pid, kind, Path::new(&spec.program), spec.dir.as_deref())
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Stops `child` and what it started (a shell's commands), so nothing keeps running nor
/// holds its outputs open.
fn kill_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        // Its own process group (`process_group(0)`): the group's id is its pid.
        if let Ok(group) = libc::pid_t::try_from(child.id()) {
            // SAFETY: `kill` only sends a signal; a negative pid names the child's group.
            #[allow(unsafe_code)]
            unsafe {
                libc::kill(-group, libc::SIGKILL);
            }
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &child.id().to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }
    let _ = child.kill();
}

/// Where `program` is found in `PATH` (with Windows' extensions), if anywhere.
pub fn which(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let names: Vec<String> = if cfg!(windows) && Path::new(program).extension().is_none() {
        ["exe", "cmd", "bat"].iter().map(|e| format!("{program}.{e}")).collect()
    } else {
        vec![program.to_owned()]
    };
    std::env::split_paths(&path)
        .flat_map(|dir| names.iter().map(move |n| dir.join(n)))
        .find(|p| p.is_file())
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
