//! Settings kept in memory, for the tests.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use settings_model::{Section, SettingsService, SettingsStore};

#[derive(Debug, Default)]
pub struct Memory {
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

pub fn settings() -> SettingsService {
    SettingsService::new(Arc::new(Memory::default()))
}
