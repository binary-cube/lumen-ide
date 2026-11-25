#[cfg(unix)]
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Receiver;

use web_engine_protocol::{Open, Size, Source};
use web_host_core::{Engine, Lifetime, Outbox, PageKey, Server, State};

use super::*;
use crate::page::{PageSpec, WebPage};

/// A fake engine: each page opened says it's loaded; what it was asked, counted.
#[derive(Default)]
struct Fake {
    closed: Arc<AtomicU64>,
    /// The GPU it says it draws on.
    adapter: Option<Adapter>,
}

impl Engine for Fake {
    fn version(&self) -> String {
        "test".into()
    }
    fn adapter(&self) -> Option<Adapter> {
        self.adapter
    }
    fn open(&mut self, page: PageKey, _open: Open, outbox: &Outbox) {
        outbox.send(page.client, Event::Loaded { page: page.page });
    }
    fn command(&mut self, _page: PageKey, _command: Command) {}
    fn close(&mut self, _page: PageKey) {
        self.closed.fetch_add(1, Ordering::Relaxed);
    }
}

/// How long a test host waits for its channel to be free.
const CLAIM_WAIT: Duration = Duration::from_secs(5);

/// A host on its own thread until `stop`.
fn host(
    channel: &Channel,
    closed: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    host_on(channel, (closed, stop), None)
}

/// [`host`] drawing on `adapter`.
fn host_on(
    channel: &Channel,
    (closed, stop): (Arc<AtomicU64>, Arc<AtomicBool>),
    adapter: Option<Adapter>,
) -> std::thread::JoinHandle<()> {
    // Free soon: whoever tried to claim it at the same moment (a client checking whether a
    // host runs) lets it go at once, slower under load.
    let started = std::time::Instant::now();
    let claim = loop {
        if let Some(claim) = channel.claim().unwrap() {
            break claim;
        }
        assert!(started.elapsed() < CLAIM_WAIT, "free");
        std::thread::sleep(Duration::from_millis(5));
    };
    let mut server = Server::start(claim, Lifetime { idle: Duration::from_secs(60) }).unwrap();
    std::thread::spawn(move || {
        let mut engine = Fake { closed, adapter };
        while !stop.load(Ordering::Relaxed) {
            if server.poll(&mut engine, Duration::from_millis(10)) == State::Ended {
                break;
            }
        }
    })
}

fn sink() -> (Sink, Receiver<Event>) {
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let sink: Sink = Arc::new(move |e| {
        if let PageEvent::Event(e) = e {
            drop(tx.lock().unwrap().send(e));
        }
    });
    (sink, rx)
}

fn spec() -> PageSpec {
    PageSpec {
        source: Source::Html("<p>x</p>".into()),
        size: Size { width: 100, height: 100, scale: 1.0 },
        hidden: false,
        background: [0; 4],
        pixels: None,
    }
}

/// Connected to the running host (nothing started), a page opened tells it's loaded,
/// dropped it's closed; the host gone, every page is told, and nothing is started when
/// there's nothing to start.
#[test]
fn pages_go_through_the_running_host() {
    let dir = std::env::temp_dir().join(format!("we-{}", std::process::id() % 100_000));
    let channel = Channel { name: format!("web-engine-test-{}", std::process::id()), dir };
    let (closed, stop) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicBool::new(false)));
    let running = host(&channel, Arc::clone(&closed), Arc::clone(&stop));

    let started = AtomicBool::new(false);
    let connection = Arc::new(
        Connection::establish(&channel, None, || {
            started.store(true, Ordering::Relaxed);
            None
        })
        .unwrap(),
    );
    assert!(!started.load(Ordering::Relaxed), "one ran: nothing started");
    assert_eq!(connection.engine(), "test");

    let (sink_a, events_a) = sink();
    let page = WebPage::open(Arc::clone(&connection), spec(), sink_a);
    let got = events_a.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(got, Event::Loaded { page: page.id() });
    drop(page);
    let since = Instant::now();
    while closed.load(Ordering::Relaxed) == 0 {
        assert!(since.elapsed() < Duration::from_secs(5), "closed");
        std::thread::sleep(Duration::from_millis(5));
    }

    let (sink_b, events_b) = sink();
    let kept = WebPage::open(Arc::clone(&connection), spec(), sink_b);
    assert!(matches!(events_b.recv_timeout(Duration::from_secs(5)), Ok(Event::Loaded { .. })));
    // The host gone: the page is told.
    stop.store(true, Ordering::Relaxed);
    running.join().unwrap();
    let crashed = events_b.recv_timeout(Duration::from_secs(5));
    assert_eq!(crashed, Ok(Event::Crashed { page: kept.id() }));
    assert!(!kept.is_alive());

    // None runs and none can be started: an error, nothing hangs.
    let err = Connection::establish(&channel, None, || None);
    assert!(err.is_err());
}

