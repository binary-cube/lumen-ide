//! The app's probes, read by a profile recording (plan 91): who asked for a frame and why,
//! how long each part took on the interface's thread, the frames drawn and how long they
//! took. Whoever does the work reports (the app for its views, panels and overlays, a part
//! for itself); the recorder only takes what was gathered, so it knows no one.
//!
//! - **Free while nothing records:** each probe is one relaxed atomic load, then nothing.
//! - **Recording:** the counts are kept by name in a small table behind a lock that only
//!   adds numbers (never I/O, never a call out); [`take`] hands them over and starts anew.
//! - **Names are ours, static:** a view's provider, a tool window's id, an overlay's name —
//!   never a path, a file, a project or anything a user typed (`.claude/rules/privacy.md`).
//!
//! **To profile something new**, a part calls one of these where the work happens, with
//! its own name and what it counts — nothing else changes: the recording keeps it, the
//! recording's view lists it.
//!
//! - [`count`]: how many times something happened (`count("indexer", "files", 12)`);
//! - [`level`]: what something holds now (`level("lsp", "pending", 3)`): its highest and
//!   its last are kept;
//! - [`time`]: how long a piece of work took (a guard; on the interface's thread or not);
//! - [`trigger`] / [`asked`]: a frame asked for, and why.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// Why a frame was asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Cause {
    /// Its poll found something new (a result, a line, a change on disk).
    Poll,
    /// It's moving (an animation asks for the next frame).
    Animation,
    /// A time it gave came (a caret's blink, a fade, a refresh).
    Deadline,
    /// The user did something (a key, the mouse, the wheel).
    Input,
    /// A thread of its woke the app up.
    Wake,
    /// The window changed (its size, its scale, its screen, its focus).
    Window,
}

impl Cause {
    /// All of them, in their order.
    pub const ALL: [Cause; 6] =
        [Cause::Poll, Cause::Animation, Cause::Deadline, Cause::Input, Cause::Wake, Cause::Window];

    /// Its name in a recording.
    pub fn as_str(self) -> &'static str {
        match self {
            Cause::Poll => "poll",
            Cause::Animation => "animation",
            Cause::Deadline => "deadline",
            Cause::Input => "input",
            Cause::Wake => "wake",
            Cause::Window => "window",
        }
    }
}

/// What a part did on the interface's thread: how many times it was called, how long it
/// took in all, its longest call.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Busy {
    pub calls: u64,
    pub total: Duration,
    pub longest: Duration,
}

/// The frames drawn: how many, how long in all, the longest.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Frames {
    pub count: u64,
    pub total: Duration,
    pub longest: Duration,
}

/// What the probes gathered since the last [`take`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Gathered {
    /// Who asked for frames, why, how many times.
    pub triggers: Vec<(&'static str, Cause, u64)>,
    /// What each part took on the interface's thread, by its name and what it did
    /// (`paint`, `poll`…).
    pub busy: Vec<(&'static str, &'static str, Busy)>,
    pub frames: Frames,
    /// What parts counted ([`count`]): whose, what, how many.
    pub counts: Vec<(&'static str, &'static str, u64)>,
    /// What parts hold ([`level`]): whose, what, the highest and the last value.
    pub levels: Vec<(&'static str, &'static str, u64, u64)>,
    /// The timed work as it nested (a flame graph's): each way down from an outermost
    /// [`time`] (`[("app", "frame"), ("log-viewer", "paint")]`), its calls and its time with
    /// what it held.
    pub spans: Vec<Way>,
}

/// A way down of timed work, its calls and its time (see [`Gathered::spans`]).
pub type Way = (Vec<(&'static str, &'static str)>, u64, Duration);

/// How deep a way down is kept (deeper timed work counts at this depth).
pub const DEPTH: usize = 8;

/// A way down of timed work, kept without allocating: its steps, how many.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Path {
    len: u8,
    steps: [(&'static str, &'static str); DEPTH],
}

