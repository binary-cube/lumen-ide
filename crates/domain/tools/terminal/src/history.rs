//! The history (scrollback) against programs that ask to erase it: `ESC [ 3 J` (ED 3).
//!
//! Some programs send it at every redraw (`ESC[2J ESC[3J ESC[H`, Ink-based ones whenever
//! their output is taller than the screen), wiping what the user ran before. A shell's own
//! `clear` / `reset` / `tput clear` should still empty it. So the decision is made by who
//! is in the foreground when it arrives ([`Foreground`]: the shell, or a program started
//! just now, or one running for a while) and by the user's choice ([`HistoryClear`]) —
//! never by a program's name.
//!
//! [`EraseFilter`] finds that one sequence in the bytes from the PTY (across reads), before
//! the emulator sees them, and drops it when it isn't honored: the screen is still erased,
//! only the history stays. The `ESC[2J` beside a dropped one (just before it, or just after
//! with only the cursor moved between) erases the screen without pushing it into the history:
//! alacritty's `ESC[2J` scrolls the screen up into it (as kitty / Ghostty do; xterm's
//! doesn't), which the program's `ESC[3J` used to wipe — kept, each redraw would leave a copy
//! of the program's previous frame there (one per resize of the panel).

use std::time::Duration;

/// What happens when a program asks to erase the history (Settings → Terminal).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HistoryClear {
    /// Always honored (as xterm).
    Standard,
    /// Honored from the shell and from a command just started from it (`clear`), ignored
    /// while a program has been running in the foreground for a while (a TUI, an agent, a
    /// REPL, a watcher).
    #[default]
    KeepForRunningPrograms,
    /// Never honored: only the user empties it (Clear Terminal).
    KeepAlways,
}

impl HistoryClear {
    /// As kept in an atomic (shared with the PTY's reader thread).
    pub fn to_u8(self) -> u8 {
        match self {
            Self::Standard => 0,
            Self::KeepForRunningPrograms => 1,
            Self::KeepAlways => 2,
        }
    }

    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Standard,
            2 => Self::KeepAlways,
            _ => Self::KeepForRunningPrograms,
        }
    }
}

/// Who has the terminal when the request arrives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Foreground {
    /// The shell itself (its prompt, its built-ins).
    Shell,
    /// A program started from the shell, running for this long.
    Program(Duration),
    /// Couldn't be told (the system didn't say).
    Unknown,
}

/// A program younger than this is taken for a short command (`clear`, `reset`).
pub const SHORT_COMMAND: Duration = Duration::from_secs(1);

/// Whether the request to erase the history is honored under `policy`; `foreground` is
/// asked only when it matters (it looks at the system's processes).
pub fn honors(policy: HistoryClear, foreground: impl FnOnce() -> Foreground) -> bool {
    match policy {
        HistoryClear::Standard => true,
        HistoryClear::KeepAlways => false,
        HistoryClear::KeepForRunningPrograms => match foreground() {
            Foreground::Shell | Foreground::Unknown => true,
            Foreground::Program(age) => age < SHORT_COMMAND,
        },
    }
}

/// What an `ESC[2J` beside a dropped `ESC[3J` becomes: the cursor home, the screen erased
/// from there (`ESC[J`: nothing goes into the history). The programs sending the pair put the
/// cursor where they draw next anyway (`ESC[H`).
pub const QUIET_ERASE: &[u8] = b"\x1b[H\x1b[J";

/// The screen erase, as held.
const ERASE_SCREEN: &[u8] = b"\x1b[2J";

/// The longest escape sequence held back while it's being read (longer ones pass as they
/// are: never an `ESC[3J`).
const HELD_MAX: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum State {
    #[default]
    Ground,
    /// After `ESC`.
    Escape,
    /// Inside `ESC [ …`, before its final byte.
    Csi,
}

/// Finds `ESC [ 3 J` in a byte stream split anywhere, the bytes of a sequence still being
/// read held until it ends (at most [`HELD_MAX`]); everything else passes unchanged and in
/// order. An `ESC[2J` is held until what follows says whether it's beside a dropped
/// `ESC[3J` ([`QUIET_ERASE`] then) — let out as it is by anything else, or by [`flush`]
/// (nothing more to read for now).
///
/// [`flush`]: Self::flush
#[derive(Debug, Default)]
pub struct EraseFilter {
    state: State,
    held: Vec<u8>,
    /// An `ESC[2J` waits to see what follows.
    erase_held: bool,
    /// An `ESC[3J` was just dropped: an `ESC[2J` next (only the cursor moved between) is
    /// quiet too.
    after_drop: bool,
    /// Since [`take_seen`](Self::take_seen): an `ESC[3J` dropped / honored.
    dropped: bool,
    honored: bool,
    /// Since [`take_seen`](Self::take_seen): the history erased (an `ESC[3J` let through, a
    /// full reset `ESC c`).
    erased: bool,
}

/// What the filter saw since it was last asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Seen {
    /// A history erase dropped (the program redraws over its screen).
    pub dropped: bool,
    /// A history erase honored (the shell's `clear`).
    pub honored: bool,
    /// The history erased, whatever came after: an `ESC[3J` let through or a full reset
    /// (`ESC c`, `reset`).
    pub erased: bool,
}

