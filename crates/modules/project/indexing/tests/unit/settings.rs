use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use settings_model::{Section, SettingsStore};

use super::*;

/// Settings kept in memory.
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

fn scope(settings: &SettingsService) -> SettingsScope<'_> {
    SettingsScope { settings, project: None, project_data: None, wake: None, language: None }
}

/// The switch card shows the choice and what the model is doing; turning it off saves the
/// choice, hands it to the model and the open settings draw the block again.
#[test]
fn the_semantic_block_shows_and_saves_the_choice() {
    let settings = SettingsService::new(Arc::new(Memory::default()));
    settings.set(&SEMANTIC_SEARCH, true).unwrap();
    let block = SemanticSettings::default();
    block.apply(&settings);
    assert!(semantic::enabled(), "the saved choice reaches the model");
    let shown = block.block(scope(&settings));
    let [SettingsRow::SwitchCard { key, on, status, tone, .. }, SettingsRow::Notes(notes)] =
        shown.rows.as_slice()
    else {
        panic!("{:?}", shown.rows)
    };
    assert_eq!((*key, *on, *tone), (ON, true, StatusTone::Neutral));
    assert_eq!(*status, t("settings.semantic_standby"), "on, the model not loaded yet");
    assert_eq!(notes.len(), 2);
    assert!(!block.poll(), "nothing changed since it was drawn");

    block.choose(scope(&settings), ON, 0);
    assert!(!settings.get(&SEMANTIC_SEARCH), "saved");
    assert!(!semantic::enabled());
    assert_eq!(semantic::mode(), SearchMode::Lexical);
    assert!(block.poll(), "the open settings draw it again");
    let shown = block.block(scope(&settings));
    let SettingsRow::SwitchCard { on: false, status, .. } = &shown.rows[0] else {
        panic!("{:?}", shown.rows)
    };
    assert_eq!(*status, t("settings.semantic_off"));
}
