use super::*;
use crate::{DataSource, Scope};

fn postgres_like() -> ConnectionSpec {
    ConnectionSpec::server(5432, &["postgresql", "postgres"]).with(
        FieldSpec::new(
            "sslmode",
            "datasources.field_sslmode",
            FieldKind::Choice(vec![
                ("disable".into(), "disable".into()),
                ("prefer".into(), "prefer".into()),
                ("require".into(), "require".into()),
            ]),
            Binding::Option("sslmode".into()),
            FieldTab::Ssl,
        )
        .with_default("prefer"),
    )
}

#[test]
fn a_server_round_trips_through_its_url() {
    let spec = postgres_like();
    let mut source = DataSource::new("shop", "postgres", Scope::Project);
    for (id, text) in [
        ("host", "db.example.com"),
        ("port", "6543"),
        ("user", "app user"),
        ("database", "shop/main"),
        ("sslmode", "require"),
    ] {
        set_value(&mut source, spec.field(id).unwrap(), text);
    }
    source.target.options.insert("application_name".into(), "my app".into());
    let url = to_url(&spec, &source).unwrap();
    assert_eq!(
        url,
        "postgresql://app%20user@db.example.com:6543/shop%2Fmain?application_name=my%20app&sslmode=require"
    );
    let mut back = DataSource::new("shop", "postgres", Scope::Project);
    back.id = source.id;
    assert_eq!(from_url(&spec, &url, &mut back), Ok(None));
    assert_eq!(back, source, "every field comes back");
}

#[test]
fn a_password_in_a_url_is_given_back_never_kept() {
    let spec = postgres_like();
    let mut source = DataSource::new("s", "postgres", Scope::Project);
    let secret = from_url(
        &spec,
        "jdbc:postgres://me:s3%40cret@[::1]:5432/db?password=other&token=x",
        &mut source,
    )
    .unwrap()
    .unwrap();
    assert_eq!(secret.expose(), "s3@cret");
    assert_eq!(source.target.host, "::1");
    assert_eq!(source.target.user, "me");
    assert!(source.target.options.is_empty(), "no secret kept as an option");
    let url = to_url(&spec, &source).unwrap();
    assert_eq!(url, "postgresql://me@[::1]:5432/db");
    assert!(!url.contains("s3"));
}

#[test]
fn a_url_only_replaces_the_options_its_fields_edit() {
    let spec = postgres_like();
    let mut source = DataSource::new("s", "postgres", Scope::Project);
    source.target.options.insert("sslmode".into(), "require".into());
    source.target.options.insert("connect_timeout".into(), "5".into());
    from_url(&spec, "postgresql://localhost/shop", &mut source).unwrap();
    assert_eq!(source.target.options.get("sslmode"), None, "the URL says none");
    assert_eq!(source.target.options.get("connect_timeout").map(String::as_str), Some("5"));
}

#[test]
fn urls_that_arent_the_drivers_are_refused() {
    let spec = postgres_like();
    let mut source = DataSource::new("s", "postgres", Scope::Project);
    assert_eq!(
        from_url(&spec, "mysql://localhost/x", &mut source),
        Err(UrlError::Scheme { expected: "postgresql".into() })
    );
    assert_eq!(from_url(&spec, "postgres://h:99999/x", &mut source), Err(UrlError::Port));
    assert!(matches!(
        from_url(&spec, "postgres://h/%zz", &mut source),
        Err(UrlError::Malformed(_))
    ));
    assert_eq!(from_url(&ConnectionSpec::default(), "x:y", &mut source), Err(UrlError::NoUrl));
}

#[test]
fn a_file_round_trips_through_its_url() {
    let spec = ConnectionSpec::file(&["sqlite", "db"], "sqlite");
    let mut source = DataSource::new("s", "sqlite", Scope::Project);
    set_value(&mut source, spec.field("file").unwrap(), "data/shop.sqlite");
    let url = to_url(&spec, &source).unwrap();
    assert_eq!(url, "sqlite:data/shop.sqlite");
    let mut back = DataSource::new("s", "sqlite", Scope::Project);
    back.id = source.id;
    from_url(&spec, "jdbc:sqlite:data/shop.sqlite", &mut back).unwrap();
    assert_eq!(back, source);
    assert!(!spec.has_secret(), "a file has no password");
}

#[test]
fn fields_are_checked() {
    let spec = postgres_like();
    let port = spec.field("port").unwrap();
    assert_eq!(validate(port, "5432"), Ok(()));
    assert_eq!(validate(port, ""), Ok(()), "the default then");
    assert_eq!(validate(port, "x"), Err(FieldError::NotANumber));
    assert_eq!(validate(port, "70000"), Err(FieldError::OutOfRange { min: 1, max: 65535 }));
    let mode = spec.field("sslmode").unwrap();
    assert_eq!(validate(mode, "require"), Ok(()));
    assert_eq!(validate(mode, "sometimes"), Err(FieldError::NotAChoice));
    let file = ConnectionSpec::file(&[], "csv");
    let source = DataSource::new("s", "csv", Scope::Project);
    assert_eq!(file.problems(&source), vec![("file".to_owned(), FieldError::Required)]);
    let flag =
        FieldSpec::new("x", "x", FieldKind::Flag, Binding::Option("x".into()), FieldTab::Options);
    assert_eq!(validate(&flag, "maybe"), Err(FieldError::NotAFlag));
}

#[test]
fn the_password_is_never_set_in_the_source() {
    let spec = postgres_like();
    let mut source = DataSource::new("s", "postgres", Scope::Project);
    let before = source.clone();
    set_value(&mut source, spec.field("password").unwrap(), "hunter2");
    assert_eq!(source, before);
    let option = FieldSpec::new(
        "p",
        "p",
        FieldKind::Text,
        Binding::Option("db_password".into()),
        FieldTab::Options,
    );
    set_value(&mut source, &option, "hunter2");
    assert_eq!(source, before, "an option named like a secret neither");
    assert_eq!(value(&source, spec.field("password").unwrap()), "");
}
