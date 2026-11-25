//! The app's one connection to the host (see the crate's doc).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use interprocess::local_socket::Stream;
use web_engine_protocol::Texture;
use web_engine_protocol::framing::{self, Reader};
use web_engine_protocol::{Adapter, Command, Event, Frame, Hello, PROTOCOL, PageId, Welcome};
use web_host_core::channel::Channel;

use crate::frames::{ArrivedFrame, Native};
use crate::launch::{Ended, Launch, Started};

/// What a page is told.
#[derive(Debug)]
pub enum PageEvent {
    Event(Event),
    /// A frame to copy on the GPU (where it arrives: the copy is made right there, the
    /// buffer goes back to the engine when the frame is dropped).
    Frame(ArrivedFrame),
}

/// Where a page's events go (called on the connection's reader thread, and on macOS a
/// frame whose surface came after its message on the ports' thread: a frame is copied
/// there, the rest only handed on, to be taken on the UI thread).
pub type Sink = Arc<dyn Fn(PageEvent) + Send + Sync>;

/// How long a host just started has to listen.
const START_WAIT: Duration = Duration::from_secs(15);
/// How often the channel is tried meanwhile.
const START_RETRY: Duration = Duration::from_millis(50);
/// How long the host has to answer the greeting.
const WELCOME_WAIT: Duration = Duration::from_secs(10);
/// The bytes read from the host at a time.
const READ_CHUNK: usize = 64 * 1024;
/// How long a host started that didn't greet is given to be seen ended (its end is waited
/// for on a thread of its own), to tell its failure from another's.
const END_GRACE: Duration = Duration::from_millis(500);
/// The commands waiting for the writer at most: past it the host has stopped reading (a
/// minute of input and frames for a few pages), and the connection is ended rather than
/// let grow.
const COMMANDS_KEPT: usize = 8192;
/// How long after a host started ended with success (another host had the channel: one
/// ending, maybe) and still nothing listens a host is started again.
const RESTART_AFTER: Duration = Duration::from_millis(200);
/// The hosts started again at most while waiting for one to listen, and again while one
/// ending answers.
const RESTARTS: u32 = 3;
/// How long after a host ending answered (it closes the connection: its apps let go) a host
/// is started again, for the ending one to let the channel go.
const ENDING_RETRY: Duration = Duration::from_millis(200);
/// Windows: how a thread blocked on the pipe is cancelled (it may not be in its call yet
/// when first tried, or enter one after): tried 10 ms apart until it ended, for 5 s at most.
#[cfg(windows)]
const CANCEL: crate::peer_windows::Cancel =
    crate::peer_windows::Cancel { pause: Duration::from_millis(10), limit: Duration::from_secs(5) };

type Sinks = Arc<Mutex<HashMap<PageId, Sink>>>;

/// A start of the engine that failed by the engine's own doing: its process ended before it
/// listened or greeted, or it doesn't speak what its package says. Only this counts against
/// its version ([`crate::install::service::StartOutcome`]); everything else (another GPU,
/// another user's program on the channel, an old host ending, a slow first start) doesn't.
#[derive(Debug)]
pub struct EngineFailed(&'static str);

impl std::fmt::Display for EngineFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for EngineFailed {}

/// Whether `error` is the engine's own failure ([`EngineFailed`]).
pub fn engine_failed(error: &std::io::Error) -> bool {
    error.get_ref().is_some_and(|inner| inner.is::<EngineFailed>())
}

fn engine_error(why: &'static str) -> std::io::Error {
    std::io::Error::other(EngineFailed(why))
}

/// How long the steps of a connection wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Waits {
    /// For a host just started to listen.
    pub start: Duration,
    /// For the host's greeting.
    pub welcome: Duration,
}

impl Default for Waits {
    fn default() -> Self {
        Self { start: START_WAIT, welcome: WELCOME_WAIT }
    }
}

/// The commands' way to the writer thread: a bounded queue. One that fills (the host
/// stopped reading) ends the connection ([`Ending`], which says it) rather than growing;
/// one sent once the connection ended is dropped.
#[derive(Clone)]
pub(crate) struct Commands {
    queue: SyncSender<Command>,
    full: Arc<dyn Fn() + Send + Sync>,
}

impl std::fmt::Debug for Commands {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Commands").finish_non_exhaustive()
    }
}

