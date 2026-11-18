use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use settings_model::{Section, SettingsStore};
use ui_workbench::backdrop::BackdropKind;

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

/// The theme's panel glass until the user chooses: their style, opacity and blur over it
/// (each "from theme" on its own), opaque where the renderer can't blur; the sliders show the
/// theme's values marked as its; a choice of the theme's value follows the theme again.
#[test]
fn the_user_s_panel_glass_goes_over_the_theme_s() {
    let settings = SettingsService::new(Arc::new(Memory::default()));
    let scope = SettingsScope {
        settings: &settings,
        project: None,
        project_data: None,
        wake: None,
        language: None,
    };
    let theme = SurfaceStyle::resolve(SurfaceSpec {
        style: Some(BackdropKind::Glass),
        opacity: Some(0.9),
        ..SurfaceSpec::default()
    });
    FloatingSettings.apply(&settings);
    assert_eq!(own(), SurfaceSpec::default());
    let mine = style(&theme, true);
    assert!(mine.is_glass());
    assert_eq!(mine.blur_radius(2.0), Some(8.0), "the default 4 px at scale 2");
    assert!((mine.fill(kernel::Rgba::WHITE).a - 0.9).abs() < 1e-6);
    assert!(!style(&theme, false).is_glass(), "a renderer that can't blur: opaque");
    let block = FloatingSettings.block(scope);
    let SettingsRow::Slider { current, stops, .. } = &block.rows[1] else { panic!() };
    assert_eq!(*current, 90);
    let label = &stops.iter().find(|(v, _)| *v == 90).unwrap().1;
    assert_eq!(*label, tr!("settings.backdrop_theirs", value = "90%"));
    FloatingSettings.choose(scope, "style", 1);
    FloatingSettings.choose(scope, "opacity", 75);
    FloatingSettings.apply(&settings);
    let simple = style(&theme, true);
    assert_eq!(simple.kind, BackdropKind::Simple);
    assert_eq!(simple.fill(kernel::Rgba::WHITE).a, 1.0, "Simple is opaque");
    FloatingSettings.choose(scope, "style", 2);
    FloatingSettings.apply(&settings);
    assert!((style(&theme, true).opacity - 0.75).abs() < 1e-6);
    FloatingSettings.choose(scope, "opacity", 90);
    FloatingSettings.choose(scope, "style", 0);
    FloatingSettings.apply(&settings);
    assert_eq!(own(), SurfaceSpec::default(), "the theme's again");
}
