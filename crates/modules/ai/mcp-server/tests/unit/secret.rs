use super::*;
use std::collections::HashMap;

/// The tests that set the store, one at a time (it's the process's).
pub(crate) static SERIAL: Mutex<()> = Mutex::new(());

/// Secrets in memory (never the keychain).
#[derive(Default)]
pub(crate) struct Memory(Mutex<HashMap<String, String>>);

impl SecretStore for Memory {
    fn get(&self, id: &str) -> Option<kernel::Secret> {
        self.0.lock().unwrap().get(id).map(|s| kernel::Secret::new(s.as_str()))
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

/// The token: an older one moved into the store, kept for the next runs, printed by the
/// helper as the header; a new one made when there's none.
#[test]
fn the_token_lives_in_the_store() {
    let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let store = Arc::new(Memory::default());
    use_secrets(store.clone());
    assert_eq!(helper_headers(), None, "none yet");
    let older = "a".repeat(64);
    assert_eq!(token(Some(older.clone())), (older.clone(), true), "moved there");
    assert_eq!(store.get(ID).unwrap().expose(), older);
    use_secrets(store);
    assert_eq!(token(None).0, older, "read back next run");
    let header: serde_json::Value = serde_json::from_str(&helper_headers().unwrap()).unwrap();
    assert_eq!(header["Authorization"], format!("Bearer {older}"));
    use_secrets(Arc::new(Memory::default()));
    let (made, kept) = token(None);
    assert!(kept && made.len() == 64 && made != older);
}

/// The helper's path quoted so a shell runs it as written.
#[test]
fn the_helper_s_path_is_quoted() {
    if cfg!(windows) {
        assert_eq!(quoted(r"C:\Program Files\x.exe"), r#""C:\Program Files\x.exe""#);
    } else {
        assert_eq!(quoted("/Apps/My App/x"), "'/Apps/My App/x'");
        assert_eq!(quoted("/it's/$HOME"), r"'/it'\''s/$HOME'");
    }
    assert!(helper_command().is_some_and(|c| c.ends_with(HELPER_ARG)));
}