impl Commands {
    /// A queue of `kept` commands at most, `full` called when one doesn't fit; what's sent
    /// comes out of the receiver.
    pub(crate) fn new(kept: usize, full: Arc<dyn Fn() + Send + Sync>) -> (Self, Receiver<Command>) {
        let (queue, outgoing) = mpsc::sync_channel(kept);
        (Self { queue, full }, outgoing)
    }

    /// `command` queued for the host (never waits).
    pub(crate) fn send(&self, command: Command) {
        match self.queue.try_send(command) {
            Ok(()) | Err(TrySendError::Disconnected(_)) => {}
            Err(TrySendError::Full(_)) => (self.full)(),
        }
    }
}

/// Why a connection ended (said once, when it does).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cause {
    /// The host closed it, or it broke while read.
    Read,
    /// It broke while written.
    Write,
    /// The host stopped reading: the commands' queue filled.
    NotReading,
}

impl Cause {
    fn name(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::NotReading => "not-reading",
        }
    }
}

/// How a connection ends (its host gone, its stream broken, its host not reading): it's said
/// gone (and logged, once), the ports' thread woken to see it, and its stream stopped both
/// ways (Unix: shut; Windows: its threads' blocked calls cancelled), so the reader ends and
/// tells every page. The stream is held only by the reader and the writer: once they ended
/// it's closed, whoever still holds the ending (a queue's clone, a dead connection).
struct Ending {
    alive: Arc<AtomicBool>,
    /// Unix: the stream, shut when it ends (Windows cancels its threads' calls instead).
    #[cfg(unix)]
    stream: std::sync::Weak<Stream>,
    /// Wakes the thread receiving the frames' ports (macOS; nothing elsewhere).
    wake: Arc<dyn Fn() + Send + Sync>,
    /// Windows: the reader's and the writer's threads (their blocked calls cancelled).
    #[cfg(windows)]
    threads: Mutex<Vec<std::thread::JoinHandle<()>>>,
}

impl Ending {
    /// The connection ended for `cause` (once: later calls do nothing).
    fn end(&self, cause: Cause) {
        if !self.alive.swap(false, Ordering::AcqRel) {
            return;
        }
        match cause {
            Cause::NotReading => {
                tracing::warn!(cause = cause.name(), "the web engine's connection ended");
            }
            Cause::Read | Cause::Write => {
                tracing::info!(cause = cause.name(), "the web engine's connection ended");
            }
        }
        (self.wake)();
        #[cfg(unix)]
        if let Some(stream) = self.stream.upgrade() {
            let Stream::UdSocket(socket) = &*stream;
            let _ = socket.inner().shutdown(std::net::Shutdown::Both);
        }
        #[cfg(windows)]
        cancel_later(std::mem::take(
            &mut *self.threads.lock().unwrap_or_else(PoisonError::into_inner),
        ));
    }

    fn alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }
}

/// Windows: `threads`' blocked calls on the pipe cancelled until they ended (at most
/// [`CANCEL`]'s limit), on a thread of its own: cancelling waits between its tries, never on
/// the caller's thread (the UI's).
#[cfg(windows)]
fn cancel_later(threads: Vec<std::thread::JoinHandle<()>>) {
    if threads.is_empty() {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("web-engine-end".into())
        .spawn(move || crate::peer_windows::cancel_blocked(&threads, CANCEL));
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "the web engine's threads can't be stopped");
    }
}

/// The connection to the host.
pub struct Connection {
    commands: Commands,
    sinks: Sinks,
    alive: Arc<AtomicBool>,
    next_page: AtomicU64,
    welcome: Welcome,
    /// macOS: the surfaces' ports and their frames matched (a page closed: its own let go).
    #[cfg(target_os = "macos")]
    arrivals: crate::frames::Arrivals,
}

/// What the reader needs once greeted.
struct Reading {
    frames: Frames,
    sinks: Sinks,
    ending: Arc<Ending>,
}

impl std::fmt::Debug for Connection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Connection").field("engine", &self.welcome.engine).finish_non_exhaustive()
    }
}

/// The app's connection as far as it got: none, being made (by one caller; the others
/// wait), made.
enum Slot {
    Empty,
    Connecting,
    Ready(Arc<Connection>),
}

