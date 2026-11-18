// The tests set up what surrounds the pure code: files on disk, threads.
#![allow(clippy::disallowed_methods)]

use super::*;
use crate::{Auth, Mode, Protocol};
use std::collections::BTreeMap;
use std::sync::Arc;

/// A server in memory: path → (bytes, modified).
#[derive(Default)]
struct Server {
    /// Reads failing (the network) before they work.
    flaky_reads: AtomicUsize,
    /// Reads failing for good (permissions).
    refused: Mutex<Option<String>>,
    files: Mutex<BTreeMap<String, (Vec<u8>, i64)>>,
    clock: AtomicUsize,
    sessions: AtomicUsize,
    /// What was set on files (their properties), and a path the server refuses to change.
    sets: Mutex<Vec<(String, crate::properties::SetProperties)>>,
    locked: Mutex<Option<String>>,
}

impl Server {
    fn put(&self, path: &str, text: &str) {
        let t = self.clock.fetch_add(1, Ordering::Relaxed) as i64 + 100;
        self.files.lock().unwrap().insert(path.into(), (text.as_bytes().to_vec(), t));
    }
    fn text(&self, path: &str) -> Option<String> {
        self.files.lock().unwrap().get(path).map(|(b, _)| String::from_utf8_lossy(b).into_owned())
    }
}

struct Session(Arc<Server>);

impl RemoteFs for Session {
    fn list(&mut self, dir: &str) -> Result<Vec<RemoteEntry>, RemoteError> {
        let files = self.0.files.lock().unwrap();
        let prefix = if dir.is_empty() { String::new() } else { format!("{dir}/") };
        let mut out: Vec<RemoteEntry> = Vec::new();
        for (path, (bytes, modified)) in files.iter() {
            let Some(rest) = path.strip_prefix(&prefix) else { continue };
            match rest.split_once('/') {
                Some((sub, _)) => {
                    let p = format!("{prefix}{sub}");
                    if !out.iter().any(|e| e.path == p) {
                        out.push(RemoteEntry { path: p, is_dir: true, size: 0, modified: 0 });
                    }
                }
                None => out.push(RemoteEntry {
                    path: path.clone(),
                    is_dir: false,
                    size: bytes.len() as u64,
                    modified: *modified,
                }),
            }
        }
        Ok(out)
    }
    fn read(&mut self, path: &str) -> Result<Vec<u8>, RemoteError> {
        if self.0.refused.lock().unwrap().as_deref() == Some(path) {
            return Err(RemoteError::Server("permission denied".into()));
        }
        if self.0.flaky_reads.load(Ordering::Relaxed) > 0 {
            self.0.flaky_reads.fetch_sub(1, Ordering::Relaxed);
            return Err(RemoteError::Connect("connection reset".into()));
        }
        // A network's latency: the workers overlap, as they do for real.
        std::thread::sleep(std::time::Duration::from_millis(2));
        self.0
            .files
            .lock()
            .unwrap()
            .get(path)
            .map(|(b, _)| b.clone())
            .ok_or(RemoteError::NotFound(path.into()))
    }
    fn write(&mut self, path: &str, data: &[u8]) -> Result<RemoteEntry, RemoteError> {
        self.0.put(path, &String::from_utf8_lossy(data));
        self.stat(path).map(|e| e.unwrap())
    }
    fn remove(&mut self, path: &str) -> Result<(), RemoteError> {
        self.0.files.lock().unwrap().remove(path);
        Ok(())
    }
    fn rename(&mut self, from: &str, to: &str) -> Result<(), RemoteError> {
        let mut files = self.0.files.lock().unwrap();
        let file = files.remove(from).ok_or(RemoteError::NotFound(from.into()))?;
        files.insert(to.into(), file);
        Ok(())
    }
    fn set_properties(
        &mut self,
        path: &str,
        set: &crate::properties::SetProperties,
    ) -> Result<(), RemoteError> {
        self.set(path, set)
    }
    fn stat(&mut self, path: &str) -> Result<Option<RemoteEntry>, RemoteError> {
        Ok(self.0.files.lock().unwrap().get(path).map(|(b, m)| RemoteEntry {
            path: path.into(),
            is_dir: false,
            size: b.len() as u64,
            modified: *m,
        }))
    }
}

