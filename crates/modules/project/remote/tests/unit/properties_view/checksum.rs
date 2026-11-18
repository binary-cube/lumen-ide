// The tests write a local copy's file.
#![allow(clippy::disallowed_methods)]

use super::*;
use remote::application::RemoteFs;
use remote::{RemoteEntry, RemoteError};

/// Known digests of "abc".
#[test]
fn digests_match_the_known_values() {
    let abc = |algo| digest(algo, &b"abc"[..]).unwrap();
    assert_eq!(abc(Algo::Md5), "900150983cd24fb0d6963f7d28e17f72");
    assert_eq!(abc(Algo::Sha1), "a9993e364706816aba3e25717850c26c9cd0d89d");
    assert_eq!(
        abc(Algo::Sha256),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    for algo in Algo::ALL {
        assert_eq!(abc(algo).len(), algo.hex_len(), "{algo:?}");
    }
}

/// A server that computes nothing (its files downloaded) or everything (`on_server`).
struct Server {
    on_server: bool,
}

impl RemoteFs for Server {
    fn list(&mut self, _: &str) -> Result<Vec<RemoteEntry>, RemoteError> {
        Ok(Vec::new())
    }
    fn read(&mut self, path: &str) -> Result<Vec<u8>, RemoteError> {
        match path {
            "same.txt" | "gone-here.txt" => Ok(b"abc".to_vec()),
            "changed.txt" => Ok(b"abcd".to_vec()),
            _ => Err(RemoteError::NotFound(path.into())),
        }
    }
    fn write(&mut self, path: &str, _: &[u8]) -> Result<RemoteEntry, RemoteError> {
        Err(RemoteError::NotFound(path.into()))
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
    fn checksum(&mut self, path: &str, algo: Algo) -> Result<Option<String>, RemoteError> {
        if !self.on_server {
            return Ok(None);
        }
        Ok(self.read(path).ok().map(|b| digest(algo, b.as_slice()).unwrap()))
    }
}

struct Connect(bool);

impl Connector for Connect {
    fn connect(&self) -> Result<Box<dyn RemoteFs>, RemoteError> {
        Ok(Box::new(Server { on_server: self.0 }))
    }
}

/// On the server or downloaded, each compared with the local copy; a missing file fails.
#[test]
fn each_file_is_computed_and_compared_with_the_local_copy() {
    let local = std::env::temp_dir().join(format!("checksum-{}", std::process::id()));
    std::fs::create_dir_all(&local).unwrap();
    std::fs::write(local.join("same.txt"), "abc").unwrap();
    std::fs::write(local.join("changed.txt"), "abc").unwrap();
    let paths: Vec<String> =
        ["same.txt", "changed.txt", "gone-here.txt", "nowhere.txt"].map(String::from).to_vec();
    for on_server in [false, true] {
        let mut rows = vec![Sum::Waiting; paths.len()];
        let stop = AtomicBool::new(false);
        compute(&Connect(on_server), &local, &paths, Algo::Md5, &stop, &mut |i, s| rows[i] = s);
        let local_of = |s: &Sum| match s {
            Sum::Done { local, sum } => {
                assert_eq!(sum.on_server, on_server);
                Some(*local)
            }
            _ => None,
        };
        assert_eq!(local_of(&rows[0]), Some(Local::Same));
        assert_eq!(local_of(&rows[1]), Some(Local::Different));
        assert_eq!(local_of(&rows[2]), Some(Local::Missing));
        assert!(matches!(rows[3], Sum::Failed(_)) || on_server);
    }
    let _ = std::fs::remove_dir_all(local);
}

/// Another algorithm lets the last results go and is remembered.
#[test]
fn choosing_an_algorithm_resets_the_rows() {
    let mut sums = Checksums::for_files(["a".to_owned()]);
    sums.rows[0].1 = Sum::Failed("x".into());
    sums.choose(Algo::Sha1);
    assert_eq!(sums.rows[0].1, Sum::Waiting);
    assert_eq!(last_algo(), Algo::Sha1);
    sums.choose(Algo::Sha256);
}