/// The app's connection, and the callers waiting while it's made. The lock only moves the
/// slot: connecting happens with it let go.
struct SharedSlot {
    slot: Mutex<Slot>,
    changed: Condvar,
}

static SHARED: SharedSlot = SharedSlot { slot: Mutex::new(Slot::Empty), changed: Condvar::new() };

/// The slot set back to empty if the caller making the connection never got to say how it
/// went (it panicked), and the waiting callers woken either way.
struct Making;

impl Drop for Making {
    fn drop(&mut self) {
        let mut slot = SHARED.slot.lock().unwrap_or_else(PoisonError::into_inner);
        if matches!(*slot, Slot::Connecting) {
            *slot = Slot::Empty;
        }
        drop(slot);
        SHARED.changed.notify_all();
    }
}

/// The app's connection, made on first use (`launch`: how to start the host when none
/// runs; `None`: none installed, nothing started). A connection whose host went is made
/// anew; a caller asking while another makes it waits for that one. Blocking (connecting, a
/// host started): never call it on the UI thread.
pub fn shared(
    adapter: Option<Adapter>,
    launch: impl FnOnce() -> Option<Launch>,
) -> std::io::Result<Arc<Connection>> {
    {
        let mut slot = SHARED.slot.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            match &*slot {
                Slot::Ready(connection) if connection.is_alive() => {
                    return Ok(Arc::clone(connection));
                }
                Slot::Connecting => {
                    slot = SHARED.changed.wait(slot).unwrap_or_else(PoisonError::into_inner);
                }
                Slot::Ready(_) | Slot::Empty => break,
            }
        }
        *slot = Slot::Connecting;
    }
    let making = Making;
    let channel = crate::launch::channel();
    let made = Connection::establish(&channel, adapter, launch).map(Arc::new);
    if let Ok(connection) = &made {
        *SHARED.slot.lock().unwrap_or_else(PoisonError::into_inner) =
            Slot::Ready(Arc::clone(connection));
    }
    drop(making);
    made
}

impl Connection {
    /// Connected to the host on `channel`, started first if none answers (`launch`): one
    /// that answers but doesn't greet (it was ending) is replaced by one started anew.
    /// `adapter`: the GPU the app draws on (its frames must be made there); a host drawing on
    /// another is refused (said as [`std::io::ErrorKind::Unsupported`]), never restarted
    /// under another app's pages. A host started that failed by itself is said as
    /// [`EngineFailed`] ([`engine_failed`]).
    pub fn establish(
        channel: &Channel,
        adapter: Option<Adapter>,
        launch: impl FnOnce() -> Option<Launch>,
    ) -> std::io::Result<Self> {
        Self::establish_within(channel, adapter, launch, Waits::default())
    }