impl Session {
    fn set(
        &mut self,
        path: &str,
        set: &crate::properties::SetProperties,
    ) -> Result<(), RemoteError> {
        if self.0.locked.lock().unwrap().as_deref() == Some(path) {
            return Err(RemoteError::Server("operation not permitted".into()));
        }
        self.0.sets.lock().unwrap().push((path.into(), *set));
        Ok(())
    }
}

impl Connector for Arc<Server> {
    fn connect(&self) -> Result<Box<dyn RemoteFs>, RemoteError> {
        self.sessions.fetch_add(1, Ordering::Relaxed);
        Ok(Box::new(Session(Arc::clone(self))))
    }
}

/// The local copy in memory.
#[derive(Default)]
struct Local {
    files: Mutex<BTreeMap<String, (Vec<u8>, i64)>>,
    clock: AtomicUsize,
}

impl Local {
    fn put(&self, path: &str, text: &str) {
        let t = self.clock.fetch_add(1, Ordering::Relaxed) as i64 + 500;
        self.files.lock().unwrap().insert(path.into(), (text.as_bytes().to_vec(), t));
    }
    fn hash(&self, path: &str) -> Option<u64> {
        self.files.lock().unwrap().get(path).map(|(b, _)| content_hash(b))
    }
}

impl LocalFiles for Local {
    fn list(&self) -> Vec<LocalEntry> {
        let files = self.files.lock().unwrap();
        files
            .iter()
            .map(|(p, (b, m))| LocalEntry { path: p.clone(), size: b.len() as u64, modified: *m })
            .collect()
    }
    fn read(&self, path: &str) -> std::io::Result<Vec<u8>> {
        self.files
            .lock()
            .unwrap()
            .get(path)
            .map(|(b, _)| b.clone())
            .ok_or_else(|| std::io::ErrorKind::NotFound.into())
    }
    fn write(&self, path: &str, data: &[u8]) -> std::io::Result<LocalEntry> {
        self.put(path, &String::from_utf8_lossy(data));
        Ok(self.stat(path).unwrap())
    }
    fn remove(&self, path: &str) -> std::io::Result<()> {
        self.files.lock().unwrap().remove(path);
        Ok(())
    }
    fn stat(&self, path: &str) -> Option<LocalEntry> {
        self.files.lock().unwrap().get(path).map(|(b, m)| LocalEntry {
            path: path.into(),
            size: b.len() as u64,
            modified: *m,
        })
    }
}

fn connection() -> Connection {
    Connection {
        id: "shop".into(),
        name: "Shop".into(),
        protocol: Protocol::Sftp,
        host: "h".into(),
        port: 22,
        user: "u".into(),
        auth: Auth::Agent,
        root: "/srv".into(),
        mode: Mode::Mirror,
        excluded: vec!["vendor".into()],
        saved_in: crate::SavedIn::Project,
        secrets_in: crate::SecretsIn::Keychain,
        retry: None,
        uploads: None,
        downloads: None,
        moves: false,
        auto_upload: true,
        write_as: Default::default(),
    }
}

fn sync(
    server: &Arc<Server>,
    local: &Local,
    manifest: &mut Manifest,
    direction: Direction,
) -> Vec<Change> {
    let c = connection();
    let mut session = server.connect().unwrap();
    let remote = list_remote(&c, session.as_mut(), &|| false).unwrap();
    let changes = plan(&c, manifest, &local.list(), &remote, direction, &|p| local.hash(p));
    let retry = crate::Retry { attempts: 3, delay: std::time::Duration::ZERO };
    let run = Run {
        workers: 4,
        retry,
        now: &|| 1,
        on_transfer: &|_| {},
        on_bytes: &|_, _| {},
        cancelled: &|| false,
        parallel: &threads,
        pause: &|_| {},
    };
    apply(&changes, server, local, manifest, &run);
    changes
}

