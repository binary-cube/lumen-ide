use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use settings_model::{Section, SettingsStore};

use super::*;

#[derive(Debug, Default)]
struct Memory(Mutex<HashMap<String, Section>>);

impl SettingsStore for Memory {
    fn section(&self, name: &str) -> Option<Section> {
        self.0.lock().unwrap().get(name).cloned()
    }
    fn update(&self, name: &str, change: &mut dyn FnMut(&mut Section)) -> std::io::Result<()> {
        change(self.0.lock().unwrap().entry(name.to_owned()).or_default());
        Ok(())
    }
}

/// On by default; turned off (macOS), it's saved and the bar loses the button at once;
/// elsewhere it's always shown.
#[test]
fn the_choice_is_saved_and_shows_the_button() {
    let settings = SettingsService::new(Arc::new(Memory::default()));
    let scope = SettingsScope {
        settings: &settings,
        project: None,
        project_data: None,
        wake: None,
        language: None,
    };
    WindowMenuSettings.apply(&settings);
    assert!(shown());
    let block = WindowMenuSettings.block(scope);
    let SettingsRow::Toggle { on, default, .. } = &block.rows[0] else {
        panic!("a toggle: {:?}", block.rows);
    };
    assert_eq!((*on, *default), (true, Some(true)));
    WindowMenuSettings.choose(scope, "shown", 0);
    assert!(!settings.get(&WINDOW_MENU));
    assert_eq!(shown(), !cfg!(target_os = "macos"));
    WindowMenuSettings.choose(scope, "shown", 1);
    assert!(settings.get(&WINDOW_MENU));
    assert!(shown());
}
