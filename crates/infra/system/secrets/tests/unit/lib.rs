use super::*;
use std::collections::HashMap;
use std::sync::Mutex;

/// A keychain in memory.
#[derive(Default)]
struct Keys(Mutex<HashMap<String, String>>);

impl SecretStore for Keys {
    fn get(&self, c: &str) -> Option<Secret> {
        self.0.lock().unwrap().get(c).cloned().map(Secret::from)
    }
    fn set(&self, c: &str, s: &str) -> Result<(), String> {
        self.0.lock().unwrap().insert(c.into(), s.into());
        Ok(())
    }
    fn remove(&self, c: &str) -> Result<(), String> {
        self.0.lock().unwrap().remove(c);
        Ok(())
    }
}

#[test]
fn the_vault_keeps_secrets_encrypted_and_bound_to_their_connection() {
    let dir = std::env::temp_dir().join(format!("vault-test-{}", std::process::id()));
    let file = dir.join("remote-secrets");
    let keys: Arc<dyn SecretStore> = Arc::new(Keys::default());
    let vault =
        ProjectVault { file: file.clone(), keys: Arc::clone(&keys), key_name: "vault".into() };
    vault.set("shop", "p4ss w0rd").unwrap();
    vault.set("blog", "other").unwrap();
    assert_eq!(vault.get("shop").as_ref().map(Secret::expose), Some("p4ss w0rd"));
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(!text.contains("p4ss"), "never in plain text");
    // A secret moved to another connection's line doesn't open.
    let swapped = text.replacen("shop\t", "xxxx\t", 1).replacen("blog\t", "shop\t", 1);
    std::fs::write(&file, swapped).unwrap();
    assert_eq!(vault.get("shop"), None);
    // Without the key (another machine), nothing opens.
    vault.set("shop", "again").unwrap();
    // `file` is still read below on Unix (its permissions).
    #[cfg_attr(windows, allow(clippy::redundant_clone))]
    let elsewhere = ProjectVault {
        file: file.clone(),
        keys: Arc::new(Keys::default()),
        key_name: "vault".into(),
    };
    assert_eq!(elsewhere.get("shop"), None);
    vault.remove("shop").unwrap();
    assert_eq!(vault.get("shop"), None);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// A vault that can't be read is never written over (the others' credentials would be
/// lost), and a read never makes a key.
#[test]
fn an_unreadable_vault_is_left_as_it_is() {
    let dir = std::env::temp_dir().join(format!("vault-unreadable-{}", std::process::id()));
    // A folder where the file should be: reading it fails.
    let file = dir.join("remote-secrets");
    std::fs::create_dir_all(&file).unwrap();
    let keys: Arc<dyn SecretStore> = Arc::new(Keys::default());
    let vault = ProjectVault { file: file.clone(), keys: Arc::clone(&keys), key_name: "k".into() };
    assert_eq!(vault.get("shop"), None);
    assert_eq!(keys.get("k"), None, "a read made no key");
    assert!(vault.set("shop", "x").is_err());
    assert!(vault.remove("shop").is_err());
    assert!(file.is_dir(), "left as it was");
    let _ = std::fs::remove_dir_all(dir);
}

/// A keychain that lost the vault's key (locked, briefly unavailable): storing another secret
/// fails instead of sealing it with a new key, which would lose every one stored.
#[test]
fn a_missing_key_is_never_replaced_while_secrets_need_it() {
    let dir = std::env::temp_dir().join(format!("vault-keep-{}", std::process::id()));
    let keys = Arc::new(Keys::default());
    let shared: Arc<dyn SecretStore> = keys.clone();
    let vault =
        ProjectVault { file: dir.join("remote-secrets"), keys: shared, key_name: "vault".into() };
    vault.set("shop", "first").unwrap();
    keys.0.lock().unwrap().clear();
    assert!(vault.set("blog", "second").is_err(), "no new key over stored secrets");
    assert!(keys.0.lock().unwrap().is_empty());
    // Replacing the only secret there is: a new key is fine.
    assert!(vault.set("shop", "again").is_ok());
    let _ = std::fs::remove_dir_all(dir);
}
