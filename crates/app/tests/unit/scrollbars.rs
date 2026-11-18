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

/// Before any choice, the default look (only the thumb growing, 5 px, its line, as each view
/// chose); each choice saved by its id and read back, shown as chosen.
#[test]
fn the_scrollbars_look_is_chosen_and_kept() {
    let settings = SettingsService::new(Arc::new(Memory::default()));
    let scope = SettingsScope {
        settings: &settings,
        project: None,
        project_data: None,
        wake: None,
        language: None,
    };
    assert_eq!(saved(&settings), ScrollbarLook::DEFAULT);
    ScrollbarSettings.choose(scope, "hover", 0);
    ScrollbarSettings.choose(scope, "thickness", 3);
    ScrollbarSettings.choose(scope, "line", 0);
    ScrollbarSettings.choose(scope, "visibility", 1);
    assert_eq!(settings.get(&SCROLLBAR_HOVER), "rail");
    assert_eq!(settings.get(&SCROLLBAR_VISIBILITY), "always");
    let look = saved(&settings);
    assert_eq!((look.hover, look.visibility), (ScrollbarHover::Rail, ScrollbarVisibility::Always));
    assert!((look.thickness - 3.0).abs() < f32::EPSILON);
    assert!(!look.line, "the thumb alone");
    let rows = ScrollbarSettings.block(scope).rows;
    let current: Vec<i64> = rows
        .iter()
        .map(|row| match row {
            SettingsRow::Choices { current, .. } | SettingsRow::Slider { current, .. } => *current,
            SettingsRow::Toggle { on, .. } => i64::from(*on),
            _ => -1,
        })
        .collect();
    assert_eq!(current, [0, 3, 0, 1]);
    // Only saved: the bars' look changes when the app applies it (not here, where other tests
    // paint bars meanwhile).
    assert_eq!(scrollbar::look(), ScrollbarLook::DEFAULT);
}