#[test]
fn the_first_pull_downloads_everything_but_the_exclusions_in_parallel() {
    let server = Arc::new(Server::default());
    for i in 0..20 {
        server.put(&format!("app/f{i}.php"), "x");
    }
    server.put("vendor/lib.php", "lib");
    let local = Local::default();
    let mut manifest = Manifest::default();
    let changes = sync(&server, &local, &mut manifest, Direction::Pull);
    assert_eq!(changes.len(), 20, "vendor isn't even listed");
    assert!(changes.iter().all(|c| c.action == Action::Download && c.chosen));
    assert_eq!(local.list().len(), 20);
    assert_eq!(manifest.len(), 20);
    assert!(server.sessions.load(Ordering::Relaxed) > 2, "several sessions at once");
    // Nothing changed since: nothing to do.
    assert!(sync(&server, &local, &mut manifest, Direction::Sync).is_empty());
}

#[test]
fn each_side_s_changes_go_the_right_way_and_both_sides_changed_is_a_conflict() {
    let server = Arc::new(Server::default());
    server.put("a.php", "a");
    server.put("b.php", "b");
    server.put("c.php", "c");
    let local = Local::default();
    let mut manifest = Manifest::default();
    sync(&server, &local, &mut manifest, Direction::Pull);

    server.put("a.php", "a on the server");
    local.put("b.php", "b here");
    server.put("c.php", "c there");
    local.put("c.php", "c here");
    // Touched, same bytes: not a change.
    let same = local.read("a.php").unwrap();
    local.write("a.php", &same).unwrap();

    let changes = sync(&server, &local, &mut manifest, Direction::Sync);
    let find = |p: &str| changes.iter().find(|c| c.path == p).unwrap();
    assert_eq!((find("a.php").action, find("a.php").chosen), (Action::Download, true));
    assert_eq!((find("b.php").action, find("b.php").chosen), (Action::Upload, true));
    assert!(find("c.php").conflict && !find("c.php").chosen, "the user decides");
    assert_eq!(local.read("a.php").unwrap(), b"a on the server");
    assert_eq!(server.text("b.php").as_deref(), Some("b here"));
    assert_eq!(server.text("c.php").as_deref(), Some("c there"), "left alone");
}

#[test]
fn removals_wait_for_the_user() {
    let server = Arc::new(Server::default());
    server.put("old.php", "o");
    let local = Local::default();
    let mut manifest = Manifest::default();
    sync(&server, &local, &mut manifest, Direction::Pull);
    server.files.lock().unwrap().remove("old.php");
    let changes = sync(&server, &local, &mut manifest, Direction::Pull);
    assert_eq!(changes[0].action, Action::DeleteLocal);
    assert!(!changes[0].chosen);
    assert!(local.stat("old.php").is_some(), "not removed unasked");
}

#[test]
fn a_dropped_connection_is_tried_again_on_a_new_session_a_refusal_is_not() {
    let server = Arc::new(Server::default());
    server.put("a.php", "a");
    server.put("b.php", "b");
    server.flaky_reads.store(2, Ordering::Relaxed);
    *server.refused.lock().unwrap() = Some("b.php".into());
    let local = Local::default();
    let mut manifest = Manifest::default();
    let c = connection();
    let mut session = server.connect().unwrap();
    let remote = list_remote(&c, session.as_mut(), &|| false).unwrap();
    let changes = plan(&c, &manifest, &[], &remote, Direction::Pull, &|_| None);
    let retry = crate::Retry { attempts: 4, delay: std::time::Duration::ZERO };
    let run = Run {
        workers: 1,
        retry,
        now: &|| 1,
        on_transfer: &|_| {},
        on_bytes: &|_, _| {},
        cancelled: &|| false,
        parallel: &threads,
        pause: &|_| {},
    };
    let before = server.sessions.load(Ordering::Relaxed);
    let transfers = apply(&changes, &server, &local, &mut manifest, &run);
    let find = |p: &str| transfers.iter().find(|t| t.path == p).unwrap();
    assert_eq!(find("a.php").error, None, "worked on the third attempt");
    assert!(
        find("b.php").error.as_deref().is_some_and(|e| e.contains("permission")),
        "{transfers:?}"
    );
    assert!(server.sessions.load(Ordering::Relaxed) - before >= 3, "a new session each time");
    assert_eq!(local.read("a.php").unwrap(), b"a");
}

