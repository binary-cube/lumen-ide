use super::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use settings_model::{Section, SettingsStore};

/// Settings kept in memory (a test's own).
#[derive(Debug, Default)]
struct Memory {
    sections: Mutex<HashMap<String, Section>>,
}

impl SettingsStore for Memory {
    fn section(&self, name: &str) -> Option<Section> {
        self.sections.lock().unwrap().get(name).cloned()
    }
    fn update(&self, name: &str, change: &mut dyn FnMut(&mut Section)) -> std::io::Result<()> {
        change(self.sections.lock().unwrap().entry(name.to_owned()).or_default());
        Ok(())
    }
}

/// Turned on: listening on the loopback (started on a worker), its token in the keychain
/// (here, in memory) and not in the settings, the port it picked saved; turned off: stopped.
#[test]
fn turned_on_it_listens_with_a_kept_token() {
    let _serial = crate::secret::tests::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    crate::secret::use_secrets(Arc::new(crate::secret::tests::Memory::default()));
    let settings = SettingsService::new(Arc::new(Memory::default()));
    settings.set(&TOKEN, "b".repeat(64)).unwrap();
    apply(&settings);
    assert!(crate::server::running().is_none(), "off at first");
    settings.set(&ENABLED, true).unwrap();
    apply(&settings);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while (crate::server::running().is_none() || LISTENED.load(Ordering::Acquire) == 0)
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let (url, token) = crate::server::running().expect("listening");
    assert!(url.starts_with("http://127.0.0.1:") && url.ends_with("/mcp"));
    assert_eq!(token, "b".repeat(64), "the older one, moved");
    assert!(take_saves(&settings));
    assert!(settings.get(&TOKEN).is_empty(), "out of the file");
    assert!(url.contains(&format!(":{}/", settings.get(&PORT))), "its port kept");
    settings.set(&ENABLED, false).unwrap();
    apply(&settings);
    assert!(crate::server::running().is_none());
}

/// A port written in its field: empty is the system's pick, a number in range is taken,
/// anything else isn't.
#[test]
fn a_port_is_written_with_care() {
    assert_eq!(port_of(""), Some(0));
    assert_eq!(port_of(" 8765 "), Some(8765));
    assert_eq!(port_of("80"), None, "the administrator's");
    assert_eq!(port_of("70000"), None);
    assert_eq!(port_of("abc"), None);
}
