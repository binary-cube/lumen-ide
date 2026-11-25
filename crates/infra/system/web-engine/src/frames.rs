//! A page's frame as it arrives: its GPU texture in this process (the system's handle) and
//! the engine's buffer held until the app copied it — dropping the [`ArrivedFrame`] tells
//! the engine (`FrameDone`), so a frame can't be forgotten unacknowledged.
//!
//! On macOS the surface's port comes on the app's Mach service (`Arrivals`), apart from
//! the message saying the frame is there: the two are matched by page and number, the
//! frame handed on by whichever thread got the second of them.

use web_engine_protocol::{Changed, Command, PageId};

use crate::connection::Commands;

/// The texture's handle in this process.
#[derive(Debug)]
pub enum Native {
    /// macOS: a send right to the frame's `IOSurface` (`IOSurfaceLookupFromMachPort`).
    #[cfg(target_os = "macos")]
    Surface(mach_ports::Right),
    /// Windows: a D3D11 shared texture's NT handle, ours (closed when dropped).
    #[cfg(windows)]
    Handle(OwnedHandle),
    /// Its pixels in the page's file (BGRA, rows `stride` bytes apart): read before the
    /// frame is dropped (the engine writes the next one only after).
    Pixels { stride: u32 },
}

/// A frame of a page, to be copied on the GPU.
#[derive(Debug)]
pub struct ArrivedFrame {
    pub page: PageId,
    pub number: u64,
    pub width: u32,
    pub height: u32,
    pub native: Native,
    /// Of the page's popup (an open `<select>`'s list), not of the page.
    pub popup: bool,
    /// What changed since the frame before it of its kind (empty: all of it), each cut to
    /// the frame ([`Changed::within`]); all of it when the one before wasn't handed on.
    pub changed: Vec<Changed>,
    /// Held for its drop: the engine told the frame was copied.
    _ack: Ack,
}

impl ArrivedFrame {
    pub(crate) fn new(
        (page, number): (PageId, u64),
        (width, height): (u32, u32),
        native: Native,
        commands: Commands,
    ) -> Self {
        let popup = false;
        let ack = Ack { page, number, commands };
        Self { page, number, width, height, native, popup, changed: Vec::new(), _ack: ack }
    }

    /// What changed, as the frame's message told it, kept once cut to the frame.
    pub(crate) fn told_changed(&mut self, told: &[Changed]) {
        self.changed = Changed::within(told, (self.width, self.height));
    }
}

/// Tells the engine the frame was copied, when dropped.
#[derive(Debug)]
struct Ack {
    page: PageId,
    number: u64,
    commands: Commands,
}

impl Drop for Ack {
    fn drop(&mut self) {
        self.commands.send(Command::FrameDone { page: self.page, frame: self.number });
    }
}

/// Windows: a handle of ours, closed when dropped.
#[cfg(windows)]
pub use crate::peer_windows::OwnedHandle;

#[cfg(target_os = "macos")]
pub(crate) use mac::{Arrivals, Meta};

#[cfg(target_os = "macos")]
mod mac {
    use std::collections::{BTreeMap, HashMap};
    use std::sync::{Arc, Mutex, PoisonError};
    use std::time::{Duration, Instant};

    use web_engine_protocol::{Command, PageId};

    use super::{ArrivedFrame, Native};
    use crate::connection::{Commands, PageEvent, Sink};

    /// How long a frame's message waits for its surface's port (sent on the Mach service,
    /// apart: they come within a moment of each other) before it's given back.
    pub const PORT_WAIT: Duration = Duration::from_millis(100);

    /// Ports kept waiting for their message at most, per page (the oldest let go past it).
    const KEPT: usize = 4;

    /// A frame's message, as needed to make it an [`ArrivedFrame`].
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Meta {
        pub page: PageId,
        pub number: u64,
        pub size: (u32, u32),
        pub popup: bool,
        pub changed: Vec<web_engine_protocol::Changed>,
    }

    /// What wakes the thread receiving the ports (a frame parked: its deadline is new).
    pub type Wake = Arc<dyn Fn() + Send + Sync>;

