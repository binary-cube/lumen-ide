use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use interprocess::local_socket::Stream;
use interprocess::local_socket::traits::Stream as _;
use web_engine_protocol::framing::{Reader, write};
use web_engine_protocol::{Command, Event, Hello, Open, PROTOCOL, PageId, Size, Source};

use super::*;
use crate::channel::Channel;
use crate::engine::{Engine, Outbox, PageKey, Undelivered};

/// What the fake engine was asked, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Asked {
    Open(u64, u64),
    Command(u64, u64),
    Close(u64, u64),
}

#[derive(Default)]
struct Fake {
    asked: Arc<Mutex<Vec<Asked>>>,
}

impl Engine for Fake {
    fn version(&self) -> String {
        "1.0-test".into()
    }
    fn open(&mut self, page: PageKey, _open: Open, outbox: &Outbox) {
        self.asked.lock().unwrap().push(Asked::Open(page.client.0, page.page.0));
        outbox.send(page.client, Event::Loaded { page: page.page });
    }
    fn command(&mut self, page: PageKey, _command: Command) {
        self.asked.lock().unwrap().push(Asked::Command(page.client.0, page.page.0));
    }
    fn close(&mut self, page: PageKey) {
        self.asked.lock().unwrap().push(Asked::Close(page.client.0, page.page.0));
    }
}

/// A channel of its own for a test, in a fresh folder (short: a socket's path is bounded).
fn channel() -> Channel {
    let dir: PathBuf = std::env::temp_dir()
        .join(format!("wh-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]));
    let _ = std::fs::create_dir_all(&dir);
    Channel { name: "web-host-test".into(), dir }
}

/// An app's end of the channel: sends commands, reads events.
struct App {
    stream: Stream,
    reader: Reader,
}

impl App {
    fn connect(channel: &Channel) -> Self {
        let stream = channel.connect().expect("the host listens");
        Self { stream, reader: Reader::default() }
    }

    fn send(&mut self, command: &Command) {
        let mut bytes = Vec::new();
        write(command, &mut bytes).unwrap();
        self.stream.write_all(&bytes).unwrap();
    }

    fn hello(&mut self, protocol: u32) {
        self.hello_as(protocol, std::process::id());
    }

    fn hello_as(&mut self, protocol: u32, pid: u32) {
        self.send(&Command::Hello(Hello { protocol, pid, mach_service: None, adapter: None }));
    }

    /// Whether the host closed the connection within a few seconds (what it sent before
    /// skipped).
    fn closed(&mut self) -> bool {
        let _ = self.stream.set_recv_timeout(Some(Duration::from_secs(5)));
        let mut chunk = [0; 4096];
        loop {
            match self.stream.read(&mut chunk) {
                Ok(0) => return true,
                Ok(_) => continue,
                Err(e) => {
                    return !matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    );
                }
            }
        }
    }

    fn open(&mut self, page: u64) {
        self.send(&Command::Open(Open {
            page: PageId(page),
            source: Source::Html(String::new()),
            size: Size { width: 10, height: 10, scale: 1.0 },
            hidden: false,
            background: [0; 4],
            pixels: None,
        }));
    }

    fn event(&mut self) -> Event {
        let mut chunk = [0; 4096];
        loop {
            if let Some(event) = self.reader.message::<Event>().unwrap() {
                return event;
            }
            let n = self.stream.read(&mut chunk).unwrap();
            assert!(n > 0, "the host closed the connection");
            self.reader.push(&chunk[..n]);
        }
    }
}

/// Polls the server until `done` (at most a few seconds).
fn until(server: &mut Server, engine: &mut Fake, done: impl Fn(&[Asked]) -> bool) -> State {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let state = server.poll(engine, Duration::from_millis(10));
        if done(&engine.asked.lock().unwrap()) || state == State::Ended {
            return state;
        }
        assert!(Instant::now() < deadline, "{:?}", engine.asked.lock().unwrap());
    }
}

