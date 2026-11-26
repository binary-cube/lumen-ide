//! The PTY as the emulator's event loop reads it, with one sequence taken out when it isn't
//! honored: `ESC[3J`, a program erasing the history (see `terminal_session::history`).
//!
//! alacritty's loop parses what it reads inside itself (no hook between the read and the
//! parser), so the reader it is given is ours: the PTY's, through an [`EraseFilter`]. Bytes
//! without an `ESC` pass untouched, without a copy; the foreground is looked at only when an
//! `ESC[3J` arrives.
//!
//! The erases of the history it lets through (an `ESC[3J` honored, a full reset `ESC c`) are
//! counted as they are handed to the emulator, for the parked history (see `parking`).

use std::io::{self, Read};
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use alacritty_terminal::event::{OnResize, WindowSize};
use alacritty_terminal::tty::{ChildEvent, EventedPty, EventedReadWrite};
use polling::{Event, PollMode, Poller};
use terminal_session::history::{EraseFilter, Foreground, HistoryClear, honors};

use crate::drain::Drain;
use crate::foreground::Probe;

/// The PTY `P`, its output filtered; drained while it's let go ([`Drain`]).
pub(crate) struct Filtered<P: EventedReadWrite> {
    reader: FilterReader<P>,
    /// Dropped after the PTY (the fields' order): it drains while the PTY's drop waits.
    drain: Drain,
}

impl<P: EventedReadWrite> Drop for Filtered<P> {
    /// The PTY dropped next (it waits for its program, or its console for its last output):
    /// its output drained meanwhile (Unix), its reader's buffer emptied first (Windows).
    fn drop(&mut self) {
        if !self.drain.wanted() {
            return;
        }
        #[cfg(windows)]
        self.drain.empty_before_close(self.reader.pty.reader());
        self.drain.start();
    }
}

impl<P: EventedReadWrite> Filtered<P> {
    /// `policy`: the user's choice ([`HistoryClear::to_u8`]), changed while it runs;
    /// `redrawing`: when the program redrawing over its screen started (see [`Redrawing`]);
    /// `erased`: the erases of the history handed to the emulator, counted.
    pub(crate) fn new(
        pty: P,
        policy: Arc<AtomicU8>,
        probe: Arc<Mutex<Probe>>,
        redrawing: Redrawing,
        erased: Arc<AtomicU64>,
    ) -> Self {
        Self {
            reader: FilterReader {
                pty,
                filter: EraseFilter::default(),
                ready: Vec::new(),
                held: None,
                policy,
                probe,
                redrawing,
                erased,
                erase_at: None,
            },
            drain: Drain::default(),
        }
    }

    /// Drained by `drain` while it's let go.
    pub(crate) fn draining(mut self, drain: Drain) -> Self {
        self.drain = drain;
        self
    }
}

/// The program that redraws over its whole screen (its history erases dropped): when it
/// started, as the system said (`None`: none, or the shell's `clear` since). A resize while
/// it's still the one in the foreground keeps the history as it was (see
/// `resize_keeping_history`).
pub(crate) type Redrawing = Arc<Mutex<Option<Instant>>>;

/// Reads the PTY, passing what it read through the filter.
pub(crate) struct FilterReader<P> {
    pty: P,
    filter: EraseFilter,
    /// Filtered bytes that didn't fit the caller's buffer yet.
    ready: Vec<u8>,
    /// A held `ESC[2J` found nothing more to read: since when, and whether a running
    /// program wrote it (then it waits a moment for an `ESC[3J` written apart).
    held: Option<(Instant, bool)>,
    policy: Arc<AtomicU8>,
    probe: Arc<Mutex<Probe>>,
    redrawing: Redrawing,
    /// The erases of the history handed to the emulator.
    erased: Arc<AtomicU64>,
    /// Where the last erase let through ends in `ready`: counted once handed out (the
    /// emulator parses what it's handed before anyone else may lock it).
    erase_at: Option<usize>,
}

/// Room left in the caller's buffer for the bytes a sequence held from an earlier read.
const HELD_ROOM: usize = 64;

/// How long a running program's `ESC[2J`, the last thing it wrote, waits for an `ESC[3J`
/// written apart (the reader drained between them) before it goes out as it is; and how
/// often the PTY is read meanwhile. Only a read ending exactly in `ESC[2J` waits.
pub(crate) const HOLD: Duration = Duration::from_millis(40);
const HOLD_STEP: Duration = Duration::from_millis(4);

