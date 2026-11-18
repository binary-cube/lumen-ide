use super::*;

#[test]
fn a_source_has_its_own_identity_and_keeps_no_secret() {
    let a = DataSource::new("shop", "postgres", Scope::Project);
    let b = DataSource::new("shop", "postgres", Scope::Project);
    assert_ne!(a.id, b.id, "two sources of one name are two sources");
    assert_eq!(a.secrets, SecretPolicy::ProjectVault, "the vault by default");
    assert!(!a.read_only);
    // Its id reads back from its text (as it's saved).
    let text = a.id.to_string();
    assert_eq!(text.len(), 36);
    assert_eq!(DataSourceId::parse(&text), Some(a.id));
    assert_eq!(DataSourceId::parse("shop"), None);
    assert_eq!(a.secret_id(), format!("data-source/{text}"));
}

#[test]
fn a_file_inside_the_project_is_kept_relative() {
    let root = Path::new("/work/shop");
    let inside = stored_path(Path::new("/work/shop/data/app.sqlite"), root);
    assert_eq!(inside, Path::new("data/app.sqlite"));
    let outside = stored_path(Path::new("/tmp/other.db"), root);
    assert_eq!(outside, Path::new("/tmp/other.db"));

    let mut source = DataSource::new("app", "sqlite", Scope::Project);
    assert_eq!(source.file_in(root), None);
    source.file = Some(inside);
    assert_eq!(
        source.file_in(Path::new("/moved/shop")),
        Some(PathBuf::from("/moved/shop/data/app.sqlite"))
    );
    source.file = Some(outside);
    assert_eq!(source.file_in(root), Some(PathBuf::from("/tmp/other.db")));
}

#[test]
fn a_source_is_configured_once_it_says_where_to_go() {
    let mut file = DataSource::new("app", "sqlite", Scope::Project);
    assert!(!file.is_configured(true), "a file-based one without its file");
    file.file = Some(PathBuf::from("data/app.sqlite"));
    assert!(file.is_configured(true));
    let mut server = DataSource::new("prod", "postgres", Scope::Global);
    assert!(!server.is_configured(false), "added from the menu, nothing filled in yet");
    server.target.host = "db.example.com".into();
    assert!(server.is_configured(false));
}

#[test]
fn the_recent_drivers_are_the_newest_sources_ones_each_once() {
    let source = |driver: &str| DataSource::new("x", driver, Scope::Project);
    let sources = [source("sqlite"), source("csv"), source("postgres"), source("sqlite")];
    assert_eq!(recent_drivers(&sources, 3), ["sqlite", "postgres", "csv"]);
    assert_eq!(recent_drivers(&sources, 2), ["sqlite", "postgres"]);
    assert!(recent_drivers(&[], 3).is_empty());
}

#[test]
fn a_new_name_is_numbered_past_those_taken() {
    assert_eq!(unique_name("shop", ["blog"]), "shop");
    assert_eq!(unique_name("shop", ["shop", "shop (2)"]), "shop (3)");
}

#[test]
fn options_holding_secrets_are_told_apart() {
    for name in ["password", "PGPASSWORD", "ssh_passphrase", "token", "api_key"] {
        assert!(is_secret_option(name), "{name}");
    }
    for name in ["sslmode", "charset", "read_only", "header"] {
        assert!(!is_secret_option(name), "{name}");
    }
}
