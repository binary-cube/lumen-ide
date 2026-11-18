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

/// Top center until the user picks another edge in a segmented control; the choice is saved
/// and used at once.
#[test]
fn the_position_is_chosen_saved_and_used() {
    let settings = SettingsService::new(Arc::new(Memory::default()));
    let scope = SettingsScope {
        settings: &settings,
        project: None,
        project_data: None,
        wake: None,
        language: None,
    };
    assert_eq!(saved(&settings), ToastPosition::TopCenter);
    let block = ToastSettings.block(scope);
    let SettingsRow::Segmented { choices, current, .. } = &block.rows[0] else {
        panic!("a segmented control: {:?}", block.rows);
    };
    assert_eq!(choices.len(), 4);
    assert_eq!(*current, 2, "top center");
    ToastSettings.choose(scope, "position", 0);
    assert_eq!(saved(&settings), ToastPosition::BottomRight);
    assert!(matches!(
        &ToastSettings.block(scope).rows[0],
        SettingsRow::Segmented { current: 0, .. }
    ));
    ToastSettings.choose(scope, "position", 9);
    assert_eq!(saved(&settings), ToastPosition::BottomRight, "no such choice");
}