impl<P: EventedReadWrite> Read for FilterReader<P> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            if !self.ready.is_empty() {
                let n = self.ready.len().min(buf.len());
                buf[..n].copy_from_slice(&self.ready[..n]);
                self.ready.drain(..n);
                if let Some(at) = self.erase_at {
                    if at <= n {
                        self.erased.fetch_add(1, Ordering::AcqRel);
                        self.erase_at = None;
                    } else {
                        self.erase_at = Some(at - n);
                    }
                }
                return Ok(n);
            }
            let room = if buf.len() > 2 * HELD_ROOM { buf.len() - HELD_ROOM } else { buf.len() };
            let n = match self.pty.reader().read(&mut buf[..room]) {
                // Nothing more for now (or the end): a held `ESC[2J` goes out as it is.
                Ok(0) if self.filter.erase_held() => {
                    self.filter.flush(&mut self.ready);
                    continue;
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock && self.filter.erase_held() => {
                    // A running program's: a moment for its `ESC[3J`, written apart.
                    let (since, program) = *self.held.get_or_insert_with(|| {
                        let seen =
                            self.probe.lock().unwrap_or_else(PoisonError::into_inner).foreground();
                        (Instant::now(), matches!(seen, Foreground::Program(_)))
                    });
                    if program && since.elapsed() < HOLD {
                        std::thread::sleep(HOLD_STEP);
                        continue;
                    }
                    self.held = None;
                    self.filter.flush(&mut self.ready);
                    continue;
                }
                read => read?,
            };
            self.held = None;
            if n == 0 {
                return Ok(0);
            }
            if self.filter.idle() && !buf[..n].contains(&0x1b) {
                return Ok(n);
            }
            let policy = HistoryClear::from_u8(self.policy.load(Ordering::Relaxed));
            let probe = &self.probe;
            let mut asked = None;
            let mut honor = || {
                honors(policy, || {
                    let seen = probe.lock().unwrap_or_else(PoisonError::into_inner).foreground();
                    asked = Some(seen);
                    seen
                })
            };
            self.filter.feed(&buf[..n], &mut self.ready, &mut honor);
            let seen = self.filter.take_seen();
            if seen.erased {
                self.erase_at = Some(self.ready.len());
            }
            if seen.dropped || seen.honored {
                // Who it is, asked now if the policy didn't need to (kept always).
                if seen.dropped && asked.is_none() {
                    asked = Some(probe.lock().unwrap_or_else(PoisonError::into_inner).foreground());
                }
                let started = match asked {
                    Some(Foreground::Program(age)) if seen.dropped => {
                        Instant::now().checked_sub(age)
                    }
                    _ => None,
                };
                *self.redrawing.lock().unwrap_or_else(PoisonError::into_inner) = started;
            }
            // All of it held or dropped: read on (the PTY says when there's nothing more).
        }
    }
}

// The trait's `register` is unsafe (alacritty's contract with its poller): passed on as is.
#[allow(unsafe_code)]
impl<P: EventedReadWrite> EventedReadWrite for Filtered<P> {
    type Reader = FilterReader<P>;
    type Writer = P::Writer;

    unsafe fn register(
        &mut self,
        poll: &Arc<Poller>,
        event: Event,
        mode: PollMode,
    ) -> io::Result<()> {
        // SAFETY: the PTY's own registration, as alacritty would have made it.
        unsafe { self.reader.pty.register(poll, event, mode) }
    }

    fn reregister(&mut self, poll: &Arc<Poller>, event: Event, mode: PollMode) -> io::Result<()> {
        self.reader.pty.reregister(poll, event, mode)
    }

    fn deregister(&mut self, poll: &Arc<Poller>) -> io::Result<()> {
        self.reader.pty.deregister(poll)
    }

    fn reader(&mut self) -> &mut Self::Reader {
        &mut self.reader
    }

    fn writer(&mut self) -> &mut Self::Writer {
        self.reader.pty.writer()
    }
}

impl<P: EventedPty> EventedPty for Filtered<P> {
    fn next_child_event(&mut self) -> Option<ChildEvent> {
        self.reader.pty.next_child_event()
    }
}

impl<P: EventedReadWrite + OnResize> OnResize for Filtered<P> {
    fn on_resize(&mut self, size: WindowSize) {
        self.reader.pty.on_resize(size);
    }
}

#[cfg(test)]
#[path = "../tests/unit/filtered.rs"]
mod tests;
