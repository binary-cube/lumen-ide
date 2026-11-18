// Two transfers at once need two threads of their own.
#![allow(clippy::disallowed_methods)]

use super::*;
use remote::application::Action;

#[test]
fn the_project_data_must_be_ignored_in_a_repository() {
    let dir = std::env::temp_dir().join(format!("remote-ignored-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    assert!(data_ignored(&dir), "no repository: nothing to warn about");
    // A fake repository (a folder named `.git`; nothing is run).
    std::fs::create_dir_all(dir.join(".git")).unwrap();
    assert!(!data_ignored(&dir));
    std::fs::write(dir.join(".gitignore"), "vendor/\n").unwrap();
    assert!(!data_ignored(&dir));
    std::fs::write(dir.join(".gitignore"), format!("vendor/\n/{}/\n", kernel::brand::PROJECT_DIR))
        .unwrap();
    assert!(data_ignored(&dir));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Against the demo servers (`EDITOR_REMOTE_DEMO=1`, `HOME` in a scratch folder): on
/// demand, the server's folders are listed as the tree asks (exclusions left out) and one
/// file comes down when opened — nothing else.
#[test]
#[ignore = "needs the demo servers"]
fn on_demand_lists_the_server_and_brings_one_file() {
    use remote::Protocol;
    use remote::application::Action;
    if std::env::var("EDITOR_REMOTE_DEMO").is_err() {
        return;
    }
    for (protocol, port, root) in [
        (Protocol::Sftp, 2222, "/www/shop"),
        (Protocol::Ftps { implicit: false }, 2121, "/home/demo/www/shop"),
    ] {
        let local = std::env::temp_dir().join(format!(
            "remote-on-demand-{}-{}",
            protocol.id(),
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&local);
        std::fs::create_dir_all(&local).unwrap();
        let saved = probe_saved(&local, protocol, port, root);
        let probe = connector(&saved, Some("demo123".into()), None);
        let accepted = match probe.connect() {
            Ok(_) => None,
            Err(remote::RemoteError::HostKey(f)) => Some(f),
            Err(e) => panic!("{protocol:?}: {e}"),
        };
        let state =
            State::detached(saved.clone(), connector(&saved, Some("demo123".into()), accepted));
        let mut session = None;
        // The root: listed, vendor left out; nothing on disk.
        assert!(state.listings.lock().unwrap().get("").is_none());
        state.list("", &mut session);
        let names: Vec<String> = state.known("", &local).into_iter().map(|e| e.name).collect();
        assert!(
            names.contains(&"app".to_owned()) && names.contains(&"composer.json".to_owned()),
            "{names:?}"
        );
        assert!(!names.contains(&"vendor".to_owned()), "excluded");
        state.list("app/Models", &mut session);
        let models: Vec<String> = state
            .known("app/Models", &local.join("app/Models"))
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert!(models.contains(&"User.php".to_owned()), "{models:?}");
        assert!(!local.join("app").exists(), "nothing downloaded for listing");
        // Opened: that file alone comes down.
        state.transfer("app/Models/User.php", Action::Download, &mut session).unwrap();
        assert!(local.join("app/Models/User.php").is_file());
        assert!(!local.join("composer.json").exists());
        let _ = std::fs::remove_dir_all(&local);
    }
}

/// Each file on disk is marked by where it stands against its last transfer: only here,
/// changed here, changed on the server, both; a folder the server hasn't is only here.
/// Once sent, the mark goes and the server's listing knows it.
#[test]
fn files_on_disk_are_marked_by_where_they_stand() {
    use remote::application::{Action, LocalFiles};
    use remote::manifest::Synced;
    use workspace::Mark;
    let local = std::env::temp_dir().join(format!("remote-marks-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&local);
    std::fs::create_dir_all(local.join("app")).unwrap();
    std::fs::create_dir_all(local.join("drafts")).unwrap();
    std::fs::create_dir_all(local.join("vendor")).unwrap();
    for name in ["same.txt", "mine.txt", "edited.txt", "newer.txt", "both.txt"] {
        std::fs::write(local.join(name), "hello").unwrap();
    }
    let saved = probe_saved(&local, remote::Protocol::Sftp, 1, "/www");
    let state = State::detached(saved.clone(), connector(&saved, None, None));
    let copy = local_copy(&local);
    let server =
        |path: &str, is_dir, modified| RemoteEntry { path: path.into(), is_dir, size: 5, modified };
    {
        let mut manifest = state.manifest.lock().unwrap();
        for name in ["same.txt", "edited.txt", "newer.txt", "both.txt"] {
            let here = copy.stat(name).unwrap();
            manifest.set(
                name,
                Synced {
                    remote_size: 5,
                    remote_modified: 100,
                    local_size: here.size,
                    local_modified: here.modified,
                    local_hash: remote::content_hash(b"hello"),
                },
            );
        }
    }
    std::fs::write(local.join("edited.txt"), "hello, again").unwrap();
    std::fs::write(local.join("both.txt"), "hello, again").unwrap();
    state.listings.lock().unwrap().insert(
        String::new(),
        vec![
            server("app", true, 0),
            server("only-there.txt", false, 100),
            server("same.txt", false, 100),
            server("edited.txt", false, 100),
            server("newer.txt", false, 200),
            server("both.txt", false, 200),
        ],
    );
    let marks = |state: &State| -> Vec<(String, Mark)> {
        let mut marks: Vec<_> =
            state.known("", &local).into_iter().map(|e| (e.name, e.mark)).collect();
        marks.sort_by(|a, b| a.0.cmp(&b.0));
        marks
    };
    let expected = |mine| {
        vec![
            ("app".to_owned(), Mark::None),
            ("both.txt".to_owned(), Mark::Conflict),
            ("drafts".to_owned(), Mark::Local),
            ("edited.txt".to_owned(), Mark::Changed),
            ("mine.txt".to_owned(), mine),
            ("newer.txt".to_owned(), Mark::Behind),
            ("only-there.txt".to_owned(), Mark::None),
            ("same.txt".to_owned(), Mark::None),
        ]
    };
    // `vendor` is excluded: never marked.
    assert_eq!(marks(&state), expected(Mark::Local));
    // Sent: the server knows it, nothing to mark.
    let mut manifest = state.manifest.lock().unwrap().clone();
    let here = copy.stat("mine.txt").unwrap();
    manifest.set(
        "mine.txt",
        Synced {
            remote_size: 5,
            remote_modified: 300,
            local_size: here.size,
            local_modified: here.modified,
            local_hash: remote::content_hash(b"hello"),
        },
    );
    let sent =
        Transfer { path: "mine.txt".into(), action: Action::Upload, bytes: 5, at: 0, error: None };
    state.keep(manifest, &[sent]);
    assert_eq!(marks(&state), expected(Mark::None));
    assert_eq!(state.relisted.lock().unwrap().as_slice(), [String::new()]);
    let _ = std::fs::remove_dir_all(&local);
}

/// The demo connection, again (for the connector with the accepted key).
fn probe_saved(local: &Path, protocol: remote::Protocol, port: u16, root: &str) -> Saved {
    use remote::{Auth, Connection, Mode, SavedIn, SecretsIn};
    Saved {
        connection: Connection {
            id: "demo".into(),
            name: "Demo".into(),
            protocol,
            host: "127.0.0.1".into(),
            port,
            user: "demo".into(),
            auth: Auth::Password,
            root: root.into(),
            mode: Mode::OnDemand,
            excluded: vec!["vendor".into()],
            saved_in: SavedIn::Project,
            secrets_in: SecretsIn::Keychain,
            retry: None,
            uploads: None,
            downloads: None,
            moves: false,
            auto_upload: true,
            write_as: Default::default(),
        },
        local: local.to_path_buf(),
        trusted: Vec::new(),
    }
}

/// A server in memory (path → bytes and when it was written), counting its sessions and
/// writes; it can go offline, and its writes can take a while (transfers overlapping).
#[derive(Default)]
struct Server {
    files: Mutex<std::collections::BTreeMap<String, (Vec<u8>, i64)>>,
    clock: std::sync::atomic::AtomicI64,
    sessions: std::sync::atomic::AtomicUsize,
    writes: std::sync::atomic::AtomicUsize,
    offline: std::sync::atomic::AtomicBool,
    write_ms: std::sync::atomic::AtomicU64,
    reads: std::sync::atomic::AtomicUsize,
    /// Writes going on now, and the most at once.
    working: std::sync::atomic::AtomicUsize,
    most: std::sync::atomic::AtomicUsize,
    /// Connections it takes at once (0: any; more are reset, as FTP servers limiting an
    /// address do), those open now and the most at once.
    limit: std::sync::atomic::AtomicUsize,
    live: std::sync::atomic::AtomicUsize,
    most_live: std::sync::atomic::AtomicUsize,
    /// Reads reset mid-way before they work again.
    resets: std::sync::atomic::AtomicUsize,
    /// What it was asked to remove (files and folders), in order.
    removed: Mutex<Vec<String>>,
}

impl Server {
    fn put(&self, path: &str, text: &str) {
        let at = self.clock.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1000;
        self.files.lock().unwrap().insert(path.into(), (text.as_bytes().to_vec(), at));
    }
    fn text(&self, path: &str) -> Option<String> {
        self.files.lock().unwrap().get(path).map(|(b, _)| String::from_utf8_lossy(b).into_owned())
    }
    fn down(&self) -> Result<(), remote::RemoteError> {
        if self.offline.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(remote::RemoteError::Connect("no route to host".into()));
        }
        Ok(())
    }
}

struct FakeSession(Arc<Server>);

impl Drop for FakeSession {
    fn drop(&mut self) {
        self.0.live.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// What a server limiting an address answers to one connection too many.
const RESET: &str = "Connection reset by peer (os error 54)";

impl remote::application::RemoteFs for FakeSession {
    fn list(&mut self, dir: &str) -> Result<Vec<RemoteEntry>, remote::RemoteError> {
        self.0.down()?;
        let prefix = if dir.is_empty() { String::new() } else { format!("{dir}/") };
        let files = self.0.files.lock().unwrap();
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
        // As a real server: a folder it doesn't have isn't there.
        if !dir.is_empty() && out.is_empty() {
            return Err(remote::RemoteError::NotFound(dir.into()));
        }
        Ok(out)
    }
    fn read(&mut self, path: &str) -> Result<Vec<u8>, remote::RemoteError> {
        use std::sync::atomic::Ordering::Relaxed;
        self.0.down()?;
        self.0.reads.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let resets = &self.0.resets;
        if resets.try_update(Relaxed, Relaxed, |n| n.checked_sub(1)).is_ok() {
            return Err(remote::RemoteError::Connect(RESET.into()));
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        let files = self.0.files.lock().unwrap();
        files.get(path).map(|(b, _)| b.clone()).ok_or(remote::RemoteError::NotFound(path.into()))
    }
    fn write(&mut self, path: &str, data: &[u8]) -> Result<RemoteEntry, remote::RemoteError> {
        self.0.down()?;
        use std::sync::atomic::Ordering::Relaxed;
        let now = self.0.working.fetch_add(1, Relaxed) + 1;
        self.0.most.fetch_max(now, Relaxed);
        let ms = self.0.write_ms.load(Relaxed);
        std::thread::sleep(std::time::Duration::from_millis(ms));
        self.0.working.fetch_sub(1, Relaxed);
        self.0.writes.fetch_add(1, Relaxed);
        self.0.put(path, &String::from_utf8_lossy(data));
        self.stat(path).map(|e| e.unwrap())
    }
    fn remove(&mut self, path: &str) -> Result<(), remote::RemoteError> {
        self.0.down()?;
        self.0.removed.lock().unwrap().push(path.to_owned());
        self.0.files.lock().unwrap().remove(path);
        Ok(())
    }
    fn rename(&mut self, from: &str, to: &str) -> Result<(), remote::RemoteError> {
        self.0.down()?;
        let mut files = self.0.files.lock().unwrap();
        // A file, or a folder (everything under it), as servers do.
        let folder = format!("{from}/");
        let moved: Vec<String> =
            files.keys().filter(|k| *k == from || k.starts_with(&folder)).cloned().collect();
        if moved.is_empty() {
            return Err(remote::RemoteError::NotFound(from.into()));
        }
        for old in moved {
            let file = files.remove(&old).unwrap();
            files.insert(format!("{to}{}", &old[from.len()..]), file);
        }
        Ok(())
    }
    fn stat(&mut self, path: &str) -> Result<Option<RemoteEntry>, remote::RemoteError> {
        self.0.down()?;
        Ok(self.0.files.lock().unwrap().get(path).map(|(b, m)| RemoteEntry {
            path: path.into(),
            is_dir: false,
            size: b.len() as u64,
            modified: *m,
        }))
    }
}

struct FakeConnector(Arc<Server>);

impl remote::application::Connector for FakeConnector {
    fn connect(&self) -> Result<Box<dyn remote::application::RemoteFs>, remote::RemoteError> {
        use std::sync::atomic::Ordering::Relaxed;
        self.0.down()?;
        let limit = self.0.limit.load(Relaxed);
        let taken = self
            .0
            .live
            .try_update(Relaxed, Relaxed, |n| (limit == 0 || n < limit).then_some(n + 1));
        let Ok(before) = taken else {
            return Err(remote::RemoteError::Connect(RESET.into()));
        };
        self.0.most_live.fetch_max(before + 1, Relaxed);
        self.0.sessions.fetch_add(1, Relaxed);
        Ok(Box::new(FakeSession(Arc::clone(&self.0))))
    }
}

/// A remote project in a scratch folder (mirrored or on demand), its session started on the
/// fake server as the project's service starts it: on the project's own events.
struct Project {
    /// Its identity (its session goes by it).
    id: kernel::ProjectId,
    root: PathBuf,
    server: Arc<Server>,
    events: kernel::EventBus,
    subscriptions: Vec<kernel::Subscription>,
}

impl Project {
    fn new(name: &str, mode: remote::Mode) -> Self {
        Self::with(name, mode, |_| {})
    }

    /// Its connection as `adjust` says (its own limits…).
    fn with(name: &str, mode: remote::Mode, adjust: impl FnOnce(&mut Saved)) -> Self {
        let root = std::env::temp_dir().join(format!("remote-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let mut saved = probe_saved(&root, remote::Protocol::Sftp, 22, "/www");
        saved.connection.mode = mode;
        // Retries quick (the tests don't wait seconds).
        saved.connection.retry =
            Some(remote::Retry { attempts: 2, delay: std::time::Duration::from_millis(5) });
        adjust(&mut saved);
        let server = Arc::new(Server::default());
        let events = kernel::EventBus::new();
        let id = kernel::ProjectId::new();
        let context = workbench_model::ProjectContext {
            marks: Default::default(),
            id,
            root: &root,
            storage: &root,
            events: &events,
            wake: Arc::new(|| {}),
            settings: None,
            language_of: std::sync::Arc::new(|_: &std::path::Path| None),
            quality_tools: Vec::new(),
            text_formatters: Vec::new(),
            notifier: std::sync::Arc::new(workbench_model::NoNotifier),
        };
        let subscriptions = start(&context, saved, Arc::new(FakeConnector(Arc::clone(&server))));
        Self { id, root, server, events, subscriptions }
    }

    fn state(&self) -> Arc<State> {
        session(self.id).expect("the project's session")
    }

    /// `name` written here and saved (as the app announces a save).
    fn save(&self, name: &str, text: &str) {
        std::fs::write(self.root.join(name), text).unwrap();
        self.events.publish(&document::events::FileSaved { path: self.root.join(name) });
    }

    fn wait(&self, done: impl Fn(&Self) -> bool) -> bool {
        let started = std::time::Instant::now();
        while started.elapsed() < std::time::Duration::from_secs(5) {
            if done(self) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        false
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        self.subscriptions.clear();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A saved file goes up (the server checked first): there, in the manifest, in the log.
#[test]
fn a_saved_file_is_uploaded_and_remembered() {
    let project = Project::new("upload", remote::Mode::Mirror);
    project.save("index.php", "<?php echo 1;");
    assert!(project.wait(|p| p.server.text("index.php").as_deref() == Some("<?php echo 1;")));
    let state = project.state();
    assert!(project.wait(|_| state.manifest.lock().unwrap().get("index.php").is_some()));
    assert!(state.log.lock().unwrap().iter().any(|t| t.path == "index.php" && t.error.is_none()));
    // Excluded and the project's own data never go.
    std::fs::create_dir_all(project.root.join("vendor")).unwrap();
    project.save("vendor/lib.php", "x");
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert!(project.server.text("vendor/lib.php").is_none());
}

/// Changed on the server since the last transfer: not uploaded — the user is asked.
#[test]
fn a_save_over_a_change_on_the_server_is_a_conflict() {
    let project = Project::new("conflict", remote::Mode::Mirror);
    project.save("a.txt", "one");
    assert!(project.wait(|p| p.server.text("a.txt").as_deref() == Some("one")));
    let state = project.state();
    assert!(project.wait(|_| state.manifest.lock().unwrap().get("a.txt").is_some()));
    project.server.put("a.txt", "theirs");
    project.save("a.txt", "mine");
    assert!(project.wait(|_| {
        state.inbox.lock().unwrap().iter().any(|n| matches!(n, Notice::Conflict { .. }))
    }));
    assert_eq!(project.server.text("a.txt").as_deref(), Some("theirs"), "nothing overwritten");
}

/// Offline: the user is told (today the upload isn't kept for later — 16c keeps it).
#[test]
fn a_save_while_offline_is_told() {
    let project = Project::new("offline", remote::Mode::Mirror);
    project.server.offline.store(true, std::sync::atomic::Ordering::Relaxed);
    project.save("a.txt", "one");
    let state = project.state();
    assert!(project.wait(|_| !state.inbox.lock().unwrap().is_empty()));
    assert!(project.server.text("a.txt").is_none());
}

/// On demand: a file opened is brought from the server (alone).
#[test]
fn on_demand_a_file_opened_is_brought() {
    use workbench_model::ProjectService;
    let project = Project::new("fetch", remote::Mode::OnDemand);
    project.server.put("app/User.php", "<?php class User {}");
    project.server.put("app/Other.php", "<?php class Other {}");
    let done = RemoteService.fetch(&project.root.join("app/User.php")).expect("fetched");
    assert_eq!(done.recv_timeout(std::time::Duration::from_secs(5)).unwrap(), Ok(()));
    assert!(project.root.join("app/User.php").is_file());
    assert!(!project.root.join("app/Other.php").exists());
}

/// A project's session ends with it: its thread, its state (they used to stay forever).
#[test]
fn the_session_ends_with_its_project() {
    let project = Project::new("ends", remote::Mode::Mirror);
    let weak = Arc::downgrade(&project.state());
    drop(project);
    let started = std::time::Instant::now();
    while weak.strong_count() > 0 && started.elapsed() < std::time::Duration::from_secs(5) {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(weak.strong_count(), 0, "the session's state is still held");
}

/// Two transfers at once keep both in the manifest (the last one's copy used to overwrite
/// the other's).
#[test]
fn two_transfers_at_once_both_stay_in_the_manifest() {
    let project = Project::new("manifest", remote::Mode::Mirror);
    project.server.write_ms.store(100, std::sync::atomic::Ordering::Relaxed);
    std::fs::write(project.root.join("a.txt"), "a").unwrap();
    std::fs::write(project.root.join("b.txt"), "b").unwrap();
    let state = project.state();
    let (one, two) = (Arc::clone(&state), Arc::clone(&state));
    let a = std::thread::spawn(move || one.transfer("a.txt", Action::Upload, &mut None));
    let b = std::thread::spawn(move || two.transfer("b.txt", Action::Upload, &mut None));
    assert_eq!(a.join().unwrap(), Ok(()));
    assert_eq!(b.join().unwrap(), Ok(()));
    let manifest = state.manifest.lock().unwrap();
    assert!(manifest.get("a.txt").is_some() && manifest.get("b.txt").is_some());
}

/// Saved five times before it went: uploaded once.
#[test]
fn a_file_saved_again_before_it_went_is_uploaded_once() {
    let project = Project::new("merged", remote::Mode::Mirror);
    for i in 0..5 {
        project.save("a.txt", &format!("version {i}"));
    }
    assert!(project.wait(|p| p.server.text("a.txt").as_deref() == Some("version 4")));
    std::thread::sleep(std::time::Duration::from_millis(400));
    assert_eq!(project.server.writes.load(std::sync::atomic::Ordering::Relaxed), 1);
}

/// Many files saved: at most two uploading at once, the connections kept and shared.
#[test]
fn uploads_run_two_at_once_on_kept_connections() {
    use std::sync::atomic::Ordering::Relaxed;
    let project = Project::new("parallel", remote::Mode::Mirror);
    project.server.write_ms.store(40, Relaxed);
    for i in 0..8 {
        project.save(&format!("f{i}.txt"), "x");
    }
    assert!(project.wait(|p| p.server.writes.load(Relaxed) == 8));
    assert!(project.server.most.load(Relaxed) >= 2, "in parallel");
    let limits = project.state().limits();
    assert!(project.server.most.load(Relaxed) <= limits.uploads, "never more than allowed");
    let connections = project.server.sessions.load(Relaxed);
    let most = limits.uploads + limits.downloads;
    assert!(connections <= most, "kept and shared: {connections} connections");
    // Another round: the kept connections serve it.
    for i in 0..4 {
        project.save(&format!("g{i}.txt"), "y");
    }
    assert!(project.wait(|p| p.server.writes.load(Relaxed) == 12));
    assert_eq!(project.server.sessions.load(Relaxed), connections, "no new handshake");
}

/// Offline: what was saved stays to upload (the user told once) and goes when the server
/// is back.
#[test]
fn saved_while_offline_goes_when_the_server_is_back() {
    use std::sync::atomic::Ordering::Relaxed;
    let project = Project::new("back", remote::Mode::Mirror);
    project.server.offline.store(true, Relaxed);
    project.save("a.txt", "one");
    project.save("b.txt", "two");
    let state = project.state();
    assert!(project.wait(|_| state.to_upload().len() == 2));
    let told = state.inbox.lock().unwrap().len();
    assert_eq!(told, 1, "told once that the server is out of reach");
    project.server.offline.store(false, Relaxed);
    assert!(
        project.wait(|p| { p.server.text("a.txt").is_some() && p.server.text("b.txt").is_some() })
    );
    assert!(project.wait(|_| state.to_upload().is_empty()));
}

/// A paste of many files (a batch of changes): nothing goes until it ends, then all at once.
#[test]
fn a_batch_of_changes_goes_up_when_it_ends() {
    use std::sync::atomic::Ordering::Relaxed;
    let project = Project::new("batch", remote::Mode::Mirror);
    project.events.publish(&workspace::events::BulkChangeStarted { id: 3 });
    for i in 0..3 {
        project.save(&format!("p{i}.txt"), "z");
    }
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert_eq!(project.server.writes.load(Relaxed), 0, "held while the paste runs");
    project.events.publish(&workspace::events::BulkChangeEnded { id: 3 });
    assert!(project.wait(|p| p.server.writes.load(Relaxed) == 3));
}

/// On demand, a file asked twice while it comes: brought once, both told.
#[test]
fn a_file_asked_twice_is_brought_once() {
    use std::sync::atomic::Ordering::Relaxed;
    use workbench_model::ProjectService;
    let project = Project::new("twice", remote::Mode::OnDemand);
    project.server.put("app/User.php", "<?php class User {}");
    let path = project.root.join("app/User.php");
    let first = RemoteService.fetch(&path).unwrap();
    let second = RemoteService.fetch(&path).unwrap();
    let wait = std::time::Duration::from_secs(5);
    assert_eq!(first.recv_timeout(wait).unwrap(), Ok(()));
    assert_eq!(second.recv_timeout(wait).unwrap(), Ok(()));
    assert_eq!(project.server.reads.load(Relaxed), 1);
}

/// What's transferred, what runs and waits: announced on the project's events (whoever
/// shows it listens), as it happens.
#[test]
fn transfers_are_announced_on_the_project_s_events() {
    use std::sync::atomic::Ordering::Relaxed;
    let project = Project::new("events", remote::Mode::Mirror);
    let heard: Arc<Mutex<Vec<String>>> = Arc::default();
    let (finished, changed) = (Arc::clone(&heard), Arc::clone(&heard));
    let _f = project.events.subscribe(move |e: &remote::events::TransferFinished| {
        finished.lock().unwrap().push(format!("{} {:?} {:?}", e.path, e.action, e.error));
    });
    let _c = project.events.subscribe(move |e: &remote::events::TransfersChanged| {
        changed.lock().unwrap().push(format!("running {} waiting {}", e.running, e.to_upload));
    });
    project.save("a.txt", "one");
    assert!(
        project.wait(|_| heard.lock().unwrap().iter().any(|h| h.starts_with("a.txt Upload None")))
    );
    assert!(
        project.wait(
            |_| heard.lock().unwrap().last().map(String::as_str) == Some("running 0 waiting 0")
        )
    );
    assert!(heard.lock().unwrap().contains(&"running 1 waiting 0".to_owned()), "{heard:?}");
    // Offline: one waits.
    project.server.offline.store(true, Relaxed);
    project.save("b.txt", "two");
    assert!(project.wait(|_| heard.lock().unwrap().contains(&"running 0 waiting 1".to_owned())));
    assert_eq!(project.state().now().to_upload, 1);
}

/// A project's own limit (one upload at a time: a server allowing one connection) is kept.
#[test]
fn a_project_s_own_limit_is_kept() {
    use std::sync::atomic::Ordering::Relaxed;
    let project = Project::with("one", remote::Mode::Mirror, |s| s.connection.uploads = Some(1));
    project.server.write_ms.store(30, Relaxed);
    for i in 0..4 {
        project.save(&format!("f{i}.txt"), "x");
    }
    assert!(project.wait(|p| p.server.writes.load(Relaxed) == 4));
    assert_eq!(project.server.most.load(Relaxed), 1);
}

/// Moves follow (the connection says so): a file moved here is moved on the server, the
/// manifest following; deleted here, the user is asked — yes: deleted there too. Off (the
/// default): nothing happens on the server.
#[test]
fn moves_and_deletions_follow_when_the_connection_says_so() {
    let project = Project::with("moves", remote::Mode::Mirror, |s| s.connection.moves = true);
    std::fs::create_dir_all(project.root.join("app")).unwrap();
    project.save("app/a.txt", "a");
    project.save("app/b.txt", "b");
    let state = project.state();
    assert!(project.wait(|_| state.sent_under("app").len() == 2));
    // Moved here (the folder): on the server too.
    std::fs::rename(project.root.join("app"), project.root.join("src")).unwrap();
    project.events.publish(&workspace::events::FileRenamed {
        from: project.root.join("app"),
        to: project.root.join("src"),
    });
    assert!(
        project
            .wait(|p| p.server.text("src/a.txt").is_some() && p.server.text("app/a.txt").is_none())
    );
    assert!(
        project.wait(|_| state.sent_under("src").len() == 2 && state.sent_under("app").is_empty())
    );
    // Deleted here: asked first.
    std::fs::remove_file(project.root.join("src/a.txt")).unwrap();
    project.events.publish(&workspace::events::EntriesDeleted {
        paths: vec![project.root.join("src/a.txt")],
    });
    assert!(project.wait(|_| {
        state
            .inbox
            .lock()
            .unwrap()
            .iter()
            .any(|n| matches!(n, Notice::ConfirmRemove(p) if p == &["src/a.txt".to_owned()]))
    }));
    assert!(project.server.text("src/a.txt").is_some(), "nothing before the answer");
    // Yes.
    state.send_job(Job::Remove(vec!["src/a.txt".into()]));
    assert!(project.wait(|p| p.server.text("src/a.txt").is_none()));
    assert!(project.server.text("src/b.txt").is_some());
    assert!(state.sent_under("src/a.txt").is_empty());
}

#[test]
fn without_the_option_moves_stay_here() {
    let project = Project::new("stay", remote::Mode::Mirror);
    project.save("a.txt", "a");
    let state = project.state();
    assert!(project.wait(|_| !state.sent_under("a.txt").is_empty()));
    std::fs::rename(project.root.join("a.txt"), project.root.join("b.txt")).unwrap();
    project.events.publish(&workspace::events::FileRenamed {
        from: project.root.join("a.txt"),
        to: project.root.join("b.txt"),
    });
    project
        .events
        .publish(&workspace::events::EntriesDeleted { paths: vec![project.root.join("a.txt")] });
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(project.server.text("a.txt").is_some());
    assert!(!state.inbox.lock().unwrap().iter().any(|n| matches!(n, Notice::ConfirmRemove(_))));
}

/// What's to upload: what couldn't go (and why), what's changed here since it was sent,
/// what was made here and never sent — not what the server has as it is, nor what's
/// excluded. "Upload" sends it (through the queue).
#[test]
fn what_s_to_upload_is_listed_and_uploaded() {
    use crate::outgoing::{Why, rows};
    use std::sync::atomic::Ordering::Relaxed;
    let project = Project::new("pending", remote::Mode::Mirror);
    project.save("same.txt", "sent");
    project.save("edited.txt", "sent");
    let state = project.state();
    assert!(
        project.wait(|_| state.sent_under("same.txt").len() == 1
            && state.sent_under("edited.txt").len() == 1)
    );
    // Changed by another program (its time a few seconds on: told apart from the send's
    // without waiting for the clock), made here, excluded.
    std::fs::write(project.root.join("edited.txt"), "changed elsewhere").unwrap();
    std::fs::File::options()
        .write(true)
        .open(project.root.join("edited.txt"))
        .unwrap()
        .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(3))
        .unwrap();
    std::fs::write(project.root.join("new.txt"), "new").unwrap();
    std::fs::create_dir_all(project.root.join("vendor")).unwrap();
    std::fs::write(project.root.join("vendor/lib.php"), "x").unwrap();
    // And one that couldn't go.
    project.server.offline.store(true, Relaxed);
    project.save("offline.txt", "o");
    assert!(project.wait(|_| state.to_upload().contains_key("offline.txt")));
    let listed = rows(&state);
    let names: Vec<(&str, &Why)> = listed.iter().map(|(p, w)| (p.as_str(), w)).collect();
    assert!(
        names.iter().any(|(p, w)| *p == "offline.txt" && matches!(w, Why::Waiting(_))),
        "{names:?}"
    );
    assert!(names.contains(&("edited.txt", &Why::Changed)), "{names:?}");
    assert!(names.contains(&("new.txt", &Why::New)), "{names:?}");
    assert!(!names.iter().any(|(p, _)| *p == "same.txt" || p.starts_with("vendor")), "{names:?}");
    // The button counts the same list, read again as the transfers moved (one couldn't go).
    let counted = |n: usize, failed: bool| {
        let kind = if failed {
            workbench_model::BadgeKind::Error
        } else {
            workbench_model::BadgeKind::Count
        };
        state.outgoing.summary() == Some((n, kind))
    };
    assert!(project.wait(|_| counted(listed.len(), true)), "{:?}", state.outgoing.summary());
    // Upload all (the server back).
    project.server.offline.store(false, Relaxed);
    state.upload_now(listed.into_iter().map(|(p, _)| p).collect());
    // Asked while a retry may be waiting in the queue: nothing of it is lost.
    let up = project.wait(|p| {
        p.server.text("new.txt").as_deref() == Some("new")
            && p.server.text("edited.txt").as_deref() == Some("changed elsewhere")
            && p.server.text("offline.txt").is_some()
    });
    assert!(
        up,
        "new {:?} edited {:?} offline {:?}; pending {:?}; notices {:?}",
        project.server.text("new.txt"),
        project.server.text("edited.txt"),
        project.server.text("offline.txt"),
        state.to_upload(),
        state.inbox.lock().unwrap()
    );
    assert!(project.wait(|_| rows(&state).is_empty()));
    assert!(project.wait(|_| counted(0, false)), "{:?}", state.outgoing.summary());
}

/// For the Task Manager: a save going up is an operation of its project (by identity),
/// counted in bytes, ended; the disk here read as much; the project's part says so.
#[test]
fn an_upload_is_an_operation_of_its_project() {
    use workbench_model::{ActivityKind, ActivityUnit, Outcome, ResourceReporter};
    let project = Project::new("operation", remote::Mode::Mirror);
    let text = "<?php echo 'counted';";
    project.save("index.php", text);
    assert!(project.wait(|p| p.server.text("index.php").as_deref() == Some(text)));
    let state = project.state();
    assert!(project.wait(|_| {
        let mut part = workbench_model::ResourceUse::new("", "");
        part.project = Some(project.id);
        state.activities.report(&mut part);
        part.activities.iter().any(|a| !a.running())
    }));
    let parts = RemoteResources.report();
    let part = parts.iter().find(|p| p.project == Some(project.id)).expect("its part");
    let upload = part
        .activities
        .iter()
        .find(|a| a.kind == ActivityKind::Upload && a.subject == "index.php")
        .expect("the upload");
    assert_eq!(upload.unit, ActivityUnit::Bytes);
    assert_eq!(upload.ended.as_ref().map(|(_, o)| o.clone()), Some(Outcome::Done));
    assert_eq!(upload.done, text.len() as u64, "its bytes, as they passed");
    assert!(part.read.unwrap() >= text.len() as u64, "read here to go up");
}

/// For the Task Manager: a transfer running can be stopped — its part offers "Stop", its
/// owner sets its flag (another project's never), and it ends Stopped.
#[test]
fn a_transfer_is_stopped_from_the_task_manager() {
    use std::sync::atomic::Ordering::Relaxed;
    use workbench_model::{ActOutcome, ActionTarget, Outcome, PartRef, ResourceReporter};
    let project = Project::new("stop", remote::Mode::Mirror);
    let state = project.state();
    let operation = state.begin(&[]);
    let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
    state.stoppable(operation, Arc::clone(&flag));
    let parts = RemoteResources.report();
    let part = parts.iter().find(|p| p.project == Some(project.id)).expect("its part");
    let stop = part.actions.first().copied().expect("its Stop");
    let named =
        PartRef { owner: part.owner.clone(), name: part.name.clone(), project: part.project };
    let elsewhere = PartRef { project: Some(kernel::ProjectId::new()), ..named.clone() };
    assert_eq!(RemoteResources.act(&elsewhere, stop.id, stop.target), ActOutcome::Unsupported);
    assert!(!flag.load(Relaxed));
    assert_eq!(RemoteResources.act(&named, stop.id, stop.target), ActOutcome::Done);
    assert!(flag.load(Relaxed), "told to stop");
    state.finish(operation, &[], flag.load(Relaxed));
    let parts = RemoteResources.report();
    let part = parts.iter().find(|p| p.project == Some(project.id)).unwrap();
    let ActionTarget::Activity(id) = stop.target else { unreachable!() };
    let ended = part.activities.iter().find(|a| a.id == id).unwrap();
    assert_eq!(ended.ended.as_ref().map(|(_, o)| o.clone()), Some(Outcome::Stopped));
    assert!(part.actions.is_empty(), "ended: nothing to stop");
}

/// Before closing: a sync running is its project's work, lost if cut, so asked about and
/// stopped through its "Stop" (the Task Manager's); once it ends, nothing is asked.
#[test]
fn a_sync_running_is_asked_about_before_closing_its_project() {
    use std::sync::atomic::Ordering::Relaxed;
    use workbench_model::{ActOutcome, ResourceReporter, StopImpact};
    let project = Project::new("closing", remote::Mode::Mirror);
    let state = project.state();
    let operation = state.begin(&[]);
    let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
    state.stoppable(operation, Arc::clone(&flag));
    let running: Vec<_> =
        RemoteResources.running().into_iter().filter(|w| w.project == Some(project.id)).collect();
    assert_eq!(running.len(), 1);
    assert_eq!(running[0].impact, StopImpact::LosesWork);
    let stop = running[0].stop.as_ref().expect("stoppable");
    assert_eq!(RemoteResources.act(&stop.part, stop.action, stop.target), ActOutcome::Done);
    assert!(flag.load(Relaxed), "told to stop");
    state.finish(operation, &[], true);
    assert!(!RemoteResources.running().iter().any(|w| w.project == Some(project.id)));
}

/// Its connection edited: the session starts again on the same project with the new
/// settings — the old one ends (its state, its connections), saves go to the new server.
#[test]
fn an_edited_connection_reconnects_the_open_project() {
    let project = Project::new("reconnect", remote::Mode::Mirror);
    let old = Arc::downgrade(&project.state());
    let mut edited = project.state().saved.clone();
    edited.connection.port = 2222;
    edited.connection.host = "10.0.0.7".into();
    let moved = Arc::new(Server::default());
    assert!(reconnect::restart(project.id, edited, Arc::new(FakeConnector(Arc::clone(&moved)))));
    assert!(project.wait(|_| old.strong_count() == 0), "the old session ended");
    let state = project.state();
    assert_eq!(
        (state.saved.connection.host.as_str(), state.saved.connection.port),
        ("10.0.0.7", 2222)
    );
    project.save("index.php", "<?php echo 2;");
    assert!(project.wait(|_| moved.text("index.php").as_deref() == Some("<?php echo 2;")));
    assert!(project.server.text("index.php").is_none(), "nothing to the old server");
    // Waiting for the window to reconnect it: only while the project is open.
    assert!(reconnect_later(state.saved.clone(), None));
    let elsewhere = probe_saved(Path::new("/nowhere/open"), remote::Protocol::Sftp, 22, "/");
    assert!(!reconnect_later(elsewhere, None));
    // Closed: nothing to restart.
    let id = project.id;
    let saved = state.saved.clone();
    drop(state);
    drop(project);
    assert!(!reconnect::restart(id, saved, Arc::new(FakeConnector(moved))));
}

/// The window reconnects an edited project after asking about its transfers still running
/// (the running-work question), and only once answered.
#[test]
fn reconnecting_asks_about_the_transfers_first() {
    let project = Project::new("reconnect-ask", remote::Mode::Mirror);
    let mut edited = project.state().saved.clone();
    edited.connection.port = 2200;
    assert!(reconnect_later(edited, None));
    let mut host = crate::tests::Host::at(&project.root, project.id);
    use workbench_model::ProjectService;
    assert!(RemoteService.poll(&mut host));
    let (asked, owner) = host.asked.take().expect("asked first");
    assert_eq!((asked, owner.as_str()), (project.id, crate::MODULE_ID));
    assert_eq!(project.state().saved.connection.port, 22, "not before the answer");
    // Answered (stopped / waited for): reconnected with the edit, the tree read again.
    let then = host.then.take().unwrap();
    then(&mut host);
    assert_eq!(project.state().saved.connection.port, 2200);
    assert_eq!(host.relisted, std::slice::from_ref(&project.root));
}

/// The whole project pulled from an FTP server taking two connections at once (the
/// transfers asking for more): its folders listed on one session (filling the tree as they
/// come), its files brought — none failed, never more than two connections; the limit
/// lowered and said once.
#[test]
fn a_pull_completes_on_a_server_taking_two_connections() {
    use remote::application::{Direction, LocalFiles, Run, apply, list_remote_under, plan};
    use std::sync::atomic::Ordering::Relaxed;
    let project = Project::with("ftp-limit", remote::Mode::OnDemand, |saved| {
        saved.connection.protocol = remote::Protocol::Ftp;
        saved.connection.host = format!("limited-{}", std::process::id());
        saved.connection.downloads = Some(4);
    });
    project.server.limit.store(2, Relaxed);
    for i in 0..12 {
        project.server.put(&format!("app/f{i}.php"), "<?php");
    }
    project.server.put("index.php", "<?php");
    let state = project.state();
    // The review's listing: the tree knows the root (and `app`) once listed.
    let learn = |dir: &str, entries: &[RemoteEntry]| state.learn_listing(dir, entries);
    let mut fs =
        crate::pool::Resilient::new(state.connector.as_ref(), state.retry()).listing_to(&learn);
    let remote = list_remote_under(&state.saved.connection, &mut fs, "", &|| false).unwrap();
    drop(fs);
    assert_eq!(remote.len(), 13);
    let names: Vec<String> = state.known("", &project.root).into_iter().map(|e| e.name).collect();
    assert!(
        names.contains(&"app".to_owned()) && names.contains(&"index.php".to_owned()),
        "{names:?}"
    );
    // The pull: four workers, two connections.
    let local = local_copy(&project.root);
    let manifest = state.manifest.lock().unwrap().clone();
    let changes =
        plan(&state.saved.connection, &manifest, &local.list(), &remote, Direction::Pull, &|_| {
            None
        });
    let mut manifest = manifest;
    let run = Run {
        workers: 4,
        retry: state.retry(),
        now: &now,
        on_transfer: &|_| {},
        on_bytes: &|_, _| {},
        cancelled: &|| false,
        parallel: &threads,
        pause: &std::thread::sleep,
    };
    let done = apply(&changes, state.connector.as_ref(), &local, &mut manifest, &run);
    let failed: Vec<_> = done.iter().filter_map(|t| t.error.clone()).collect();
    assert!(failed.is_empty(), "{failed:?}");
    assert_eq!(done.len(), 13);
    assert!(project.server.most_live.load(Relaxed) <= 2);
    let pool = state.pool.as_ref().unwrap();
    assert_eq!(pool.limit(), 2);
    assert!(pool.take_lowered().is_none() || pool.limit() == 2, "said at most once");
    assert!(pool.take_lowered().is_none());
}

/// A read reset mid-way (the server dropped the data connection): tried again on a new
/// session, it comes; the review's listing too.
#[test]
fn a_reset_mid_transfer_is_tried_again() {
    use remote::application::Action;
    use std::sync::atomic::Ordering::Relaxed;
    let project = Project::new("ftp-reset", remote::Mode::Mirror);
    project.server.put("a.txt", "hello");
    project.server.resets.store(1, Relaxed);
    let state = project.state();
    let mut session = None;
    state.transfer("a.txt", Action::Download, &mut session).unwrap();
    assert_eq!(std::fs::read_to_string(project.root.join("a.txt")).unwrap(), "hello");
    // Through a resilient session: reset once, read again.
    project.server.resets.store(1, Relaxed);
    let mut fs = crate::pool::Resilient::new(state.connector.as_ref(), state.retry());
    assert_eq!(remote::application::RemoteFs::read(&mut fs, "a.txt").unwrap(), b"hello");
    // A listing that keeps failing says which folder and the server's words.
    project.server.offline.store(true, Relaxed);
    let mut fs = crate::pool::Resilient::new(state.connector.as_ref(), state.retry());
    let e = remote::application::list_remote_under(&state.saved.connection, &mut fs, "", &|| false)
        .unwrap_err();
    assert!(e.to_string().contains("listing /") && e.to_string().contains("no route"), "{e}");
}

/// Against a throwaway vsftpd taking two connections per address (`max_per_ip=2`;
/// `EDITOR_REMOTE_LIMITED=<port>`, user `tester` / `testpw`, a `shop` folder): the third
/// connection is refused; the session's cautious pool pulls everything anyway.
#[test]
#[ignore = "needs a limited FTP server"]
fn a_pull_from_a_server_limiting_connections() {
    use remote::application::{Direction, LocalFiles, Run, apply, list_remote, plan};
    let Ok(port) = std::env::var("EDITOR_REMOTE_LIMITED") else { return };
    let local = std::env::temp_dir().join(format!("remote-limited-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&local);
    std::fs::create_dir_all(&local).unwrap();
    let mut saved =
        probe_saved(&local, remote::Protocol::Ftp, port.parse().unwrap(), "/ftp/tester/shop");
    saved.connection.user = "tester".into();
    saved.connection.excluded.clear();
    let bare = || connector(&saved, Some("testpw".into()), None);
    // What the server says to a third connection.
    let (a, b) = (bare().connect().unwrap(), bare().connect().unwrap());
    let third = bare().connect().err();
    eprintln!("the third connection: {third:?}");
    assert!(third.as_ref().is_some_and(crate::pool::refused_for_load), "{third:?}");
    drop((a, b));
    std::thread::sleep(std::time::Duration::from_millis(300));
    let pull = |connector: Arc<dyn remote::application::Connector>| {
        let mut fs = connector.connect().unwrap();
        let remote = list_remote(&saved.connection, fs.as_mut(), &|| false).unwrap();
        drop(fs);
        let copy = local_copy(&local);
        let changes = plan(
            &saved.connection,
            &Default::default(),
            &copy.list(),
            &remote,
            Direction::Pull,
            &|_| None,
        );
        let run = Run {
            workers: 4,
            retry: remote::Retry { attempts: 2, delay: std::time::Duration::from_millis(50) },
            now: &now,
            on_transfer: &|_| {},
            on_bytes: &|_, _| {},
            cancelled: &|| false,
            parallel: &threads,
            pause: &std::thread::sleep,
        };
        let done = apply(&changes, connector.as_ref(), &copy, &mut Default::default(), &run);
        done.into_iter().filter_map(|t| t.error).collect::<Vec<_>>()
    };
    // As before: six connections asked for at once.
    let before = pull(Arc::new(crate::pool::Pooled(crate::pool::Pool::new(bare(), 6))));
    eprintln!("before: {} failed: {:?}", before.len(), before.first());
    std::thread::sleep(std::time::Duration::from_millis(300));
    let _ = std::fs::remove_dir_all(&local);
    std::fs::create_dir_all(&local).unwrap();
    // Now: cautious.
    let after = pull(Arc::new(crate::pool::Pooled(pool_for(&saved, bare(), 6))));
    assert!(after.is_empty(), "{after:?}");
    assert!(!before.is_empty(), "the old pool failed some");
    let _ = std::fs::remove_dir_all(&local);
}

/// Against the same throwaway vsftpd, dropping idle sessions after two seconds
/// (`idle_session_timeout=2`): a pooled session kept idle is gone on the server; listing on
/// it fails at once, a resilient session opens another.
#[test]
#[ignore = "needs a limited FTP server"]
fn a_session_kept_idle_too_long_is_opened_again() {
    use remote::application::list_remote;
    let Ok(port) = std::env::var("EDITOR_REMOTE_IDLE") else { return };
    let local = std::env::temp_dir().join(format!("remote-idle-{}", std::process::id()));
    let mut saved =
        probe_saved(&local, remote::Protocol::Ftp, port.parse().unwrap(), "/ftp/tester/shop");
    saved.connection.user = "tester".into();
    saved.connection.excluded.clear();
    let pooled =
        crate::pool::Pooled(pool_for(&saved, connector(&saved, Some("testpw".into()), None), 6));
    drop(remote::application::Connector::connect(&pooled).unwrap().list("").unwrap());
    std::thread::sleep(std::time::Duration::from_secs(4));
    let mut stale = remote::application::Connector::connect(&pooled).unwrap();
    let before = list_remote(&saved.connection, stale.as_mut(), &|| false);
    eprintln!("on the idle session: {:?}", before.as_ref().err());
    drop(stale);
    drop(remote::application::Connector::connect(&pooled).unwrap().list("").unwrap());
    std::thread::sleep(std::time::Duration::from_secs(4));
    let retry = remote::Retry { attempts: 3, delay: std::time::Duration::from_millis(50) };
    let mut fs = crate::pool::Resilient::new(&pooled, retry);
    let after = list_remote(&saved.connection, &mut fs, &|| false);
    assert!(after.as_ref().is_ok_and(|l| l.len() > 20), "{after:?}");
    // A folder that isn't there: said so (vsftpd lists it as empty).
    let missing = remote::application::RemoteFs::list(&mut fs, "not-there");
    assert!(matches!(missing, Err(remote::RemoteError::NotFound(_))), "{missing:?}");
}

/// Uploads off: a save stays here, listed; moves and deletions are kept (on disk: they
/// outlive the session) and listed; sent, a move is made on the server (its content
/// following), a deletion asked about first.
#[test]
fn with_uploads_off_changes_wait_for_the_user() {
    use crate::outgoing::{Why, rows};
    let project = Project::with("manual", remote::Mode::Mirror, |s| {
        s.connection.auto_upload = false;
    });
    let state = project.state();
    // Sent once (by the user), then changed, moved and deleted here.
    project.save("a.txt", "a");
    project.save("b.txt", "b");
    std::thread::sleep(std::time::Duration::from_millis(400));
    assert!(project.server.text("a.txt").is_none(), "nothing goes by itself");
    state.send(&rows(&state));
    assert!(project.wait(|p| p.server.text("a.txt").is_some() && p.server.text("b.txt").is_some()));
    assert!(project.wait(|_| rows(&state).is_empty()), "{:?}", rows(&state));
    std::fs::create_dir_all(project.root.join("src")).unwrap();
    std::fs::rename(project.root.join("a.txt"), project.root.join("src/a.txt")).unwrap();
    project.events.publish(&workspace::events::FileRenamed {
        from: project.root.join("a.txt"),
        to: project.root.join("src/a.txt"),
    });
    std::fs::remove_file(project.root.join("b.txt")).unwrap();
    project
        .events
        .publish(&workspace::events::EntriesDeleted { paths: vec![project.root.join("b.txt")] });
    let listed = rows(&state);
    assert!(
        listed.contains(&("src/a.txt".into(), Why::Renamed { from: "a.txt".into() })),
        "{listed:?}"
    );
    assert!(listed.contains(&("b.txt".into(), Why::Deleted)), "{listed:?}");
    // Kept with the project.
    assert_eq!(super::load_outbox(&project.root), *state.outbox.lock().unwrap());
    // Sent: the move made there, the deletion asked about (nothing deleted before).
    state.send(&listed);
    assert!(
        project.wait(|p| p.server.text("src/a.txt").is_some() && p.server.text("a.txt").is_none())
    );
    assert!(project.wait(|_| {
        state
            .inbox
            .lock()
            .unwrap()
            .iter()
            .any(|n| matches!(n, Notice::ConfirmRemove(p) if p == &["b.txt".to_owned()]))
    }));
    assert!(project.server.text("b.txt").is_some());
    state.send_job(Job::Remove(vec!["b.txt".into()]));
    assert!(project.wait(|p| p.server.text("b.txt").is_none()));
    assert!(project.wait(|_| rows(&state).is_empty()), "{:?}", rows(&state));
    assert!(state.outbox.lock().unwrap().is_empty());
}

/// Uploads off, a folder deleted here: its files listed one by one; sent and confirmed, the
/// files go there and so do their folders gone here (the deepest first) — never one still
/// here.
#[test]
fn a_folder_deleted_here_goes_from_the_server_with_its_files() {
    use crate::outgoing::rows;
    let project = Project::with("folder", remote::Mode::Mirror, |s| {
        s.connection.auto_upload = false;
    });
    let state = project.state();
    std::fs::create_dir_all(project.root.join("app/aaa/deep")).unwrap();
    project.save("app/keep.txt", "k");
    project.save("app/aaa/a.txt", "a");
    project.save("app/aaa/deep/b.txt", "b");
    state.send(&rows(&state));
    assert!(project.wait(|p| p.server.text("app/aaa/deep/b.txt").is_some()));
    assert!(project.wait(|_| rows(&state).is_empty()), "{:?}", rows(&state));
    std::fs::remove_dir_all(project.root.join("app/aaa")).unwrap();
    project
        .events
        .publish(&workspace::events::EntriesDeleted { paths: vec![project.root.join("app/aaa")] });
    let listed = rows(&state);
    assert_eq!(listed.len(), 2, "{listed:?}");
    state.send(&listed);
    let asked = |state: &Arc<State>| {
        state.inbox.lock().unwrap().iter().find_map(|n| match n {
            Notice::ConfirmRemove(p) => Some(p.clone()),
            _ => None,
        })
    };
    assert!(project.wait(|_| asked(&state).is_some()));
    state.send_job(Job::Remove(asked(&state).unwrap()));
    assert!(project.wait(|p| p.server.removed.lock().unwrap().contains(&"app/aaa".to_owned())));
    let removed = project.server.removed.lock().unwrap().clone();
    let at = |p: &str| removed.iter().position(|r| r == p);
    assert!(at("app/aaa/deep") < at("app/aaa"), "the deepest first: {removed:?}");
    assert!(at("app/aaa/deep").is_some() && !removed.contains(&"app".to_owned()), "{removed:?}");
    assert!(project.server.text("app/keep.txt").is_some());
}

/// What the user sends is going (the buttons' spinners) from the press until it's up; a
/// deletion isn't (it's asked about first).
#[test]
fn what_is_sent_is_going_until_it_is_up() {
    use crate::outgoing::{Why, rows};
    use std::sync::atomic::Ordering::Relaxed;
    let project = Project::with("sending", remote::Mode::Mirror, |s| {
        s.connection.auto_upload = false;
    });
    let state = project.state();
    project.server.write_ms.store(150, Relaxed);
    project.save("a.txt", "a");
    assert!(project.wait(|_| !rows(&state).is_empty()));
    let before = state.sending_generation();
    state.send(&[("a.txt".into(), Why::Changed), ("gone.txt".into(), Why::Deleted)]);
    assert!(state.is_sending("a.txt") && state.sending_any());
    assert!(!state.is_sending("gone.txt"), "a deletion is asked about first");
    assert!(state.sending_generation() > before, "the views draw it");
    assert!(project.wait(|p| p.server.text("a.txt").is_some()));
    assert!(project.wait(|_| !state.sending_any()));
}

/// Sent again while it's going up: the loader waits for the second upload, not the first.
#[test]
fn a_file_sent_again_while_going_up_stays_going_until_its_own_upload() {
    use crate::outgoing::Why;
    use std::sync::atomic::Ordering::Relaxed;
    let project = Project::with("again", remote::Mode::Mirror, |s| {
        s.connection.auto_upload = false;
    });
    let state = project.state();
    project.server.write_ms.store(300, Relaxed);
    project.save("a.txt", "a");
    let row = [("a.txt".to_owned(), Why::Changed)];
    state.send(&row);
    assert!(project.wait(|p| p.server.working.load(Relaxed) == 1), "the first is going up");
    state.send(&row);
    assert!(project.wait(|p| p.server.writes.load(Relaxed) >= 1));
    if project.server.writes.load(Relaxed) == 1 {
        assert!(state.is_sending("a.txt"), "the second upload is still to come");
    }
    assert!(project.wait(|_| !state.is_sending("a.txt")));
}

/// Only folders gone here are removed there; one still here stops the walk.
#[test]
fn only_the_folders_gone_here_are_removed_there() {
    let root = std::env::temp_dir().join(format!("gone-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("app/kept")).unwrap();
    let deleted = |path: &str| Transfer {
        path: path.into(),
        action: Action::DeleteRemote,
        bytes: 0,
        at: 0,
        error: None,
    };
    let done = [deleted("app/kept/a.txt"), deleted("app/gone/deep/b.txt"), deleted("x/c.txt")];
    let folders = super::gone_folders(&root, done.iter());
    assert_eq!(folders, ["app/gone/deep", "app/gone", "x"]);
    std::fs::remove_dir_all(&root).unwrap();
}

/// The switch flipped: kept in the connection, the session going on with it.
#[test]
fn the_upload_switch_is_kept_in_the_connection() {
    let project = Project::new("switch", remote::Mode::Mirror);
    let state = project.state();
    assert!(state.auto_upload());
    super::config::save(&state.saved).unwrap();
    state.set_auto_upload(false).unwrap();
    assert!(!state.auto_upload());
    assert!(!super::config::find(&project.root).unwrap().connection.auto_upload);
    project.save("a.txt", "a");
    std::thread::sleep(std::time::Duration::from_millis(400));
    assert!(project.server.text("a.txt").is_none());
}

/// A project with uploads off, files `names` sent once (by the user) and the session's
/// state.
fn manual(name: &str, names: &[&str]) -> (Project, Arc<State>) {
    let project = Project::with(name, remote::Mode::Mirror, |s| s.connection.auto_upload = false);
    let state = project.state();
    for name in names {
        if let Some(dir) = project.root.join(name).parent() {
            std::fs::create_dir_all(dir).unwrap();
        }
        project.save(name, name);
    }
    state.send_now(&crate::outgoing::rows(&state));
    assert!(project.wait(|p| names.iter().all(|n| p.server.text(n).is_some())));
    assert!(project.wait(|_| crate::outgoing::rows(&state).is_empty()));
    (project, state)
}

/// Several to upload, one changed on the server too: the server is checked for all first
/// and nothing goes; the conflict is listed, the user asked (resolve first, or the rest);
/// the rest then go, the conflict stays until decided.
#[test]
fn an_upload_of_many_checks_for_conflicts_first() {
    use crate::outgoing::{Why, rows};
    let (project, state) = manual("bulk", &["a.txt", "b.txt"]);
    project.server.put("a.txt", "theirs");
    project.save("a.txt", "mine");
    project.save("b.txt", "mine too");
    let listed = rows(&state);
    assert_eq!(listed.len(), 2, "{listed:?}");
    state.send(&listed);
    let asked = |state: &State| {
        state.inbox.lock().unwrap().iter().find_map(|n| match n {
            Notice::BulkConflicts { conflicts, rest } => Some((conflicts.clone(), rest.clone())),
            _ => None,
        })
    };
    assert!(project.wait(|_| asked(&state).is_some()));
    let (conflicts, rest) = asked(&state).unwrap();
    assert_eq!(conflicts, ["a.txt"]);
    assert_eq!(rest, [("b.txt".to_owned(), Why::Changed)]);
    assert_eq!(project.server.text("b.txt").as_deref(), Some("b.txt"), "nothing went before");
    assert!(project.wait(|_| rows(&state).contains(&("a.txt".into(), Why::Conflict))));
    // The rest now.
    state.send_now(&rest);
    assert!(project.wait(|p| p.server.text("b.txt").as_deref() == Some("mine too")));
    assert_eq!(project.server.text("a.txt").as_deref(), Some("theirs"));
    // Decided: ours goes up, no longer a conflict.
    state.send_job(Job::UploadMine("a.txt".into()));
    assert!(project.wait(|p| p.server.text("a.txt").as_deref() == Some("mine")));
    assert!(project.wait(|_| rows(&state).is_empty()), "{:?}", rows(&state));
}

/// Giving up the changes here (confirmed): a changed file takes the server's copy back, one
/// made here goes, a deleted one comes back, a moved one comes back where it was.
#[test]
fn discarding_takes_the_server_s_copies_back() {
    use crate::outgoing::rows;
    let (project, state) = manual("discard", &["changed.txt", "deleted.txt", "moved.txt"]);
    project.save("changed.txt", "edited");
    project.save("new.txt", "new");
    std::fs::remove_file(project.root.join("deleted.txt")).unwrap();
    project.events.publish(&workspace::events::EntriesDeleted {
        paths: vec![project.root.join("deleted.txt")],
    });
    std::fs::rename(project.root.join("moved.txt"), project.root.join("there.txt")).unwrap();
    project.events.publish(&workspace::events::FileRenamed {
        from: project.root.join("moved.txt"),
        to: project.root.join("there.txt"),
    });
    let listed = rows(&state);
    assert_eq!(listed.len(), 4, "{listed:?}");
    state.discard(listed.clone());
    assert!(project.wait(|_| {
        state
            .inbox
            .lock()
            .unwrap()
            .iter()
            .any(|n| matches!(n, Notice::ConfirmDiscard(r) if r.len() == 4))
    }));
    // Yes.
    state.send_job(Job::Discard(listed));
    let text = |name: &str| std::fs::read_to_string(project.root.join(name)).ok();
    assert!(project.wait(|_| {
        text("changed.txt").as_deref() == Some("changed.txt")
            && text("new.txt").is_none()
            && text("deleted.txt").as_deref() == Some("deleted.txt")
            && text("moved.txt").as_deref() == Some("moved.txt")
            && text("there.txt").is_none()
    }));
    assert!(project.wait(|_| rows(&state).is_empty()), "{:?}", rows(&state));
    assert!(state.outbox.lock().unwrap().is_empty());
    assert!(project.server.text("new.txt").is_none(), "nothing sent");
}

/// Compared with the server: both sides read (the server's in the background), the
/// comparison asked of the window.
#[test]
fn a_file_is_compared_with_the_server_s_copy() {
    let (project, state) = manual("compare", &["a.txt"]);
    project.save("a.txt", "mine");
    state.compare_with_server("a.txt", "a.txt");
    assert!(project.wait(|_| {
        state.inbox.lock().unwrap().iter().any(|n| {
            matches!(n, Notice::Compare { relative, local, server, .. }
                if relative == "a.txt" && local == "mine" && server == "a.txt")
        })
    }));
}

/// Uploads off: a file waiting has its bar over the editor (none with uploads on), the
/// folders above it are marked in the tree, the button counts it; a conflict marks them as
/// one and turns the count's color to a warning.
#[test]
fn what_waits_shows_in_the_editor_the_tree_and_the_button() {
    use crate::outgoing::{Why, refresh};
    use workbench_model::BadgeKind;
    let (project, state) = manual("marks", &["src/app/a.txt"]);
    project.save("src/app/a.txt", "edited");
    refresh(&state);
    assert!(project.wait(|_| state.outgoing.under("src").len() == 1));
    let bar =
        crate::upload_bar::UploadBar::new(project.id, project.root.clone(), "src/app/a.txt".into());
    use ui_components::editor_toolbar::EditorToolbar;
    assert!(bar.height() > 0.0, "waiting: the bar shows");
    assert_eq!(state.folder_mark("src"), workspace::Mark::Changed);
    assert_eq!(state.folder_mark("other"), workspace::Mark::None);
    assert_eq!(state.outgoing.summary().map(|s| s.1), Some(BadgeKind::Count));
    // A conflict.
    state.conflicts.lock().unwrap().insert("src/app/a.txt".into());
    refresh(&state);
    assert!(
        project.wait(|_| state.outgoing.under("src") == [("src/app/a.txt".into(), Why::Conflict)])
    );
    assert_eq!(state.folder_mark("src/app"), workspace::Mark::Conflict);
    assert_eq!(state.outgoing.summary().map(|s| s.1), Some(BadgeKind::Warning));
    // Uploads on: no bar.
    super::config::save(&state.saved).unwrap();
    state.set_auto_upload(true).unwrap();
    let bar = std::sync::Mutex::new(bar);
    assert!(project.wait(|_| {
        let mut bar = bar.lock().unwrap();
        bar.poll();
        bar.height() == 0.0
    }));
}

/// A folder made here (not on the server yet), opened in the tree: listed as empty, no
/// error told — the server simply has nothing there.
#[test]
fn a_folder_only_here_is_listed_empty_without_an_error() {
    let local = std::env::temp_dir().join(format!("remote-only-here-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&local);
    std::fs::create_dir_all(local.join("abc")).unwrap();
    let server = Arc::new(Server::default());
    server.put("index.php", "<?php");
    let saved = probe_saved(&local, remote::Protocol::Sftp, 1, "/www");
    let state = State::detached(saved, Arc::new(FakeConnector(server)));
    let mut session = None;
    state.list("abc", &mut session);
    assert_eq!(state.listings.lock().unwrap().get("abc").map(Vec::len), Some(0));
    assert!(state.inbox.lock().unwrap().is_empty(), "{:?}", state.inbox.lock().unwrap());
    let _ = std::fs::remove_dir_all(&local);
}

/// On the server, uploads off: files made here in a new folder (several), sent all at once,
/// go up.
#[test]
fn on_demand_new_files_in_a_new_folder_go_up_when_sent() {
    use crate::outgoing::rows;
    let project = Project::with("new-folder", remote::Mode::OnDemand, |s| {
        s.connection.auto_upload = false;
    });
    let state = project.state();
    let names = ["admin/x/config/a.php", "admin/x/src/Api/b.php", "admin/x/src/c.php"];
    for name in names {
        std::fs::create_dir_all(project.root.join(name).parent().unwrap()).unwrap();
        project.save(name, name);
    }
    assert!(project.wait(|_| rows(&state).len() == 3), "{:?}", rows(&state));
    state.send(&rows(&state));
    assert!(
        project.wait(|p| names.iter().all(|n| p.server.text(n).is_some())),
        "{:?} {:?}",
        rows(&state),
        state.inbox.lock().unwrap().len()
    );
    assert!(project.wait(|_| rows(&state).is_empty()), "{:?}", rows(&state));
}

/// A batch of changes left open (a paste whose end never came) doesn't keep what the user
/// sends from going up.
#[test]
fn a_file_sent_goes_up_during_a_batch_of_changes() {
    use crate::outgoing::rows;
    let project = Project::with("sent-held", remote::Mode::Mirror, |s| {
        s.connection.auto_upload = false;
    });
    let state = project.state();
    project.events.publish(&workspace::events::BulkChangeStarted { id: 7 });
    project.save("a.txt", "a");
    assert!(project.wait(|_| rows(&state).len() == 1));
    state.send(&rows(&state));
    assert!(project.wait(|p| p.server.text("a.txt").is_some()));
}

/// The files changed here in a span of time: each once, newest first, those never sent
/// left out, each with whether the server has it as it is.
#[test]
fn the_files_changed_in_a_span_are_listed_with_where_they_stand() {
    let project = Project::new("changed", remote::Mode::Mirror);
    project.save("sent.txt", "sent");
    let state = project.state();
    assert!(project.wait(|_| state.sent_under("sent.txt").len() == 1));
    let now = std::time::SystemTime::now();
    let at = |name: &str, ago: u64| {
        std::fs::File::options()
            .write(true)
            .open(project.root.join(name))
            .unwrap()
            .set_modified(now - std::time::Duration::from_secs(ago))
            .unwrap();
    };
    std::fs::write(project.root.join("new.txt"), "new").unwrap();
    at("new.txt", 60);
    std::fs::write(project.root.join("old.txt"), "old").unwrap();
    at("old.txt", 10 * 3600);
    std::fs::create_dir_all(project.root.join("vendor")).unwrap();
    std::fs::write(project.root.join("vendor/lib.php"), "x").unwrap();
    let secs = now.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    let last_4h = state.changed_between(secs - 4 * 3600, None);
    let names: Vec<(&str, bool)> = last_4h.iter().map(|f| (f.path.as_str(), f.sent)).collect();
    assert_eq!(names, [("sent.txt", true), ("new.txt", false)], "newest first, vendor excluded");
    let day = state.changed_between(secs - 24 * 3600, None);
    assert_eq!(day.last().map(|f| f.path.as_str()), Some("old.txt"));
    // A span ending before now: what changed after it is out.
    let before = state.changed_between(secs - 24 * 3600, Some(secs - 3600));
    let names: Vec<&str> = before.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(names, ["old.txt"]);
}