#[test]
fn retries_wait_longer_each_time() {
    let r = crate::Retry { attempts: 4, delay: std::time::Duration::from_secs(1) };
    let waits: Vec<u64> = (1..=4).map(|n| r.wait_before(n).as_secs()).collect();
    assert_eq!(waits, [0, 1, 2, 4]);
}

#[test]
fn a_folder_or_a_file_alone_is_listed_and_planned() {
    let server = Arc::new(Server::default());
    server.put("app/a.php", "a");
    server.put("app/sub/b.php", "b");
    server.put("public/index.php", "i");
    let c = connection();
    let mut session = server.connect().unwrap();
    let paths = |scope: &str, session: &mut Box<dyn RemoteFs>| {
        let listed = list_remote_under(&c, session.as_mut(), scope, &|| false).unwrap();
        listed.into_iter().map(|e| e.path).collect::<Vec<_>>()
    };
    assert_eq!(paths("app", &mut session), ["app/a.php", "app/sub/b.php"]);
    assert_eq!(paths("public/index.php", &mut session), ["public/index.php"]);
    assert!(paths("gone", &mut session).is_empty(), "nothing there");
    assert!(paths("vendor", &mut session).is_empty(), "excluded");
    assert!(in_scope("app/sub/b.php", "app") && in_scope("app", "app") && in_scope("x", ""));
    assert!(!in_scope("application.php", "app"));
}

#[test]
fn a_conflict_is_decided_for_one_side() {
    let server = Arc::new(Server::default());
    server.put("c.php", "c");
    let local = Local::default();
    let mut manifest = Manifest::default();
    sync(&server, &local, &mut manifest, Direction::Pull);
    server.put("c.php", "c there");
    local.put("c.php", "c here");
    let c = connection();
    let mut session = server.connect().unwrap();
    let remote = list_remote(&c, session.as_mut(), &|| false).unwrap();
    let mut changes =
        plan(&c, &manifest, &local.list(), &remote, Direction::Sync, &|p| local.hash(p));
    assert!(changes[0].conflict && !changes[0].chosen);
    changes[0].take(false);
    assert_eq!((changes[0].action, changes[0].chosen), (Action::Upload, true));
    changes[0].take(true);
    assert_eq!(changes[0].action, Action::Download);
    let retry = crate::Retry { attempts: 1, delay: std::time::Duration::ZERO };
    let run = Run {
        workers: 1,
        retry,
        now: &|| 1,
        on_transfer: &|_| {},
        on_bytes: &|_, _| {},
        cancelled: &|| false,
        parallel: &threads,
        pause: &|_| {},
    };
    apply(&changes, &server, &local, &mut manifest, &run);
    assert_eq!(local.read("c.php").unwrap(), b"c there", "the server's copy taken");
}

#[test]
fn a_file_stands_against_its_last_transfer() {
    let mut manifest = Manifest::default();
    let synced = Synced {
        remote_size: 5,
        remote_modified: 100,
        local_size: 5,
        local_modified: 200,
        local_hash: 7,
    };
    manifest.set("a.php", synced);
    let local = |path: &str, size, modified| LocalEntry { path: path.into(), size, modified };
    let server =
        |size, modified| RemoteEntry { path: "a.php".into(), is_dir: false, size, modified };
    let same_hash = |_: &str| Some(7);
    let other_hash = |_: &str| Some(8);
    let (here, moved) = (local("a.php", 5, 200), local("a.php", 6, 300));
    let (was, newer) = (server(5, 100), server(9, 400));

    // The server's side unknown: only ours is judged.
    assert_eq!(standing(&local("b.php", 1, 1), &manifest, None, &same_hash), Standing::OnlyHere);
    assert_eq!(standing(&here, &manifest, None, &other_hash), Standing::Same);
    assert_eq!(standing(&moved, &manifest, None, &other_hash), Standing::Ahead);
    // Touched, the same content: nothing to send.
    assert_eq!(standing(&local("a.php", 5, 300), &manifest, None, &same_hash), Standing::Same);

    let b = local("b.php", 1, 1);
    assert_eq!(standing(&b, &manifest, Some(None), &same_hash), Standing::OnlyHere);
    assert_eq!(standing(&here, &manifest, Some(Some(&was)), &same_hash), Standing::Same);
    assert_eq!(standing(&moved, &manifest, Some(Some(&was)), &other_hash), Standing::Ahead);
    assert_eq!(standing(&here, &manifest, Some(Some(&newer)), &same_hash), Standing::Behind);
    // Gone from the server since.
    assert_eq!(standing(&here, &manifest, Some(None), &same_hash), Standing::Behind);
    assert_eq!(standing(&moved, &manifest, Some(Some(&newer)), &other_hash), Standing::Diverged);
}