    /// [`Self::establish`], waiting as long as `waits` says.
    pub(crate) fn establish_within(
        channel: &Channel,
        adapter: Option<Adapter>,
        launch: impl FnOnce() -> Option<Launch>,
        waits: Waits,
    ) -> std::io::Result<Self> {
        crate::launch::own_channel_dir(&channel.dir)?;
        if let Ok(stream) = channel.connect() {
            match Self::greet(stream, adapter, waits.welcome) {
                Ok(connection) => return Ok(connection),
                Err(e) if e.kind() == std::io::ErrorKind::Unsupported => return Err(e),
                Err(e) => tracing::info!(error = %e, "the running web engine didn't answer"),
            }
        }
        let Some(launch) = launch() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no web engine installed",
            ));
        };
        let deadline = Instant::now() + waits.start;
        let mut started = start(&launch)?;
        let mut restarts = 0;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let (stream, last) = wait_for(Waiting { channel, launch: &launch, started }, left)?;
            started = last;
            let error = match Self::greet(stream, adapter, waits.welcome) {
                Ok(connection) => return Ok(connection),
                Err(e) => e,
            };
            // A host ending (it closes every connection until it's gone) answered, and the
            // one started yielded to it: started again once that one let the channel go.
            let again = restarts < RESTARTS
                && Instant::now() < deadline
                && retried(&error)
                && started.wait_ended(END_GRACE) == Ended::Yielded;
            if !again {
                return Err(blamed(error, &started));
            }
            tracing::info!(restarts, error = %error, "the web engine answering was ending");
            restarts += 1;
            std::thread::sleep(ENDING_RETRY);
            started = start(&launch)?;
        }
    }

    /// The greeting, then the connection's threads. The greeting's answer is waited for
    /// `wait` at most: past it the reader is made to stop (Unix: its read times out, and the
    /// socket is shut; Windows: its blocked read is cancelled) and nothing of it stays.
    fn greet(stream: Stream, adapter: Option<Adapter>, wait: Duration) -> std::io::Result<Self> {
        // macOS: the frames' surfaces come on a Mach service of ours, named for this process.
        #[cfg(target_os = "macos")]
        let inbox = mach_ports::Inbox::check_in(&frames_service())?;
        #[cfg(target_os = "macos")]
        let mach_service = Some(inbox.name().to_owned());
        #[cfg(not(target_os = "macos"))]
        let mach_service = None;
        let hello = Hello { protocol: PROTOCOL, pid: std::process::id(), mach_service, adapter };
        let mut bytes = Vec::new();
        framing::write(&Command::Hello(hello), &mut bytes).map_err(std::io::Error::other)?;
        // The greeting's read ends by itself past its wait (a pipe on Windows takes none: its
        // read is cancelled instead).
        #[cfg(unix)]
        {
            use interprocess::local_socket::traits::Stream as _;
            stream.set_recv_timeout(Some(wait))?;
        }
        // One connection, read on one thread and written on the other (`&Stream` does both).
        let stream = Arc::new(stream);
        (&*stream).write_all(&bytes)?;
        let (told, welcomed) = mpsc::sync_channel::<Welcome>(1);
        let (go, started) = mpsc::sync_channel::<Reading>(1);
        let receive = Arc::clone(&stream);
        // Set when the greeting is given up on: the reader stops at its next look.
        let given_up = Arc::new(AtomicBool::new(false));
        let giving_up = Arc::clone(&given_up);
        let reader =
            std::thread::Builder::new().name("web-engine-read".into()).spawn(move || {
                let mut reader = Reader::default();
                let mut chunk = vec![0; READ_CHUNK];
                let greeting =
                    Greeting { reader: &mut reader, chunk: &mut chunk, given_up: &giving_up };
                let Some(welcome) = read_welcome(&*receive, greeting) else {
                    return;
                };
                // Greeted: from now on the host may stay silent as long as it likes.
                #[cfg(unix)]
                {
                    use interprocess::local_socket::traits::Stream as _;
                    if receive.set_recv_timeout(None).is_err() {
                        return;
                    }
                }
                if told.send(welcome).is_err() {
                    return;
                }
                let Ok(reading) = started.recv() else { return };
                read_events(&*receive, (reader, chunk), &reading);
                reading.ending.end(Cause::Read);
                // The stream let go now (the writer holds it only while writing): it closes.
                drop(receive);
                // Every page told its engine is gone.
                let pages: Vec<(PageId, Sink)> =
                    lock(&reading.sinks).iter().map(|(id, sink)| (*id, Arc::clone(sink))).collect();
                for (page, sink) in pages {
                    sink(PageEvent::Event(Event::Crashed { page }));
                }
            })?;
        let Ok(welcome) = welcomed.recv_timeout(wait) else {
            given_up.store(true, Ordering::Release);
            give_up(&stream, reader);
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "the web engine didn't greet",
            ));
        };
        if welcome.protocol != PROTOCOL {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "the web engine speaks another protocol",
            ));
        }
        #[cfg(target_os = "macos")]
        let host = trusted(&stream, &welcome)?;
        #[cfg(not(target_os = "macos"))]
        trusted(&stream, &welcome)?;
        if adapter.is_some() && welcome.adapter != adapter {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "the web engine draws on another GPU",
            ));
        }
        tracing::info!(engine = %welcome.engine, host = welcome.pid, "the web engine joined");
        let sinks: Sinks = Arc::default();
        let alive = Arc::new(AtomicBool::new(true));
        #[cfg(target_os = "macos")]
        let ports_waker = Arc::new(inbox.waker()?);
        #[cfg(target_os = "macos")]
        let wake: Arc<dyn Fn() + Send + Sync> = {
            let waker = Arc::clone(&ports_waker);
            Arc::new(move || waker.wake())
        };
        #[cfg(not(target_os = "macos"))]
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});
        let ending = Arc::new(Ending {
            alive: Arc::clone(&alive),
            #[cfg(unix)]
            stream: Arc::downgrade(&stream),
            wake: Arc::clone(&wake),
            #[cfg(windows)]
            threads: Mutex::new(Vec::new()),
        });
        let full = Arc::clone(&ending);
        let (commands, outgoing) =
            Commands::new(COMMANDS_KEPT, Arc::new(move || full.end(Cause::NotReading)));
        // Held only while a command is written: the reader holds the stream between.
        let send = Arc::downgrade(&stream);
        drop(stream);
        let (writing, broken) = (Arc::clone(&sinks), Arc::clone(&ending));
        let writer =
            std::thread::Builder::new().name("web-engine-write".into()).spawn(move || {
                let mut bytes = Vec::new();
                for command in outgoing {
                    if !broken.alive() {
                        break;
                    }
                    bytes.clear();
                    // One that can't be written (larger than a message may be): skipped, the page
                    // it opened or loaded told it's gone; the others go on.
                    if let Err(e) = framing::write(&command, &mut bytes) {
                        tracing::warn!(error = %e, "a command to the web engine skipped");
                        if let Some(page) = opened_by(&command)
                            && let Some(sink) = lock(&writing).get(&page).cloned()
                        {
                            sink(PageEvent::Event(Event::Crashed { page }));
                        }
                        continue;
                    }
                    // The reader ended: the stream is closed.
                    let Some(stream) = send.upgrade() else { break };
                    // The connection broke: it's gone for every page (each made again).
                    if (&*stream).write_all(&bytes).is_err() {
                        broken.end(Cause::Write);
                        break;
                    }
                    // An HTML page sent: the room it took let go.
                    framing::release_excess(&mut bytes);
                }
            })?;
        #[cfg(windows)]
        {
            ending.threads.lock().unwrap_or_else(PoisonError::into_inner).extend([reader, writer]);
            // Ended before its threads were told: they're cancelled now.
            if !ending.alive() {
                cancel_later(std::mem::take(
                    &mut *ending.threads.lock().unwrap_or_else(PoisonError::into_inner),
                ));
            }
        }
        #[cfg(not(windows))]
        let _ = (reader, writer);
        #[cfg(target_os = "macos")]
        let arrivals = crate::frames::Arrivals::new(commands.clone(), wake);
        #[cfg(target_os = "macos")]
        receive_ports(PortsThread {
            inbox,
            waker: ports_waker,
            host,
            alive: Arc::clone(&alive),
            arrivals: arrivals.clone(),
        })?;
        let frames = Frames {
            commands: commands.clone(),
            #[cfg(target_os = "macos")]
            arrivals: arrivals.clone(),
        };
        let reading = Reading { frames, sinks: Arc::clone(&sinks), ending };
        go.send(reading).map_err(|_| std::io::Error::other("the web engine's reader ended"))?;
        Ok(Self {
            commands,
            sinks,
            alive,
            next_page: AtomicU64::new(1),
            welcome,
            #[cfg(target_os = "macos")]
            arrivals,
        })
    }

    /// The host still answers.
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }

    /// The host's process (its programs' use is measured from it).
    pub fn host(&self) -> u32 {
        self.welcome.pid
    }

    /// The engine's version.
    pub fn engine(&self) -> &str {
        &self.welcome.engine
    }

    /// A new page's id, its events to go to `sink`.
    pub(crate) fn register(&self, sink: Sink) -> PageId {
        let page = PageId(self.next_page.fetch_add(1, Ordering::Relaxed));
        #[cfg(target_os = "macos")]
        self.arrivals.open(page.0);
        lock(&self.sinks).insert(page, sink);
        page
    }

    pub(crate) fn unregister(&self, page: PageId) {
        lock(&self.sinks).remove(&page);
        #[cfg(target_os = "macos")]
        self.arrivals.forget(page.0);
    }

    /// `command` sent (the host gone: dropped).
    pub(crate) fn send(&self, command: Command) {
        self.commands.send(command);
    }
}