impl EraseFilter {
    /// Nothing held: bytes without `ESC` may pass without being looked at.
    pub fn idle(&self) -> bool {
        self.state == State::Ground && !self.erase_held
    }

    /// What it saw since the last call (the latest of the two counts when both).
    pub fn take_seen(&mut self) -> Seen {
        Seen {
            dropped: std::mem::take(&mut self.dropped),
            honored: std::mem::take(&mut self.honored),
            erased: std::mem::take(&mut self.erased),
        }
    }

    /// An `ESC[2J` is held, waiting for what follows.
    pub fn erase_held(&self) -> bool {
        self.erase_held
    }

    /// Nothing more to read for now: a held `ESC[2J` let out as it is (a sequence still being
    /// read stays held).
    pub fn flush(&mut self, out: &mut Vec<u8>) {
        self.let_erase_out(out);
    }

    fn let_erase_out(&mut self, out: &mut Vec<u8>) {
        if self.erase_held {
            out.extend_from_slice(ERASE_SCREEN);
            self.erase_held = false;
        }
    }

    /// Passes `input` to `out`, asking `honor` at each `ESC[3J` whether it's kept.
    pub fn feed(&mut self, input: &[u8], out: &mut Vec<u8>, honor: &mut dyn FnMut() -> bool) {
        let mut i = 0;
        while i < input.len() {
            match self.state {
                State::Ground => {
                    // Up to the next ESC at once.
                    let rest = &input[i..];
                    match rest.iter().position(|&b| b == 0x1b) {
                        None => {
                            if !rest.is_empty() {
                                self.let_erase_out(out);
                                self.after_drop = false;
                            }
                            out.extend_from_slice(rest);
                            return;
                        }
                        Some(at) => {
                            if at > 0 {
                                self.let_erase_out(out);
                                self.after_drop = false;
                            }
                            out.extend_from_slice(&rest[..at]);
                            self.held.push(0x1b);
                            self.state = State::Escape;
                            i += at + 1;
                        }
                    }
                }
                State::Escape => {
                    let b = input[i];
                    if b == b'[' {
                        self.held.push(b);
                        self.state = State::Csi;
                        i += 1;
                    } else {
                        // Another sequence: passed on, its byte read again from the ground.
                        // A full reset (`ESC c`) erases the history too.
                        self.erased |= b == b'c';
                        self.let_erase_out(out);
                        self.after_drop = false;
                        self.release(out);
                    }
                }
                State::Csi => {
                    let b = input[i];
                    match b {
                        // Parameters, private markers, intermediates.
                        0x20..=0x3f => {
                            self.held.push(b);
                            i += 1;
                            if self.held.len() > HELD_MAX {
                                self.let_erase_out(out);
                                self.after_drop = false;
                                self.release(out);
                            }
                        }
                        // The final byte.
                        0x40..=0x7e => {
                            self.held.push(b);
                            i += 1;
                            self.sequence(out, honor);
                        }
                        // A control inside it (or a new ESC): passed on as it is, the
                        // emulator makes of it what it does.
                        _ => {
                            self.let_erase_out(out);
                            self.after_drop = false;
                            self.release(out);
                        }
                    }
                }
            }
        }
    }

    /// A whole CSI sequence read (in `held`).
    fn sequence(&mut self, out: &mut Vec<u8>, honor: &mut dyn FnMut() -> bool) {
        match erase_kind(&self.held) {
            Some(3) => {
                if honor() {
                    self.let_erase_out(out);
                    self.release(out);
                    (self.honored, self.dropped) = (true, false);
                    self.erased = true;
                } else {
                    (self.honored, self.dropped) = (false, true);
                    // The screen erase just before: quiet.
                    if std::mem::take(&mut self.erase_held) {
                        out.extend_from_slice(QUIET_ERASE);
                    }
                    self.held.clear();
                    self.state = State::Ground;
                    self.after_drop = true;
                }
            }
            Some(2) => {
                self.held.clear();
                self.state = State::Ground;
                if std::mem::take(&mut self.after_drop) {
                    self.let_erase_out(out);
                    out.extend_from_slice(QUIET_ERASE);
                } else {
                    // The one held before goes out as it is; this one waits.
                    self.let_erase_out(out);
                    self.erase_held = true;
                }
            }
            _ => {
                // Only the cursor moved: still beside the dropped one.
                let moves = matches!(self.held.last(), Some(b'H' | b'f'));
                self.let_erase_out(out);
                if !moves {
                    self.after_drop = false;
                }
                self.release(out);
            }
        }
    }

    fn release(&mut self, out: &mut Vec<u8>) {
        out.append(&mut self.held);
        self.state = State::Ground;
    }
}

/// `ESC [ Ps J` (no private marker, no intermediates): its Ps, the first parameter as the
/// emulator reads it (3: the history, 2: the screen).
fn erase_kind(seq: &[u8]) -> Option<u32> {
    let params = seq.strip_prefix(b"\x1b[").and_then(|s| s.strip_suffix(b"J"))?;
    if !params.iter().all(|b| b.is_ascii_digit() || *b == b';' || *b == b':') {
        return None;
    }
    let first = params.split(|b| *b == b';' || *b == b':').next().unwrap_or_default();
    Some(first.iter().fold(0u32, |n, d| n.saturating_mul(10).saturating_add(u32::from(d - b'0'))))
}

#[cfg(test)]
#[path = "../tests/unit/history.rs"]
mod tests;