#[test]
fn bytes_are_told_as_they_move_and_a_stop_ends_the_file_going_on() {
    let server = Arc::new(Server::default());
    for name in ["a.php", "b.php", "c.php"] {
        server.put(name, "hello");
    }
    let c = connection();
    let mut session = server.connect().unwrap();
    let remote = list_remote(&c, session.as_mut(), &|| false).unwrap();
    let (local, mut manifest) = (Local::default(), Manifest::default());
    let changes = plan(&c, &manifest, &[], &remote, Direction::Pull, &|_| None);
    let moved = AtomicUsize::new(0);
    let stop = std::sync::atomic::AtomicBool::new(false);
    let retry = crate::Retry { attempts: 3, delay: std::time::Duration::ZERO };
    let run = Run {
        workers: 1,
        retry,
        now: &|| 1,
        on_transfer: &|_| {},
        on_bytes: &|action, n| {
            assert_eq!(action, Action::Download);
            moved.fetch_add(n as usize, Ordering::Relaxed);
            // Stopped once the second file's bytes come.
            if moved.load(Ordering::Relaxed) > 5 {
                stop.store(true, Ordering::Relaxed);
            }
        },
        cancelled: &|| stop.load(Ordering::Relaxed),
        parallel: &threads,
        pause: &|_| {},
    };
    let done = apply(&changes, &server, &local, &mut manifest, &run);
    assert_eq!(moved.load(Ordering::Relaxed), 10);
    assert_eq!(done.len(), 2, "the third never started");
    assert!(done[0].error.is_none());
    assert_eq!(done[1].error.as_deref(), Some("cancelled"), "not tried again");
    assert_eq!(local.list().len(), 1, "the stopped file isn't written");
    assert_eq!(manifest.len(), 1);
}

/// The workers on threads at once, as the platform runs them (no waiting before retries).
fn threads(n: usize, work: &(dyn Fn() + Sync)) {
    std::thread::scope(|scope| {
        for _ in 0..n {
            scope.spawn(work);
        }
    });
}

/// A plan applied after a save here (a review left open): that file isn't written over, the
/// others go on.
#[test]
fn a_save_made_after_the_plan_is_never_written_over() {
    let server = Arc::new(Server::default());
    server.put("a.php", "a");
    server.put("b.php", "b");
    let local = Local::default();
    let mut manifest = Manifest::default();
    sync(&server, &local, &mut manifest, Direction::Pull);
    server.put("a.php", "a on the server");
    server.put("b.php", "b on the server");
    let c = connection();
    let mut session = server.connect().unwrap();
    let remote = list_remote(&c, session.as_mut(), &|| false).unwrap();
    let changes = plan(&c, &manifest, &local.list(), &remote, Direction::Sync, &|p| local.hash(p));
    // Saved here meanwhile.
    local.put("a.php", "a saved here");
    let failed = std::sync::Mutex::new(Vec::new());
    let retry = crate::Retry { attempts: 3, delay: std::time::Duration::ZERO };
    let run = Run {
        workers: 2,
        retry,
        now: &|| 1,
        on_transfer: &|t| {
            if t.error.is_some() {
                failed.lock().unwrap().push(t.path.clone());
            }
        },
        on_bytes: &|_, _| {},
        cancelled: &|| false,
        parallel: &threads,
        pause: &|_| {},
    };
    apply(&changes, &server, &local, &mut manifest, &run);
    assert_eq!(local.read("a.php").unwrap(), b"a saved here");
    assert_eq!(local.read("b.php").unwrap(), b"b on the server");
    assert_eq!(*failed.lock().unwrap(), vec!["a.php".to_owned()]);
}