/// Two apps share one host: each opens its pages and gets its own events; one gone closes
/// only its pages; an app of another protocol is never listened to; with no page left for
/// its idle time, the host ends.
#[test]
fn one_host_serves_every_app_and_ends_when_idle() {
    let channel = channel();
    let claim = channel.claim().unwrap().expect("nobody holds it");
    let lifetime = Lifetime { idle: Duration::from_millis(300) };
    let mut server = Server::start(claim, lifetime).unwrap();
    let mut engine = Fake::default();

    let mut first = App::connect(&channel);
    first.hello(PROTOCOL);
    until(&mut server, &mut engine, |_| true);
    assert!(
        matches!(first.event(), Event::Welcome(w) if w.protocol == PROTOCOL && w.engine == "1.0-test")
    );
    first.open(1);
    first.open(2);
    let mut second = App::connect(&channel);
    second.hello(PROTOCOL);
    second.open(1);
    let mut stranger = App::connect(&channel);
    stranger.hello(PROTOCOL + 1);
    stranger.open(9);
    until(&mut server, &mut engine, |a| {
        a.iter().filter(|x| matches!(x, Asked::Open(..))).count() == 3
    });
    assert_eq!(server.pages(), 3, "the stranger's page isn't opened");
    assert!(matches!(first.event(), Event::Loaded { page: PageId(1) }));
    assert!(matches!(first.event(), Event::Loaded { page: PageId(2) }));
    assert!(matches!(second.event(), Event::Welcome(_)));
    assert!(matches!(second.event(), Event::Loaded { page: PageId(1) }));

    // A command reaches only its page; one for a page not open, nothing.
    second.send(&Command::BeginFrame { page: PageId(1) });
    second.send(&Command::BeginFrame { page: PageId(5) });
    until(&mut server, &mut engine, |a| a.iter().any(|x| matches!(x, Asked::Command(..))));

    // The first app gone: its two pages closed, the second's kept.
    drop(first);
    until(&mut server, &mut engine, |a| {
        a.iter().filter(|x| matches!(x, Asked::Close(..))).count() == 2
    });
    assert_eq!(server.pages(), 1);
    let asked = engine.asked.lock().unwrap().clone();
    let closed: Vec<&Asked> = asked.iter().filter(|x| matches!(x, Asked::Close(..))).collect();
    let first_id = match closed[0] {
        Asked::Close(client, _) => *client,
        _ => unreachable!(),
    };
    assert!(closed.iter().all(|x| matches!(x, Asked::Close(c, _) if *c == first_id)));
    assert_eq!(asked.iter().filter(|x| matches!(x, Asked::Command(..))).count(), 1);

    // Pages still open: the host stays, however long.
    std::thread::sleep(lifetime.idle * 2);
    assert_eq!(server.poll(&mut engine, Duration::from_millis(1)), State::Running);
    // The last page closed: it ends only after its idle time.
    second.send(&Command::Close { page: PageId(1) });
    until(&mut server, &mut engine, |a| {
        a.iter().filter(|x| matches!(x, Asked::Close(..))).count() == 3
    });
    assert_eq!(server.poll(&mut engine, Duration::from_millis(1)), State::Running);
    let since = Instant::now();
    while server.poll(&mut engine, Duration::from_millis(20)) == State::Running {
        assert!(since.elapsed() < Duration::from_secs(5));
    }
    assert!(since.elapsed() >= lifetime.idle / 2);
    drop(stranger);

    // A wake from another thread ends the wait at once.
    let wake = server.waker();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        wake();
    });
    let since = Instant::now();
    server.poll(&mut engine, Duration::from_secs(5));
    assert!(since.elapsed() < Duration::from_secs(2), "woken");
    let _ = std::fs::remove_dir_all(&channel.dir);
}