/// A greeting given up on: its reader made to stop now rather than when (if) the host
/// answers, so nothing of the connection stays (Unix: the socket shut; Windows: its blocked
/// read cancelled until it ended, on a thread of its own).
fn give_up(stream: &Stream, reader: std::thread::JoinHandle<()>) {
    #[cfg(unix)]
    {
        drop(reader);
        let Stream::UdSocket(socket) = stream;
        let _ = socket.inner().shutdown(std::net::Shutdown::Both);
    }
    #[cfg(windows)]
    {
        let _ = stream;
        cancel_later(vec![reader]);
    }
}

/// `error` (a host started didn't greet) said as the engine's failure when the host itself
/// ended with an error meanwhile, or speaks another protocol than its package says; as it
/// was otherwise.
fn blamed(error: std::io::Error, started: &Started) -> std::io::Error {
    use std::io::ErrorKind;
    match error.kind() {
        ErrorKind::Unsupported | ErrorKind::PermissionDenied => return error,
        ErrorKind::InvalidData => return engine_error("the web engine speaks another protocol"),
        _ => {}
    }
    match started.wait_ended(END_GRACE) {
        Ended::Failed => engine_error("the web engine ended before it greeted"),
        Ended::Yielded | Ended::Running => error,
    }
}

/// Whether a greeting failed as `error` may be a host ending (it closed the connection):
/// never a host refused for what it is (another GPU, another user, another protocol).
fn retried(error: &std::io::Error) -> bool {
    use std::io::ErrorKind;
    !matches!(
        error.kind(),
        ErrorKind::Unsupported | ErrorKind::PermissionDenied | ErrorKind::InvalidData
    )
}

