//! An app's page: only its folder's files served, each with its type, the content policy on
//! HTML only, its own URLs told from the rest, messages written as safe script literals.

use super::*;

/// A fresh folder under the system's temp folder (removed by the test).
fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("web-app-{:032x}", kernel::id::new_uuid()));
    std::fs::create_dir_all(dir.join("app/js")).unwrap();
    std::fs::write(dir.join("app/index.html"), "<p>app</p>").unwrap();
    std::fs::write(dir.join("app/js/main.js"), "let a = 1;").unwrap();
    std::fs::write(dir.join("secret.txt"), "outside").unwrap();
    dir
}

#[test]
fn paths_resolve_inside_the_folder_only() {
    let root = Path::new("/srv/app");
    assert_eq!(resolve(root, "/index.html"), Some(root.join("index.html")));
    assert_eq!(resolve(root, "/js//main.js"), Some(root.join("js").join("main.js")));
    assert_eq!(resolve(root, "/a%20b.html"), Some(root.join("a b.html")));
    for hostile in [
        "/../secret.txt",
        "/js/../../secret.txt",
        "/%2e%2e/secret.txt",
        "/%2E%2E%2Fsecret.txt",
        "/..%5csecret.txt",
        "/js/..\\..\\secret.txt",
        "/C:/Windows/win.ini",
        "/c:%5cwindows",
        "/index.html%00.png",
        "/./index.html",
        "/con.txt",
        "/js/NUL",
        "/index.html.",
        "/bad%zz",
        "/%ff%fe",
        "/",
        "",
    ] {
        assert_eq!(resolve(root, hostile), None, "{hostile}");
    }
}

