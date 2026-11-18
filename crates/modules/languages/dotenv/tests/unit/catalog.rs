use super::*;
use crate::fixture::{TempDir, language_of, wait_until};

fn catalog(dir: &TempDir) -> Arc<Catalog> {
    Catalog::new(dir.path().to_path_buf(), language_of(), Arc::new(|| {}))
}

/// The files serving `file`, once read.
fn files_for(catalog: &Arc<Catalog>, file: &Path) -> Arc<[EnvDoc]> {
    let mut found = None;
    wait_until(|| {
        found = catalog.files_for(file);
        found.is_some()
    });
    found.unwrap()
}

#[test]
fn a_folders_files_in_the_order_they_load() {
    let dir = TempDir::new("order");
    dir.write(".env.local", "APP_ENV=prod\n");
    dir.write(".env", "APP_ENV=dev\nAPP_SECRET=s\n");
    dir.write(".env.example", "APP_ENV=\n");
    dir.write(".env.local.php", "<?php return [];");
    dir.write("composer.json", "{}");
    let catalog = catalog(&dir);
    let docs = files_for(&catalog, &dir.path().join("src/Kernel.php"));
    let names: Vec<&str> = docs.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, [".env", ".env.local", ".env.example"]);
    assert_eq!(docs[1].value("APP_ENV"), Some("prod"));
    assert_eq!(docs[0].defined("APP_SECRET").map(|(l, _)| l.first), Some(1));
    assert!(docs[2].documents && !docs[0].documents);
}

#[test]
fn the_nearest_folder_with_files_serves_the_code_below_it() {
    let dir = TempDir::new("nearest");
    dir.write(".env", "ROOT=1\n");
    dir.write("apps/api/.env", "API=1\n");
    let catalog = catalog(&dir);
    let api = files_for(&catalog, &dir.path().join("apps/api/src/main.js"));
    assert_eq!(api[0].value("API"), Some("1"));
    let web = files_for(&catalog, &dir.path().join("apps/web/index.js"));
    assert_eq!(web[0].value("ROOT"), Some("1"));
}

#[test]
fn a_change_reads_its_folder_again() {
    let dir = TempDir::new("change");
    let env = dir.write(".env", "A=1\n");
    let catalog = catalog(&dir);
    let code = dir.path().join("index.php");
    assert_eq!(files_for(&catalog, &code)[0].value("A"), Some("1"));
    let before = catalog.generation();
    std::fs::write(&env, "A=2\n").unwrap();
    catalog.changed(&env);
    wait_until(|| catalog.generation() > before && !catalog.busy());
    assert_eq!(files_for(&catalog, &code)[0].value("A"), Some("2"));
    // Not a `.env` file: nothing read again.
    let generation = catalog.generation();
    catalog.changed(&code);
    assert!(!catalog.busy());
    assert_eq!(catalog.generation(), generation);
}

#[test]
fn a_project_without_env_files_has_none() {
    let dir = TempDir::new("none");
    let catalog = catalog(&dir);
    assert!(files_for(&catalog, &dir.path().join("a/b.php")).is_empty());
}

#[test]
fn a_projects_catalog_is_kept_while_it_is_open() {
    let dir = TempDir::new("kept");
    let id = ProjectId::new();
    let kept = keep(id, catalog(&dir));
    assert!(of(id).is_some());
    drop(kept);
    assert!(of(id).is_none());
}
