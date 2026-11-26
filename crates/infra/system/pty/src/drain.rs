//! A terminal's output drained while it's let go. Dropping the PTY hangs up on its program
//! and waits for it to end, with the PTY still open and nobody reading it any more; a
//! program that was writing (a shell at its hangup, an agent stopped) can't end while what
//! it wrote is still to be read: it stays exiting, the wait never returns, and the terminal
//! with its memory is held for good.
//!
//! - **Unix:** while the PTY is dropped, a thread of its own reads what's left and throws it
//!   away, until the drop is done (or the PTY has nothing more: every program on it gone).
//!   The copy it reads is closed at its next look once the drop is done (within
//!   [`LOOK_EVERY`]): a job left on the terminal sees it closed that much later.
//! - **Windows:** closing the pseudo console waits until what it still writes is read, and
//!   the emulator's reader holds a buffer of a fixed size that nobody empties once the
//!   terminal is let go. There the program is ended first (closing the console would end it
//!   anyway), then the buffer is emptied until nothing more comes ([`drain_until_quiet`]),
//!   on the thread closing it: room for what the console writes as it ends.
//!
//! Only while the program still runs (its end not seen): one that ended has nothing left
//! to wait for. Nothing is read before the terminal is let go: a live one loses nothing.

use std::io::Read;
#[cfg(unix)]
use std::os::fd::{AsRawFd, OwnedFd};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// How long a look at the PTY waits before seeing whether the drop is done: short enough
/// for the copy to go soon after the PTY, long enough for the thread to sleep meanwhile.
#[cfg(unix)]
const LOOK_EVERY: Duration = Duration::from_millis(100);

/// A pause after a read that found nothing though the PTY said it had something: never a
/// loop on the processor while it settles.
#[cfg(unix)]
const SETTLE: Duration = Duration::from_millis(10);

/// The bytes read at a time (thrown away).
const CHUNK: usize = 16 * 1024;

/// The most [`drain_left`] reads at once: a program writing faster than it's read never
/// holds it, 16 MB at most.
// Used on Windows only (and by the tests).
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
const LEFT_MOST: usize = 1024 * CHUNK;

/// Between two looks at a reader found empty, while waiting for it to stay so.
// Used on Windows only (and by the tests).
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
const QUIET_STEP: Duration = Duration::from_millis(20);

/// The looks in a row a reader must be found empty to be done (100 ms of nothing).
// Used on Windows only (and by the tests).
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
const QUIET_LOOKS: u32 = 5;

/// The longest [`drain_until_quiet`] goes on (a program still writing then: closed anyway).
#[cfg(windows)]
const QUIET_MOST: Duration = Duration::from_secs(2);

/// Whether the terminal's program still runs (its end not seen).
pub(crate) type Running = Box<dyn Fn() -> bool + Send>;

/// What drains the PTY once it's let go: a second handle to it (Unix) or its program's id
/// (Windows), taken at the start.
#[derive(Default)]
pub(crate) struct Drain {
    #[cfg(unix)]
    fd: Option<OwnedFd>,
    #[cfg(windows)]
    pid: Option<u32>,
    /// `None`: nothing to drain.
    running: Option<Running>,
    /// Set when the drop is done: the thread stops.
    done: Arc<AtomicBool>,
}

impl Drain {
    /// A drain for a program `running` says still runs.
    pub(crate) fn new(running: Running) -> Self {
        let mut drain = Self::default();
        drain.running = Some(running);
        drain
    }

    /// Drained through `fd` (a copy of the PTY's handle: the PTY keeps its own).
    #[cfg(unix)]
    pub(crate) fn through(mut self, fd: Option<OwnedFd>) -> Self {
        self.fd = fd;
        self
    }

    /// Its program `pid`, ended before the console is closed.
    #[cfg(windows)]
    pub(crate) fn ending(mut self, pid: Option<u32>) -> Self {
        self.pid = pid;
        self
    }

