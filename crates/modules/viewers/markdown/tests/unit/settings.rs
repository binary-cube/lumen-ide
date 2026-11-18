use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use settings_model::{Section, SettingsStore};

use super::*;

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

#[test]
fn web_images_are_on_by_default_and_saved() {
    let settings = SettingsService::new(Arc::new(Memory::default()));
    let scope = SettingsScope {
        settings: &settings,
        project: None,
        project_data: None,
        language: None,
        wake: None,
    };
    let block = MarkdownSettings.block(scope);
    assert!(matches!(
        block.rows.first(),
        Some(SettingsRow::Toggle { key: "web_images", on: true, .. })
    ));
    MarkdownSettings.choose(scope, "web_images", 0);
    assert!(!settings.get(&WEB_IMAGES));
}

#[test]
fn sync_scroll_is_on_by_default_and_saved() {
    let settings = SettingsService::new(Arc::new(Memory::default()));
    let scope = SettingsScope {
        settings: &settings,
        project: None,
        project_data: None,
        language: None,
        wake: None,
    };
    let block = MarkdownSettings.block(scope);
    assert!(
        block
            .rows
            .iter()
            .any(|r| matches!(r, SettingsRow::Toggle { key: "sync_scroll", on: true, .. }))
    );
    MarkdownSettings.choose(scope, "sync_scroll", 0);
    assert!(!settings.get(&SYNC_SCROLL));
}
