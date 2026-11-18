use super::*;

#[test]
fn tells_what_a_value_looks_like() {
    for (value, expected) in [
        ("", ValueKind::Empty),
        ("true", ValueKind::Boolean),
        ("Off", ValueKind::Boolean),
        ("42", ValueKind::Number),
        ("-1.5", ValueKind::Number),
        ("0x1F", ValueKind::Number),
        ("1.2.3", ValueKind::Text),
        ("redis://localhost:6379", ValueKind::Url),
        ("sqlite:///%kernel.project_dir%/var/data.db", ValueKind::Url),
        ("/var/log", ValueKind::Path),
        ("../shared", ValueKind::Path),
        ("%kernel.project_dir%/config/jwt", ValueKind::Path),
        ("hello", ValueKind::Text),
        ("base64:abc=", ValueKind::Text),
    ] {
        assert_eq!(kind(value), expected, "{value}");
    }
}

#[test]
fn secret_keys_by_their_words() {
    for key in [
        "APP_SECRET",
        "GITHUB_TOKEN",
        "DB_PASSWORD",
        "DB_PASS",
        "APP_KEY",
        "APIKEY",
        "JWT_PASSPHRASE",
        "AWS_SECRET_ACCESS_KEY",
        "PRIVATE_KEY",
        "GOOGLE_CREDENTIALS",
    ] {
        assert!(secret_key(key), "{key}");
    }
    for key in ["APP_ENV", "DB_HOST", "KEYBOARD_LAYOUT", "PASSENGERS_MAX", "TOKENIZER_MODE"] {
        assert!(!secret_key(key), "{key}");
    }
}

#[test]
fn a_urls_password() {
    let value = "postgresql://app:!ChangeMe!@127.0.0.1:5432/app?x=1";
    assert_eq!(url_password(value).map(|r| &value[r]), Some("!ChangeMe!"));
    assert_eq!(url_password("redis://:pw@cache").map(|r| r.len()), Some(2));
    assert_eq!(url_password("smtp://user@host"), None);
    assert_eq!(url_password("doctrine://default?a=b:c@d"), None);
    assert_eq!(url_password("not a url"), None);
}

#[test]
fn what_is_hidden() {
    assert_eq!(hidden_part("APP_SECRET", "abc"), Some(0..3));
    assert_eq!(hidden_part("APP_SECRET", ""), None);
    // A URL's password only, whatever the key.
    let dsn = "smtp://mailer:s3cret@localhost:1025";
    assert_eq!(hidden_part("MAILER_DSN", dsn).map(|r| &dsn[r]), Some("s3cret"));
    assert_eq!(hidden_part("APP_ENV", "dev"), None);
}

#[test]
fn a_value_as_shown() {
    assert_eq!(shown("APP_SECRET", "abc123"), "••••••");
    assert_eq!(shown("MAILER_DSN", "smtp://u:pw@host:25"), "smtp://u:••••••@host:25");
    assert_eq!(shown("APP_ENV", "dev"), "dev");
}
