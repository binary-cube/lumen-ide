use std::time::{Duration, Instant};

use pinned_downloads::fetch::Memory;
use pinned_downloads::samples::{Entry, tar, xz};
use ring::signature::{Ed25519KeyPair, KeyPair};

use super::*;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Waits (a few seconds at most) for the status to be `done`.
fn until(done: impl Fn(&Status) -> bool) -> Status {
    until_within(done, Duration::from_secs(10))
}

/// Waits (`limit` at most) for the status to be `done`.
fn until_within(done: impl Fn(&Status) -> bool, limit: Duration) -> Status {
    let started = Instant::now();
    loop {
        let now = status();
        if done(&now) || started.elapsed() > limit {
            return now;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// `memory` serving a signed list with one package of `version` for this system, and the
/// package.
fn publish(memory: &Memory, key: &Ed25519KeyPair, version: &str) {
    let engine =
        format!(r#"{{"version":"{version}","protocol":{}}}"#, web_engine_protocol::PROTOCOL);
    let program = layout::program(Path::new("")).to_string_lossy().replace('\\', "/");
    let bytes = xz(&tar(&[
        Entry(&program, b'0', 0o755, b"host"),
        Entry("engine.json", b'0', 0o644, engine.as_bytes()),
    ]));
    let sha =
        pinned_downloads::digest::hex(ring::digest::digest(&ring::digest::SHA256, &bytes).as_ref());
    let url =
        format!("https://github.com/o/r/releases/download/web-engine/engine-{version}.tar.xz");
    let list = format!(
        r#"{{"packages":[{{"version":"{version}","protocol":{},"platform":"{}","url":"{url}","size":{},"sha256":"{sha}"}}]}}"#,
        web_engine_protocol::PROTOCOL,
        super::super::manifest::platform().unwrap_or("none"),
        bytes.len()
    );
    memory.serve(&url, bytes);
    let (list_url, signature_url) = super::super::manifest_urls(kernel::brand::REPOSITORY);
    memory.serve(&signature_url, hex(key.sign(list.as_bytes()).as_ref()).into_bytes());
    memory.serve(&list_url, list.into_bytes());
}

/// Missing until installed; a page wanting it is told once; installed from the signed list;
/// a newer one published comes as an update (the installed one said installed meanwhile);
/// removed, it's missing again.
#[test]
fn the_engine_is_offered_installed_updated_and_removed() {
    if super::super::manifest::platform().is_none() {
        return;
    }
    let root = pinned_downloads::samples::temp("web-engine-service");
    let memory = Arc::new(Memory::default());
    let key = Ed25519KeyPair::from_seed_unchecked(&[5u8; 32]).unwrap();
    isolate(root.clone(), memory.clone(), &hex(key.public_key().as_ref()));
    // A page made at the app's start wants it before the first look ended: kept, and the
    // offer is there once it's found missing.
    assert!(!wanted());
    let before = generation();
    want();
    assert!(wanted() && generation() > before, "told while looking");
    assert_eq!(until(|s| *s != Status::Looking), Status::Missing);
    assert!(wanted(), "still wanted once missing");
    let again = generation();
    want();
    assert_eq!(generation(), again, "once");

    publish(&memory, &key, "156.0.0.1-1");
    assert!(start());
    let installed = until(|s| matches!(s, Status::Installed { .. } | Status::Failed(_)));
    assert!(
        matches!(&installed, Status::Installed { version, .. } if version == "156.0.0.1-1"),
        "{installed:?}"
    );

    publish(&memory, &key, "156.0.0.1-2");
    assert!(start());
    assert!(matches!(status(), Status::Installed { .. }), "the old one stands meanwhile");
    assert!(updating() && updated().is_none(), "an update under way");
    assert!(!start() && !remove(), "under way: neither starts again nor removes");
    let second =
        until(|s| matches!(s, Status::Installed { version, .. } if version == "156.0.0.1-2"));
    assert!(
        matches!(&second, Status::Installed { version, .. } if version == "156.0.0.1-2"),
        "{second:?}"
    );
    assert_eq!(updated(), Some(Updated::Newer("156.0.0.1-2".into())));
    assert!(root.join("156.0.0.1-1").exists(), "the one before kept until the new one worked");
    started("156.0.0.1-2", StartOutcome::Ran);
    let since = Instant::now();
    while root.join("156.0.0.1-1").exists() && since.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!root.join("156.0.0.1-1").exists(), "then cleaned");

    // A newer one whose own starts fail twice is given up for the one before; a start that
    // failed for another reason (another GPU, a slow start) counts nothing.
    publish(&memory, &key, "156.0.0.1-3");
    start();
    let newer =
        until(|s| matches!(s, Status::Installed { version, .. } if version == "156.0.0.1-3"));
    assert!(
        matches!(&newer, Status::Installed { version, .. } if version == "156.0.0.1-3"),
        "{newer:?}"
    );
    for outcome in
        [StartOutcome::NotTheEngine, StartOutcome::EngineFailed, StartOutcome::NotTheEngine]
    {
        started("156.0.0.1-3", outcome);
    }
    let one_failure = Instant::now();
    while one_failure.elapsed() < Duration::from_millis(200) {
        assert!(matches!(status(), Status::Installed { version, .. } if version == "156.0.0.1-3"));
        std::thread::sleep(Duration::from_millis(10));
    }
    started("156.0.0.1-3", StartOutcome::EngineFailed);
    let back =
        until(|s| matches!(s, Status::Installed { version, .. } if version == "156.0.0.1-2"));
    assert!(
        matches!(&back, Status::Installed { version, .. } if version == "156.0.0.1-2"),
        "{back:?}"
    );
    assert_eq!(layout::read_state(&root).bad, ["156.0.0.1-3".to_owned()]);

    // Published again it isn't taken (it failed here); an older one never replaces the one
    // installed.
    for version in ["156.0.0.1-3", "156.0.0.1-1"] {
        publish(&memory, &key, version);
        memory.forget_asked();
        let before = generation();
        start();
        let since = Instant::now();
        while generation() < before + 2 && since.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(matches!(status(), Status::Installed { version, .. } if version == "156.0.0.1-2"));
        assert!(memory.asked().iter().all(|url| !url.contains(".tar.xz")), "nothing downloaded");
        assert_eq!(updated(), Some(Updated::UpToDate), "said up to date");
    }
    // The list can't be had: the update says why, the one installed stands.
    memory.set_offline(true);
    let before = generation();
    assert!(start());
    let since = Instant::now();
    while updated().is_none() && since.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(generation() > before);
    assert_eq!(updated(), Some(Updated::Failed(Failure::Offline)));
    assert!(matches!(status(), Status::Installed { .. }));
    memory.set_offline(false);

    assert!(remove());
    assert_eq!(until(|s| *s == Status::Missing), Status::Missing);
    assert!(!root.join("156.0.0.1-2").exists());
    // Removed, it's offered again when a page wants it.
    assert!(!wanted());
    let before = generation();
    want();
    assert!(wanted() && generation() > before, "told while missing");
    std::fs::remove_dir_all(root).unwrap();
}

/// The real package (made by `cargo xtask web-engine-package`, named by
/// `EDITOR_WEB_ENGINE_PACKAGE`, its description beside it) installed by our code from a
/// signed list, then started from where it went: it greets, and while it runs its version
/// is held (never cleaned).
#[test]
#[ignore = "needs a real package (EDITOR_WEB_ENGINE_PACKAGE)"]
fn a_real_package_installs_and_runs() {
    let Some(archive) = std::env::var_os("EDITOR_WEB_ENGINE_PACKAGE").map(std::path::PathBuf::from)
    else {
        return;
    };
    let description: serde_json::Value = serde_json::from_slice(
        &std::fs::read(format!("{}.json", archive.display())).expect("its description"),
    )
    .unwrap();
    let version = description["version"].as_str().unwrap().to_owned();
    let bytes = std::fs::read(&archive).unwrap();
    let memory = Arc::new(Memory::default());
    let key = Ed25519KeyPair::from_seed_unchecked(&[6u8; 32]).unwrap();
    let url = format!(
        "https://github.com/o/r/releases/download/web-engine/{}",
        description["name"].as_str().unwrap()
    );
    let list = serde_json::json!({"packages": [{
        "version": version, "protocol": description["protocol"], "platform": description["platform"],
        "url": url, "size": description["size"], "sha256": description["sha256"],
    }]})
    .to_string();
    memory.serve(&url, bytes);
    let (list_url, signature_url) = super::super::manifest_urls(kernel::brand::REPOSITORY);
    memory.serve(&signature_url, hex(key.sign(list.as_bytes()).as_ref()).into_bytes());
    memory.serve(&list_url, list.into_bytes());
    let root = pinned_downloads::samples::temp("web-engine-real");
    isolate(root.clone(), memory, &hex(key.public_key().as_ref()));
    assert_eq!(until(|s| *s != Status::Looking), Status::Missing);
    let started = Instant::now();
    start();
    // A test's build unpacks slowly (no optimizations): minutes at most.
    let done = |s: &Status| matches!(s, Status::Installed { .. } | Status::Failed(_));
    let status = until_within(done, Duration::from_secs(600));
    eprintln!("installed in {:?}: {status:?}", started.elapsed());
    let Status::Installed { dir, .. } = status else { panic!("{status:?}") };

    let channel = web_host_core::channel::Channel {
        name: format!("web-host-real-{}", std::process::id()),
        // As the app's: the user's temporary folder (a socket's path is short).
        dir: std::env::temp_dir(),
    };
    let launch = crate::Launch {
        program: layout::program(&dir),
        channel: channel.clone(),
        profile: layout::profile(&root),
        hold: Some(dir.join(layout::IN_USE)),
        adapter: None,
    };
    let connection =
        crate::Connection::establish(&channel, None, || Some(launch)).expect("it greets");
    assert!(connection.engine().starts_with(&version), "{}", connection.engine());
    // Running from it: a clean keeps it even when it's not chosen any more.
    let _ = layout::update_state(&root, |state| *state = layout::State::default());
    assert!(layout::clean(&root).versions.is_empty(), "held while it runs");
    drop(connection);
    let _ = std::fs::remove_dir_all(root);
}

/// Unix: a program that sleeps, saying its process in `pid` beside it.
#[cfg(unix)]
fn sleeping_program(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = pinned_downloads::samples::temp(name);
    std::fs::create_dir_all(&dir).unwrap();
    let program = dir.join("sleeping.sh");
    std::fs::write(&program, "#!/bin/sh\necho $$ > \"$(dirname \"$0\")/pid\"\nexec sleep 30\n")
        .unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    (dir, program)
}

/// Unix: whether the process said in `dir`'s `pid` still runs (`kill -0`).
#[cfg(unix)]
fn still_runs(dir: &Path) -> bool {
    let pid = std::fs::read_to_string(dir.join("pid")).unwrap_or_default();
    let pid = pid.trim();
    !pid.is_empty()
        && std::process::Command::new("kill")
            .args(["-0", pid])
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
}

/// Unix: a warm-up that hangs is ended past its limit, and at once when stopped: its child
/// is gone either way.
#[cfg(unix)]
#[test]
fn a_warm_up_that_hangs_is_ended() {
    let (dir, program) = sleeping_program("web-engine-warm-wait");
    let since = Instant::now();
    warm_within(&program, &AtomicBool::new(false), Duration::from_millis(300));
    assert!(since.elapsed() < Duration::from_secs(5), "{:?}", since.elapsed());
    assert!(dir.join("pid").exists(), "it ran");
    assert!(!still_runs(&dir), "ended past its limit");
    std::fs::remove_dir_all(dir).unwrap();

    let (dir, program) = sleeping_program("web-engine-warm-stop");
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = Arc::clone(&stop);
    let stopper = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        stopping.store(true, Ordering::Relaxed);
    });
    let since = Instant::now();
    warm_within(&program, &stop, Duration::from_secs(30));
    stopper.join().unwrap();
    assert!(since.elapsed() < Duration::from_secs(5), "stopped: {:?}", since.elapsed());
    assert!(dir.join("pid").exists(), "it ran");
    assert!(!still_runs(&dir), "ended when stopped");
    std::fs::remove_dir_all(dir).unwrap();
}
