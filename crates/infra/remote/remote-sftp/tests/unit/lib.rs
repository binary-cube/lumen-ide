use super::*;

#[test]
fn a_path_is_one_shell_word_and_each_digest_has_a_fallback_tool() {
    assert_eq!(shell_quote("/var/www/it's here"), r"'/var/www/it'\''s here'");
    assert_eq!(digest_commands(remote::properties::Algo::Sha256)[1], "shasum -a 256");
    assert_eq!(digest_commands(remote::properties::Algo::Md5)[0], "md5sum");
}

/// A file written with setuid / setgid gets its mode back after a change of its owner or
/// group; another file, a change of the mode itself, or a file without them: nothing more.
#[test]
fn a_new_owner_keeps_the_setuid_bits_of_the_file_just_written() {
    let owner = SetProperties { mode: None, owner: Some(33), group: Some(33) };
    let group = SetProperties { mode: None, owner: None, group: Some(33) };
    let written = || Some(("bin/run.php".to_owned(), 0o4755));
    assert_eq!(mode_to_restore(written(), "bin/run.php", &owner), Some(0o4755));
    assert_eq!(mode_to_restore(written(), "bin/run.php", &group), Some(0o4755));
    assert_eq!(mode_to_restore(written(), "index.php", &owner), None, "another file");
    let chmod = SetProperties { mode: Some(remote::properties::Mode::new(0o644)), ..owner };
    assert_eq!(mode_to_restore(written(), "bin/run.php", &chmod), None, "its mode is set");
    assert_eq!(mode_to_restore(None, "bin/run.php", &owner), None, "none kept");
}

#[test]
fn paths_join_under_the_root() {
    assert_eq!(SftpConnector::full("/var/www/", "app/a.php"), "/var/www/app/a.php");
    assert_eq!(SftpConnector::full("/var/www", ""), "/var/www");
    assert_eq!(SftpConnector::full("/", ""), "/");
    assert_eq!(SftpConnector::full("/", "var/www"), "/var/www");
    assert_eq!(
        format!("{:?}", Credentials::Password("secret".into())),
        "Password(…)",
        "never printed"
    );
}