/// A host drawing on the app's GPU is joined; one on another GPU is refused — said as such,
/// and no other host started in its place.
#[test]
fn a_host_on_another_gpu_is_refused() {
    let dir = std::env::temp_dir().join(format!("we-gpu-{}", std::process::id() % 100_000));
    let channel = Channel { name: format!("web-engine-gpu-{}", std::process::id()), dir };
    let stop = Arc::new(AtomicBool::new(false));
    let ours = Adapter { high: 0, low: 1234 };
    let running = host_on(&channel, (Arc::default(), Arc::clone(&stop)), Some(ours));
    assert!(Connection::establish(&channel, Some(ours), || None).is_ok(), "the same GPU");
    let started = AtomicBool::new(false);
    let other = Connection::establish(&channel, Some(Adapter { high: 0, low: 99 }), || {
        started.store(true, Ordering::Relaxed);
        None
    });
    let error = other.expect_err("refused");
    assert_eq!(error.kind(), std::io::ErrorKind::Unsupported);
    assert!(!started.load(Ordering::Relaxed), "nothing started in its place");
    stop.store(true, Ordering::Relaxed);
    running.join().unwrap();
}

/// A host of our own making on `channel`: it accepts one app, answers its greeting with
/// `welcome` (`None`: never answers), then waits for the app to let the connection go —
/// told on the channel returned.
fn answering(channel: &Channel, welcome: Option<Welcome>) -> Receiver<()> {
    use interprocess::local_socket::traits::Listener as _;
    let claim = channel.claim().unwrap().expect("free");
    let (gone, let_go) = mpsc::channel();
    std::thread::spawn(move || {
        let mut stream = claim.listener.accept().unwrap();
        if let Some(welcome) = welcome {
            let mut bytes = Vec::new();
            framing::write(&Event::Welcome(welcome), &mut bytes).unwrap();
            stream.write_all(&bytes).unwrap();
        }
        let mut chunk = [0u8; 1024];
        while matches!(stream.read(&mut chunk), Ok(n) if n > 0) {}
        let _ = gone.send(());
    });
    let_go
}

fn test_channel(name: &str) -> Channel {
    let tag = std::process::id() % 100_000;
    Channel {
        name: format!("we-{name}-{tag}"),
        dir: std::env::temp_dir().join(format!("we-{name}-{tag}")),
    }
}

fn welcome(protocol: u32, pid: u32) -> Welcome {
    Welcome { protocol, pid, engine: "test".into(), adapter: None }
}

/// The greeting given up on waits no more than it may, and lets the connection go then
/// (nothing of it stays waiting for a host that never answers).
#[test]
fn a_greeting_that_never_comes_is_given_up() {
    let channel = test_channel("mute");
    let let_go = answering(&channel, None);
    let started = Instant::now();
    let error = Connection::greet(channel.connect().unwrap(), None, Duration::from_millis(300))
        .expect_err("no greeting");
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
    assert!(let_go.recv_timeout(Duration::from_secs(5)).is_ok(), "the connection let go");
    assert!(!engine_failed(&error), "not the engine's own failure");
}

