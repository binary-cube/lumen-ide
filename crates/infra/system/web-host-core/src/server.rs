//! The host's clients and pages, and when it ends (see the crate's doc).

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use interprocess::local_socket::Stream;
use interprocess::local_socket::traits::ListenerExt as _;
use web_engine_protocol::framing::{self, Reader};
use web_engine_protocol::{Command, Event, Hello, PROTOCOL, Welcome};

use crate::acks::Acks;
use crate::channel::Claim;
use crate::engine::{Engine, Outbox, Outgoing, PageKey, Routed};
use crate::peer;

/// A connected app (one per app process: its windows and projects share it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientId(pub u64);

/// How long the host lives on with nothing to do.
#[derive(Debug, Clone, Copy)]
pub struct Lifetime {
    /// With no page this long, it ends (a page opened meanwhile finds it running).
    pub idle: Duration,
}

impl Default for Lifetime {
    fn default() -> Self {
        Self { idle: Duration::from_secs(5 * 60) }
    }
}

/// Whether the host goes on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Running,
    /// Nothing to do for [`Lifetime::idle`]: the host ends (its engine shut down first).
    Ended,
}

/// What the clients' threads bring to the main thread.
enum Incoming {
    Command(ClientId, Command),
    Gone(ClientId),
    /// Something else wants the main thread (the engine's own work is due).
    Wake,
}

/// What the clients' readers may queue for the main thread at most: past it a reader waits
/// (and its app's writes wait on the connection), never the host's memory growing for an app
/// sending faster than the engine takes it.
const INCOMING_QUEUE: usize = 256;

/// The bytes read from a client at a time.
const READ_CHUNK: usize = 64 * 1024;

/// How long the accepting waits after an error before it tries again.
const ACCEPT_BACKOFF: Duration = Duration::from_millis(200);

/// The events queued for a client's writer at most: past them its app isn't reading (its
/// connection's buffer is full too), and it's let go rather than held in the host's memory.
/// A page's frames are a small event each: this is many seconds of them.
const CLIENT_QUEUE: usize = 1024;

/// The bytes a client's writer keeps for its next message: one larger (a page's message of
/// many MB) is let go once written, not held for as long as the client lives.
const WRITE_KEPT: usize = 1 << 20;

/// A client as the server knows it: its writer's queue and its connection.
struct Client {
    events: SyncSender<Outgoing>,
    link: Arc<Link>,
}

/// A client's connection, shared by its threads and cut from the host's side when it's let
/// go (another protocol, a queue full, the host ending): its reader ends (and tells the
/// main thread the client is gone), its app sees the end at once.
struct Link {
    stream: Stream,
    cut: AtomicBool,
}

impl Link {
    fn cut(&self) {
        self.cut.store(true, Ordering::Release);
        peer::cut(&self.stream);
    }

    fn is_cut(&self) -> bool {
        self.cut.load(Ordering::Acquire)
    }
}

/// Each client's writer (its thread sends what's given, in order).
type Writers = Arc<Mutex<HashMap<ClientId, Client>>>;

/// A client let go from the host's side: no more events for it, its connection cut (its
/// reader then ends and tells the main thread it's gone, which closes its pages).
fn let_go(writers: &Writers, client: ClientId) {
    let gone = lock(writers).remove(&client);
    if let Some(gone) = gone {
        gone.link.cut();
    }
}

/// Every client let go (the host ending).
fn let_all_go(writers: &Writers) {
    let gone: Vec<Client> = lock(writers).drain().map(|(_, client)| client).collect();
    for client in gone {
        client.link.cut();
    }
}

/// What a client's threads share with the server.
#[derive(Clone)]
struct Serving {
    writers: Writers,
    to_main: SyncSender<Incoming>,
    /// Set once the host ends: a client connecting now is let go at once.
    ending: Arc<AtomicBool>,
    acks: Acks,
}

/// The host's clients and pages.
pub struct Server {
    incoming: Receiver<Incoming>,
    /// For [`Self::waker`].
    wake: SyncSender<Incoming>,
    writers: Writers,
    outbox: Outbox,
    /// The clients that said hello in this protocol (only theirs are listened to).
    welcomed: HashSet<ClientId>,
    pages: HashSet<PageKey>,
    /// Since when no page is open.
    empty_since: Instant,
    lifetime: Lifetime,
    /// Set once the host ends ([`State::Ended`]): every client let go, one connecting
    /// afterwards closed at once (its app sees the end and starts a host anew, which takes
    /// the channel once this one has let it go).
    ending: Arc<AtomicBool>,
    /// Set as the server is let go: the accepting thread ends at its next connection.
    dropped: Arc<AtomicBool>,
    acks: Acks,
    /// The channel's lock, held as long as the server (and so until the engine has ended:
    /// the server is let go after it): another host never takes the channel, nor the
    /// engine's profile, meanwhile — while the engine shuts down, the channel still answers
    /// (closing each connection), never leaving an app to wait for nothing. The listener
    /// lives on the accepting thread.
    _lock: File,
}

impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server").field("pages", &self.pages.len()).finish_non_exhaustive()
    }
}

impl Server {
    /// The host's server on its claimed channel: clients accepted on a thread of their own.
    pub fn start(claim: Claim, lifetime: Lifetime) -> std::io::Result<Self> {
        // Taken apart: the lock stays with the server, the listener goes to its thread (a
        // closure capturing only `claim.listener` would let the lock go here).
        let Claim { listener, lock: claim_lock } = claim;
        let (to_main, incoming) = mpsc::sync_channel(INCOMING_QUEUE);
        let wake = to_main.clone();
        let writers: Writers = Arc::default();
        let (events, routed) = mpsc::channel::<Routed>();
        let outbox = Outbox { sender: events };
        let queues = Arc::clone(&writers);
        std::thread::Builder::new()
            .name("web-host-route".into())
            .spawn(move || route(&routed, &queues))?;
        let ending = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(AtomicBool::new(false));
        let accepting = Arc::clone(&dropped);
        // A copy coming for a page that was behind wakes the main loop (it paints the page
        // again at once, not at its next wake).
        let woken = to_main.clone();
        let acks = Acks::waking(move || {
            let _ = woken.try_send(Incoming::Wake);
        });
        let serving = Serving {
            writers: Arc::clone(&writers),
            to_main,
            ending: Arc::clone(&ending),
            acks: acks.clone(),
        };
        std::thread::Builder::new().name("web-host-accept".into()).spawn(move || {
            let mut next = 0u64;
            for stream in listener.incoming() {
                if accepting.load(Ordering::Acquire) {
                    break;
                }
                match stream {
                    // Ending: closed at once, not served (its app sees the end, and starts a
                    // host anew once this one has let the channel go).
                    Ok(stream) if serving.ending.load(Ordering::Acquire) => drop(stream),
                    Ok(stream) => {
                        next += 1;
                        serve(ClientId(next), stream, &serving);
                    }
                    Err(e) => {
                        // Out of descriptors, say: never a hot loop.
                        tracing::warn!(error = %e, "a connection wasn't accepted");
                        std::thread::sleep(ACCEPT_BACKOFF);
                    }
                }
            }
        })?;
        Ok(Self {
            incoming,
            wake,
            writers,
            outbox,
            welcomed: HashSet::new(),
            pages: HashSet::new(),
            empty_since: Instant::now(),
            lifetime,
            ending,
            dropped,
            acks,
            _lock: claim_lock,
        })
    }

