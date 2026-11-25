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

/// The choice shown in the row.
fn current(block: &SettingsBlock) -> (i64, Option<i64>, usize) {
    match &block.rows[0] {
        SettingsRow::Segmented { current, default, choices, .. } => {
            (*current, *default, choices.len())
        }
        other => panic!("the choice first: {other:?}"),
    }
}

/// The system's choice until the user picks one; what's picked is saved as its name and
/// shown chosen; another row's key changes nothing.
#[test]
fn the_graphics_cards_choice_is_saved_and_shown() {
    let settings = SettingsService::new(Arc::new(Memory::default()));
    let scope = SettingsScope {
        settings: &settings,
        project: None,
        project_data: None,
        wake: None,
        language: None,
    };
    assert_eq!(current(&GpuSettings.block(scope)), (0, Some(0), 3), "the system's");
    GpuSettings.choose(scope, KEY, value_of(GpuPreference::Dedicated));
    assert_eq!(settings.get(&GPU), "dedicated");
    assert_eq!(current(&GpuSettings.block(scope)).0, value_of(GpuPreference::Dedicated));
    GpuSettings.choose(scope, "other", value_of(GpuPreference::Integrated));
    assert_eq!(settings.get(&GPU), "dedicated", "another key: nothing");
    GpuSettings.choose(scope, KEY, value_of(GpuPreference::Integrated));
    assert_eq!(settings.get(&GPU), "integrated");
    // A value the row never has: the system's.
    GpuSettings.choose(scope, KEY, 99);
    assert_eq!(settings.get(&GPU), "system");
}

/// Every preference has one value and back.
#[test]
fn each_preference_has_its_value() {
    for preference in [GpuPreference::System, GpuPreference::Integrated, GpuPreference::Dedicated] {
        assert_eq!(preference_of(value_of(preference)), preference);
    }
}
