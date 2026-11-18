//! The environments as tables: both files read together, written back each to its own,
//! the settings and the JSON values kept.

use super::*;

const PUBLIC: &str = r#"{"$shared": {"version": "v1"}, "dev": {"host": "e.com", "port": 8080, "client": {"id": "c"}, "SSLConfiguration": {"verifyHostCertificate": false}}}"#;
const PRIVATE: &str = r#"{"dev": {"token": "t0k"}}"#;

#[test]
fn both_files_are_one_table_per_environment() {
    let read = tables(Some(PUBLIC), Some(PRIVATE));
    let names: Vec<&str> = read.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["$shared", "dev"]);
    let dev = &read[1];
    let row = |name: &str| dev.rows.iter().find(|r| r.name == name).unwrap();
    assert_eq!((row("host").value.as_str(), row("host").json), ("e.com", false));
    assert_eq!((row("port").value.as_str(), row("port").json), ("8080", true));
    assert!(row("token").private);
    assert!(!dev.rows.iter().any(|r| r.name == "SSLConfiguration"), "a setting, not a variable");
}

#[test]
fn tables_are_written_back_each_to_its_file() {
    let mut edited = tables(Some(PUBLIC), Some(PRIVATE));
    let dev = &mut edited[1];
    dev.rows.iter_mut().find(|r| r.name == "host").unwrap().value = "api.e.com".into();
    dev.rows.push(Row { name: "secret".into(), value: "s".into(), private: true, json: false });
    dev.rows.retain(|r| r.name != "token");
    let (public, private) = write(&edited, Some(PUBLIC), Some(PRIVATE)).unwrap();
    let public: Value = serde_json::from_str(&public).unwrap();
    let private: Value = serde_json::from_str(&private.unwrap()).unwrap();
    assert_eq!(public["dev"]["host"], "api.e.com");
    assert_eq!(public["dev"]["port"], 8080, "a number again");
    assert_eq!(public["dev"]["client"]["id"], "c");
    assert_eq!(public["dev"]["SSLConfiguration"]["verifyHostCertificate"], false, "kept");
    assert_eq!(private["dev"]["secret"], "s");
    assert!(private["dev"].get("token").is_none(), "removed");
    let (_, none) = write(&tables(Some(PUBLIC), None), Some(PUBLIC), None).unwrap();
    assert_eq!(none, None, "no private file made for nothing");
}

#[test]
fn a_file_that_isnt_json_is_never_written_over() {
    let broken = "{\"dev\": {\"host\": \"e.com\",}";
    assert!(write(&tables(Some(broken), None), Some(broken), None).is_err());
    assert!(write(&tables(None, Some("[1]")), None, Some("[1]")).is_err());
}

#[test]
fn a_name_in_both_files_keeps_both_values() {
    let public = r#"{"dev": {"token": "public"}}"#;
    let private = r#"{"dev": {"token": "secret"}}"#;
    let read = tables(Some(public), Some(private));
    assert_eq!(read[0].rows.len(), 2, "{read:?}");
    let (p, s) = write(&read, Some(public), Some(private)).unwrap();
    assert!(p.contains("\"public\"") && s.unwrap().contains("\"secret\""));
}

#[test]
fn what_isnt_an_environment_is_kept_and_a_private_only_one_stays_private() {
    let public = r#"{"$schema": "x", "dev": {"host": "e.com"}}"#;
    let private = r#"{"prod": {"token": "t"}}"#;
    let read = tables(Some(public), Some(private));
    let names: Vec<&str> = read.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["dev", "prod"], "`$schema` isn't one");
    let (p, s) = write(&read, Some(public), Some(private)).unwrap();
    let p: Value = serde_json::from_str(&p).unwrap();
    assert_eq!(p["$schema"], "x");
    assert!(p.get("prod").is_none(), "{p}");
    assert!(s.unwrap().contains("\"t\""));
    let json = tables(Some(r#"{"dev": {"o": {"a": 1, "b": [2]}}}"#), None);
    assert!(!json[0].rows[0].value.contains('\n'), "one line, as edited");
}

#[test]
fn an_environment_renamed_keeps_its_settings_and_one_deleted_leaves_both_files() {
    let mut edited = tables(Some(PUBLIC), Some(PRIVATE));
    edited[1].name = "staging".into();
    let (public, private) = write(&edited, Some(PUBLIC), Some(PRIVATE)).unwrap();
    let public: Value = serde_json::from_str(&public).unwrap();
    let private: Value = serde_json::from_str(&private.unwrap()).unwrap();
    assert!(public.get("dev").is_none() && private.get("dev").is_none(), "the old name gone");
    assert_eq!(public["staging"]["host"], "e.com");
    assert_eq!(public["staging"]["SSLConfiguration"]["verifyHostCertificate"], false);
    assert_eq!(private["staging"]["token"], "t0k", "its private values follow it");
    // Deleted: out of both files, `$shared` and the rest kept.
    let kept: Vec<Table> = tables(Some(PUBLIC), Some(PRIVATE)).into_iter().take(1).collect();
    let (public, private) = write(&kept, Some(PUBLIC), Some(PRIVATE)).unwrap();
    assert!(!public.contains("\"dev\"") && !private.unwrap().contains("\"dev\""));
    assert!(public.contains("$shared"));
}

#[test]
fn an_environment_s_name_is_checked() {
    let others = ["dev", "prod"];
    assert_eq!(valid_name(" staging ", others), None);
    assert_eq!(valid_name("  ", others), Some(NameError::Empty));
    assert_eq!(valid_name("$shared", others), Some(NameError::Reserved));
    assert_eq!(valid_name("Security", others), Some(NameError::Reserved));
    assert_eq!(valid_name("prod", others), Some(NameError::Taken));
}
