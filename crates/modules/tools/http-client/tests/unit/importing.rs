//! A picked collection imported into its own folder under the project's `http/`: a Postman
//! file, a Bruno collection; never over a folder already there.

use super::*;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("http-import-{name}-{:x}", kernel::id::new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_postman_file_becomes_a_folder_of_its_own() {
    let dir = scratch("postman");
    let project = dir.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let picked = dir.join("shop.postman_collection.json");
    std::fs::write(
        &picked,
        r#"{"info": {"name": "My Shop"}, "item": [{"name": "Health", "request": "https://e.com/h"}],
            "variable": [{"key": "host", "value": "e.com"}]}"#,
    )
    .unwrap();
    let made = import_file(&project, &picked).unwrap();
    assert_eq!(made.open, project.join("http/my-shop/requests.http"));
    assert_eq!(made.requests, 1);
    assert!(project.join("http/my-shop/http-client.env.json").is_file());
    // Again: beside it, not over it.
    let again = import_file(&project, &picked).unwrap();
    assert_eq!(again.open, project.join("http/my-shop-2/requests.http"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_bruno_file_brings_its_whole_collection() {
    let dir = scratch("bruno");
    let collection = dir.join("api");
    std::fs::create_dir_all(collection.join("users")).unwrap();
    std::fs::create_dir_all(collection.join("environments")).unwrap();
    std::fs::write(collection.join("bruno.json"), r#"{"name": "Bruno API"}"#).unwrap();
    let bru = |name: &str, url: &str| {
        format!("meta {{\n  name: {name}\n}}\n\nget {{\n  url: {url}\n}}\n")
    };
    std::fs::write(collection.join("users/list.bru"), bru("List", "{{host}}/users")).unwrap();
    std::fs::write(collection.join("health.bru"), bru("Health", "{{host}}/health")).unwrap();
    std::fs::write(collection.join("environments/dev.bru"), "vars {\n  host: e.com\n}\n").unwrap();
    let project = dir.join("project");
    let made = import_file(&project, &collection.join("users/list.bru")).unwrap();
    assert_eq!(made.requests, 2, "the whole collection, not just the file picked");
    assert_eq!(made.open, project.join("http/bruno-api/requests.http"));
    let env = std::fs::read_to_string(project.join("http/bruno-api/http-client.env.json")).unwrap();
    assert!(env.contains("\"dev\""));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn what_isn_t_a_collection_is_refused() {
    let dir = scratch("bad");
    let picked = dir.join("x.json");
    std::fs::write(&picked, "[1]").unwrap();
    assert!(import_file(&dir, &picked).is_err());
    assert_eq!(slug("  Ünïcode API v2!"), "ünïcode-api-v2");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_bru_file_with_no_collection_above_it_comes_alone() {
    let dir = scratch("lone");
    std::fs::write(
        dir.join("one.bru"),
        "meta {\n  name: One\n}\n\nget {\n  url: https://e.com\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("other.bru"),
        "meta {\n  name: Other\n}\n\nget {\n  url: https://e.com/o\n}\n",
    )
    .unwrap();
    let project = dir.join("project");
    let made = import_file(&project, &dir.join("one.bru")).unwrap();
    assert_eq!(made.requests, 1, "not the folder around it");
    let _ = std::fs::remove_dir_all(dir);
}