/// A host speaking another protocol is let go, said as such.
#[test]
fn a_host_of_another_protocol_is_refused() {
    let channel = test_channel("proto");
    let let_go = answering(&channel, Some(welcome(PROTOCOL + 1, std::process::id())));
    let error = Connection::greet(channel.connect().unwrap(), None, Duration::from_secs(5))
        .expect_err("another protocol");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(let_go.recv_timeout(Duration::from_secs(5)).is_ok(), "the connection let go");
}

/// A host that isn't the process it says it is (the system tells its process) is refused.
#[test]
fn a_host_that_lies_about_its_process_is_refused() {
    let channel = test_channel("liar");
    let let_go = answering(&channel, Some(welcome(PROTOCOL, std::process::id() + 1)));
    let error = Connection::greet(channel.connect().unwrap(), None, Duration::from_secs(5))
        .expect_err("refused");
    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    assert!(let_go.recv_timeout(Duration::from_secs(5)).is_ok(), "the connection let go");
}

/// Unix: a host started that ends with an error before it listens is the engine's own
/// failure; one that ends with success (another host has the channel) and none listening
/// in time isn't.
#[cfg(unix)]
#[test]
fn only_the_engines_own_failure_counts() {
    let program = |name: &str| {
        ["/usr/bin", "/bin"].iter().map(|d| PathBuf::from(d).join(name)).find(|p| p.is_file())
    };
    let (Some(failing), Some(yielding)) = (program("false"), program("true")) else { return };
    let channel = test_channel("start");
    let launch = |program: PathBuf| Launch {
        program,
        channel: channel.clone(),
        profile: channel.dir.join("profile"),
        hold: None,
        adapter: None,
    };
    let waits = Waits { start: Duration::from_millis(500), welcome: Duration::from_secs(1) };
    let failed = Connection::establish_within(&channel, None, || Some(launch(failing)), waits)
        .expect_err("it ended");
    assert!(engine_failed(&failed), "{failed}");
    assert_eq!(
        crate::install::service::StartOutcome::of(&Err::<(), _>(failed)),
        crate::install::service::StartOutcome::EngineFailed
    );
    let other = Connection::establish_within(&channel, None, || Some(launch(yielding)), waits)
        .expect_err("none listens");
    assert!(!engine_failed(&other), "{other}");
    assert_eq!(other.kind(), std::io::ErrorKind::TimedOut);
}

/// A host that didn't greet is blamed as soon as its process is seen failing (told by the
/// thread waiting on it, never looked for again and again); one still running past the
/// grace, or one that yielded, isn't.
#[test]
fn a_failed_start_is_blamed_as_soon_as_it_ends() {
    let timed_out = || std::io::Error::new(std::io::ErrorKind::TimedOut, "no greeting");
    let started = Instant::now();
    let failing = Started::ending(Ended::Failed, Duration::from_millis(30));
    assert!(engine_failed(&blamed(timed_out(), &failing)));
    assert!(started.elapsed() < END_GRACE, "{:?}", started.elapsed());
    let yielding = Started::ending(Ended::Yielded, Duration::from_millis(30));
    assert!(!engine_failed(&blamed(timed_out(), &yielding)));
    let running = Started::ending(Ended::Failed, Duration::from_secs(30));
    let since = Instant::now();
    assert!(!engine_failed(&blamed(timed_out(), &running)), "still running: not blamed");
    assert!(since.elapsed() >= END_GRACE);
}

