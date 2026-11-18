use super::*;

#[test]
fn fingerprints_and_paths() {
    assert_eq!(fingerprint(b"abc").len(), "SHA256:".len() + 64);
    let connector = FtpConnector {
        host: "h".into(),
        port: 21,
        user: "u".into(),
        password: "secret".into(),
        security: Security::Explicit,
        root: "/www/".into(),
        trusted: Vec::new(),
        tls12: Default::default(),
        chmod: Default::default(),
        hashes: Default::default(),
    };
    assert_eq!(connector.full("a/b.php"), "/www/a/b.php");
    assert!(!format!("{connector:?}").contains("secret"), "never printed");
}

/// Against a real server (pure-ftpd with TLS, a self-signed certificate):
/// `EDITOR_FTP_TEST=127.0.0.1:2121 cargo test -p remote-ftp -- --ignored`.
#[test]
#[ignore = "needs an FTP server (see the comment)"]
fn a_real_server_over_plain_ftp_and_explicit_tls() {
    let Ok(address) = std::env::var("EDITOR_FTP_TEST") else { return };
    let (host, port) = address.split_once(':').unwrap();
    let mut connector = FtpConnector {
        host: host.into(),
        port: port.parse().unwrap(),
        user: "test".into(),
        password: "secret".into(),
        security: Security::Explicit,
        root: "/home/test".into(),
        trusted: Vec::new(),
        tls12: Default::default(),
        chmod: Default::default(),
        hashes: Default::default(),
    };
    // Its own certificate: refused, with the fingerprint; accepted, it's trusted.
    let fingerprint = match connector.connect() {
        Err(RemoteError::HostKey(f)) => f,
        other => panic!("expected an untrusted certificate: {:?}", other.err()),
    };
    connector.trusted.push(fingerprint);
    for security in [Security::Explicit, Security::Plain] {
        connector.security = security;
        let mut fs = match connector.connect() {
            Ok(fs) => fs,
            // A server that wants encryption refuses plain sign-ins.
            Err(RemoteError::Auth(_)) if security == Security::Plain => continue,
            Err(e) => panic!("{security:?}: {e}"),
        };
        // As the domain does: a transient failure, a new session, again.
        macro_rules! again {
            ($op:expr) => {{
                let mut tries = 0;
                loop {
                    match $op(&mut fs) {
                        Err(e) if e.is_transient() && tries < 3 => {
                            tries += 1;
                            fs = connector.connect().unwrap();
                        }
                        other => break other,
                    }
                }
            }};
        }
        let written =
            again!(|fs: &mut Box<dyn RemoteFs>| fs.write("dir/sub/a.txt", b"hello")).unwrap();
        assert_eq!(written.size, 5);
        assert_eq!(
            again!(|fs: &mut Box<dyn RemoteFs>| fs.read("dir/sub/a.txt")).unwrap(),
            b"hello"
        );
        again!(|fs: &mut Box<dyn RemoteFs>| fs.write("dir/sub/a.txt", b"hello again")).unwrap();
        let read = again!(|fs: &mut Box<dyn RemoteFs>| fs.read("dir/sub/a.txt")).unwrap();
        assert_eq!(read, b"hello again", "replaced");
        let listed = again!(|fs: &mut Box<dyn RemoteFs>| fs.list("dir")).unwrap();
        assert!(listed.iter().any(|e| e.path == "dir/sub" && e.is_dir));
        // Its mode and owner (`MLST`, or the folder's `LIST`).
        let paths = ["dir/sub/a.txt".to_owned(), "dir/sub".to_owned(), "gone".to_owned()];
        let props = again!(|fs: &mut Box<dyn RemoteFs>| fs.properties(&paths)).unwrap();
        let file = props[0].as_ref().expect("there");
        assert_eq!(file.size, 11);
        assert!(file.mode.is_some_and(|m| m.bits() & 0o600 == 0o600), "{file:?}");
        assert!(file.owner.is_some(), "{file:?}");
        assert!(props[1].as_ref().is_some_and(|d| d.is_dir));
        assert_eq!(props[2], None);
        // `SITE CHMOD`, read back; owners can't be changed over FTP.
        let set = remote::properties::SetProperties {
            mode: Some(remote::properties::Mode::new(0o640)),
            ..Default::default()
        };
        // A server without SITE commands (Alpine's pure-ftpd) refuses it: learned, the
        // permissions then shown only.
        match again!(|fs: &mut Box<dyn RemoteFs>| fs.set_properties("dir/sub/a.txt", &set)) {
            Ok(()) => {
                let after = again!(|fs: &mut Box<dyn RemoteFs>| fs.properties(&paths)).unwrap();
                assert_eq!(after[0].as_ref().and_then(|p| p.mode).map(|m| m.bits()), Some(0o640));
                assert!(fs.capabilities().chmod);
            }
            Err(e) => {
                assert!(e.to_string().contains("SITE CHMOD"), "{e}");
                assert!(!fs.capabilities().chmod);
            }
        }
        assert!(!fs.capabilities().chown);
        // The folder in one listing (MLSD's facts or LIST's lines): each with its mode.
        let listed = again!(|fs: &mut Box<dyn RemoteFs>| fs.list_properties("dir/sub")).unwrap();
        let a = listed.iter().find(|p| p.path == "dir/sub/a.txt").expect("listed");
        assert!(a.mode.is_some() && a.owner.is_some(), "{a:?}");
        // Its digest by the server (if it says it can), else `None`: computed here.
        let sum = again!(|fs: &mut Box<dyn RemoteFs>| fs
            .checksum("dir/sub/a.txt", remote::properties::Algo::Sha256))
        .unwrap();
        assert!(sum.as_ref().is_none_or(|h| h.len() == 64), "{sum:?}");
        let chown = remote::properties::SetProperties { owner: Some(0), ..Default::default() };
        assert!(fs.set_properties("dir/sub/a.txt", &chown).is_err());
        again!(|fs: &mut Box<dyn RemoteFs>| fs.rename("dir/sub/a.txt", "dir/b.txt")).unwrap();
        assert!(again!(|fs: &mut Box<dyn RemoteFs>| fs.stat("dir/sub/a.txt")).unwrap().is_none());
        let b = again!(|fs: &mut Box<dyn RemoteFs>| fs.stat("dir/b.txt")).unwrap();
        assert_eq!(b.map(|e| e.size), Some(11));
        again!(|fs: &mut Box<dyn RemoteFs>| fs.remove("dir/b.txt")).unwrap();
        again!(|fs: &mut Box<dyn RemoteFs>| fs.remove("dir/sub")).unwrap();
        again!(|fs: &mut Box<dyn RemoteFs>| fs.remove("dir")).unwrap();
    }
}
