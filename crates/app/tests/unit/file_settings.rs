use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use settings_model::{Section, SettingsService, SettingsStore};

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

/// Both blocks are on the Files page, in their places; a switch is saved under its old key.
#[test]
fn saving_and_the_final_newline_are_blocks_of_the_files_page() {
    let settings = SettingsService::new(Arc::new(Memory::default()));
    let scope = SettingsScope {
        settings: &settings,
        project: None,
        project_data: None,
        wake: None,
        language: None,
    };
    assert_eq!(AutosaveSettings.page(), "files");
    assert_eq!(FinalNewlineSettings.page(), "files");
    assert!(AutosaveSettings.order() < FinalNewlineSettings.order());
    let block = AutosaveSettings.block(scope);
    assert!(matches!(block.rows[0], SettingsRow::Toggle { on: true, key: "autosave", .. }));
    AutosaveSettings.choose(scope, "autosave", 0);
    assert!(!settings.get(&AUTOSAVE), "`[editor] autosave`, as before");
    FinalNewlineSettings.choose(scope, "final_newline", 0);
    assert!(!settings.get(&FINAL_NEWLINE), "`[editor] final_newline`, as before");
    let block = FinalNewlineSettings.block(scope);
    assert!(matches!(block.rows[0], SettingsRow::Toggle { on: false, .. }));
}