/// A greeting given up on ends its reader at its next look, whatever comes after.
#[test]
fn a_greeting_given_up_reads_nothing_more() {
    let mut bytes = Vec::new();
    framing::write(&Event::Welcome(welcome(PROTOCOL, 1)), &mut bytes).unwrap();
    let (mut reader, mut chunk) = (Reader::default(), vec![0; 64]);
    let given_up = AtomicBool::new(true);
    let greeting = Greeting { reader: &mut reader, chunk: &mut chunk, given_up: &given_up };
    assert!(read_welcome(&bytes[..], greeting).is_none(), "given up: not taken");
    let given_up = AtomicBool::new(false);
    let greeting = Greeting { reader: &mut reader, chunk: &mut chunk, given_up: &given_up };
    assert!(read_welcome(&bytes[..], greeting).is_some());
}

/// A host that stops reading ends the connection once the commands' queue is full (never
/// grown past it): it's said gone, and its pages are told.
#[test]
fn a_host_that_stops_reading_ends_the_connection() {
    use interprocess::local_socket::traits::Listener as _;
    let channel = test_channel("deaf");
    let claim = channel.claim().unwrap().expect("free");
    let (done, finished) = mpsc::channel::<()>();
    let pid = std::process::id();
    let deaf = std::thread::spawn(move || {
        let mut stream = claim.listener.accept().unwrap();
        let mut bytes = Vec::new();
        framing::write(&Event::Welcome(welcome(PROTOCOL, pid)), &mut bytes).unwrap();
        stream.write_all(&bytes).unwrap();
        // Never reads: held until the test is done.
        let _ = finished.recv_timeout(Duration::from_secs(30));
    });
    let connection =
        Connection::greet(channel.connect().unwrap(), None, Duration::from_secs(5)).unwrap();
    let (page_sink, events) = sink();
    let page = connection.register(page_sink);
    let mut sent = 0usize;
    while connection.is_alive() && sent < 10_000_000 {
        connection.send(Command::BeginFrame { page });
        sent += 1;
    }
    assert!(!connection.is_alive(), "ended after {sent}");
    assert!(sent > COMMANDS_KEPT, "{sent}");
    assert_eq!(events.recv_timeout(Duration::from_secs(5)), Ok(Event::Crashed { page }));
    drop(done);
    deaf.join().unwrap();
}

/// Unix: the channel's folder must be the user's alone: another user's (made first in the
/// temporary folder) or one others may write in is refused before anything is connected
/// to or started there — not the engine's failure.
#[cfg(unix)]
#[test]
fn a_channel_folder_not_ours_alone_is_refused() {
    use std::os::unix::fs::PermissionsExt as _;
    let me = crate::launch::user();
    assert_eq!(crate::launch::untrusted_dir(me, 0o40700, me), None);
    assert_eq!(crate::launch::untrusted_dir(me, 0o40755, me), None);
    assert!(crate::launch::untrusted_dir(me.wrapping_add(1), 0o40700, me).is_some(), "another's");
    assert!(crate::launch::untrusted_dir(me, 0o40777, me).is_some(), "anyone writes");
    assert!(crate::launch::untrusted_dir(me, 0o40770, me).is_some(), "its group writes");

    let channel = test_channel("open");
    std::fs::create_dir_all(&channel.dir).unwrap();
    std::fs::set_permissions(&channel.dir, std::fs::Permissions::from_mode(0o777)).unwrap();
    let started = AtomicBool::new(false);
    let error = Connection::establish(&channel, None, || {
        started.store(true, Ordering::Relaxed);
        None
    })
    .expect_err("refused");
    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    assert!(!started.load(Ordering::Relaxed), "nothing started there");
    assert!(!engine_failed(&error));
    std::fs::remove_dir_all(&channel.dir).unwrap();
}

