use super::*;

fn store(name: &str, text: &str) -> (TomlSettingsStore, PathBuf) {
    let dir = std::env::temp_dir().join(format!("settings-{name}-{}", std::process::id()));
    let path = dir.join("settings.toml");
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(&path, text);
    (TomlSettingsStore::new(Some(path)), dir)
}

#[test]
fn update_keeps_other_sections_and_unknown_keys() {
    let (store, dir) = store("keep", "[a]\nx = 1\nratio = 0.5\n\n[b]\ny = true\n");
    let result = store.set("a", "x", SettingValue::Int(2));
    let a = store.section("a");
    let b = store.section("b");
    flush();
    let text = std::fs::read_to_string(dir.join("settings.toml")).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(result.is_ok());
    assert_eq!(a.and_then(|a| a.get("x").cloned()), Some(SettingValue::Int(2)));
    assert_eq!(b.and_then(|b| b.get("y").cloned()), Some(SettingValue::Bool(true)));
    assert!(text.contains("ratio = 0.5"), "{text}");
}

#[test]
fn invalid_file_is_never_overwritten() {
    let (store, dir) = store("invalid", "[a\nx = ");
    let result = store.set("b", "y", SettingValue::Bool(false));
    let text = std::fs::read_to_string(dir.join("settings.toml")).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(result.is_err());
    assert_eq!(text, "[a\nx = ");
    assert_eq!(store.section("a"), None);
}

/// A project's `project.toml`: a module's table written, the others' (levels, frameworks)
/// kept as they were.
#[test]
fn a_module_table_in_project_toml_keeps_the_others() {
    let dir = std::env::temp_dir().join(format!("project-toml-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("project.toml");
    std::fs::write(&path, "[frameworks]\nlaravel = true\n\n[levels]\nphp = \"8.2\"\n").unwrap();
    let store = TomlSettingsStore::new(Some(path.clone()));
    store
        .update("formatting", &mut |s| {
            s.insert("php".into(), SettingValue::Text("pint".into()));
        })
        .unwrap();
    flush();
    let text = std::fs::read_to_string(&path).unwrap();
    let table: toml::Table = text.parse().unwrap();
    assert_eq!(table["frameworks"]["laravel"].as_bool(), Some(true));
    assert_eq!(table["levels"]["php"].as_str(), Some("8.2"));
    assert_eq!(store.section("formatting").unwrap()["php"], SettingValue::Text("pint".into()));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Read again in the background, a file half-edited by hand (invalid now) changes nothing:
/// the settings read before stay, and come back changed once it's valid again.
#[test]
fn a_refresh_keeps_what_was_read_while_the_file_is_invalid() {
    let (store, dir) = store("refresh-invalid", "[editor]\nautosave = false\n");
    let path = dir.join("settings.toml");
    let service = settings_model::SettingsService::new(std::sync::Arc::new(store));
    const AUTOSAVE: settings_model::Setting<bool> =
        settings_model::Setting::new("editor", "autosave", || true);
    assert!(!service.get(&AUTOSAVE));
    std::fs::write(&path, "[editor\nautosave = tr").unwrap();
    assert!(service.refresh().is_empty(), "invalid: nothing changes");
    assert!(!service.get(&AUTOSAVE));
    std::fs::write(&path, "[editor]\nautosave = true\n").unwrap();
    assert_eq!(service.refresh(), ["editor"]);
    assert!(service.get(&AUTOSAVE));
    let _ = std::fs::remove_dir_all(dir);
}

/// Written behind: the changes known at once, the file written once they stop (the newest
/// text: a slider's stops never each wait for the disk); edited by hand after, read again.
#[test]
fn changes_are_known_at_once_and_written_behind() {
    let (store, dir) = store("behind", "[a]\nx = 0\n");
    let store = TomlSettingsStore::written_behind(store.path);
    for i in 1..=20 {
        store.set("a", "x", SettingValue::Int(i)).unwrap();
    }
    assert_eq!(store.section("a").unwrap()["x"], SettingValue::Int(20), "known at once");
    flush();
    let path = dir.join("settings.toml");
    let table: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    assert_eq!(table["a"]["x"].as_integer(), Some(20), "the newest written");
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(&path, "[a]\nx = 99\n\n[b]\ny = true\n").unwrap();
    assert_eq!(store.section("a").unwrap()["x"], SettingValue::Int(99), "edited by hand");
    store.set("a", "x", SettingValue::Int(7)).unwrap();
    flush();
    let table: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    assert_eq!(table["b"]["y"].as_bool(), Some(true), "the hand's edit kept");
    let _ = std::fs::remove_dir_all(&dir);
}
