use super::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use settings_model::{Section, SettingsStore};

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

fn scope(settings: &SettingsService) -> SettingsScope<'_> {
    SettingsScope { settings, project: None, project_data: None, language: None, wake: None }
}

/// The page's switch saves the choice and the hold follows it; off by default.
#[test]
fn the_switch_keeps_awake_and_lets_go() {
    let settings = SettingsService::new(Arc::new(Memory::default()));
    let block = KeepAwakeSettings.block(scope(&settings));
    assert!(matches!(block.rows[0], SettingsRow::Toggle { key: "on", on: false, .. }));
    KeepAwakeSettings.choose(scope(&settings), "on", 1);
    assert!(settings.get(&ON) && config().on && crate::keeper::keeper().is_held());
    KeepAwakeSettings.choose(scope(&settings), "on", 0);
    assert!(!settings.get(&ON) && !config().on && !crate::keeper::keeper().is_held());
}

/// The screen's choices are saved and read as its care: minutes (0: never), darkness,
/// the drifting mark.
#[test]
fn the_screens_choices_become_its_care() {
    let settings = SettingsService::new(Arc::new(Memory::default()));
    let _ = apply(&settings);
    assert_eq!(config().care, Config::DEFAULT.care);
    KeepAwakeSettings.choose(scope(&settings), "dim", 0);
    KeepAwakeSettings.choose(scope(&settings), "level", 85);
    KeepAwakeSettings.choose(scope(&settings), "drift", 0);
    let care = config().care;
    assert_eq!(care.dim_after, None, "never");
    assert!((care.level - 0.85).abs() < 1e-6 && !care.drift);
    KeepAwakeSettings.choose(scope(&settings), "dim", 15);
    assert_eq!(config().care.dim_after, Some(Duration::from_secs(15 * 60)));
    // A value not offered is refused.
    KeepAwakeSettings.choose(scope(&settings), "level", 3);
    assert!((config().care.level - 0.85).abs() < 1e-6);
}

/// Only while work runs is the user's choice: switched on the page, the button stays lit
/// (it's on) while nothing is held until work runs.
#[test]
fn while_working_is_chosen_and_nothing_is_held_until_work_runs() {
    let settings = SettingsService::new(Arc::new(Memory::default()));
    assert_eq!(Config::read(&settings).when, Mode::Always, "always while on, by default");
    KeepAwakeSettings.choose(scope(&settings), "when", 1);
    KeepAwakeSettings.choose(scope(&settings), "on", 1);
    assert_eq!(config().mode(), Mode::WhileWorking);
    assert!(config().on && !crate::keeper::keeper().is_held(), "nothing runs");
    crate::watcher::step(true);
    assert!(crate::keeper::keeper().is_held());
    KeepAwakeSettings.choose(scope(&settings), "on", 0);
    assert!(!crate::keeper::keeper().is_held() && config().mode() == Mode::Off);
}