/// A timed piece of work open on a thread: whose, what, and the time its inner ones took
/// so far (its own is its time less that).
type Open = (&'static str, &'static str, Duration);

thread_local! {
    /// The timed work open on this thread, outermost first.
    static OPEN: RefCell<Vec<Open>> = const { RefCell::new(Vec::new()) };
    /// This thread is the interface's (its timed work is what "the interface's time" says).
    static INTERFACE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The calling thread is the interface's: its timed work counts as the interface's time
/// ([`Gathered::busy`]); every other thread's goes only into the ways down.
pub fn mark_interface_thread() {
    INTERFACE.with(|i| i.set(true));
}

#[derive(Default)]
struct Table {
    triggers: HashMap<(&'static str, Cause), u64>,
    busy: HashMap<(&'static str, &'static str), Busy>,
    frames: Frames,
    counts: HashMap<(&'static str, &'static str), u64>,
    levels: HashMap<(&'static str, &'static str), (u64, u64)>,
    spans: HashMap<Path, (u64, Duration)>,
}

static ON: AtomicBool = AtomicBool::new(false);
static TABLE: Mutex<Option<Table>> = Mutex::new(None);

fn table() -> std::sync::MutexGuard<'static, Option<Table>> {
    TABLE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Whether a recording is on (the probes count).
#[inline]
pub fn recording() -> bool {
    ON.load(Ordering::Relaxed)
}

/// A recording starts: the probes count from now, from nothing.
pub fn start() {
    *table() = Some(Table::default());
    ON.store(true, Ordering::Relaxed);
}

/// The recording ends: the probes stop counting, what they held let go.
pub fn stop() {
    ON.store(false, Ordering::Relaxed);
    *table() = None;
}

/// `owner` asked for a frame, for `cause`.
#[inline]
pub fn trigger(owner: &'static str, cause: Cause) {
    if !recording() {
        return;
    }
    if let Some(t) = table().as_mut() {
        *t.triggers.entry((owner, cause)).or_default() += 1;
    }
}

/// `owner` asked for a frame for `cause` when `asked` (so a probe sits on an `if` already
/// there): `asked` returned as it is.
#[inline]
pub fn asked(owner: &'static str, cause: Cause, asked: bool) -> bool {
    if asked {
        trigger(owner, cause);
    }
    asked
}

/// `owner` counted `n` more of `what` (files read, requests sent, lines parsed).
#[inline]
pub fn count(owner: &'static str, what: &'static str, n: u64) {
    if !recording() {
        return;
    }
    if let Some(t) = table().as_mut() {
        let c = t.counts.entry((owner, what)).or_default();
        *c = c.saturating_add(n);
    }
}

/// `owner` holds `value` of `what` now (a queue's length, entries cached, bytes kept).
#[inline]
pub fn level(owner: &'static str, what: &'static str, value: u64) {
    if !recording() {
        return;
    }
    if let Some(t) = table().as_mut() {
        let l = t.levels.entry((owner, what)).or_insert((value, value));
        l.0 = l.0.max(value);
        l.1 = value;
    }
}

/// A frame drawn in `took`.
#[inline]
pub fn frame(took: Duration) {
    if !recording() {
        return;
    }
    if let Some(t) = table().as_mut() {
        t.frames.count += 1;
        t.frames.total += took;
        t.frames.longest = t.frames.longest.max(took);
    }
}

/// Times what `owner` does (`what`: `paint`, `poll`…) until it's dropped; nothing (not even
/// the clock read) while nothing records.
/// Timers nest: one opened inside another is a step under it (the flame graph's ways down).
#[inline]
#[must_use]
pub fn time(owner: &'static str, what: &'static str) -> Timer {
    if !recording() {
        return Timer { started: None, depth: 0, owner, what, here: std::marker::PhantomData };
    }
    let depth = OPEN.with(|open| {
        let mut open = open.borrow_mut();
        open.push((owner, what, Duration::ZERO));
        open.len() - 1
    });
    Timer { started: Some(Instant::now()), depth, owner, what, here: std::marker::PhantomData }
}

/// What [`time`] gives: counts when dropped, on the thread it was made on (it can't be sent
/// to another: its place among the open ones is this thread's).
#[derive(Debug)]
pub struct Timer {
    started: Option<Instant>,
    /// Its place among this thread's open ones (0: the outermost).
    depth: usize,
    owner: &'static str,
    what: &'static str,
    here: std::marker::PhantomData<*const ()>,
}

impl Timer {
    /// Names what it timed once it's known (a phase named when it ends).
    pub fn set_what(&mut self, what: &'static str) {
        if self.started.is_none() {
            return;
        }
        self.what = what;
        OPEN.with(|open| {
            if let Some(me) = open.borrow_mut().get_mut(self.depth) {
                me.1 = what;
            }
        });
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        let Some(started) = self.started else { return };
        let took = started.elapsed();
        // Its way down (the open ones down to it; any left open inside it let go), its own
        // time (less its inner ones'), and its time told to the one holding it.
        let mut path = Path { len: 0, steps: [("", ""); DEPTH] };
        let mut inner = Duration::ZERO;
        let kept = OPEN.with(|open| {
            let mut open = open.borrow_mut();
            if let Some(me) = open.get(self.depth) {
                inner = me.2;
            }
            for (i, step) in open.iter().take(self.depth + 1).take(DEPTH).enumerate() {
                path.steps[i] = (step.0, step.1);
                path.len = (i + 1) as u8;
            }
            open.truncate(self.depth);
            if let Some(holder) = open.last_mut() {
                holder.2 += took;
            }
            // Deeper than kept: its time stays in its holder's (never counted twice).
            self.depth < DEPTH
        });
        let own = took.saturating_sub(inner);
        let interface = INTERFACE.with(std::cell::Cell::get);
        if let Some(t) = table().as_mut() {
            if kept {
                let span = t.spans.entry(path).or_default();
                span.0 += 1;
                span.1 += took;
            }
            // The interface's time: its own part only, on the interface's thread only.
            if interface {
                let busy = t.busy.entry((self.owner, self.what)).or_default();
                busy.calls += 1;
                busy.total += own;
                busy.longest = busy.longest.max(own);
            }
        }
    }
}

/// What was gathered since the last call, handed over (the counts start anew); nothing
/// while nothing records.
pub fn take() -> Gathered {
    let mut guard = table();
    let Some(t) = guard.as_mut() else { return Gathered::default() };
    // Drained, not replaced: the maps keep their room for the next interval.
    let mut triggers: Vec<_> = t.triggers.drain().map(|((o, c), n)| (o, c, n)).collect();
    triggers.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(b.0)).then(a.1.cmp(&b.1)));
    let mut busy: Vec<_> = t.busy.drain().map(|((o, w), b)| (o, w, b)).collect();
    busy.sort_by(|a, b| b.2.total.cmp(&a.2.total).then(a.0.cmp(b.0)));
    let mut counts: Vec<_> = t.counts.drain().map(|((o, w), n)| (o, w, n)).collect();
    counts.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(b.0)).then(a.1.cmp(b.1)));
    let mut levels: Vec<_> =
        t.levels.drain().map(|((o, w), (hi, last))| (o, w, hi, last)).collect();
    levels.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(b.0)).then(a.1.cmp(b.1)));
    let spans = t
        .spans
        .drain()
        .map(|(path, (calls, total))| (path.steps[..usize::from(path.len)].to_vec(), calls, total))
        .collect();
    let frames = std::mem::take(&mut t.frames);
    drop(guard);
    Gathered { triggers, busy, frames, counts, levels, spans }
}

#[cfg(test)]
#[path = "../tests/unit/probe.rs"]
mod tests;
