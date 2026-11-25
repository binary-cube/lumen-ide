use std::sync::Mutex;

use super::*;

fn temp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("pinned-fetch-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn sha256(bytes: &[u8]) -> Pin {
    Pin::Sha256(crate::digest::hex(ring::digest::digest(&ring::digest::SHA256, bytes).as_ref()))
}

const URL: &str = "https://nodejs.org/dist/v1/node.tar.xz";

/// The hosts these tests download from.
const HOSTS: &[&str] = &["nodejs.org", "github.com"];

#[test]
fn only_https_from_the_hosts_allowed() {
    for url in ["https://nodejs.org/dist/v26.13.0/node.tar.xz", "https://NODEJS.ORG/dist/x"] {
        assert!(allowed(url, HOSTS), "{url}");
    }
    for url in [
        "http://nodejs.org/dist/x",
        "https://example.com/x",
        "https://nodejs.org.evil.com/x",
        "https://evil.com/nodejs.org/x",
        "https://user@nodejs.org/x",
        "https://nodejs.org:8443/x",
        "file:///etc/passwd",
        "https://",
    ] {
        assert!(!allowed(url, HOSTS), "{url}");
    }
    // Another caller's hosts are its own.
    assert!(!allowed("https://nodejs.org/x", &["github.com"]));
    assert!(!allowed("https://nodejs.org/x", &[]));
}

#[test]
fn a_redirect_is_resolved_against_where_it_came_from() {
    assert_eq!(
        redirected("https://github.com/a/b", "https://objects.githubusercontent.com/x"),
        "https://objects.githubusercontent.com/x"
    );
    assert_eq!(
        redirected("https://nodejs.org/dist/v1/x.zip", "/dist/v2/x.zip"),
        "https://nodejs.org/dist/v2/x.zip"
    );
    assert_eq!(
        redirected("https://nodejs.org/dist/v1/x.zip", "y.zip"),
        "https://nodejs.org/dist/v1/y.zip"
    );
}

#[test]
fn a_download_is_checked_against_its_pin() {
    let memory = Memory::default();
    memory.serve(URL, b"the archive".to_vec());
    let stop = AtomicBool::new(false);
    let got = Mutex::new(Vec::new());
    let bytes = fetch_bytes(
        Going { fetch: &memory, progress: &|r, t| got.lock().unwrap().push((r, t)), stop: &stop },
        Wanted { url: URL, hosts: HOSTS, pin: &sha256(b"the archive"), limit: 1024 },
    )
    .unwrap();
    assert_eq!(bytes, b"the archive");
    assert_eq!(got.lock().unwrap().last(), Some(&(11, Some(11))));
    let wrong = fetch_bytes(
        Going { fetch: &memory, progress: &|_, _| {}, stop: &stop },
        Wanted { url: URL, hosts: HOSTS, pin: &sha256(b"another"), limit: 1024 },
    );
    assert_eq!(wrong, Err(FetchError::Mismatch(URL.into())));
    let integrity = Pin::Integrity(format!(
        "sha512-{}",
        crate::digest::base64(ring::digest::digest(&ring::digest::SHA512, b"the archive").as_ref())
    ));
    assert!(
        fetch_bytes(
            Going { fetch: &memory, progress: &|_, _| {}, stop: &stop },
            Wanted { url: URL, hosts: HOSTS, pin: &integrity, limit: 1024 }
        )
        .is_ok()
    );
    // Too large, refused, not there.
    assert_eq!(
        fetch_bytes(
            Going { fetch: &memory, progress: &|_, _| {}, stop: &stop },
            Wanted { url: URL, hosts: HOSTS, pin: &sha256(b"the archive"), limit: 4 }
        ),
        Err(FetchError::TooLarge(URL.into()))
    );
    assert!(matches!(
        fetch_bytes(
            Going { fetch: &memory, progress: &|_, _| {}, stop: &stop },
            Wanted { url: "https://example.com/x", hosts: HOSTS, pin: &sha256(b""), limit: 4 }
        ),
        Err(FetchError::Refused(_))
    ));
    assert!(
        !memory.asked().contains(&"https://example.com/x".to_owned()),
        "a refused URL is never asked"
    );
    assert!(matches!(
        fetch_bytes(
            Going { fetch: &memory, progress: &|_, _| {}, stop: &stop },
            Wanted { url: "https://nodejs.org/none", hosts: HOSTS, pin: &sha256(b""), limit: 4 }
        ),
        Err(FetchError::Status(404, _))
    ));
}

