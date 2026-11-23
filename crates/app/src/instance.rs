//! One running instance per executable: a second start (a project chosen from the
//! taskbar's jump list, the app started again, the command line) hands its paths to the
//! one running and exits; that one opens them (a project already open: its window to the
//! front; nothing: the app to the front).
//!
//! They talk over the system's local channel, never the network: a named pipe on Windows
//! (named after the user; the system lets only its owner write to it and turns remote
//! clients away), a Unix socket elsewhere, in the user's private folder (macOS: the user's
//! temporary folder; Linux: `XDG_RUNTIME_DIR`). Its name comes from the executable's path:
//! another binary (a development build beside the installed app) has its own. A socket
//! left by a crash answers nobody: this start runs and takes its place.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{ListenerOptions, Name, Stream};

/// The paths handed by later starts, in order (an empty list: just the app to the front).
static INCOMING: Mutex<VecDeque<Vec<PathBuf>>> = Mutex::new(VecDeque::new());
/// Wakes the event loop when paths arrive (set once a window exists).
static WAKER: Mutex<Option<Arc<dyn Fn() + Send + Sync>>> = Mutex::new(None);

/// What this start does.
#[derive(Debug, PartialEq, Eq)]
pub enum Start {
    /// It runs (it listens for later starts).
    Run,
    /// The running instance took the paths: this one exits.
    Handed,
}

/// Hands `paths` to the running instance, or becomes it.
pub fn claim(paths: &[PathBuf]) -> Start {
    let Some((tag, exe)) = executable_tag() else { return Start::Run };
    // Its channel's tag and its program in the log: a start that didn't reach the running
    // one shows whether it looked for the same (another program: another tag) and why not.
    match hand_over(&tag, paths) {
        Ok(()) => {
            tracing::info!(paths = paths.len(), tag = %tag, "handed to the running instance");
            return Start::Handed;
        }
        Err(why) => tracing::info!(tag = %tag, exe = %exe, why, "no running instance took it"),
    }
    match listen(&tag) {
        Ok(()) => tracing::info!(tag = %tag, exe = %exe, "listening for later starts"),
        Err(e) => tracing::warn!(tag = %tag, error = %e, "can't listen for later starts"),
    }
    Start::Run
}

/// The event loop's waker: paths arriving wake it.
pub fn set_waker(waker: Arc<dyn Fn() + Send + Sync>) {
    if let Ok(mut slot) = WAKER.lock() {
        *slot = Some(waker);
    }
}

/// Paths to open as a later start's (the system's own asking: macOS' "Open With"): queued,
/// the app woken.
pub fn hand_in(paths: Vec<PathBuf>) {
    if let Ok(mut queue) = INCOMING.lock() {
        queue.push_back(paths);
    }
    if let Some(wake) = WAKER.lock().ok().and_then(|w| w.clone()) {
        wake();
    }
}

/// The paths handed since the last call.
pub fn take() -> Vec<Vec<PathBuf>> {
    INCOMING.lock().map(|mut q| q.drain(..).collect()).unwrap_or_default()
}

/// What tells this executable's instance apart: a hash of its path (and the path, as it was
/// read, for the log).
fn executable_tag() -> Option<(String, String)> {
    let exe = std::env::current_exe().ok()?;
    let exe = exe.canonicalize().unwrap_or(exe);
    Some((format!("{:016x}", fnv(&exe)), exe.display().to_string()))
}

/// A stable hash of a path (FNV-1a: the same in every run, unlike `DefaultHasher`'s seed),
/// of its one written form: on Windows the same program may be started by another spelling
/// of its path (a shortcut's, "Open with"'s), all of them one channel.
pub(crate) fn fnv(path: &Path) -> u64 {
    kernel::hash::fnv64(one_form(&path.to_string_lossy(), cfg!(windows)))
}

/// `path` written one way: on Windows (`windows`), without the long-path prefix (`\\?\`,
/// `\\?\UNC\` as `\\`), with `\` only, in lower case (its paths ignore it), with no
/// separator at its end; elsewhere as it is.
pub(crate) fn one_form(path: &str, windows: bool) -> String {
    if !windows {
        return path.to_owned();
    }
    let path = path.replace('/', "\\");
    let path = match path.strip_prefix(r"\\?\UNC\") {
        Some(rest) => format!(r"\\{rest}"),
        None => path.strip_prefix(r"\\?\").map_or(path.clone(), str::to_owned),
    };
    let path = path.trim_end_matches('\\');
    path.to_lowercase()
}

/// The local channel of instance `tag`: a named pipe of the user (Windows), else a socket
/// in the user's private folder.
fn address(tag: &str) -> std::io::Result<Name<'static>> {
    #[cfg(windows)]
    {
        let user = std::env::var("USERNAME").unwrap_or_default();
        format!("editor-instance-{tag}-{user}")
            .to_ns_name::<interprocess::local_socket::GenericNamespaced>()
    }
    #[cfg(not(windows))]
    {
        socket_dir()
            .join(format!("editor-instance-{tag}.sock"))
            .to_fs_name::<interprocess::local_socket::GenericFilePath>()
    }
}

/// The user's private folder for sockets.
#[cfg(not(windows))]
pub(crate) fn socket_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        // Per user (`/var/folders/…/T/`), only theirs.
        return std::env::temp_dir();
    }
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .or_else(crate::config::config_dir)
        .unwrap_or_else(std::env::temp_dir)
}

/// Sends the paths to the running instance; why not when it didn't take them (none
/// listens: the channel isn't there; it didn't answer).
fn hand_over(tag: &str, paths: &[PathBuf]) -> Result<(), String> {
    let name = address(tag).map_err(|e| format!("no channel name: {e}"))?;
    let stream = Stream::connect(name).map_err(|e| format!("no channel: {e}"))?;
    let mut message = String::new();
    for path in paths {
        // Relative to where this start was made: absolute for the other one.
        let path = std::path::absolute(path).unwrap_or_else(|_| path.clone());
        message.push_str(&path.to_string_lossy());
        message.push('\n');
    }
    message.push('\n');
    let mut reader = BufReader::new(stream);
    reader.get_mut().write_all(message.as_bytes()).map_err(|e| format!("not sent: {e}"))?;
    let mut answer = String::new();
    reader.read_line(&mut answer).map_err(|e| format!("no answer: {e}"))?;
    if answer.trim() == "ok" { Ok(()) } else { Err(format!("answered {:?}", answer.trim())) }
}

/// Listens for later starts (a socket left by a crash is taken over).
fn listen(tag: &str) -> std::io::Result<()> {
    let listener = ListenerOptions::new().name(address(tag)?).try_overwrite(true).create_sync()?;
    std::thread::Builder::new().name("instance".into()).spawn(move || {
        for stream in listener.incoming().flatten() {
            receive(stream);
        }
    })?;
    Ok(())
}

/// A later start's paths (one per line, up to an empty one): queued (the app woken), then
/// answered `ok` — once it's answered, they're there.
fn receive(stream: Stream) -> Option<()> {
    let mut reader = BufReader::new(stream);
    let mut paths = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).ok()? == 0 {
            break;
        }
        let path = line.trim_end_matches(['\r', '\n']);
        if path.is_empty() {
            break;
        }
        paths.push(PathBuf::from(path));
    }
    hand_in(paths);
    reader.get_mut().write_all(b"ok\n").ok()
}

#[cfg(test)]
#[path = "../tests/unit/instance.rs"]
mod tests;