    /// The frames' surfaces' ports, matched with their frames' messages by page and number.
    /// Whichever comes second completes the frame and hands it to its page's sink: the
    /// reader never waits for a port (a late one never holds the other pages' events), and a
    /// frame whose port doesn't come in [`PORT_WAIT`] is given back. Only pages open here
    /// are kept: a closed page's ports and frames are let go, nothing kept for it. A frame
    /// given back unseen makes the next one of its kind whole (what it changed was since
    /// a frame never shown).
    #[derive(Clone)]
    pub struct Arrivals {
        shared: Arc<Mutex<Ports>>,
        commands: Commands,
        wake: Wake,
    }

    impl std::fmt::Debug for Arrivals {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Arrivals").finish_non_exhaustive()
        }
    }

    /// A frame waiting for its port.
    struct Parked {
        meta: Meta,
        sink: Sink,
        deadline: Instant,
    }

    impl std::fmt::Debug for Parked {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Parked").field("meta", &self.meta).finish_non_exhaustive()
        }
    }

    /// What's waiting, by page (only the pages open here).
    #[derive(Debug, Default)]
    struct Ports {
        pages: HashMap<u64, Page>,
    }

    /// A page's ports waiting for their messages and frames waiting for their ports, by
    /// frame number.
    #[derive(Debug, Default)]
    struct Page {
        waiting: BTreeMap<u64, mach_ports::Right>,
        parked: BTreeMap<u64, Parked>,
        /// The newest frame whose message came (a port of it or older that isn't waited for
        /// comes late: let go at once, never kept).
        passed: Option<u64>,
        /// By kind (the page's, its popup's): a frame of it was given back unseen, the next
        /// one handed on is told whole.
        whole: [bool; 2],
    }

    /// The kind of a frame, as [`Page::whole`] is indexed.
    fn kind(popup: bool) -> usize {
        usize::from(popup)
    }

    impl Page {
        /// The page's frames parked before `meta`'s, of its kind (page or popup), given
        /// back: a newer one is shown, an older one never after it.
        fn supersede(&mut self, meta: &Meta, after: &mut After) {
            let Self { parked, whole, .. } = self;
            parked.retain(|number, parked| {
                let older = *number < meta.number && parked.meta.popup == meta.popup;
                if older {
                    after.given_back.push((parked.meta.page, parked.meta.number));
                    whole[kind(meta.popup)] = true;
                }
                !older
            });
        }

        /// The frames parked past `now` given back.
        fn expire(&mut self, now: Instant, after: &mut After) {
            let Self { parked, whole, .. } = self;
            parked.retain(|_, parked| {
                let due = parked.deadline <= now;
                if due {
                    after.given_back.push((parked.meta.page, parked.meta.number));
                    whole[kind(parked.meta.popup)] = true;
                }
                !due
            });
        }

        /// `meta` as it's handed on: whole if one of its kind was given back unseen.
        fn handed_on(&mut self, mut meta: Meta) -> Meta {
            if std::mem::take(&mut self.whole[kind(meta.popup)]) {
                meta.changed.clear();
            }
            meta
        }

        /// The page's ports older than frame `number` let go: their messages came already.
        fn let_go_before(&mut self, number: u64, after: &mut After) {
            while let Some(entry) = self.waiting.first_entry() {
                if *entry.key() >= number {
                    break;
                }
                after.let_go.push(entry.remove());
            }
        }
    }

    /// What's done once the lock is let go (never a call out, nor a right let go, under it).
    #[derive(Default)]
    struct After {
        deliver: Option<(Sink, ArrivedFrame)>,
        given_back: Vec<(PageId, u64)>,
        let_go: Vec<mach_ports::Right>,
        /// A frame was parked: the ports' thread waits for its deadline from now on.
        parked: bool,
    }

    impl Arrivals {
        /// Frames given back through `commands`; `wake` called when a frame is parked.
        pub fn new(commands: Commands, wake: Wake) -> Self {
            Self { shared: Arc::default(), commands, wake }
        }

        fn locked(&self) -> std::sync::MutexGuard<'_, Ports> {
            self.shared.lock().unwrap_or_else(PoisonError::into_inner)
        }

        /// What was decided under the lock, done.
        fn finish(&self, after: After) {
            for (page, frame) in after.given_back {
                self.commands.send(Command::FrameDone { page, frame });
            }
            drop(after.let_go);
            if after.parked {
                (self.wake)();
            }
            if let Some((sink, frame)) = after.deliver {
                sink(PageEvent::Frame(frame));
            }
        }

        fn complete(&self, meta: Meta, right: mach_ports::Right) -> ArrivedFrame {
            let native = Native::Surface(right);
            let mut frame = ArrivedFrame::new(
                (meta.page, meta.number),
                meta.size,
                native,
                self.commands.clone(),
            );
            frame.popup = meta.popup;
            frame.told_changed(&meta.changed);
            frame
        }

        /// A page opened: its ports are kept from now on.
        pub fn open(&self, page: u64) {
            self.locked().pages.entry(page).or_default();
        }

        /// A page closed: its ports and frames let go, any coming later too.
        pub fn forget(&self, page: u64) {
            let mut after = After::default();
            if let Some(gone) = self.locked().pages.remove(&page) {
                after.let_go.extend(gone.waiting.into_values());
                after
                    .given_back
                    .extend(gone.parked.into_values().map(|p| (p.meta.page, p.meta.number)));
            }
            self.finish(after);
        }

        /// Frame `frame` of `page`'s port came (on the ports' thread): its frame completed if
        /// it's parked, else kept until its message comes (unless it comes late).
        pub fn arrived(&self, page: u64, frame: u64, right: mach_ports::Right) {
            let mut after = After::default();
            {
                let mut ports = self.locked();
                match ports.pages.get_mut(&page) {
                    None => after.let_go.push(right),
                    Some(open) => match open.parked.remove(&frame) {
                        Some(parked) => {
                            open.supersede(&parked.meta, &mut after);
                            let meta = open.handed_on(parked.meta);
                            after.deliver = Some((parked.sink, self.complete(meta, right)));
                        }
                        None if open.passed.is_some_and(|passed| frame <= passed) => {
                            after.let_go.push(right);
                        }
                        None => {
                            if open.waiting.len() >= KEPT
                                && let Some((_, oldest)) = open.waiting.pop_first()
                            {
                                after.let_go.push(oldest);
                            }
                            open.waiting.insert(frame, right);
                        }
                    },
                }
            }
            self.finish(after);
        }

        /// A frame's message came (on the reader's thread), for `sink`'s page: handed on now
        /// if its port is here, else parked until it comes (at most [`PORT_WAIT`]). Only its
        /// own page's overdue frames are given back here (the others' by the ports' thread).
        pub fn frame(&self, meta: Meta, sink: Sink) {
            let now = Instant::now();
            let mut after = After::default();
            {
                let mut ports = self.locked();
                let number = meta.number;
                match ports.pages.get_mut(&meta.page.0) {
                    None => after.given_back.push((meta.page, number)),
                    Some(open) => {
                        open.expire(now, &mut after);
                        open.passed = Some(open.passed.map_or(number, |p| p.max(number)));
                        open.supersede(&meta, &mut after);
                        match open.waiting.remove(&number) {
                            Some(right) => {
                                let meta = open.handed_on(meta);
                                after.deliver = Some((sink, self.complete(meta, right)));
                            }
                            None => {
                                let parked = Parked { meta, sink, deadline: now + PORT_WAIT };
                                open.parked.insert(number, parked);
                                after.parked = true;
                            }
                        }
                        open.let_go_before(number, &mut after);
                    }
                }
            }
            self.finish(after);
        }

        /// The frames parked past their deadline given back; when the next one is due
        /// (`None`: none is parked).
        pub fn expire(&self, now: Instant) -> Option<Instant> {
            let mut after = After::default();
            let next = {
                let mut ports = self.locked();
                for page in ports.pages.values_mut() {
                    page.expire(now, &mut after);
                }
                ports.pages.values().flat_map(|p| p.parked.values()).map(|p| p.deadline).min()
            };
            self.finish(after);
            next
        }

        /// Tests: what's kept (ports waiting, frames parked, pages followed).
        #[cfg(test)]
        pub fn kept(&self) -> (usize, usize, usize) {
            let ports = self.locked();
            let waiting = ports.pages.values().map(|p| p.waiting.len()).sum();
            let parked = ports.pages.values().map(|p| p.parked.len()).sum();
            (waiting, parked, ports.pages.len())
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/frames.rs"]
mod tests;
