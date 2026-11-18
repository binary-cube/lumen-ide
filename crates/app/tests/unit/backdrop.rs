use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use settings_model::{Section, SettingsStore};
use ui_workbench::backdrop::{BackdropRole, DEFAULT_BLUR};

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

/// The theme's backdrop until the user chooses: their style, tint and blur over it (each
/// "from theme" on its own), never blurred where the renderer can't; the sliders show
/// "from theme" first.
#[test]
fn the_user_s_backdrop_goes_over_the_theme_s() {
    let settings = SettingsService::new(Arc::new(Memory::default()));
    let scope = SettingsScope {
        settings: &settings,
        project: None,
        project_data: None,
        wake: None,
        language: None,
    };
    let theme = BackdropStyle::resolve(
        BackdropSpec { opacity: Some(0.2), ..BackdropSpec::default() },
        kernel::Rgba::rgb(0x151618),
    );
    BackdropSettings.apply(&settings);
    assert_eq!(own(), BackdropSpec::default());
    assert_eq!(style(&theme, true), theme);
    // The sliders at the theme's own values (its 20 % tint, the default blur), said so.
    let block = BackdropSettings.block(scope);
    assert_eq!(block.rows.len(), 3);
    let slider = |row: &SettingsRow| match row {
        SettingsRow::Slider { stops, current, default, .. } => (stops.clone(), *current, *default),
        other => panic!("a slider: {other:?}"),
    };
    let (stops, current, default) = slider(&block.rows[1]);
    assert_eq!((current, default), (20, Some(20)));
    let theirs = stops.iter().find(|(v, _)| *v == 20).unwrap();
    assert!(theirs.1.contains("20%") && theirs.1.len() > "20%".len(), "{theirs:?}");
    let (stops, current, _) = slider(&block.rows[2]);
    assert_eq!(current, DEFAULT_BLUR as i64);
    assert_eq!(stops.last().map(|s| s.0), Some(MAX_BLUR as i64));
    // Choosing the theme's own value follows the theme.
    BackdropSettings.choose(scope, "opacity", 20);
    BackdropSettings.apply(&settings);
    assert_eq!(own(), BackdropSpec::default());
    BackdropSettings.choose(scope, "style", 2);
    BackdropSettings.choose(scope, "blur", 6);
    BackdropSettings.choose(scope, "opacity", 30);
    BackdropSettings.apply(&settings);
    let mine = style(&theme, true);
    assert_eq!(mine.kind, BackdropKind::Glass);
    assert_eq!(mine.blur_radius(2.0), Some(12.0));
    assert!((mine.opacity(BackdropRole::Dialog) - 0.3).abs() < 1e-6);
    assert_eq!(style(&theme, false).blur_radius(2.0), None, "a renderer that can't blur");
    BackdropSettings.choose(scope, "opacity", 20);
    BackdropSettings.choose(scope, "style", 0);
    BackdropSettings.apply(&settings);
    let back = style(&theme, true);
    assert_eq!(back.kind, BackdropKind::Simple);
    assert!((back.opacity(BackdropRole::Dialog) - 0.2).abs() < 1e-6, "the theme's again");
    BackdropSettings.choose(scope, "blur", DEFAULT_BLUR as i64);
    BackdropSettings.apply(&settings);
    assert_eq!(own(), BackdropSpec::default());
    // In the same test: the user's choice is the process's.
    the_settings_rewritten_from_outside_reach_the_palette();
}

/// `settings.toml` rewritten by another program: read again, its blocks applied (the
/// modules' "*" subscription), counted as an outside change (each window then resolves
/// its theme again) — and a palette resolved now has the new backdrop (plan 55e).
fn the_settings_rewritten_from_outside_reach_the_palette() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let settings = crate::config::settings();
    // The section read once (as the app does at startup): it's what is read again.
    assert_eq!(settings.get(&crate::settings::BACKDROP_STYLE), "theme");
    let id = format!("{:?}", std::thread::current().id())
        .replace(|c: char| !c.is_ascii_alphanumeric(), "");
    let file = crate::config::config_dir().unwrap().join(format!("settings-{id}.toml"));
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "[appearance]\nbackdrop = \"glass\"\nbackdrop_opacity = 40\n").unwrap();
    let before = crate::config::outside_changes();
    assert_eq!(crate::config::refresh_from_outside(settings), ["appearance"]);
    assert_eq!(crate::config::outside_changes(), before + 1);
    assert_eq!(own().style, Some(BackdropKind::Glass));
    let mut wb =
        crate::workbench::Workbench::new(Arc::new(support), crate::workbench::Ports::local());
    wb.set_palette(ui_workbench::Palette::default());
    let backdrop = wb.palette().backdrop;
    assert_eq!(backdrop.kind, BackdropKind::Glass);
    assert!((backdrop.opacity(BackdropRole::Dialog) - 0.4).abs() < 1e-6);
    // Read again unchanged: nothing to resolve again.
    assert!(crate::config::refresh_from_outside(settings).is_empty());
    assert_eq!(crate::config::outside_changes(), before + 1);
    let _ = std::fs::remove_file(&file);
    settings.refresh();
    BackdropSettings.apply(settings);
    assert_eq!(own(), BackdropSpec::default());
}

/// A snapshot's choice, as written.
#[test]
fn a_snapshot_s_choice_is_read() {
    let spec = parse_own("glass:6:20");
    assert_eq!(spec.style, Some(BackdropKind::Glass));
    assert_eq!(spec.blur, Some(6.0));
    assert!((spec.opacity.unwrap() - 0.2).abs() < 1e-6);
    assert_eq!(parse_own("theme"), BackdropSpec::default());
}
