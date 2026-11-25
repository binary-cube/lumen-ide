//! The host's one channel per user: its name, a lock that says who the host is, and the
//! way an app connects.
//!
//! The app computes the [`Channel`] (its folder: the user's private one, as the app's own
//! instance channel) and gives it to the host it starts, so both always agree.

use std::fs::{File, TryLockError};
use std::io;
use std::path::PathBuf;

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{ListenerOptions, Name, Stream};

/// Where the host is found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Channel {
    /// The channel's name ([`web_engine_protocol::channel_name`], and the user's on Windows:
    /// a named pipe is the machine's).
    pub name: String,
    /// The user's private folder: the socket (not on Windows) and the lock.
    pub dir: PathBuf,
}

/// The host's claim on its channel: the lock (held as long as this lives, or the server
/// it's given to) and the listener.
#[derive(Debug)]
pub struct Claim {
    pub listener: interprocess::local_socket::Listener,
    /// Kept by the server it's given to (the listener goes to its accepting thread) until
    /// the server is let go, after the engine ended: a new host takes the channel (its socket
    /// made anew over this one's) only once this one no longer uses the engine's profile.
    pub(crate) lock: File,
}

/// Windows: the pipe open to its owner (the user the host runs as) and the system only —
/// a named pipe is the machine's, and another user must not reach the host. It's made as
/// the first instance of its name: one another user made before is refused, never shared.
#[cfg(windows)]
fn owner_only() -> io::Result<interprocess::os::windows::security_descriptor::SecurityDescriptor> {
    let text = widestring::u16cstr!("D:P(A;;GA;;;OW)(A;;GA;;;SY)");
    interprocess::os::windows::security_descriptor::SecurityDescriptor::deserialize(text)
}

impl Channel {
    /// The channel of this protocol for the user, its files in `dir`.
    pub fn for_user(dir: PathBuf) -> Self {
        let mut name = web_engine_protocol::channel_name();
        if cfg!(windows) {
            let user = std::env::var("USERNAME").unwrap_or_default();
            name = format!("{name}-{user}");
        }
        Self { name, dir }
    }

    fn address(&self) -> io::Result<Name<'static>> {
        #[cfg(windows)]
        {
            self.name.clone().to_ns_name::<interprocess::local_socket::GenericNamespaced>()
        }
        #[cfg(not(windows))]
        {
            self.dir
                .join(format!("{}.sock", self.name))
                .to_fs_name::<interprocess::local_socket::GenericFilePath>()
        }
    }

    /// The lock's file: whoever holds it is the host.
    fn lock_path(&self) -> PathBuf {
        self.dir.join(format!("{}.lock", self.name))
    }

    /// A connection to the host (none runs: an error).
    pub fn connect(&self) -> io::Result<Stream> {
        Stream::connect(self.address()?)
    }

    /// The channel taken by this process, if no other host holds it (`None`: one does —
    /// this one has nothing to do). A socket left behind by a host that crashed is taken
    /// over: the lock went with that process.
    pub fn claim(&self) -> io::Result<Option<Claim>> {
        std::fs::create_dir_all(&self.dir)?;
        let lock =
            File::options().create(true).truncate(false).write(true).open(self.lock_path())?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(TryLockError::Error(e)) => return Err(e),
        }
        let options = ListenerOptions::new().name(self.address()?).try_overwrite(true);
        #[cfg(windows)]
        let options = {
            use interprocess::os::windows::local_socket::ListenerOptionsExt as _;
            options.security_descriptor(owner_only()?)
        };
        let listener = options.create_sync()?;
        Ok(Some(Claim { lock, listener }))
    }
}

#[cfg(test)]
#[path = "../tests/unit/channel.rs"]
mod tests;