/// Unix: a host started that yields (another had the channel, ending meanwhile) while
/// nothing listens is started again a moment later, a few times at most: the one that
/// listens then is joined.
#[cfg(unix)]
#[test]
fn a_host_that_yielded_to_one_ending_is_started_again() {
    use std::os::unix::fs::PermissionsExt as _;
    let channel = test_channel("again");
    std::fs::create_dir_all(&channel.dir).unwrap();
    let runs = channel.dir.join("runs");
    let _ = std::fs::remove_file(&runs);
    // Started as `program --channel-dir <dir> …`: it says it ran there, and yields.
    let program = channel.dir.join("yielding.sh");
    std::fs::write(&program, "#!/bin/sh\necho run >> \"$2/runs\"\n").unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    let count = |runs: &std::path::Path| {
        std::fs::read_to_string(runs).map(|t| t.lines().count()).unwrap_or(0)
    };
    // The host that listens comes only once the yielding one was started again.
    let stop = Arc::new(AtomicBool::new(false));
    let watcher = {
        let (channel, runs, stop) = (channel.clone(), runs.clone(), Arc::clone(&stop));
        std::thread::spawn(move || {
            let since = Instant::now();
            while since.elapsed() < Duration::from_secs(10) {
                if count(&runs) >= 2 {
                    return Some(host(&channel, Arc::default(), stop));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            None
        })
    };
    let launch = Launch {
        program,
        channel: channel.clone(),
        profile: channel.dir.join("profile"),
        hold: None,
        adapter: None,
    };
    let waits = Waits { start: Duration::from_secs(10), welcome: Duration::from_secs(5) };
    let joined = Connection::establish_within(&channel, None, || Some(launch), waits);
    let running = watcher.join().unwrap();
    assert!(joined.is_ok(), "{:?}", joined.err());
    let ran = count(&runs);
    assert!((2..=1 + RESTARTS as usize).contains(&ran), "{ran}");
    stop.store(true, Ordering::Relaxed);
    running.expect("listened").join().unwrap();
    drop(joined);
    let _ = std::fs::remove_dir_all(&channel.dir);
}

/// Unix: a host ending answers (it closes every connection until it's gone) while the one
/// started yields to it: a host is started again once it let the channel go, and joined.
#[cfg(unix)]
#[test]
fn a_host_ending_is_waited_out_and_another_started() {
    use interprocess::local_socket::traits::Listener as _;
    use std::os::unix::fs::PermissionsExt as _;
    let channel = test_channel("ending");
    std::fs::create_dir_all(&channel.dir).unwrap();
    let runs = channel.dir.join("runs");
    let _ = std::fs::remove_file(&runs);
    let program = channel.dir.join("yielding.sh");
    std::fs::write(&program, "#!/bin/sh\necho run >> \"$2/runs\"\n").unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    let count = |runs: &std::path::Path| {
        std::fs::read_to_string(runs).map(|t| t.lines().count()).unwrap_or(0)
    };
    // The ending host: every connection closed at once; gone once a host was started, a
    // listening one in its place.
    let claim = channel.claim().unwrap().expect("free");
    let stop = Arc::new(AtomicBool::new(false));
    let ending = {
        let (channel, runs, stop) = (channel.clone(), runs.clone(), Arc::clone(&stop));
        std::thread::spawn(move || {
            loop {
                let Ok(stream) = claim.listener.accept() else { return None };
                drop(stream);
                if count(&runs) >= 1 {
                    break;
                }
            }
            drop(claim);
            Some(host(&channel, Arc::default(), stop))
        })
    };
    let launch = Launch {
        program,
        channel: channel.clone(),
        profile: channel.dir.join("profile"),
        hold: None,
        adapter: None,
    };
    let waits = Waits { start: Duration::from_secs(10), welcome: Duration::from_secs(5) };
    let joined = Connection::establish_within(&channel, None, || Some(launch), waits);
    assert!(joined.is_ok(), "{:?}", joined.err());
    let ran = count(&runs);
    assert!((2..=1 + 2 * RESTARTS as usize).contains(&ran), "{ran}");
    stop.store(true, Ordering::Relaxed);
    ending.join().unwrap().expect("listened").join().unwrap();
    drop(joined);
    let _ = std::fs::remove_dir_all(&channel.dir);
}
