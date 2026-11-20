use super::*;
use workbench_model::{ResourceSnapshot, ResourceSource, ResourceUse};

/// The app's parts as a project's would name them: a path, a project's name, a host.
struct Secretive;

impl ResourceSource for Secretive {
    fn snapshot(&self) -> ResourceSnapshot {
        let mut part =
            ResourceUse::new("Remote: /Users/alice/secret-shop on db.secret.example", "remote");
        part.threads = vec!["remote-sftp"];
        part.detail = "/Users/alice/secret-shop/.env".into();
        let mut index = ResourceUse::new("Index: secret-shop", "code.index");
        index.threads = vec!["indexer"];
        ResourceSnapshot { parts: vec![part, index], gpu: Some(42), ..ResourceSnapshot::default() }
    }
}

/// Whose each thread is comes from the parts' owners and threads only: no part's name or
/// detail (a project's, a path, a host) ever reaches a recording.
#[test]
fn owners_never_carry_a_project_s_names() {
    let owners = recording::owners_of(Some(&Secretive));
    assert_eq!(owners.of("indexer"), Some("code.index"));
    assert_eq!(owners.of("remote-sftp"), Some("remote"));
    let mut recording = profiling::Recording::new(String::new(), 100, profiling::System::default());
    recording.owners = owners;
    let json = recording.to_json();
    for secret in ["alice", "secret", "/Users", "example", ".env"] {
        assert!(!json.contains(secret), "{secret} reached the recording: {json}");
    }
}

/// The consent window's view and the recordings' are this module's; other files aren't.
#[test]
fn its_views_are_the_consent_and_the_recordings() {
    let views = Views;
    assert!(views.kind(Path::new(&format!("x.{CONSENT}"))).is_some());
    if let Some(folder) = recording::folder() {
        assert!(views.kind(&folder.join("profile-20261120-101500.json")).is_some());
    }
    // The same name in a project: the user's, not this view's.
    assert!(views.kind(Path::new("/work/shop/profile-20261120-101500.json")).is_none());
    assert!(views.kind(Path::new("/p/profile.json")).is_none());
    assert!(views.kind(Path::new("/p/composer.json")).is_none());
    // Nothing records: its button isn't lit, no sign in the status bar.
    assert!(!Button.active());
    assert!(Recording.indicator().is_none());
}

/// A module's own figures (`contracts::profiling`): counts, bytes and time as counters in
/// their units, what it holds as a level; read again only after a second.
#[test]
fn a_module_s_figures_become_counters_and_levels() {
    use contracts::profiling::{ProfileSource, Reading, ReadingKind};
    #[derive(Debug)]
    struct Fake;
    impl ProfileSource for Fake {
        fn owner(&self) -> &'static str {
            "tests.source"
        }
        fn read(&self) -> Vec<Reading> {
            vec![
                Reading::new("requests", ReadingKind::Count, 3),
                Reading::new("sent", ReadingKind::Bytes, 2048),
                Reading::new("answer-time", ReadingKind::Micros, 1500),
                Reading::new("waiting", ReadingKind::Level, 4),
            ]
        }
    }
    let read = recording::module_readings(vec![std::sync::Arc::new(Fake)]);
    let (counters, levels) = read();
    let units: Vec<(&str, &str, u64)> =
        counters.iter().map(|c| (c.what.as_str(), c.unit.as_str(), c.count)).collect();
    assert_eq!(
        units,
        [("requests", "count", 3), ("sent", "bytes", 2048), ("answer-time", "us", 1500)]
    );
    assert!(counters.iter().all(|c| c.owner == "tests.source"));
    assert_eq!((levels[0].what.as_str(), levels[0].peak), ("waiting", 4));
    assert_eq!(read(), (Vec::new(), Vec::new()), "not read again within a second");
}