/// A frame's copy told by its app reaches the waiting paint without the main thread; one
/// not told in time, given up.
#[test]
fn a_frame_copied_is_told_to_the_waiting_paint() {
    let channel = channel();
    let claim = channel.claim().unwrap().expect("free");
    let server = Server::start(claim, Lifetime::default()).unwrap();
    let acks = server.acks().clone();
    let mut app = App::connect(&channel);
    app.hello(PROTOCOL);
    // The server's main thread never polls here: the ack comes through the reader thread.
    let key_of = |client| PageKey { client, page: PageId(3) };
    // Its client id is the first the server gave.
    let key = key_of(ClientId(1));
    acks.open(key);
    assert!(!acks.wait(key, 1, Duration::from_millis(50)), "not told: given up");
    // Behind: the next one isn't waited for (a slow app slows nobody else).
    let since = Instant::now();
    assert!(!acks.wait(key, 2, Duration::from_secs(5)));
    assert!(since.elapsed() < Duration::from_millis(50), "not waited for");
    app.send(&Command::FrameDone { page: PageId(3), frame: 2 });
    let until = Instant::now() + Duration::from_secs(5);
    while !acks.wait(key, 2, Duration::from_millis(10)) {
        assert!(Instant::now() < until, "told");
    }
    assert!(acks.wait(key, 1, Duration::ZERO), "an older one: copied too");
    // A page gone: its copies forgotten, a late one not kept.
    acks.forget(key);
    app.send(&Command::FrameDone { page: PageId(3), frame: 9 });
    std::thread::sleep(Duration::from_millis(50));
    assert!(!acks.wait(key, 9, Duration::ZERO));
    drop(server);
    let _ = std::fs::remove_dir_all(&channel.dir);
}

/// An app of another protocol isn't left waiting for a greeting that never comes: its
/// connection is cut at once, and its pages never open.
#[test]
fn an_app_of_another_protocol_is_cut_at_once() {
    let channel = channel();
    let claim = channel.claim().unwrap().expect("free");
    let mut server = Server::start(claim, Lifetime::default()).unwrap();
    let mut engine = Fake::default();
    let mut stranger = App::connect(&channel);
    stranger.hello(PROTOCOL + 1);
    stranger.open(1);
    let since = Instant::now();
    // The host's main thread polls on a thread of its own while the app waits.
    let polling = std::thread::spawn(move || {
        while since.elapsed() < Duration::from_millis(500) {
            server.poll(&mut engine, Duration::from_millis(10));
        }
        (server, engine)
    });
    assert!(stranger.closed(), "the connection ends");
    assert!(since.elapsed() < Duration::from_secs(2), "at once, not after the app's wait");
    let (server, engine) = polling.join().unwrap();
    assert_eq!(server.pages(), 0);
    assert!(engine.asked.lock().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&channel.dir);
}

/// Where the system tells the peer's process, a greeting naming another one (frames'
/// handles would go to it) ends the connection before the host hears of it.
#[cfg(any(target_os = "macos", target_os = "linux", windows))]
#[test]
fn a_greeting_naming_another_process_is_refused() {
    let channel = channel();
    let claim = channel.claim().unwrap().expect("free");
    let mut server = Server::start(claim, Lifetime::default()).unwrap();
    let mut engine = Fake::default();
    let mut impostor = App::connect(&channel);
    impostor.hello_as(PROTOCOL, std::process::id().wrapping_add(1));
    assert!(impostor.closed(), "let go");
    for _ in 0..10 {
        server.poll(&mut engine, Duration::from_millis(10));
    }
    assert_eq!(server.pages(), 0, "never welcomed");
    // The app's own process is welcomed.
    let mut app = App::connect(&channel);
    app.hello(PROTOCOL);
    for _ in 0..50 {
        server.poll(&mut engine, Duration::from_millis(10));
    }
    assert!(matches!(app.event(), Event::Welcome(_)));
    let _ = std::fs::remove_dir_all(&channel.dir);
}

/// A greeting is taken as the process the system tells, or as said where it tells none.
#[test]
fn a_greeting_is_the_process_the_system_tells() {
    let hello = Hello { protocol: PROTOCOL, pid: 7, mach_service: None, adapter: None };
    assert!(greeted_as(&hello, Some(7)));
    assert!(!greeted_as(&hello, Some(8)));
    assert!(greeted_as(&hello, None));
}