/// What the reader needs to make a frame's message an [`ArrivedFrame`].
struct Frames {
    commands: Commands,
    #[cfg(target_os = "macos")]
    arrivals: crate::frames::Arrivals,
}

impl Frames {
    /// A frame not shown (its page gone, its texture not here): its buffer given back, and
    /// on Windows the texture's handle duplicated into this process closed.
    fn give_back(&self, frame: &Frame) {
        #[cfg(windows)]
        if let Texture::Windows { handle } = frame.texture {
            drop(crate::frames::OwnedHandle(handle));
        }
        self.commands.send(Command::FrameDone { page: frame.page, frame: frame.frame });
    }

    /// The frame handed to `sink` with its texture's handle here; macOS: once its surface's
    /// port came too (parked until then, never waited for here).
    fn arrived(&self, frame: Frame, sink: Sink) {
        let native = match frame.texture {
            Texture::Pixels { stride } => Native::Pixels { stride },
            #[cfg(target_os = "macos")]
            Texture::MachPort => {
                let meta = crate::frames::Meta {
                    page: frame.page,
                    number: frame.frame,
                    size: (frame.width, frame.height),
                    popup: frame.popup,
                    changed: frame.changed,
                };
                self.arrivals.frame(meta, sink);
                return;
            }
            #[cfg(windows)]
            Texture::Windows { handle } => Native::Handle(crate::frames::OwnedHandle(handle)),
            #[allow(unreachable_patterns)]
            _ => {
                self.give_back(&frame);
                return;
            }
        };
        let mut arrived = ArrivedFrame::new(
            (frame.page, frame.frame),
            (frame.width, frame.height),
            native,
            self.commands.clone(),
        );
        arrived.popup = frame.popup;
        arrived.told_changed(&frame.changed);
        sink(PageEvent::Frame(arrived));
    }
}

/// The host as the system tells it: macOS, its audit token's process and its version (the
/// frames' ports are checked against it).
#[cfg(target_os = "macos")]
type HostId = mach_ports::Peer;
#[cfg(not(target_os = "macos"))]
type HostId = ();

/// The other end's process as the system tells it, and its identity.
#[cfg(target_os = "macos")]
fn peer_of(
    stream: &Stream,
    _creds: &interprocess::local_socket::PeerCreds,
) -> std::io::Result<(Option<u32>, HostId)> {
    let peer = crate::peer_mac::peer(stream)?;
    Ok((Some(peer.pid), peer))
}

/// The other end's process as the system tells it, and its identity.
#[cfg(not(target_os = "macos"))]
fn peer_of(
    _stream: &Stream,
    creds: &interprocess::local_socket::PeerCreds,
) -> std::io::Result<(Option<u32>, HostId)> {
    // The peer's process id as the system gives it (`u32` on Windows, `pid_t` elsewhere).
    #[allow(clippy::useless_conversion)]
    let pid = creds.pid().and_then(|pid| u32::try_from(pid).ok());
    Ok((pid, ()))
}

