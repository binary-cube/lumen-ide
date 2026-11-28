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

fn service() -> SettingsService {
    SettingsService::new(Arc::new(Memory::default()))
}

fn scope(settings: &SettingsService) -> SettingsScope<'_> {
    SettingsScope { settings, project: None, project_data: None, wake: None, language: None }
}

/// Sleeping is off by default (nothing stopped unasked); turned on, an hour unless another
/// offered time is chosen (one not offered is refused); its time shown only while it's on.
#[test]
fn diagrams_sleep_only_when_chosen() {
    let settings = service();
    assert_eq!(idle(&settings), None);
    assert_eq!(SleepSettings.block(scope(&settings)).rows.len(), 1, "only the switch");
    SleepSettings.choose(scope(&settings), "sleep_idle", 1);
    assert_eq!(idle(&settings), Some(std::time::Duration::from_secs(3600)));
    SleepSettings.choose(scope(&settings), "sleep_after", 15);
    assert_eq!(idle(&settings), Some(std::time::Duration::from_secs(900)));
    SleepSettings.choose(scope(&settings), "sleep_after", 7);
    assert_eq!(idle(&settings), Some(std::time::Duration::from_secs(900)), "not offered");
    assert_eq!(SleepSettings.block(scope(&settings)).rows.len(), 2, "its time too");
    // Told to the sleeping as it's applied.
    SleepSettings.apply(&settings);
    assert_eq!(
        ui_components::sleep::idle_for(crate::view::DIAGRAMS.id),
        Some(std::time::Duration::from_secs(900))
    );
    let fresh = service();
    enable_sleep(&fresh);
    assert_eq!(idle(&fresh), Some(std::time::Duration::from_secs(3600)), "the hint's Turn on");
}
