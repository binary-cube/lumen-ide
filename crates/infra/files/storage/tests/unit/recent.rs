use std::path::Path;

use workspace::recent::Place;

use super::*;

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("recent-{name}-{}", std::process::id()))
}

#[test]
fn round_trip() {
    let dir = temp("round-trip");
    let store = TomlRecentStore::new(Some(dir.join("recent.toml")));
    let mut list = RecentProjects::of(vec![PathBuf::from("/w/a"), PathBuf::from("/w/b ă")]);
    let clients = list.new_group("Clienți", None);
    let inner = list.new_group("Northwind", Some(clients));
    list.set_collapsed(clients, true);
    list.move_to(Path::new("/w/b ă"), inner, Place::Before(PathBuf::from("/w/x")));
    list.move_to(Path::new("/w/a"), inner, Place::Before(PathBuf::from("/w/b ă")));
    let saved = store.save(&list);
    let loaded = store.load();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(saved.is_ok());
    assert_eq!(loaded, list);
}

#[test]
fn an_older_file_has_every_project_outside_groups() {
    let dir = temp("older");
    let file = dir.join("recent.toml");
    let _ = std::fs::create_dir_all(&dir);
    let written = std::fs::write(&file, "projects = [\"/w/a\", \"/w/b\"]\n");
    let loaded = TomlRecentStore::new(Some(file)).load();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(written.is_ok());
    assert_eq!(loaded, RecentProjects::of(vec![PathBuf::from("/w/a"), PathBuf::from("/w/b")]));
}
