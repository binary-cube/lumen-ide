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

/// Both on by default; a switch turned is saved and used at once.
#[test]
fn the_switches_are_saved_and_used() {
    let settings = SettingsService::new(Arc::new(Memory::default()));
    let scope = SettingsScope {
        settings: &settings,
        project: None,
        project_data: None,
        wake: None,
        language: None,
    };
    let block = InsightSettings.block(scope);
    assert!(matches!(block.rows[0], SettingsRow::Toggle { on: true, key: "inlay_hints", .. }));
    assert!(matches!(block.rows[1], SettingsRow::Toggle { on: true, key: "code_lens", .. }));
    InsightSettings.choose(scope, "code_lens", 0);
    assert!(!settings.get(&CODE_LENS));
    assert!(!lenses() && hints());
    InsightSettings.choose(scope, "code_lens", 1);
    assert!(lenses());
}