/// The channel stays the running server's: another host's claim finds it taken while the
/// server lives (its lock isn't let go as the server starts), free once it's gone.
#[test]
fn a_running_server_keeps_its_channel() {
    let channel = channel();
    let claim = channel.claim().unwrap().expect("free");
    let server = Server::start(claim, Lifetime::default()).unwrap();
    assert!(channel.claim().unwrap().is_none(), "the server holds it");
    // Still reachable: its listener is served.
    let mut app = App::connect(&channel);
    app.hello(PROTOCOL);
    drop(server);
    assert!(channel.claim().unwrap().is_some(), "free again");
    let _ = std::fs::remove_dir_all(&channel.dir);
}

/// An app that doesn't read its events is let go once its queue is full: its connection
/// cut, its pages closed, the host's memory not holding its events.
#[test]
fn an_app_not_reading_is_let_go() {
    let channel = channel();
    let claim = channel.claim().unwrap().expect("free");
    let mut server = Server::start(claim, Lifetime::default()).unwrap();
    let mut engine = Fake::default();
    let mut app = App::connect(&channel);
    app.hello(PROTOCOL);
    app.open(1);
    until(&mut server, &mut engine, |a| a.iter().any(|x| matches!(x, Asked::Open(..))));
    // Never read: the connection's buffer fills, then the writer's queue.
    let text = "x".repeat(16 * 1024);
    let client = ClientId(1);
    for _ in 0..CLIENT_QUEUE * 2 {
        server.outbox().send(client, Event::Message { page: PageId(1), text: text.clone() });
    }
    until(&mut server, &mut engine, |a| a.iter().any(|x| matches!(x, Asked::Close(..))));
    assert_eq!(server.pages(), 0, "its page closed");
    assert!(app.closed(), "its connection cut");
    let _ = std::fs::remove_dir_all(&channel.dir);
}

/// Commands before an app's greeting are ignored; a second greeting is answered again but
/// opens nothing twice (a page already open stays one page).
#[test]
fn commands_before_the_greeting_are_ignored_and_a_second_one_opens_nothing_twice() {
    let channel = channel();
    let claim = channel.claim().unwrap().expect("free");
    let mut server = Server::start(claim, Lifetime::default()).unwrap();
    let mut engine = Fake::default();
    let mut app = App::connect(&channel);
    app.open(1);
    app.send(&Command::BeginFrame { page: PageId(1) });
    app.hello(PROTOCOL);
    until(&mut server, &mut engine, |_| true);
    assert!(matches!(app.event(), Event::Welcome(_)));
    assert_eq!(server.pages(), 0, "the open before the greeting is ignored");
    assert!(engine.asked.lock().unwrap().is_empty());
    app.open(1);
    app.hello(PROTOCOL);
    app.open(1);
    app.send(&Command::BeginFrame { page: PageId(1) });
    until(&mut server, &mut engine, |a| a.iter().any(|x| matches!(x, Asked::Command(..))));
    assert!(matches!(app.event(), Event::Loaded { page: PageId(1) }));
    assert!(matches!(app.event(), Event::Welcome(_)), "greeted again");
    let asked = engine.asked.lock().unwrap().clone();
    assert_eq!(asked.iter().filter(|x| matches!(x, Asked::Open(..))).count(), 1, "{asked:?}");
    assert_eq!(server.pages(), 1);
    let _ = std::fs::remove_dir_all(&channel.dir);
}

/// An event too large for the protocol is skipped; the next one still reaches its app.
#[test]
fn an_event_too_large_is_skipped_and_the_next_arrives() {
    let channel = channel();
    let claim = channel.claim().unwrap().expect("free");
    let mut server = Server::start(claim, Lifetime::default()).unwrap();
    let mut engine = Fake::default();
    let mut app = App::connect(&channel);
    app.hello(PROTOCOL);
    until(&mut server, &mut engine, |_| true);
    assert!(matches!(app.event(), Event::Welcome(_)));
    let huge = "x".repeat(web_engine_protocol::framing::MAX_MESSAGE + 1);
    server.outbox().send(ClientId(1), Event::Message { page: PageId(1), text: huge });
    server.outbox().send(ClientId(1), Event::Loaded { page: PageId(2) });
    assert!(matches!(app.event(), Event::Loaded { page: PageId(2) }));
    let _ = std::fs::remove_dir_all(&channel.dir);
}