/// The other end is the user's own host: the process it says it is (as the system tells
/// the peer's process: one it can't tell is refused) and run as this process's user (its
/// channel's folder is checked to be theirs alone before: [`crate::launch::own_channel_dir`]).
/// Anyone else who answered there (another user's program squatting the channel) is refused.
fn trusted(stream: &Stream, welcome: &Welcome) -> std::io::Result<HostId> {
    use interprocess::local_socket::traits::StreamCommon as _;
    let refused =
        |why: &str| std::io::Error::new(std::io::ErrorKind::PermissionDenied, why.to_owned());
    let creds = stream.peer_creds()?;
    let (pid, host) = peer_of(stream, &creds)?;
    if pid != Some(welcome.pid) {
        return Err(refused("the web engine isn't the process it says"));
    }
    #[cfg(unix)]
    if creds.euid() != Some(crate::launch::user()) {
        return Err(refused("the web engine runs as another user"));
    }
    #[cfg(windows)]
    {
        // The server's process told by the system, run as this user.
        if !pid.is_some_and(crate::peer_windows::same_user) {
            return Err(refused("the web engine runs as another user"));
        }
    }
    Ok(host)
}

/// The page a command opens or loads anew (what's lost with it if it can't be sent).
fn opened_by(command: &Command) -> Option<PageId> {
    match command {
        Command::Open(open) => Some(open.page),
        Command::LoadHtml { page, .. } => Some(*page),
        _ => None,
    }
}

/// What the greeting's answer is read with.
struct Greeting<'a> {
    reader: &'a mut Reader,
    chunk: &'a mut [u8],
    /// Set once it's given up on: nothing read after is taken.
    given_up: &'a AtomicBool,
}

/// The greeting's answer read (`None`: the connection ended, broke or timed out first, or it
/// was given up on meanwhile — looked at after every read, so a read the cancel missed ends
/// it at the latest when it returns).
fn read_welcome(mut receive: impl Read, greeting: Greeting<'_>) -> Option<Welcome> {
    let Greeting { reader, chunk, given_up } = greeting;
    loop {
        if given_up.load(Ordering::Acquire) {
            return None;
        }
        match reader.message::<Event>() {
            Ok(Some(Event::Welcome(welcome))) => return Some(welcome),
            Ok(Some(_)) => continue,
            Ok(None) => {}
            Err(_) => return None,
        }
        match receive.read(chunk) {
            Ok(0) | Err(_) => return None,
            Ok(n) => reader.push(&chunk[..n]),
        }
    }
}

/// The host's events to their pages until the connection ends (it broke, or it was ended:
/// looked at after every read, so a read a cancel missed ends it at the latest when it
/// returns). A page's sink is looked up before anything is made of its event: a gone page's
/// frame is given back at once.
fn read_events(
    mut receive: impl Read,
    (mut reader, mut chunk): (Reader, Vec<u8>),
    reading: &Reading,
) {
    let Reading { frames, sinks, ending } = reading;
    loop {
        if !ending.alive() {
            return;
        }
        loop {
            match reader.message::<Event>() {
                Ok(Some(event)) => {
                    let Some(page) = page_of(&event) else { continue };
                    let Some(sink) = lock(sinks).get(&page).cloned() else {
                        if let Event::Frame(frame) = &event {
                            frames.give_back(frame);
                        }
                        continue;
                    };
                    match event {
                        Event::Frame(frame) => frames.arrived(frame, sink),
                        other => sink(PageEvent::Event(other)),
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    tracing::warn!(error = %e, "the web engine's message can't be read");
                    return;
                }
            }
        }
        match receive.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => reader.push(&chunk[..n]),
        }
    }
}

/// macOS: this process's frames service's name (its process and a UUID: nobody else
/// guesses it, and its messages are checked to come from the host anyway).
#[cfg(target_os = "macos")]
fn frames_service() -> String {
    let id = kernel::id::uuid_text(kernel::id::new_uuid());
    format!("editor-web-frames.{}.{id}", std::process::id())
}