#[test]
fn no_network_is_said_as_such() {
    let memory = Memory::default();
    memory.serve(URL, b"x".to_vec());
    memory.set_offline(true);
    let error = fetch_bytes(
        Going { fetch: &memory, progress: &|_, _| {}, stop: &AtomicBool::new(false) },
        Wanted { url: URL, hosts: HOSTS, pin: &sha256(b"x"), limit: 10 },
    )
    .unwrap_err();
    assert!(error.offline(), "{error}");
    assert!(!FetchError::Mismatch(URL.into()).offline());
}

#[test]
fn a_broken_off_download_is_resumed_where_it_stopped() {
    let dir = temp("resume");
    let part = dir.join(".downloads/abc.part");
    let body: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
    let memory = Memory::default();
    memory.serve(URL, body.clone());
    memory.cut_next_after(30_000);
    let stop = AtomicBool::new(false);
    let pin = sha256(&body);
    let first = fetch_file(
        Going { fetch: &memory, progress: &|_, _| {}, stop: &stop },
        Wanted { url: URL, hosts: HOSTS, pin: &pin, limit: 1 << 20 },
        &part,
    )
    .unwrap_err();
    assert!(first.offline(), "{first}");
    // What came is kept.
    assert_eq!(std::fs::metadata(&part).unwrap().len(), 30_000);
    let got = Mutex::new(Vec::new());
    fetch_file(
        Going { fetch: &memory, progress: &|r, _| got.lock().unwrap().push(r), stop: &stop },
        Wanted { url: URL, hosts: HOSTS, pin: &pin, limit: 1 << 20 },
        &part,
    )
    .unwrap();
    assert_eq!(std::fs::read(&part).unwrap(), body);
    // Asked again from where it stopped; its progress starts there.
    assert_eq!(memory.asked_from().last(), Some(&(URL.to_owned(), 30_000)));
    assert_eq!(got.lock().unwrap().first(), Some(&30_000));
}

#[test]
fn a_part_that_does_not_match_is_removed() {
    let dir = temp("mismatch");
    let part = dir.join("x.part");
    let memory = Memory::default();
    memory.serve(URL, b"what came".to_vec());
    let stop = AtomicBool::new(false);
    let error = fetch_file(
        Going { fetch: &memory, progress: &|_, _| {}, stop: &stop },
        Wanted { url: URL, hosts: HOSTS, pin: &sha256(b"what's pinned"), limit: 1024 },
        &part,
    );
    assert_eq!(error, Err(FetchError::Mismatch(URL.into())));
    assert!(!part.exists());
    // A part longer than the file (it changed): from the start again.
    std::fs::write(&part, vec![0u8; 50]).unwrap();
    fetch_file(
        Going { fetch: &memory, progress: &|_, _| {}, stop: &stop },
        Wanted { url: URL, hosts: HOSTS, pin: &sha256(b"what came"), limit: 1024 },
        &part,
    )
    .unwrap();
    assert_eq!(std::fs::read(&part).unwrap(), b"what came");
}

#[test]
fn a_stopped_download_keeps_what_came() {
    let dir = temp("stopped");
    let part = dir.join("x.part");
    let memory = Memory::default();
    memory.serve(URL, vec![7u8; 10_000]);
    let stop = AtomicBool::new(true);
    assert_eq!(
        fetch_file(
            Going { fetch: &memory, progress: &|_, _| {}, stop: &stop },
            Wanted { url: URL, hosts: HOSTS, pin: &sha256(&[7u8; 10_000]), limit: 1 << 20 },
            &part
        ),
        Err(FetchError::Stopped)
    );
    assert!(part.exists());
}

/// A small file comes whole within its limit; past it, off its hosts or offline: refused
/// as such.
#[test]
fn a_small_file_is_bounded() {
    let memory = Memory::default();
    memory.serve(URL, b"list".to_vec());
    let wanted = Bounded { url: URL, hosts: HOSTS, limit: 4 };
    assert_eq!(small(&memory, wanted).unwrap(), b"list");
    let tight = Bounded { limit: 3, ..wanted };
    assert!(matches!(small(&memory, tight), Err(FetchError::TooLarge(_))));
    let elsewhere = Bounded { hosts: &["example.com"], ..wanted };
    assert!(matches!(small(&memory, elsewhere), Err(FetchError::Refused(_))));
    memory.set_offline(true);
    assert!(small(&memory, wanted).unwrap_err().offline());
}
