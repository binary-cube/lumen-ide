use super::*;

/// The names of secrets, however written; ordinary names left alone.
#[test]
fn a_secret_s_name_hides_its_value_whole() {
    for name in [
        "password",
        "Password",
        "db_password",
        "access-token",
        "apiKey",
        "API_KEY",
        "client_secret",
        "set_cookie",
        "Authorization",
        "private_key",
        "passphrase",
    ] {
        assert!(secret_name(name), "{name}");
    }
    for name in
        ["path", "keys", "count", "tokens_used", "language", "author", "passing", "keyboard"]
    {
        assert!(!secret_name(name), "{name}");
    }
}

/// Each shape hidden, what's around it kept.
#[test]
fn secrets_of_a_known_shape_are_hidden_in_a_text() {
    let cases = [
        ("Authorization: Bearer abc.def-123_xyz", "Authorization: Bearer ***"),
        ("basic dXNlcjpwYXNz", "basic ***"),
        ("postgres://shop:hunter2@db.local:5432/shop", "postgres://shop:***@db.local:5432/shop"),
        ("https://user:p%40ss@github.com/a.git", "https://user:***@github.com/a.git"),
        ("host=db user=u password=hunter2 port=5432", "host=db user=u password=*** port=5432"),
        ("Server=x;Pwd=s3cret;Database=y", "Server=x;Pwd=***;Database=y"),
        ("?api_key=abcdef&page=2", "?api_key=***&page=2"),
        ("token: \"quoted secret\" next", "token: *** next"),
        ("jwt eyJhbGciOiJIUzI1.eyJzdWIiOiIxMjM0.SflKxwRJSMeKKF2QT4fw", "jwt ***"),
        ("pushed with ghp_0123456789abcdefghijABCDEFGHIJ", "pushed with ***"),
        ("key AKIAIOSFODNN7EXAMPLE used", "key *** used"),
        ("sk-proj-abcdefghijklmnopqrstuvwx", "***"),
        ("CREATE USER shop IDENTIFIED BY 'hunter2';", "CREATE USER shop IDENTIFIED BY '***';"),
        ("ALTER ROLE app WITH PASSWORD 'p@ss w0rd'", "ALTER ROLE app WITH PASSWORD '***'"),
    ];
    for (given, hidden) in cases {
        assert_eq!(text(given), hidden, "{given}");
    }
    let pem = "a\n-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXk\n-----END OPENSSH PRIVATE KEY-----\nz";
    assert_eq!(text(pem), "a\n-----PRIVATE KEY ***-----\nz");
}

/// What only looks a little like one stays (a path, a URL without a password, code).
#[test]
fn ordinary_text_is_left_whole() {
    for keep in [
        "/Users/me/work/shop/src/Token.php",
        "https://github.com/binary-cube/lumen-ide",
        "ssh://git@github.com/a/b.git",
        "the token stream of index.php",
        "fn password_field(input: &str)",
        "SELECT * FROM users WHERE id = 1",
    ] {
        assert!(matches!(text(keep), Cow::Borrowed(_)), "{keep} → {}", text(keep));
    }
}

/// Through a line: a field named as a secret hidden whole, a message's secret hidden.
#[test]
fn a_line_never_holds_a_secret() {
    use tracing_subscriber::layer::SubscriberExt;
    let (tx, rx) = std::sync::mpsc::sync_channel(4);
    let layer = crate::JsonLayer::new(tx, "r".into());
    let subscriber = tracing_subscriber::registry().with(layer);
    tracing::subscriber::with_default(subscriber, || {
        let url = "mysql://root:toor@localhost/shop";
        tracing::info!(password = "hunter2", count = 2, %url, "connected with Bearer abcdefgh");
    });
    let Ok(crate::writer::Msg::Line { text, .. }) = rx.try_recv() else { panic!("a line") };
    assert!(
        !text.contains("hunter2") && !text.contains("toor") && !text.contains("abcdefgh"),
        "{text}"
    );
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["fields"]["password"], "***");
    assert_eq!(v["fields"]["count"], 2);
    assert_eq!(v["fields"]["url"], "mysql://root:***@localhost/shop");
    assert_eq!(v["msg"], "connected with Bearer ***");
}

/// Every shape written compiles (none silently left out).
#[test]
fn every_shape_compiles() {
    assert_eq!(shapes(), PATTERNS.len());
}