#[test]
fn a_folder_is_measured_with_everything_under_it_and_can_be_stopped() {
    let server = Arc::new(Server::default());
    server.put("app/a.php", "12345");
    server.put("app/sub/b.php", "123");
    server.put("vendor/c.php", "1");
    let mut fs = Session(Arc::clone(&server));
    let mut told = Vec::new();
    let measured = measure(&mut fs, "app", &|| false, &mut |m| told.push(m)).unwrap();
    assert_eq!(measured, Measured { bytes: 8, files: 2, folders: 1 });
    assert_eq!(told.last(), Some(&measured));
    let all = measure(&mut fs, "", &|| false, &mut |_| {}).unwrap();
    assert_eq!((all.files, all.folders), (3, 3));
    assert_eq!(measure(&mut fs, "", &|| true, &mut |_| {}), Err(RemoteError::Cancelled));
}

#[test]
fn a_session_without_permissions_still_gives_what_its_listing_says() {
    let server = Arc::new(Server::default());
    server.put("a.php", "123");
    let mut fs = Session(Arc::clone(&server));
    let props = fs.properties(&["a.php".into(), "gone.php".into()]).unwrap();
    assert_eq!(props[0].as_ref().map(|p| (p.size, p.mode)), Some((3, None)));
    assert_eq!(props[1], None);
    assert_eq!(fs.capabilities(), crate::properties::Capabilities::default());
}

#[test]
fn properties_go_down_the_folders_x_on_folders_and_a_refusal_is_noted() {
    use crate::properties::{Mode, ModeEdit, Properties, PropertiesChange};
    let server = Arc::new(Server::default());
    server.put("app/a.php", "1");
    server.put("app/sub/b.php", "2");
    *server.locked.lock().unwrap() = Some("app/sub/b.php".into());
    let mut fs = Session(Arc::clone(&server));
    let app = Properties {
        path: "app".into(),
        is_dir: true,
        size: 0,
        modified: 0,
        mode: Some(Mode::new(0o755)),
        owner: None,
        group: None,
    };
    let mut edit = ModeEdit::of([Mode::new(0o755)]);
    assert!(edit.set_octal("644"));
    let change = PropertiesChange {
        mode: Some(edit),
        owner: Some(33),
        group: None,
        recursive: true,
        x_on_folders: true,
    };
    let mut told = 0;
    let applied =
        apply_properties(&mut fs, std::slice::from_ref(&app), &change, &|| false, &mut |n| {
            told = n
        })
        .unwrap();
    let sets = server.sets.lock().unwrap().clone();
    let mode_of = |p: &str| sets.iter().find(|(s, _)| s == p).and_then(|(_, set)| set.mode);
    // Folders keep X where readable (`app` already is so: only its owner set); files take
    // the mode as it is; the owner everywhere.
    assert_eq!(mode_of("app"), None);
    assert_eq!(mode_of("app/sub"), Some(Mode::new(0o755)));
    assert_eq!(mode_of("app/a.php"), Some(Mode::new(0o644)));
    assert!(sets.iter().all(|(_, s)| s.owner == Some(33)));
    assert_eq!(applied.failed.len(), 1);
    assert_eq!(applied.failed[0].0, "app/sub/b.php");
    assert_eq!((applied.seen, told), (4, 4));
    // Stopped: nothing more.
    server.sets.lock().unwrap().clear();
    let stopped = apply_properties(&mut fs, &[app], &change, &|| true, &mut |_| {}).unwrap();
    assert!(stopped.stopped && server.sets.lock().unwrap().is_empty());
}
