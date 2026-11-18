use super::*;

fn saved(local: &Path) -> Saved {
    Saved {
        connection: Connection {
            id: "shop".into(),
            name: "Shop".into(),
            protocol: Protocol::Sftp,
            host: "example.com".into(),
            port: 2222,
            user: "ana".into(),
            auth: Auth::Key { path: "/k/id".into() },
            root: "/var/www".into(),
            mode: Mode::Mirror,
            excluded: vec!["vendor".into()],
            saved_in: SavedIn::Project,
            secrets_in: SecretsIn::ProjectEncrypted,
            retry: Some(Retry { attempts: 6, delay: std::time::Duration::from_millis(500) }),
            uploads: Some(1),
            downloads: Some(8),
            moves: false,
            auto_upload: true,
            write_as: Default::default(),
        },
        local: local.to_path_buf(),
        trusted: vec!["SHA256:ab".into()],
    }
}

#[test]
fn a_project_s_connection_is_kept_in_it_without_secrets() {
    let root = std::env::temp_dir().join(format!("remote-config-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let s = saved(&root);
    save(&s).unwrap();
    assert_eq!(find(&root), Some(s));
    let text = std::fs::read_to_string(data_dir(&root).join("remote.toml")).unwrap();
    assert!(text.contains("example.com") && !text.to_lowercase().contains("password ="));
    assert_eq!(find(&root.join("elsewhere")), None, "a local project: none");
    let _ = std::fs::remove_dir_all(root);
}

/// Uploading on save is on unless the file says otherwise (files written before the switch
/// existed); off is kept, on isn't written.
#[test]
fn the_upload_switch_is_on_unless_saved_off() {
    let root = std::env::temp_dir().join(format!("remote-switch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mut s = saved(&root);
    save(&s).unwrap();
    let file = data_dir(&root).join("remote.toml");
    assert!(!std::fs::read_to_string(&file).unwrap().contains("auto_upload"));
    assert!(find(&root).unwrap().connection.auto_upload);
    s.connection.auto_upload = false;
    save(&s).unwrap();
    assert!(std::fs::read_to_string(&file).unwrap().contains("auto_upload = false"));
    assert_eq!(find(&root), Some(s));
    let _ = std::fs::remove_dir_all(root);
}

/// A folder has one connection: a new one for it replaces the old, whose secret goes with
/// it (the vault's line here; nothing is left behind under the old id).
/// Who files written belong to is kept: the server's way not written, the project's folder,
/// a chosen owner and group (their names and ids).
#[test]
fn who_files_written_belong_to_is_kept() {
    let root = std::env::temp_dir().join(format!("remote-write-as-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let file = data_dir(&root).join("remote.toml");
    let mut s = saved(&root);
    save(&s).unwrap();
    assert!(!std::fs::read_to_string(&file).unwrap().contains("write_as"));
    s.connection.write_as = WriteAs::Project;
    save(&s).unwrap();
    assert_eq!(find(&root), Some(s.clone()));
    s.connection.write_as =
        WriteAs::Chosen { owner: Some(Principal::named("www-data", Some(33))), group: None };
    save(&s).unwrap();
    assert!(std::fs::read_to_string(&file).unwrap().contains("owner_id = 33"));
    assert_eq!(find(&root), Some(s));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_new_connection_for_a_folder_replaces_the_old_and_its_secret() {
    let root = std::env::temp_dir().join(format!("remote-replace-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let old = saved(&root);
    save(&old).unwrap();
    let vault = data_dir(&root).join("remote-secrets");
    std::fs::write(&vault, "shop\tnonce\tcipher\n").unwrap();
    let mut new = saved(&root);
    new.connection.id = "6f1c0a52-8d0e-4c1e-9a52-2f0f6d1b3c4d".into();
    new.connection.name = "Shop".into();
    save(&new).unwrap();
    assert_eq!(find(&root), Some(new));
    let text = std::fs::read_to_string(data_dir(&root).join("remote.toml")).unwrap();
    assert_eq!(text.matches("example.com").count(), 1, "one connection: {text}");
    assert!(!std::fs::read_to_string(&vault).unwrap().contains("shop\t"), "its secret removed");
    let _ = std::fs::remove_dir_all(root);
}

/// A project's own connection may come with the project (a cloned repository): it runs only
/// as the user allowed it (made here, or approved), never one changed since, and its copy
/// is always the project itself.
#[test]
fn a_connection_that_came_with_the_project_runs_only_once_allowed() {
    let root = std::env::temp_dir().join(format!("remote-approve-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let s = saved(&root);
    save(&s).unwrap();
    assert!(approved(&s), "made here: allowed");
    let mut changed = s.clone();
    changed.connection.host = "other.example".into();
    assert!(!approved(&changed), "another server: asked again");
    let mut trusting = s.clone();
    trusting.trusted.push("SHA256:attacker".into());
    assert!(!approved(&trusting), "another certificate trusted: asked again");
    approve(&changed);
    assert!(approved(&changed) && !approved(&s), "one approval per folder");
    // The file says another folder: the copy stays the project.
    let text = std::fs::read_to_string(data_dir(&root).join("remote.toml")).unwrap();
    let elsewhere = text.replace(&root.display().to_string(), "/tmp/elsewhere");
    std::fs::write(data_dir(&root).join("remote.toml"), elsewhere).unwrap();
    assert_eq!(find(&root).map(|f| f.local), Some(root.clone()));
    let _ = std::fs::remove_dir_all(root);
}

/// Edited, a connection keeps its identity; kept elsewhere now, it leaves where it was and
/// its secret moves with it (never left behind).
#[test]
fn an_edited_connection_moves_with_its_secret() {
    use std::collections::HashMap;
    use std::sync::Mutex;
    #[derive(Default)]
    struct Memory(Mutex<HashMap<String, String>>);
    impl SecretStore for Memory {
        fn get(&self, id: &str) -> Option<remote::Secret> {
            self.0.lock().unwrap().get(id).map(|s| remote::Secret::new(s.clone()))
        }
        fn set(&self, id: &str, secret: &str) -> Result<(), String> {
            self.0.lock().unwrap().insert(id.into(), secret.into());
            Ok(())
        }
        fn remove(&self, id: &str) -> Result<(), String> {
            self.0.lock().unwrap().remove(id);
            Ok(())
        }
    }
    let root = std::env::temp_dir().join(format!("remote-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (keychain, vault) = (Arc::new(Memory::default()), Arc::new(Memory::default()));
    let stores = |s: &Saved| -> Arc<dyn SecretStore> {
        match s.connection.secrets_in {
            SecretsIn::Keychain => keychain.clone(),
            SecretsIn::ProjectEncrypted => vault.clone(),
        }
    };
    let original = saved(&root);
    save(&original).unwrap();
    vault.set("shop", "pw").unwrap();
    let mut edited = original.clone();
    edited.connection.id = "another".into();
    edited.connection.port = 2200;
    edited.connection.secrets_in = SecretsIn::Keychain;
    edited.connection.saved_in = SavedIn::Global;
    update(&original, &edited, None, &stores).unwrap();
    assert_eq!(keychain.get("shop").unwrap().expose(), "pw", "moved, under its own identity");
    assert!(vault.get("shop").is_none(), "not left behind");
    let found = find(&root).unwrap();
    assert_eq!((found.connection.id.as_str(), found.connection.port), ("shop", 2200));
    assert_eq!(found.connection.saved_in, SavedIn::Global, "the project's entry gone");
    let text = std::fs::read_to_string(data_dir(&root).join("remote.toml")).unwrap();
    assert!(!text.contains("example.com"), "{text}");
    let _ = std::fs::remove_dir_all(root);
}
