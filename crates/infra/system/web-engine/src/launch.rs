//! Where the host is found and how it's started.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

use web_host_core::channel::Channel;

/// The folder the channel goes in where the system gives no private one of the user's
/// (Linux without `XDG_RUNTIME_DIR` nor a settings folder): under the temporary folder,
/// named by the user's id, the user's alone (`0700`).
#[cfg(all(unix, not(target_os = "macos")))]
const PRIVATE_DIR_PREFIX: &str = "editor-web-";

/// The host's channel for this user: its files in the user's private folder (macOS: the
/// user's temporary folder; elsewhere `XDG_RUNTIME_DIR`, else the app's settings folder,
/// else a folder of the user's own under the temporary one), as the app's own instance
/// channel.
pub fn channel() -> Channel {
    #[cfg(any(target_os = "macos", windows))]
    let dir = std::env::temp_dir();
    #[cfg(all(unix, not(target_os = "macos")))]
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .or_else(files::places::config_dir)
        .unwrap_or_else(private_temporary);
    #[cfg(not(any(unix, windows)))]
    let dir = std::env::temp_dir();
    Channel::for_user(dir)
}

/// The user's own folder under the temporary one (the temporary one itself is everyone's,
/// and owned by the system: the host there would never be trusted).
#[cfg(all(unix, not(target_os = "macos")))]
fn private_temporary() -> PathBuf {
    std::env::temp_dir().join(format!("{PRIVATE_DIR_PREFIX}{}", user()))
}

/// Unix: the user this process runs as (its effective id).
#[cfg(unix)]
pub(crate) fn user() -> u32 {
    // SAFETY: `geteuid` reads this process's user; it can't fail.
    #[allow(unsafe_code)]
    unsafe {
        libc::geteuid()
    }
}

/// The channel's folder made if it isn't there, and trusted only when it's the user's alone:
/// on Unix one another user owns (made first in the temporary folder, to squat the channel),
/// or one others may write in, is refused ([`std::io::ErrorKind::PermissionDenied`]) before
/// anything is connected to or started there.
pub(crate) fn own_channel_dir(dir: &Path) -> std::io::Result<()> {
    make_channel_dir(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let found = std::fs::metadata(dir)?;
        if let Some(why) = untrusted_dir(found.uid(), found.mode(), user()) {
            return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, why));
        }
    }
    Ok(())
}

/// Unix: the bits of a folder's mode that let its group or anyone else write in it.
#[cfg(unix)]
const WRITABLE_BY_OTHERS: u32 = 0o022;

/// Unix: why a folder of `owner` and `mode` can't hold the channel of user `me` (`None`:
/// it's theirs alone).
#[cfg(unix)]
pub(crate) fn untrusted_dir(owner: u32, mode: u32, me: u32) -> Option<&'static str> {
    if owner != me {
        return Some("the web engine's channel folder is another user's");
    }
    (mode & WRITABLE_BY_OTHERS != 0)
        .then_some("others may write in the web engine's channel folder")
}

/// `dir` made for the channel if it isn't there, the user's alone on Unix (`0700`); an
/// existing one is left as it is (whose it is, [`own_channel_dir`] checks).
fn make_channel_dir(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(dir)
    }
}

/// How to start the host: its program (the engine's, installed in the app's data), the
/// channel it takes, and where the pages keep their own storage (draw.io's: kept between
/// runs, so never in a temporary folder the system cleans).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub program: PathBuf,
    pub channel: Channel,
    pub profile: PathBuf,
    /// The file the host holds locked while it runs (its version's `.in-use`: a version no
    /// host runs from may be cleaned); `None`: none (an engine built here).
    pub hold: Option<PathBuf>,
    /// The GPU it draws on (`--adapter-luid`; `None`: Chromium's choice).
    pub adapter: Option<web_engine_protocol::Adapter>,
}

/// How a host started ends, as seen from here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    /// It still runs.
    Running,
    /// It ended by itself, with success: another host had the channel (it had nothing to do).
    Yielded,
    /// It ended with an error, or was killed: the engine failed.
    Failed,
}

/// A host started: whether it still runs (its end is waited for on a thread of its own,
/// which says it when it comes).
#[derive(Debug, Clone)]
pub struct Started {
    ended: Arc<(Mutex<Ended>, Condvar)>,
}

impl Started {
    /// How it stands now (memory only).
    pub fn ended(&self) -> Ended {
        *self.ended.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// How it stands once it ended, or after `limit` (then [`Ended::Running`]).
    pub fn wait_ended(&self, limit: Duration) -> Ended {
        let (how, changed) = &*self.ended;
        let how = how.lock().unwrap_or_else(PoisonError::into_inner);
        let (how, _) = changed
            .wait_timeout_while(how, limit, |how| *how == Ended::Running)
            .unwrap_or_else(PoisonError::into_inner);
        *how
    }

    /// Tests: one said to end as `how` after `after`.
    #[cfg(test)]
    pub(crate) fn ending(how: Ended, after: Duration) -> Self {
        let started = Self { ended: Arc::new((Mutex::new(Ended::Running), Condvar::new())) };
        let ended = Arc::clone(&started.ended);
        std::thread::spawn(move || {
            std::thread::sleep(after);
            *ended.0.lock().unwrap_or_else(PoisonError::into_inner) = how;
            ended.1.notify_all();
        });
        started
    }
}

impl Launch {
    /// The host started on its own (not a child the app waits for: it outlives the app,
    /// shared by every app of the user, and ends by itself when idle).
    pub fn start(&self) -> std::io::Result<Started> {
        make_channel_dir(&self.channel.dir)?;
        let mut command = Command::new(&self.program);
        command
            .arg("--channel-dir")
            .arg(&self.channel.dir)
            .arg("--channel-name")
            .arg(&self.channel.name)
            .arg("--profile")
            .arg(&self.profile);
        if let Some(hold) = &self.hold {
            command.arg("--hold").arg(hold);
        }
        if let Some(adapter) = &self.adapter {
            command.arg("--adapter-luid").arg(adapter.switch_value());
        }
        command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        // Its own folder as its working one (it never holds the app's: on Windows a folder in
        // use can't be renamed).
        if let Some(folder) = self.program.parent() {
            command.current_dir(folder);
        }
        detach(&mut command);
        let mut child = command.spawn()?;
        let started = Started { ended: Arc::new((Mutex::new(Ended::Running), Condvar::new())) };
        let ended = Arc::clone(&started.ended);
        // Waited for when it ends (else it would stay a zombie until the app does).
        std::thread::Builder::new().name("web-engine-reap".into()).spawn(move || {
            let how = match child.wait() {
                Ok(status) if status.success() => Ended::Yielded,
                Ok(_) | Err(_) => Ended::Failed,
            };
            *ended.0.lock().unwrap_or_else(PoisonError::into_inner) = how;
            ended.1.notify_all();
        })?;
        Ok(started)
    }
}

/// `command` started apart from the app: not in its process group (a terminal's Ctrl-C or
/// the terminal closing doesn't stop it), no console window on Windows.
fn detach(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Threading::{
            CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, DETACHED_PROCESS,
        };
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }
}
