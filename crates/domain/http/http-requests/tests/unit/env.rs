//! The environments of a folder: the public file's, the user's own over it, `$shared`
//! under each.

use super::*;

const PUBLIC_TEXT: &str = r#"{
  "$shared": { "version": "v1", "host": "shared.example.com" },
  "dev": { "host": "localhost", "port": 8080, "debug": true, "Security": { "Auth": {} } },
  "prod": { "host": "api.example.com" }
}"#;
const PRIVATE_TEXT: &str =
    r#"{ "dev": { "token": "secret-dev" }, "prod": { "host": "override" } }"#;

#[test]
fn the_user_s_own_file_wins_and_its_values_are_secret() {
    let envs = Environments::read(&[
        (Source::Private, Some(PRIVATE_TEXT)),
        (Source::Public, Some(PUBLIC_TEXT)),
    ]);
    assert!(envs.problems.is_empty());
    assert_eq!(envs.names().collect::<Vec<_>>(), ["dev", "prod"]);
    let dev = |name| envs.variable(Some("dev"), name).map(|v| (v.value.as_str(), v.source));
    assert_eq!(dev("host"), Some(("localhost", Source::Public)), "its own over $shared");
    assert_eq!(dev("port"), Some(("8080", Source::Public)), "numbers as text");
    assert_eq!(dev("debug"), Some(("true", Source::Public)));
    assert_eq!(dev("Security"), None, "settings aren't variables");
    assert_eq!(dev("version"), Some(("v1", Source::Public)), "$shared under it");
    assert_eq!(dev("token"), Some(("secret-dev", Source::Private)));
    assert!(Source::Private.is_secret() && !Source::Public.is_secret());
    let prod = envs.variable(Some("prod"), "host").unwrap();
    assert_eq!((prod.value.as_str(), prod.source), ("override", Source::Private), "private wins");
    assert_eq!(envs.variable(None, "host").map(|v| v.value.as_str()), Some("shared.example.com"));
    let all = envs.variables(Some("dev"));
    assert_eq!(
        all.keys().copied().collect::<Vec<_>>(),
        ["debug", "host", "port", "token", "version"]
    );
}

#[test]
fn a_broken_file_is_said_and_the_others_still_read() {
    let envs =
        Environments::read(&[(Source::Public, Some(PUBLIC_TEXT)), (Source::Local, Some("{ nope"))]);
    assert_eq!(envs.problems.len(), 1);
    assert_eq!(envs.problems[0].0, Source::Local);
    assert!(envs.variable(Some("dev"), "host").is_some());
    assert!(is_private_file(PRIVATE) && is_private_file(LOCAL) && !is_private_file(PUBLIC));
}

#[test]
fn objects_and_arrays_are_variables_by_their_path() {
    let envs = Environments::read(&[(
        Source::Public,
        Some(
            r#"{"dev": {"a": {"b": 123, "c": {"d": "x"}}, "hosts": ["h1", "h2"], "none": null, "Security": {"Auth": {"x": 1}}}}"#,
        ),
    )]);
    let value = |name| envs.variable(Some("dev"), name).map(|v| v.value.clone());
    assert_eq!(value("a.b").as_deref(), Some("123"));
    assert_eq!(value("a.c.d").as_deref(), Some("x"));
    assert_eq!(value("hosts[1]").as_deref(), Some("h2"));
    assert_eq!(value("a"), None, "an object itself isn't one");
    assert_eq!(value("none"), None);
    assert!(
        envs.variables(Some("dev")).keys().all(|k| !k.starts_with("Security")),
        "settings aren't"
    );
}

#[test]
fn a_variable_is_a_jsonpath_expression_over_the_environment() {
    let envs = Environments::read(&[
        (
            Source::Public,
            Some(
                r#"{"dev": {"client": {"host": {"url": "example.org"}, "host.url": "dotted"}, "users": [{"name": "ana"}, {"name": "bob"}], "clients": {"a": {"id": 1}, "b": {"id": 2}}}}"#,
            ),
        ),
        (Source::Private, Some(r#"{"dev": {"secret": {"key": "k"}}}"#)),
    ]);
    let get = |name| envs.resolve(Some("dev"), name).map(|v| v.value);
    assert_eq!(get("client.host.url").as_deref(), Some("example.org"));
    assert_eq!(get("client.['host.url']").as_deref(), Some("dotted"), "a key with dots");
    assert_eq!(get("users[1].name").as_deref(), Some("bob"));
    assert_eq!(get("users[*].name").as_deref(), Some(r#"["ana","bob"]"#), "several: their array");
    assert_eq!(get("$.clients..id").as_deref(), Some("[1,2]"));
    assert_eq!(get("client.nope"), None);
    assert!(
        envs.resolve(Some("dev"), "secret.key").unwrap().source.is_secret(),
        "a private value stays secret"
    );
}

#[test]
fn an_environment_s_tls_settings_are_read() {
    let env = Environments::read(&[(
        Source::Public,
        Some(
            r#"{"dev": {"host": "e.com", "SSLConfiguration": {"verifyHostCertificate": false, "clientCertificate": "cert.pem", "clientCertificateKey": "key.pem"}}}"#,
        ),
    )]);
    let ssl = env.ssl(Some("dev"));
    assert!(!ssl.verify_host);
    assert_eq!(ssl.client_certificate.as_deref(), Some("cert.pem"));
    assert_eq!(ssl.client_key.as_deref(), Some("key.pem"));
    assert!(env.ssl(None).verify_host, "none: checked");
    assert!(env.variable(Some("dev"), "SSLConfiguration").is_none(), "not a variable");
}
