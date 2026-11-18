use std::path::Path;
use std::sync::Arc;

use super::*;
use crate::{Capabilities, DbError, Driver, Kind, Language, Secret, Session, Target};

#[derive(Debug)]
struct Nothing;

impl Driver for Nothing {
    fn kind(&self) -> Kind {
        Kind::Relational
    }

    fn language(&self) -> Language {
        Language::Sql
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities { file_based: true, ..Capabilities::default() }
    }

    fn connect(&self, _: &Target, _: &Secret) -> Result<Box<dyn Session>, DbError> {
        Err(DbError::Connect("no".into()))
    }
}

fn factory(id: &'static str, name: &'static str) -> DriverFactory {
    DriverFactory::new(id, name, Kind::Relational, None, || Arc::new(Nothing))
}

fn starts_with_magic(head: &[u8]) -> bool {
    head.starts_with(b"MAGIC")
}

#[test]
fn drivers_are_found_by_their_id_and_replaced_by_a_new_one() {
    register(factory("test-registry", "First"));
    register(factory("test-registry", "Second"));
    let found = find("test-registry").unwrap();
    assert_eq!(found.name, "Second");
    assert_eq!(all().iter().filter(|f| f.id == "test-registry").count(), 1);
    assert!((found.create)().capabilities().file_based, "created when asked");
    assert!(find("test-missing").is_none());
}

#[test]
fn a_file_is_known_by_its_extension_then_by_its_first_bytes() {
    register_files(DataFiles::new("test-files", &["magic", "mg"], 5, starts_with_magic, false));
    let path = Path::new("/data/Base.MG");
    let mut asked = None;
    let found = for_file(path, |n| {
        asked = Some(n);
        Some(b"MAGIC and more".to_vec())
    });
    assert_eq!(found.map(|f| f.driver), Some("test-files".into()));
    assert_eq!(asked, Some(5), "only the first bytes are read");
    assert!(for_file(path, |_| Some(b"other".to_vec())).is_none(), "the bytes decide");
    assert!(for_file(path, |_| None).is_none(), "unreadable");
    let mut read = false;
    assert!(
        for_file(Path::new("/data/notes.txt"), |_| {
            read = true;
            None
        })
        .is_none()
    );
    assert!(!read, "another extension: nothing is read");
}

#[test]
fn a_target_says_whether_it_is_read_only() {
    let mut target = Target::default();
    assert!(!target.read_only());
    target.options.insert(Target::READ_ONLY.into(), "true".into());
    assert!(target.read_only());
    target.options.insert("create".into(), "off".into());
    assert_eq!(target.flag("create"), Some(false));
    target.options.insert("create".into(), "maybe".into());
    assert_eq!(target.flag("create"), None);
}

#[test]
fn a_discovery_runs_once_when_the_drivers_are_first_asked_for() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static RUNS: AtomicUsize = AtomicUsize::new(0);
    register_discovery(|| {
        RUNS.fetch_add(1, Ordering::SeqCst);
        let mut found = factory("test-discovered", "Found");
        found.external = true;
        register(found);
        register_files(DataFiles::starting_with(
            "test-discovered".into(),
            vec!["DUCK".into()],
            b"DK".to_vec(),
            false,
        ));
    });
    let found = find("test-discovered").expect("registered by the discovery");
    assert!(found.external);
    assert!(all().iter().any(|f| f.id == "test-discovered"));
    let files = for_file(Path::new("/data/x.duck"), |n| Some(b"DK and more"[..n].to_vec()));
    assert_eq!(files.map(|f| f.driver), Some("test-discovered".into()), "any case, by prefix");
    assert!(for_file(Path::new("/data/x.duck"), |_| Some(b"no".to_vec())).is_none());
    assert_eq!(RUNS.load(Ordering::SeqCst), 1, "run once");
}