/// macOS: what the thread receiving the frames' ports works with.
#[cfg(target_os = "macos")]
struct PortsThread {
    /// The app's Mach service the host sends the ports to.
    inbox: mach_ports::Inbox,
    /// Ends its wait (a frame parked, the connection ended): held so it stays valid.
    waker: Arc<mach_ports::Waker>,
    /// The host's process and its id's version (only its ports are taken).
    host: mach_ports::Peer,
    /// The connection still lives.
    alive: Arc<AtomicBool>,
    /// Where the ports go to meet their frames.
    arrivals: crate::frames::Arrivals,
}

/// macOS: the surfaces' ports the host sends, received on a thread of their own while the
/// connection lives; the frames parked for them given back once overdue. It waits for the
/// next port or the next deadline only (woken when a frame is parked or the connection
/// ends): nothing is looked at while nothing happens.
#[cfg(target_os = "macos")]
fn receive_ports(ports: PortsThread) -> std::io::Result<()> {
    let PortsThread { inbox, waker, host, alive, arrivals } = ports;
    std::thread::Builder::new().name("web-engine-ports".into()).spawn(move || {
        let _waker = waker;
        while alive.load(Ordering::Acquire) {
            let now = Instant::now();
            let wait = arrivals.expire(now).map(|at| at.saturating_duration_since(now));
            match inbox.receive(host, wait) {
                Ok(Some(delivery)) => {
                    arrivals.arrived(delivery.first, delivery.second, delivery.right)
                }
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!(error = %e, "the frames' ports can't be received");
                    break;
                }
            }
        }
    })?;
    Ok(())
}

/// The page an event is for (`None`: the greeting).
fn page_of(event: &Event) -> Option<PageId> {
    match event {
        Event::Welcome(_) => None,
        Event::Frame(frame) => Some(frame.page),
        Event::Cursor { page, .. }
        | Event::Message { page, .. }
        | Event::OpenLink { page, .. }
        | Event::Loaded { page }
        | Event::Popup { page, .. }
        | Event::ContextMenu { page, .. }
        | Event::Caret { page, .. }
        | Event::Access { page, .. }
        | Event::Crashed { page } => Some(*page),
    }
}

/// The host started by `launch` (its program that can't even be started: the engine's
/// install is broken).
fn start(launch: &Launch) -> std::io::Result<Started> {
    let started = launch.start().map_err(|e| {
        tracing::warn!(error = %e, "the web engine can't be started");
        engine_error("the web engine's program can't be started")
    })?;
    tracing::info!("the web engine started");
    Ok(started)
}

/// A host just started, waited for.
struct Waiting<'a> {
    channel: &'a Channel,
    /// How it was started (to start it again).
    launch: &'a Launch,
    started: Started,
}

/// A host just started, connected once it listens (at most `limit`), with the host started
/// last. Its process ending with an error first is the engine's failure. One ending with
/// success had another host on the channel: when that one ended meanwhile (a page opened
/// while the host shut down) nothing listens, so a host is started again [`RESTART_AFTER`]
/// later ([`RESTARTS`] times at most). Not listening in time isn't the engine's failure.
fn wait_for(waiting: Waiting<'_>, limit: Duration) -> std::io::Result<(Stream, Started)> {
    let Waiting { channel, launch, mut started } = waiting;
    let since = Instant::now();
    let mut restarts = 0;
    let mut yielded_at: Option<Instant> = None;
    loop {
        let error = match channel.connect() {
            Ok(stream) => return Ok((stream, started)),
            Err(e) => e,
        };
        match started.ended() {
            Ended::Failed => return Err(engine_error("the web engine ended before it listened")),
            _ if since.elapsed() >= limit => {
                return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, error));
            }
            Ended::Yielded if restarts < RESTARTS => {
                let at = *yielded_at.get_or_insert_with(Instant::now);
                if at.elapsed() >= RESTART_AFTER {
                    tracing::info!(restarts, "nothing listens after the web engine yielded");
                    restarts += 1;
                    yielded_at = None;
                    started = start(launch)?;
                    continue;
                }
            }
            Ended::Yielded | Ended::Running => {}
        }
        std::thread::sleep(START_RETRY);
    }
}

fn lock(sinks: &Sinks) -> std::sync::MutexGuard<'_, HashMap<PageId, Sink>> {
    sinks.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
#[path = "../tests/unit/connection.rs"]
mod tests;