    /// What wakes [`Self::poll`] from any thread (the engine asking for its own work: its
    /// loop sleeps until a client or it needs the main thread, never polling).
    pub fn waker(&self) -> impl Fn() + Send + Sync + 'static {
        let wake = self.wake.clone();
        // Full: the main loop has work queued, and wakes for it anyway.
        move || {
            let _ = wake.try_send(Incoming::Wake);
        }
    }

    /// Where the apps' copies of the frames are told (the engine's paint waits on it).
    pub fn acks(&self) -> &Acks {
        &self.acks
    }

    /// Where the engine's events go.
    pub fn outbox(&self) -> &Outbox {
        &self.outbox
    }

    /// The pages open now.
    pub fn pages(&self) -> usize {
        self.pages.len()
    }

    /// What the clients asked, handed to `engine` (waiting at most `wait` for the first);
    /// whether the host goes on.
    pub fn poll(&mut self, engine: &mut dyn Engine, wait: Duration) -> State {
        let mut next = match self.incoming.recv_timeout(wait) {
            Ok(first) => Some(first),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return State::Ended,
        };
        while let Some(incoming) = next {
            match incoming {
                Incoming::Command(client, command) => self.handle(engine, client, command),
                Incoming::Gone(client) => self.gone(engine, client),
                Incoming::Wake => {}
            }
            next = self.incoming.try_recv().ok();
        }
        if !self.pages.is_empty() {
            self.empty_since = Instant::now();
            return State::Running;
        }
        if self.empty_since.elapsed() < self.lifetime.idle {
            return State::Running;
        }
        // Ended: no connection is served from now on, and the ones open are let go now, not
        // once the engine has shut down (one waiting meanwhile would wait for nothing).
        self.ending.store(true, Ordering::Release);
        let_all_go(&self.writers);
        State::Ended
    }

    fn handle(&mut self, engine: &mut dyn Engine, client: ClientId, command: Command) {
        if let Command::Hello(hello) = &command {
            if hello.protocol != PROTOCOL {
                tracing::info!(
                    theirs = hello.protocol,
                    ours = PROTOCOL,
                    "an app of another protocol: let go"
                );
                // Its connection cut: its app sees the end at once (not after its wait); what
                // it had (a greeting before, pages) goes now, its reader's word ignored.
                let_go(&self.writers, client);
                self.gone(engine, client);
                return;
            }
            self.welcomed.insert(client);
            engine.joined(client, hello);
            let welcome = Welcome {
                protocol: PROTOCOL,
                pid: std::process::id(),
                engine: engine.version(),
                adapter: engine.adapter(),
            };
            self.outbox.send(client, Event::Welcome(welcome));
            return;
        }
        if !self.welcomed.contains(&client) {
            return;
        }
        match command {
            Command::Hello(_) => {}
            Command::Open(open) => {
                let key = PageKey { client, page: open.page };
                if self.pages.insert(key) {
                    self.acks.open(key);
                    engine.open(key, open, &self.outbox);
                }
            }
            Command::Close { page } => {
                let key = PageKey { client, page };
                if self.pages.remove(&key) {
                    self.acks.forget(key);
                    engine.close(key);
                }
            }
            other => {
                if let Some(page) = page_of(&other) {
                    let key = PageKey { client, page };
                    if self.pages.contains(&key) {
                        engine.command(key, other);
                    }
                }
            }
        }
    }

    /// A client gone (its app closed or crashed): its pages closed, the others' kept.
    /// Told once or twice (let go for another protocol, then its reader ending): only the
    /// first does anything.
    fn gone(&mut self, engine: &mut dyn Engine, client: ClientId) {
        let joined = self.welcomed.remove(&client);
        lock(&self.writers).remove(&client);
        self.outbox.forget(client);
        let theirs: Vec<PageKey> =
            self.pages.iter().filter(|k| k.client == client).copied().collect();
        for key in theirs {
            self.pages.remove(&key);
            self.acks.forget(key);
            engine.close(key);
        }
        if joined {
            engine.left(client);
        }
    }
}

/// The page a command is for (`None`: `Hello`).
fn page_of(command: &Command) -> Option<web_engine_protocol::PageId> {
    match command {
        Command::Hello(_) => None,
        Command::Open(open) => Some(open.page),
        Command::Resize { page, .. }
        | Command::Hidden { page, .. }
        | Command::Close { page }
        | Command::LoadHtml { page, .. }
        | Command::Post { page, .. }
        | Command::BeginFrame { page }
        | Command::FrameDone { page, .. }
        | Command::Mouse { page, .. }
        | Command::Wheel { page, .. }
        | Command::Key { page, .. }
        | Command::Focus { page, .. }
        | Command::Edit { page, .. }
        | Command::Ime { page, .. }
        | Command::Drag { page, .. }
        | Command::Access { page, .. }
        | Command::Repaint { page } => Some(*page),
    }
}

/// The engine's events to each client's writer, in order. Each client's queue is kept here
/// (the writers' map locked only to find a new one), let go when the server forgets the
/// client or its writer ended; a client whose queue is full isn't reading: it's let go (its
/// connection cut: its reader tells the main thread, this thread never waits on it).
fn route(routed: &Receiver<Routed>, writers: &Writers) {
    let mut queues: HashMap<ClientId, SyncSender<Outgoing>> = HashMap::new();
    for routed in routed {
        // An event not queued (its client gone or let go) is dropped here: what it handed
        // over is taken back as it drops.
        let (client, event) = match routed {
            Routed::Event(client, event) => (client, event),
            Routed::Forget(client) => {
                queues.remove(&client);
                continue;
            }
        };
        let queue = match queues.entry(client) {
            Entry::Occupied(known) => known.into_mut(),
            Entry::Vacant(new) => {
                let Some(found) = lock(writers).get(&client).map(|c| c.events.clone()) else {
                    continue;
                };
                new.insert(found)
            }
        };
        match queue.try_send(event) {
            Ok(()) => {}
            Err(TrySendError::Disconnected(_)) => {
                queues.remove(&client);
            }
            Err(TrySendError::Full(_)) => {
                tracing::warn!(client = client.0, "an app doesn't read its events: let go");
                queues.remove(&client);
                let_go(writers, client);
            }
        }
    }
}

