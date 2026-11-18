use super::*;

fn connection() -> Connection {
    Connection {
        id: "shop".into(),
        name: "Shop".into(),
        protocol: Protocol::Sftp,
        host: "example.com".into(),
        port: 22,
        user: "ana".into(),
        auth: Auth::Agent,
        root: "/var/www/shop".into(),
        mode: Mode::Mirror,
        excluded: vec!["vendor".into(), "storage/logs/".into(), ".env".into()],
        saved_in: SavedIn::Project,
        secrets_in: SecretsIn::Keychain,
        retry: None,
        uploads: None,
        downloads: None,
        moves: false,
        auto_upload: true,
        write_as: Default::default(),
    }
}

#[test]
fn a_folder_excludes_what_is_in_it_and_nothing_else() {
    let c = connection();
    assert!(c.is_excluded("vendor"));
    assert!(c.is_excluded("vendor/autoload.php"));
    assert!(c.is_excluded("storage/logs/today.log"));
    assert!(c.is_excluded(".env"));
    assert!(!c.is_excluded("vendors.php"), "only whole names");
    assert!(!c.is_excluded("storage/app/x"));
    assert!(!c.is_excluded(".env.example"));
    let mut bare = connection();
    bare.excluded.clear();
    let data = format!("{}/remote-secrets", kernel::brand::PROJECT_DIR);
    assert!(bare.is_excluded(&data), "the project's data folder, whatever the list says");
}

/// `*.log`: every file named so, anywhere (whatever its case); a bare `*` excludes nothing.
#[test]
fn a_pattern_excludes_files_by_their_ending() {
    let mut c = connection();
    c.excluded = vec!["*.log".into(), "*".into()];
    assert!(c.is_excluded("today.log"));
    assert!(c.is_excluded("storage/app/Today.LOG"));
    assert!(!c.is_excluded("logs/app.php"));
    assert!(!c.is_excluded("catalog"));
}

#[test]
fn protocols_and_addresses() {
    assert_eq!(Protocol::from_id("ftps-implicit"), Some(Protocol::Ftps { implicit: true }));
    assert_eq!(Protocol::Ftps { implicit: true }.default_port(), 990);
    assert_eq!(connection().address(), "ana@example.com/var/www/shop");
    let mut c = connection();
    c.port = 2222;
    assert_eq!(c.address(), "ana@example.com:2222/var/www/shop");
}

/// A connection's limits and retries: its own, else the global settings (the app's).
#[test]
fn a_connection_follows_the_global_settings_unless_it_says_otherwise() {
    let mut c = connection();
    let global = Limits { uploads: 3, downloads: 5 };
    assert_eq!(c.limits(global), Limits { uploads: 3, downloads: 5 });
    c.uploads = Some(1);
    assert_eq!(c.limits(global), Limits { uploads: 1, downloads: 5 });
    c.downloads = Some(0);
    assert_eq!(c.limits(global).downloads, 1, "at least one");
    let retry = Retry { attempts: 7, ..Retry::default() };
    assert_eq!(c.retries(retry), retry);
}

/// A server's names that would lead out of the local copy are refused.
#[test]
fn only_plain_names_from_a_server() {
    for good in ["index.php", ".env", "a b", "ă.txt", "..hidden"] {
        assert!(is_plain_name(good), "{good}");
    }
    for bad in ["", ".", "..", "a/b", "..\\..\\x", "C:", "x\0y", "a\nb"] {
        assert!(!is_plain_name(bad), "{bad:?}");
    }
    assert!(is_inside("src/app/x.php"));
    assert!(!is_inside("src/../../x"));
    assert!(!is_inside("src\\..\\x"));
}

/// What a server lists can't land in the protected folders by another spelling: any case
/// (macOS and Windows don't tell `.GIT` from `.git`), nor Windows' other names for them.
#[test]
fn protected_folders_whatever_their_spelling() {
    let bare = Connection { excluded: Vec::new(), ..connection() };
    for path in [".git/config", ".GIT/config", ".Git/hooks/pre-commit"] {
        assert!(bare.is_excluded(path), "{path}");
    }
    let data = kernel::brand::PROJECT_DIR.to_uppercase();
    assert!(bare.is_excluded(&format!("{data}/remote.toml")));
    assert!(!bare.is_excluded(".github/workflows/ci.yml"));
    for bad in [".git.", ".git ", "GIT~1", "con", "NUL.txt", "com1", "lpt9.log", "a?b"] {
        assert!(!is_plain_on_windows(bad), "{bad:?}");
    }
    for good in ["index.php", "com10", "console.php", "a~b", "README"] {
        assert!(is_plain_on_windows(good), "{good}");
    }
}