#[test]
fn a_request_is_answered_from_the_folder_and_refused_outside_it() {
    let dir = scratch();
    let root = dir.join("app");
    let page = serve(&root, "GET", Some("localhost"), "/index.html");
    assert_eq!((page.status, page.mime, page.policy), (200, "text/html; charset=utf-8", true));
    assert_eq!(page.body, b"<p>app</p>");
    let script = serve(&root, "GET", Some("localhost"), "/js/main.js");
    assert_eq!(
        (script.status, script.mime, script.policy),
        (200, "text/javascript; charset=utf-8", false)
    );
    let head = serve(&root, "HEAD", Some("localhost"), "/js/main.js");
    assert_eq!((head.status, head.body.len()), (200, 0));
    assert_eq!(serve(&root, "GET", Some("localhost"), "/../secret.txt").status, 404);
    assert_eq!(serve(&root, "GET", Some("localhost"), "/missing.js").status, 404);
    assert_eq!(serve(&root, "GET", Some("localhost"), "/js").status, 404, "a folder");
    assert_eq!(serve(&root, "GET", Some("example.com"), "/index.html").status, 404);
    assert_eq!(serve(&root, "GET", None, "/index.html").status, 404);
    assert_eq!(serve(&root, "POST", Some("localhost"), "/index.html").status, 405);
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn a_link_leading_out_of_the_folder_is_refused() {
    let dir = scratch();
    let root = dir.join("app");
    std::os::unix::fs::symlink(dir.join("secret.txt"), root.join("leak.txt")).unwrap();
    std::os::unix::fs::symlink(&dir, root.join("up")).unwrap();
    std::os::unix::fs::symlink(root.join("js/main.js"), root.join("inside.js")).unwrap();
    assert_eq!(serve(&root, "GET", Some("localhost"), "/leak.txt").status, 404);
    assert_eq!(serve(&root, "GET", Some("localhost"), "/up/secret.txt").status, 404);
    assert_eq!(serve(&root, "GET", Some("localhost"), "/inside.js").status, 200, "inside: fine");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn files_have_their_types() {
    for (name, mime_type) in [
        ("a.HTML", "text/html; charset=utf-8"),
        ("a.js", "text/javascript; charset=utf-8"),
        ("a.css", "text/css; charset=utf-8"),
        ("a.json", "application/json"),
        ("a.svg", "image/svg+xml"),
        ("a.png", "image/png"),
        ("a.gif", "image/gif"),
        ("a.jpeg", "image/jpeg"),
        ("a.woff", "font/woff"),
        ("a.woff2", "font/woff2"),
        ("a.ttf", "font/ttf"),
        ("a.txt", "text/plain; charset=utf-8"),
        ("a.xml", "application/xml"),
        ("a.wasm", "application/wasm"),
        ("a", "application/octet-stream"),
        ("a.exe", "application/octet-stream"),
    ] {
        assert_eq!(mime(Path::new(name)), mime_type, "{name}");
    }
}

#[test]
fn the_policy_keeps_the_page_off_the_network() {
    for part in [
        "default-src 'self' local-app:",
        "connect-src 'self' local-app: data: blob:",
        "object-src 'none'",
        "base-uri 'none'",
        "frame-src 'self' local-app:",
    ] {
        assert!(CSP.contains(part), "{part}");
    }
    assert!(!CSP.contains("http"), "no web origin allowed");
    assert!(!CSP.contains('*'));
}

#[test]
fn only_the_apps_own_urls_are_followed() {
    assert_eq!(entry_url("index.html?embed=1"), "local-app://localhost/index.html?embed=1");
    assert_eq!(entry_url("/index.html"), "local-app://localhost/index.html");
    for own in [
        "local-app://localhost/index.html",
        "https://local-app.localhost/js/main.js",
        "http://local-app.localhost/",
        "about:blank",
    ] {
        assert!(allows_navigation(own), "{own}");
    }
    for other in [
        "https://example.com/",
        "https://local-app.localhost.example.com/",
        "https://local-app.evil/",
        "local-app://other/index.html",
        "local-app://localhost",
        "file:///etc/passwd",
        "data:text/html,<p>",
        "javascript:alert(1)",
        "about:config",
    ] {
        assert!(!allows_navigation(other), "{other}");
    }
}

/// The own URLs' prefixes are the scheme's and the host's.
#[test]
fn the_own_prefixes_name_the_scheme_and_host() {
    assert_eq!(
        OWN_PREFIXES,
        [
            format!("{SCHEME}://{HOST}").as_str(),
            format!("https://{SCHEME}.{HOST}").as_str(),
            format!("http://{SCHEME}.{HOST}").as_str(),
        ]
    );
}

/// A page loads its own scheme's resources and what it made itself, in any case; nothing of
/// the network, nor an address only starting like a scheme.
#[test]
fn a_page_loads_only_its_own_resources() {
    assert!(allows_load("local-app://localhost/app.js"));
    assert!(allows_load("LOCAL-APP://localhost/app.js"));
    assert!(allows_load("data:image/png;base64,AA=="));
    assert!(allows_load("Blob:local-app://localhost/1"));
    assert!(!allows_load("https://tracker.example/pixel.gif"));
    assert!(!allows_load("local-apps://x"), "another scheme starting the same");
    assert!(!allows_load("database://x"));
    assert!(!allows_load("data"), "a scheme's name alone");
    assert!(!allows_load("dätä:x"), "not ASCII: never split inside a character");
    assert!(!allows_load(""));
}

#[test]
fn a_message_becomes_a_safe_script_literal() {
    assert_eq!(js_string("a\"b\\c\nd"), r#""a\"b\\c\nd""#);
    assert_eq!(js_string("x\u{2028}y\u{2029}z"), r#""x\u2028y\u2029z""#);
    assert_eq!(js_string("</script><b>"), r#""\u003c/script>\u003cb>""#);
    assert_eq!(js_string("\u{0}\u{1f}"), r#""\u0000\u001f""#);
    assert_eq!(js_string("ăș 🙂"), "\"ăș 🙂\"");
    let script = receive_script("{\"action\":\"load\"}</script>");
    assert_eq!(
        script,
        r#"window.__hostReceive && window.__hostReceive("{\"action\":\"load\"}\u003c/script>");"#
    );
}

/// A folder kept serves many requests, its place on disk found once; one not there yet is
/// looked for again (and served once it is).
#[test]
fn a_root_kept_serves_many_requests() {
    let dir = scratch();
    let root = Root::new(dir.join("app"));
    assert_eq!(root.serve("GET", Some("localhost"), "/index.html").status, 200);
    assert_eq!(root.serve("GET", Some("localhost"), "/js/main.js").status, 200);
    assert_eq!(root.serve("GET", Some("LOCALHOST"), "/index.html").status, 200, "any case");
    assert_eq!(root.serve("GET", Some("page"), "/index.html").status, 404, "another host");
    assert_eq!(root.serve("GET", Some("localhost"), "/../secret.txt").status, 404);
    let later = Root::new(dir.join("later"));
    assert_eq!(later.serve("GET", Some("localhost"), "/index.html").status, 404, "not there");
    std::fs::create_dir_all(dir.join("later")).unwrap();
    std::fs::write(dir.join("later/index.html"), "<p>later</p>").unwrap();
    let page = later.serve("GET", Some("localhost"), "/index.html");
    assert_eq!((page.status, page.body.as_slice()), (200, b"<p>later</p>".as_slice()));
    let _ = std::fs::remove_dir_all(dir);
}
