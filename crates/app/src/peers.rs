//! The app's other instances of this user's settings (another executable beside this one:
//! the installed app and a development build; or one started apart on purpose): what
//! stands for the whole app is one for all of them — a view over every window (the dimmed
//! screen, a calm scene) opened or closed in one is opened or closed in the others.
//!
//! Each instance listens on a channel of its own, named after the settings' folder (an
//! instance with other settings is in no group with this one) and its process; it finds
//! the others by their names. The system's local channels only, as [`crate::instance`]'s:
//! a named pipe on Windows, a socket in the user's private folder elsewhere. A message is
//! one line ([`crate::over`]): `over <extension>` (opened), with its ticket when it's on
//! the whole screen, `over` (closed).

use std::io::{BufRead, BufReader, Write};
use std::sync::{Arc, Mutex};

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{ListenerOptions, Name, Stream};

use crate::over::{Over, message, parse};

/// What the others said last (`Some(None)`: closed), not taken yet.
static HEARD: Mutex<Option<Option<Over>>> = Mutex::new(None);
/// Wakes the event loop when they say something (set once a window exists).
static WAKER: Mutex<Option<Arc<dyn Fn() + Send + Sync>>> = Mutex::new(None);
/// This instance's group and name (set once it listens).
static ME: Mutex<Option<Peer>> = Mutex::new(None);

/// An instance: its group (the settings' folder) and its own number (its process).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Peer {
    pub group: String,
    pub id: u32,
}

impl Peer {
    /// The channel's name: what the others find it by.
    fn name(&self) -> String {
        format!("{}{}", prefix(&self.group), self.id)
    }
}

/// What every channel of `group` is named from.
fn prefix(group: &str) -> String {
    format!("editor-peer-{group}-")
}

/// The group of the instances sharing this user's settings.
fn group() -> Option<String> {
    let dir = crate::config::config_dir()?;
    let dir = dir.canonicalize().unwrap_or(dir);
    Some(format!("{:016x}", crate::instance::fnv(&dir)))
}

/// Starts listening to the others (once, at startup).
pub fn join() {
    let Some(group) = group() else { return };
    let me = Peer { group, id: std::process::id() };
    match listen(&me, on_message) {
        Ok(()) => {
            if let Ok(mut slot) = ME.lock() {
                *slot = Some(me);
            }
        }
        Err(e) => tracing::debug!("can't listen to the app's other instances: {e}"),
    }
}

/// The event loop's waker: a message wakes it.
pub fn set_waker(waker: Arc<dyn Fn() + Send + Sync>) {
    if let Ok(mut slot) = WAKER.lock() {
        *slot = Some(waker);
    }
}

/// What the others said since the last call: the view over every window (`None`: closed).
pub fn take() -> Option<Option<Over>> {
    HEARD.lock().ok()?.take()
}

/// Tells the others the view over every window is now `wanted` (`None`: closed); in the
/// background (none is waited on).
pub fn tell(wanted: Option<Over>) {
    let Some(me) = ME.lock().ok().and_then(|m| m.clone()) else { return };
    kernel::work::run("peers", move || {
        let line = message(wanted.as_ref());
        for peer in others(&me) {
            send(&peer, &line);
        }
    });
}

fn on_message(said: Option<Over>) {
    if let Ok(mut heard) = HEARD.lock() {
        *heard = Some(said);
    }
    if let Some(wake) = WAKER.lock().ok().and_then(|w| w.clone()) {
        wake();
    }
}

/// `peer`'s channel.
fn address(peer: &Peer) -> std::io::Result<Name<'static>> {
    #[cfg(windows)]
    {
        peer.name().to_ns_name::<interprocess::local_socket::GenericNamespaced>()
    }
    #[cfg(not(windows))]
    {
        crate::instance::socket_dir()
            .join(format!("{}.sock", peer.name()))
            .to_fs_name::<interprocess::local_socket::GenericFilePath>()
    }
}

/// Listens as `me`, each message given to `heard` (on the listener's thread).
pub(crate) fn listen(me: &Peer, heard: fn(Option<Over>)) -> std::io::Result<()> {
    let listener = ListenerOptions::new().name(address(me)?).try_overwrite(true).create_sync()?;
    std::thread::Builder::new().name("peers".into()).spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut line = String::new();
            if BufReader::new(stream).read_line(&mut line).is_ok()
                && let Some(said) = parse(&line)
            {
                heard(said);
            }
        }
    })?;
    Ok(())
}

/// The other instances of `me`'s group, by their channels' names.
pub(crate) fn others(me: &Peer) -> Vec<Peer> {
    let prefix = prefix(&me.group);
    #[cfg(windows)]
    let (dir, suffix) = (std::path::PathBuf::from(r"\\.\pipe\"), "");
    #[cfg(not(windows))]
    let (dir, suffix) = (crate::instance::socket_dir(), ".sock");
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let id = name.to_str()?.strip_prefix(&prefix)?.strip_suffix(suffix)?.parse().ok()?;
            Some(Peer { group: me.group.clone(), id })
        })
        .filter(|peer| peer.id != me.id)
        .collect()
}

/// `line` to `peer`; one that doesn't answer is gone (its socket left by a crash: removed).
fn send(peer: &Peer, line: &str) {
    let Ok(name) = address(peer) else { return };
    let Ok(mut stream) = Stream::connect(name) else {
        #[cfg(not(windows))]
        let _ = std::fs::remove_file(
            crate::instance::socket_dir().join(format!("{}.sock", peer.name())),
        );
        return;
    };
    let _ = stream.write_all(line.as_bytes());
}

/// Sends `line` to `peer` (tests).
#[cfg(test)]
pub(crate) fn send_to(peer: &Peer, line: &str) {
    send(peer, line);
}

#[cfg(test)]
#[path = "../tests/unit/peers.rs"]
mod tests;
