use super::*;
use crate::properties::Principal;

/// What a fake server was asked to set, and how many times its project folder was read.
#[derive(Default)]
struct Seen {
    set: Vec<(String, SetProperties)>,
    project_reads: usize,
}

/// A server whose project folder belongs to 33:33 (`www-data`), changing owners as `caps`
/// say, refusing them when `refuses`.
struct Fake {
    seen: Arc<Mutex<Seen>>,
    caps: Capabilities,
    refuses: bool,
}

impl RemoteFs for Fake {
    fn list(&mut self, _: &str) -> Result<Vec<RemoteEntry>, RemoteError> {
        Ok(Vec::new())
    }
    fn read(&mut self, path: &str) -> Result<Vec<u8>, RemoteError> {
        Err(RemoteError::NotFound(path.into()))
    }
    fn write(&mut self, path: &str, data: &[u8]) -> Result<RemoteEntry, RemoteError> {
        Ok(RemoteEntry { path: path.into(), is_dir: false, size: data.len() as u64, modified: 0 })
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
    fn properties(&mut self, paths: &[String]) -> Result<Vec<Option<Properties>>, RemoteError> {
        self.seen.lock().unwrap().project_reads += 1;
        Ok(paths
            .iter()
            .map(|p| {
                Some(Properties {
                    path: p.clone(),
                    is_dir: true,
                    size: 0,
                    modified: 0,
                    mode: None,
                    owner: Some(Principal::named("www-data", Some(33))),
                    group: Some(Principal::named("www-data", Some(33))),
                })
            })
            .collect())
    }
    fn capabilities(&mut self) -> Capabilities {
        self.caps.clone()
    }
    fn set_properties(&mut self, path: &str, set: &SetProperties) -> Result<(), RemoteError> {
        if self.refuses {
            return Err(RemoteError::Server("permission denied".into()));
        }
        self.seen.lock().unwrap().set.push((path.into(), *set));
        Ok(())
    }
}

struct FakeConnector {
    seen: Arc<Mutex<Seen>>,
    caps: Capabilities,
    refuses: bool,
}

impl Connector for FakeConnector {
    fn connect(&self) -> Result<Box<dyn RemoteFs>, RemoteError> {
        Ok(Box::new(Fake {
            seen: Arc::clone(&self.seen),
            caps: self.caps.clone(),
            refuses: self.refuses,
        }))
    }
}

/// A wrapped connector, what its server was asked, the paths refused.
type Wrapped = (Arc<dyn Connector>, Arc<Mutex<Seen>>, Arc<Mutex<Vec<String>>>);

fn sftp() -> Capabilities {
    Capabilities { chmod: true, chown: true, chgrp: true, server_hashes: Vec::new() }
}

/// A connector writing as `write_as`; what it set, and what was refused.
fn connector(write_as: WriteAs, caps: Capabilities, refuses: bool) -> Wrapped {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let refused = Arc::new(Mutex::new(Vec::new()));
    let told = Arc::clone(&refused);
    let inner = Arc::new(FakeConnector { seen: Arc::clone(&seen), caps, refuses });
    let wrapped = OwnedWrites::wrap(
        inner,
        write_as,
        Arc::new(move |path: &str, _: &RemoteError| told.lock().unwrap().push(path.into())),
    );
    (wrapped, seen, refused)
}

#[test]
fn files_written_take_the_project_folder_s_owner() {
    let (connector, seen, _) = connector(WriteAs::Project, sftp(), false);
    for _ in 0..2 {
        let mut fs = connector.connect().unwrap();
        fs.write("app/new.php", b"<?php").unwrap();
        fs.write_moving("app/old.php", b"<?php", &mut |_| true).unwrap();
        fs.rename("a", "b").unwrap();
    }
    let seen = seen.lock().unwrap();
    let www = SetProperties { mode: None, owner: Some(33), group: Some(33) };
    assert_eq!(seen.set.len(), 4, "each file written, nothing else");
    assert!(seen.set.iter().all(|(_, set)| *set == www));
    assert_eq!(seen.project_reads, 1, "the project's folder read once");
}

#[test]
fn a_chosen_owner_or_group_alone_is_set() {
    let chosen =
        WriteAs::Chosen { owner: None, group: Some(Principal::named("deploy", Some(1001))) };
    let (connector, seen, _) = connector(chosen, sftp(), false);
    connector.connect().unwrap().write("index.php", b"").unwrap();
    let set = seen.lock().unwrap().set.clone();
    assert_eq!(
        set,
        [("index.php".to_owned(), SetProperties { mode: None, owner: None, group: Some(1001) })]
    );
}

#[test]
fn ftp_and_the_server_s_way_write_as_before() {
    // FTP can't change owners: nothing is asked of it.
    let (connector, seen, _) = connector(WriteAs::Project, Capabilities::default(), false);
    connector.connect().unwrap().write("index.php", b"").unwrap();
    assert!(seen.lock().unwrap().set.is_empty());
    assert_eq!(seen.lock().unwrap().project_reads, 0, "its folder never read");
    let (connector, seen, _) = connector_server();
    connector.connect().unwrap().write("index.php", b"").unwrap();
    assert!(seen.lock().unwrap().set.is_empty());
    assert_eq!(seen.lock().unwrap().project_reads, 0);
}

fn connector_server() -> Wrapped {
    connector(WriteAs::Server, sftp(), false)
}

#[test]
fn a_refusal_is_told_once_and_the_files_stay_written() {
    let (connector, _, refused) = connector(WriteAs::Project, sftp(), true);
    let mut fs = connector.connect().unwrap();
    assert!(fs.write("index.php", b"x").is_ok());
    assert!(fs.write("other.php", b"y").is_ok());
    assert_eq!(*refused.lock().unwrap(), ["index.php"], "not asked again in that session");
}

/// A project folder with no owner known (ids hidden) is read once, not at every session.
#[test]
fn a_project_folder_without_ids_is_read_once() {
    let seen = Arc::new(Mutex::new(Seen::default()));
    struct NoIds(Fake);
    impl RemoteFs for NoIds {
        fn list(&mut self, d: &str) -> Result<Vec<RemoteEntry>, RemoteError> {
            self.0.list(d)
        }
        fn read(&mut self, p: &str) -> Result<Vec<u8>, RemoteError> {
            self.0.read(p)
        }
        fn write(&mut self, p: &str, d: &[u8]) -> Result<RemoteEntry, RemoteError> {
            self.0.write(p, d)
        }
        fn remove(&mut self, p: &str) -> Result<(), RemoteError> {
            self.0.remove(p)
        }
        fn rename(&mut self, a: &str, b: &str) -> Result<(), RemoteError> {
            self.0.rename(a, b)
        }
        fn stat(&mut self, p: &str) -> Result<Option<RemoteEntry>, RemoteError> {
            self.0.stat(p)
        }
        fn capabilities(&mut self) -> Capabilities {
            self.0.capabilities()
        }
        fn properties(&mut self, paths: &[String]) -> Result<Vec<Option<Properties>>, RemoteError> {
            self.0.seen.lock().unwrap().project_reads += 1;
            Ok(paths.iter().map(|_| None).collect())
        }
    }
    struct NoIdsConnector(Arc<Mutex<Seen>>);
    impl Connector for NoIdsConnector {
        fn connect(&self) -> Result<Box<dyn RemoteFs>, RemoteError> {
            let fake = Fake { seen: Arc::clone(&self.0), caps: sftp(), refuses: false };
            Ok(Box::new(NoIds(fake)))
        }
    }
    let wrapped = OwnedWrites::wrap(
        Arc::new(NoIdsConnector(Arc::clone(&seen))),
        WriteAs::Project,
        Arc::new(|_: &str, _: &RemoteError| {}),
    );
    for _ in 0..3 {
        wrapped.connect().unwrap().write("a.php", b"").unwrap();
    }
    assert_eq!(seen.lock().unwrap().project_reads, 1);
    assert!(seen.lock().unwrap().set.is_empty());
}