/// Once the host ends, its apps are let go at once and an app connecting meanwhile sees the
/// end at once (not a wait for nothing), while the channel stays the host's until its
/// server is let go (its engine's profile still in use).
#[test]
fn an_ending_host_lets_its_apps_go_and_closes_new_connections() {
    let channel = channel();
    let claim = channel.claim().unwrap().expect("free");
    let mut server = Server::start(claim, Lifetime { idle: Duration::from_secs(1) }).unwrap();
    let mut engine = Fake::default();
    let mut connected = App::connect(&channel);
    connected.hello(PROTOCOL);
    until(&mut server, &mut engine, |_| true);
    assert!(matches!(connected.event(), Event::Welcome(_)));
    let deadline = Instant::now() + Duration::from_secs(5);
    while server.poll(&mut engine, Duration::from_millis(20)) == State::Running {
        assert!(Instant::now() < deadline, "it ends");
    }
    let since = Instant::now();
    assert!(connected.closed(), "the app connected is let go");
    // Each one, however many: the channel answers until the host is gone.
    for _ in 0..2 {
        let mut late = App::connect(&channel);
        // Its greeting may find the connection closed already.
        let hello = Hello {
            protocol: PROTOCOL,
            pid: std::process::id(),
            mach_service: None,
            adapter: None,
        };
        let mut bytes = Vec::new();
        write(&Command::Hello(hello), &mut bytes).unwrap();
        let _ = late.stream.write_all(&bytes);
        assert!(late.closed(), "a connection while ending is closed");
    }
    assert!(since.elapsed() < Duration::from_secs(2), "at once");
    assert!(channel.claim().unwrap().is_none(), "still the ending host's");
    drop(server);
    assert!(channel.claim().unwrap().is_some(), "free once it's gone");
    let _ = std::fs::remove_dir_all(&channel.dir);
}

/// What an event handed over is taken back when it never reaches its app (too large to
/// send, its app gone), never when it was written.
#[test]
fn an_event_not_delivered_takes_back_what_it_handed_over() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let channel = channel();
    let claim = channel.claim().unwrap().expect("free");
    let mut server = Server::start(claim, Lifetime::default()).unwrap();
    let mut engine = Fake::default();
    let mut app = App::connect(&channel);
    app.hello(PROTOCOL);
    until(&mut server, &mut engine, |_| true);
    assert!(matches!(app.event(), Event::Welcome(_)));
    let taken_back = Arc::new(AtomicUsize::new(0));
    let tracked = || {
        let taken_back = Arc::clone(&taken_back);
        Undelivered::new(move || {
            taken_back.fetch_add(1, Ordering::SeqCst);
        })
    };
    let huge = "x".repeat(web_engine_protocol::framing::MAX_MESSAGE + 1);
    let outbox = server.outbox().clone();
    outbox.send_tracked(ClientId(1), Event::Message { page: PageId(1), text: huge }, tracked());
    outbox.send_tracked(ClientId(1), Event::Loaded { page: PageId(2) }, tracked());
    assert!(matches!(app.event(), Event::Loaded { page: PageId(2) }));
    assert_eq!(taken_back.load(Ordering::SeqCst), 1, "only the one never sent");
    // A client nobody knows: dropped, taken back.
    outbox.send_tracked(ClientId(99), Event::Loaded { page: PageId(1) }, tracked());
    let deadline = Instant::now() + Duration::from_secs(5);
    while taken_back.load(Ordering::SeqCst) < 2 {
        assert!(Instant::now() < deadline, "taken back");
        std::thread::sleep(Duration::from_millis(5));
    }
    let _ = std::fs::remove_dir_all(&channel.dir);
}