    /// Whether there's something to drain: its program still runs.
    pub(crate) fn wanted(&self) -> bool {
        self.running.as_ref().is_some_and(|running| running())
    }

    /// Before the console is closed: its program ended, then `reader` emptied until
    /// nothing more comes (on the thread closing it).
    #[cfg(windows)]
    pub(crate) fn empty_before_close(&self, reader: &mut impl Read) {
        if let Some(pid) = self.pid {
            process::descendants::Descendants::with_root(pid).first().stop(Duration::ZERO);
        }
        drain_until_quiet(reader, QUIET_MOST);
    }

    /// The thread draining started (once; nothing without a handle).
    pub(crate) fn start(&mut self) {
        #[cfg(unix)]
        if let Some(fd) = self.fd.take() {
            let done = Arc::clone(&self.done);
            let spawned = std::thread::Builder::new()
                .name("pty-drain".into())
                .spawn(move || drain(&fd, &done));
            if let Err(error) = spawned {
                tracing::warn!(%error, "can't drain a closed terminal");
            }
        }
    }
}

impl Drop for Drain {
    /// The PTY's drop done (it's dropped before this): the thread stops.
    fn drop(&mut self) {
        self.done.store(true, Ordering::Release);
    }
}

/// Reads `fd` and throws it away until `done`, or until it has nothing more to give (every
/// program on it gone: an end of file, a hangup or an error).
#[cfg(unix)]
#[allow(unsafe_code)]
fn drain(fd: &OwnedFd, done: &AtomicBool) {
    let raw = fd.as_raw_fd();
    let wait = libc::c_int::try_from(LOOK_EVERY.as_millis()).unwrap_or(libc::c_int::MAX);
    let mut buffer = vec![0u8; CHUNK];
    while !done.load(Ordering::Acquire) {
        let mut look = libc::pollfd { fd: raw, events: libc::POLLIN, revents: 0 };
        // SAFETY: one `pollfd` of an fd this thread owns, counted as one.
        let ready = unsafe { libc::poll(&mut look, 1, wait) };
        if ready < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return;
        }
        if ready == 0 {
            continue;
        }
        // Nothing to read, only a hangup or an error: nothing more will come.
        if look.revents & libc::POLLIN == 0 {
            return;
        }
        // SAFETY: `buffer` is owned and `CHUNK` bytes long; the fd is this thread's own.
        let read = unsafe { libc::read(raw, buffer.as_mut_ptr().cast(), buffer.len()) };
        if read == 0 {
            return;
        }
        if read < 0 {
            match std::io::Error::last_os_error().kind() {
                std::io::ErrorKind::Interrupted => {}
                std::io::ErrorKind::WouldBlock => std::thread::sleep(SETTLE),
                _ => return,
            }
        }
    }
}

/// What `reader` holds now, read and thrown away: until it's empty (`Ok(0)`, as the
/// emulator's buffered reader says it; an error too), or [`LEFT_MOST`] was read. The bytes
/// read.
// Used on Windows only (and by the tests).
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
pub(crate) fn drain_left(reader: &mut impl Read) -> usize {
    let mut buffer = vec![0u8; CHUNK];
    let mut read = 0;
    while read < LEFT_MOST {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => read += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    read
}

/// `reader` emptied until it stays empty [`QUIET_LOOKS`] looks in a row, for at most
/// `most`. The bytes read.
// Used on Windows only (and by the tests).
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
pub(crate) fn drain_until_quiet(reader: &mut impl Read, most: Duration) -> usize {
    let until = Instant::now() + most;
    let (mut read, mut quiet) = (0, 0);
    while quiet < QUIET_LOOKS && Instant::now() < until {
        match drain_left(reader) {
            0 => {
                quiet += 1;
                std::thread::sleep(QUIET_STEP);
            }
            n => {
                read += n;
                quiet = 0;
            }
        }
    }
    read
}

#[cfg(test)]
#[path = "../tests/unit/drain.rs"]
mod tests;