/// Against a real server: `docker run -d -p 127.0.0.1:2222:22 atmoz/sftp test:secret:::upload`
/// then `EDITOR_SFTP_TEST=127.0.0.1:2222 cargo test -p remote-sftp -- --ignored`; a server
/// with a shell (its digests computed there): `EDITOR_SFTP_ROOT=/home/test/upload
/// EDITOR_SFTP_SHELL=1`.
#[test]
#[ignore = "needs an SFTP server (see the comment)"]
fn a_real_server_reads_writes_lists_renames_and_removes() {
    let Ok(address) = std::env::var("EDITOR_SFTP_TEST") else { return };
    let (host, port) = address.split_once(':').unwrap();
    let dir = std::env::temp_dir().join(format!("sftp-test-{}", std::process::id()));
    let mut connector = SftpConnector {
        host: host.into(),
        port: port.parse().unwrap(),
        user: "test".into(),
        credentials: Credentials::Password("secret".into()),
        // atmoz/sftp's chroot by default; another server's folder by `EDITOR_SFTP_ROOT`.
        root: std::env::var("EDITOR_SFTP_ROOT").unwrap_or_else(|_| "/upload".into()),
        host_keys: HostKeys {
            known: vec![dir.join("known_hosts")],
            learn_into: dir.join("known_hosts"),
            accepted: None,
        },
    };
    // An unknown server: refused, with its fingerprint for the user.
    let fingerprint = match connector.connect() {
        Err(RemoteError::HostKey(f)) => f,
        other => panic!("expected an unknown key: {:?}", other.err()),
    };
    assert!(fingerprint.starts_with("SHA256:"));
    // Accepted: learned, then known.
    connector.host_keys.accepted = Some(fingerprint);
    let mut fs = connector.connect().unwrap();
    connector.host_keys.accepted = None;
    let mut fs2 = connector.connect().expect("known now");

    let written = fs.write("dir/sub/a.txt", b"hello").unwrap();
    assert_eq!((written.size, written.is_dir), (5, false));
    assert_eq!(fs2.read("dir/sub/a.txt").unwrap(), b"hello");
    fs.write("dir/sub/a.txt", b"hello again").unwrap();
    assert_eq!(fs.read("dir/sub/a.txt").unwrap(), b"hello again", "replaced");
    let listed = fs.list("dir").unwrap();
    assert!(listed.iter().any(|e| e.path == "dir/sub" && e.is_dir));
    // Its mode, its owner's id from `stat`, the name from the folder's long names.
    let props = fs.properties(&["dir/sub/a.txt".into(), "dir/sub".into(), "gone".into()]).unwrap();
    let file = props[0].as_ref().expect("there");
    assert!(file.mode.is_some_and(|m| m.bits() & 0o600 == 0o600), "{file:?}");
    let owner = file.owner.as_ref().expect("an owner");
    assert!(owner.id.is_some());
    assert!(props[1].as_ref().is_some_and(|d| d.is_dir));
    assert_eq!(props[2], None);
    // A chroot without accounts (atmoz/sftp's) lists ids only; with them (`etc/passwd` in
    // it), names — and the accounts are read.
    let accounts = fs.accounts().unwrap();
    if let Some(user) = accounts.users.iter().find(|u| u.id == owner.id) {
        assert_eq!(owner.name, user.name, "{owner:?}");
    }
    // Its mode changed, its owner set to itself (allowed): read back as set.
    let set = remote::properties::SetProperties {
        mode: Some(remote::properties::Mode::new(0o640)),
        owner: owner.id,
        group: None,
    };
    fs.set_properties("dir/sub/a.txt", &set).unwrap();
    let after = fs.properties(&["dir/sub/a.txt".into()]).unwrap().remove(0).unwrap();
    assert_eq!(after.mode.map(remote::properties::Mode::bits), Some(0o640));
    assert_eq!(after.owner.and_then(|o| o.id), owner.id);
    // A folder made by a write takes its parent's mode.
    let parent = remote::properties::SetProperties {
        mode: Some(remote::properties::Mode::new(0o775)),
        owner: None,
        group: None,
    };
    fs.set_properties("dir/sub", &parent).unwrap();
    fs.write("dir/sub/new/deeper/b.txt", b"b").unwrap();
    for made in ["dir/sub/new", "dir/sub/new/deeper"] {
        let p = fs.properties(&[made.into()]).unwrap().remove(0).unwrap();
        assert_eq!(p.mode.map(remote::properties::Mode::bits), Some(0o775), "{made}");
    }
    // Written again (a temporary file put in its place): its mode and owner kept.
    fs.write("dir/sub/a.txt", b"hello once more").unwrap();
    let rewritten = fs.properties(&["dir/sub/a.txt".into()]).unwrap().remove(0).unwrap();
    assert_eq!(rewritten.mode.map(remote::properties::Mode::bits), Some(0o640));
    assert_eq!(rewritten.owner.and_then(|o| o.id), owner.id);
    // The folder in one listing: each entry with its mode and owner.
    let listed = fs.list_properties("dir/sub").unwrap();
    let a = listed.iter().find(|p| p.path == "dir/sub/a.txt").expect("listed");
    assert_eq!(a.mode.map(remote::properties::Mode::bits), Some(0o640));
    assert!(a.owner.is_some());
    // Its digest by the server's tools, or `None` (atmoz/sftp: SFTP only, no shell).
    let sum = fs.checksum("dir/sub/a.txt", remote::properties::Algo::Sha256).unwrap();
    assert!(sum.as_ref().is_none_or(|h| h.len() == 64), "{sum:?}");
    // A server with a shell (`EDITOR_SFTP_SHELL=1`) computes it: "hello again"'s SHA-256.
    if std::env::var("EDITOR_SFTP_SHELL").is_ok() {
        assert_eq!(
            sum.as_deref(),
            Some("3908c567feda72bc0dbdb2dff040fe0d3470dcd51b942374378a476930dbf6b3")
        );
    }
    fs.rename("dir/sub/a.txt", "dir/b.txt").unwrap();
    assert!(fs.stat("dir/sub/a.txt").unwrap().is_none());
    assert_eq!(fs.stat("dir/b.txt").unwrap().map(|e| e.size), Some(11));
    fs.remove("dir/b.txt").unwrap();
    fs.remove("dir/sub").unwrap();
    fs.remove("dir").unwrap();
    assert!(fs.list("").unwrap().iter().all(|e| !e.path.starts_with("dir")));
    // Uploads side by side into the same new folders (as "Upload all"): each makes what's
    // missing, none refused because another made it first.
    let new = format!("par-{}", std::process::id());
    std::thread::scope(|s| {
        for i in 0..6 {
            let connector = &connector;
            let new = &new;
            s.spawn(move || {
                let mut fs = connector.connect().unwrap();
                fs.write(&format!("{new}/deep/f{i}.txt"), b"x").unwrap();
            });
        }
    });
    assert_eq!(fs.list(&format!("{new}/deep")).unwrap().len(), 6);
    for i in 0..6 {
        fs.remove(&format!("{new}/deep/f{i}.txt")).unwrap();
    }
    fs.remove(&format!("{new}/deep")).unwrap();
    fs.remove(&new).unwrap();
    let _ = std::fs::remove_dir_all(dir);
}