/// A client's writer: what's queued for it written in order, the events waiting at once
/// written together (one write for many small frames' messages). An event that can't be
/// framed (past the protocol's largest message) is skipped, the others still written.
fn write_events(link: &Link, outgoing: &Receiver<Outgoing>) {
    let mut bytes = Vec::new();
    while let Ok(first) = outgoing.recv() {
        bytes.clear();
        let mut next = Some(first);
        while let Some(Outgoing { event, undelivered }) = next {
            // A failed one leaves nothing of it in `bytes` (what it handed over taken back);
            // one framed goes out now (a write failing partway may have reached its app).
            match (framing::write(&event, &mut bytes), undelivered) {
                (Ok(()), Some(undelivered)) => undelivered.delivered(),
                (Ok(()), None) => {}
                (Err(e), _) => tracing::warn!(error = %e, "an event can't be sent: skipped"),
            }
            next = if bytes.len() < WRITE_KEPT { outgoing.try_recv().ok() } else { None };
        }
        if (&link.stream).write_all(&bytes).is_err() {
            return;
        }
        if bytes.capacity() > WRITE_KEPT {
            bytes.shrink_to(WRITE_KEPT);
        }
    }
}

/// A client served: its reader and writer threads (a slow or broken one holds nobody else),
/// both ending once its connection does or is cut (the server ending cuts every one). Only
/// the user's own programs are served: a peer run by another user is refused, and a
/// greeting naming another process than the one the system tells ends the connection.
fn serve(client: ClientId, stream: Stream, serving: &Serving) {
    if !peer::same_user(&stream) {
        tracing::warn!(client = client.0, "a connection of another user: refused");
        return;
    }
    let peer_pid = peer::pid(&stream);
    // One connection, read on one thread and written on the other (`&Stream` does both).
    let link = Arc::new(Link { stream, cut: AtomicBool::new(false) });
    let (receive, send) = (Arc::clone(&link), Arc::clone(&link));
    let (events, outgoing) = mpsc::sync_channel::<Outgoing>(CLIENT_QUEUE);
    lock(&serving.writers).insert(client, Client { events, link });
    let spawned = std::thread::Builder::new()
        .name("web-host-write".into())
        .spawn(move || write_events(&send, &outgoing));
    if spawned.is_err() {
        let_go(&serving.writers, client);
        return;
    }
    let reading = serving.clone();
    let spawned = std::thread::Builder::new()
        .name("web-host-read".into())
        .spawn(move || read_commands(client, &receive, peer_pid, &reading));
    if spawned.is_err() {
        // Nobody would read it: its connection cut (its writer ends with its queue).
        tracing::warn!(client = client.0, "a client's reader didn't start: let go");
        let_go(&serving.writers, client);
        return;
    }
    // The host began to end while this one was being taken: it's let go with the others.
    if serving.ending.load(Ordering::Acquire) {
        let_go(&serving.writers, client);
    }
}

/// A client's reader: its commands to the main thread (a frame's copy straight to the paint
/// waiting for it), until its connection ends or is cut (every way the host lets a client
/// go cuts it: a blocked read ends at once, on every system).
fn read_commands(client: ClientId, link: &Link, peer_pid: Option<u32>, serving: &Serving) {
    let mut reader = Reader::default();
    let mut chunk = vec![0; READ_CHUNK];
    'read: loop {
        let read = match (&link.stream).read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted && !link.is_cut() => continue,
            Err(_) => break,
        };
        reader.push(&chunk[..read]);
        loop {
            if link.is_cut() {
                break 'read;
            }
            match reader.message::<Command>() {
                // A copy told straight to the paint waiting for it.
                Ok(Some(Command::FrameDone { page, frame })) => {
                    serving.acks.done(PageKey { client, page }, frame);
                }
                Ok(Some(Command::Hello(hello))) if !greeted_as(&hello, peer_pid) => {
                    tracing::warn!(
                        client = client.0,
                        "an app's greeting names another process: let go"
                    );
                    link.cut();
                    break 'read;
                }
                // A full queue waits here: the app's writes wait on the connection.
                Ok(Some(command)) => {
                    if serving.to_main.send(Incoming::Command(client, command)).is_err() {
                        break 'read;
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    tracing::warn!(error = %e, "a client's message can't be read: let go");
                    break 'read;
                }
            }
        }
    }
    let _ = serving.to_main.send(Incoming::Gone(client));
}

/// Whether a greeting is the process at the other end (`peer`: as the system tells it;
/// `None`, it doesn't here: the greeting is taken).
fn greeted_as(hello: &Hello, peer: Option<u32>) -> bool {
    peer.is_none_or(|pid| pid == hello.pid)
}

impl Drop for Server {
    /// Every client let go: its connection cut (its app is told the engine is gone), its
    /// threads ending with it.
    fn drop(&mut self) {
        self.ending.store(true, Ordering::Release);
        self.dropped.store(true, Ordering::Release);
        let_all_go(&self.writers);
    }
}

fn lock(writers: &Writers) -> std::sync::MutexGuard<'_, HashMap<ClientId, Client>> {
    writers.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
#[path = "../tests/unit/server.rs"]
mod tests;
