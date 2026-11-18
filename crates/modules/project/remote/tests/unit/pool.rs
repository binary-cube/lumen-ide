use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// A server counting its connections and the sessions working at once; its sessions can
/// break (the network).
#[derive(Default)]
struct Server {
    connections: AtomicUsize,
    working: AtomicUsize,
    most: AtomicUsize,
    broken: AtomicBool,
}

struct Session(Arc<Server>);

impl RemoteFs for Session {
    fn list(&mut self, _: &str) -> Result<Vec<RemoteEntry>, RemoteError> {
        Ok(Vec::new())
    }
    fn read(&mut self, _: &str) -> Result<Vec<u8>, RemoteError> {
        if self.0.broken.swap(false, Ordering::Relaxed) {
            return Err(RemoteError::Connect("reset".into()));
        }
        // Working a while: how many at once is counted.
        let now = self.0.working.fetch_add(1, Ordering::Relaxed) + 1;
        self.0.most.fetch_max(now, Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(30));
        self.0.working.fetch_sub(1, Ordering::Relaxed);
        Ok(b"x".to_vec())
    }
    fn write(&mut self, _: &str, _: &[u8]) -> Result<RemoteEntry, RemoteError> {
        unreachable!()
    }
    fn remove(&mut self, _: &str) -> Result<(), RemoteError> {
        Ok(())
    }
    fn rename(&mut self, _: &str, _: &str) -> Result<(), RemoteError> {
        Ok(())
    }
    fn stat(&mut self, _: &str) -> Result<Option<RemoteEntry>, RemoteError> {
        Ok(None)
    }
}

struct Connects(Arc<Server>);

impl Connector for Connects {
    fn connect(&self) -> Result<Box<dyn RemoteFs>, RemoteError> {
        self.0.connections.fetch_add(1, Ordering::Relaxed);
        Ok(Box::new(Session(Arc::clone(&self.0))))
    }
}

fn pool(limit: usize) -> (Arc<Server>, Pooled) {
    let server = Arc::new(Server::default());
    let pool = Pool::new(Arc::new(Connects(Arc::clone(&server))), limit);
    (server, Pooled(pool))
}

#[test]
fn an_idle_session_is_handed_out_again() {
    let (server, pooled) = pool(2);
    for _ in 0..5 {
        let mut session = pooled.connect().unwrap();
        session.read("a").unwrap();
    }
    assert_eq!(server.connections.load(Ordering::Relaxed), 1, "one handshake, five uses");
    assert_eq!(pooled.0.open(), 1);
}

#[test]
#[allow(clippy::disallowed_methods)] // The test's own threads, asking at once.
fn no_more_than_the_limit_at_once() {
    let (server, pooled) = pool(2);
    std::thread::scope(|s| {
        for _ in 0..8 {
            let pooled = pooled.clone();
            s.spawn(move || pooled.connect().unwrap().read("a").unwrap());
        }
    });
    assert!(server.most.load(Ordering::Relaxed) <= 2, "at most two working at once");
    assert!(server.connections.load(Ordering::Relaxed) <= 2);
}

#[test]
fn a_session_the_network_broke_isn_t_kept() {
    let (server, pooled) = pool(1);
    server.broken.store(true, Ordering::Relaxed);
    {
        let mut session = pooled.connect().unwrap();
        assert!(session.read("a").is_err());
    }
    assert_eq!(pooled.0.open(), 0, "let go, its slot free");
    pooled.connect().unwrap().read("a").unwrap();
    assert_eq!(server.connections.load(Ordering::Relaxed), 2, "a new one opened");
}

/// An idle session is let go once it's been idle long enough, even if nothing asks for a
/// session again (it stayed open until the project closed).
#[test]
fn an_idle_session_is_let_go_without_another_request() {
    let server = Arc::new(Server::default());
    let connector = Arc::new(Connects(Arc::clone(&server)));
    let pooled = Pooled(Pool::with_idle(connector, 2, Duration::from_millis(50)));
    pooled.connect().unwrap().read("a").unwrap();
    assert_eq!(pooled.0.open(), 1, "kept idle");
    let until = std::time::Instant::now() + Duration::from_secs(5);
    while pooled.0.open() > 0 && std::time::Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(pooled.0.open(), 0, "let go once idle");
    // Asked again: a new one.
    pooled.connect().unwrap().read("a").unwrap();
    assert_eq!(server.connections.load(Ordering::Relaxed), 2);
}

