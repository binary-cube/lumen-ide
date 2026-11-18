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

/// The theme's caret until the user chooses their own; their choices shown and used.
#[test]
fn the_theme_s_caret_until_the_user_chooses_their_own() {
    let settings = SettingsService::new(Arc::new(Memory::default()));
    let scope = SettingsScope {
        settings: &settings,
        project: None,
        project_data: None,
        wake: None,
        language: None,
    };
    CaretSettings.apply(&settings);
    assert_eq!(own(), None, "the theme's");
    assert_eq!(CaretSettings.block(scope).rows.len(), 1, "only the choice");
    CaretSettings.choose(scope, "own", 1);
    CaretSettings.choose(scope, "shape", 1);
    CaretSettings.choose(scope, "blink", 2);
    CaretSettings.choose(scope, "motion", 1);
    CaretSettings.choose(scope, "glow", 50);
    CaretSettings.choose(scope, "trail", 220);
    CaretSettings.apply(&settings);
    let look = own().unwrap();
    assert_eq!(
        (look.shape, look.blink, look.motion),
        (CaretShape::Block, Blink::Smooth, Motion::Glide)
    );
    assert!((look.glow - 0.5).abs() < f32::EPSILON);
    assert_eq!(look.trail_ms, 220);
    assert_eq!(CaretSettings.block(scope).rows.len(), 7);
    CaretSettings.choose(scope, "own", 0);
    CaretSettings.apply(&settings);
    assert_eq!(own(), None, "back to the theme's");
}