/// A server taking `limit` connections at once (more are reset), counting the most open.
struct Limited {
    limit: usize,
    live: Arc<AtomicUsize>,
    most: AtomicUsize,
    refused: AtomicUsize,
}

struct Held(Arc<AtomicUsize>);

impl Drop for Held {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

struct LimitedSession(#[allow(dead_code)] Held);

impl RemoteFs for LimitedSession {
    fn list(&mut self, _: &str) -> Result<Vec<RemoteEntry>, RemoteError> {
        std::thread::sleep(Duration::from_millis(20));
        Ok(Vec::new())
    }
    fn read(&mut self, _: &str) -> Result<Vec<u8>, RemoteError> {
        Ok(Vec::new())
    }
    fn write(&mut self, _: &str, _: &[u8]) -> Result<RemoteEntry, RemoteError> {
        unreachable!()
    }
    fn remove(&mut self, _: &str) -> Result<(), RemoteError> {
        Ok(())
    }
    fn rename(&mut self, _: &str, _: &str) -> Result<(), RemoteError> {
        Ok(())
    }
    fn stat(&mut self, _: &str) -> Result<Option<RemoteEntry>, RemoteError> {
        Ok(None)
    }
}

impl Connector for Limited {
    fn connect(&self) -> Result<Box<dyn RemoteFs>, RemoteError> {
        let taken = self.live.try_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
            (n < self.limit).then_some(n + 1)
        });
        match taken {
            Ok(before) => {
                self.most.fetch_max(before + 1, Ordering::Relaxed);
                Ok(Box::new(LimitedSession(Held(Arc::clone(&self.live)))))
            }
            Err(_) => {
                self.refused.fetch_add(1, Ordering::Relaxed);
                Err(RemoteError::Auth("530 Sorry, the maximum number of clients (2) from your host are already connected.".into()))
            }
        }
    }
}

/// Cautious (FTP): two at first, more only while the server takes them; refused, as many as
/// it took — waited for instead of failing, remembered for the next pool of that server.
#[test]
// Eight sessions asked for at once need threads of their own.
#[allow(clippy::disallowed_methods)]
fn a_cautious_pool_keeps_to_what_the_server_takes() {
    let server = Arc::new(Limited {
        limit: 3,
        live: Arc::default(),
        most: AtomicUsize::new(0),
        refused: AtomicUsize::new(0),
    });
    let name = format!("ftp.example:{}", std::process::id());
    let pool = Pool::cautious(Arc::clone(&server) as Arc<dyn Connector>, 6, name.clone());
    assert_eq!(pool.limit(), 2, "two at first");
    let pooled = Pooled(Arc::clone(&pool));
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                let mut fs = pooled.connect().expect("waited for, never failed");
                fs.list("").unwrap();
            });
        }
    });
    assert_eq!(server.most.load(Ordering::Relaxed), 3, "grown while accepted");
    assert!(server.refused.load(Ordering::Relaxed) >= 1);
    assert_eq!(pool.limit(), 3);
    assert_eq!(pool.take_lowered(), Some(3), "said once");
    assert_eq!(pool.take_lowered(), None);
    assert!(pool.was_lowered());
    // The next pool for that server starts where it stopped, and never asks for more (this
    // one's connections closed first).
    drop((pooled, pool));
    let refused = server.refused.load(Ordering::Relaxed);
    let again = Pool::cautious(Arc::clone(&server) as Arc<dyn Connector>, 6, name);
    assert_eq!(again.limit(), 3);
    let pooled = Pooled(again);
    std::thread::scope(|scope| {
        for _ in 0..6 {
            scope.spawn(|| pooled.connect().unwrap().list("").unwrap());
        }
    });
    assert_eq!(server.refused.load(Ordering::Relaxed), refused, "no refusal again");
    assert!(refused_for_load(&RemoteError::Connect(
        "Connection reset by peer (os error 54)".into()
    )));
    assert!(refused_for_load(&RemoteError::Server("421 Too many connections".into())));
    assert!(!refused_for_load(&RemoteError::Auth("530 Login incorrect.".into())));
}
