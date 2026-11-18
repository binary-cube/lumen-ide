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

fn settings() -> SettingsService {
    SettingsService::new(Arc::new(Memory::default()))
}

fn scope(settings: &SettingsService) -> SettingsScope<'_> {
    SettingsScope { settings, project: None, project_data: None, language: None, wake: None }
}

#[test]
fn the_defaults_are_calm_and_never_start_by_themselves() {
    let s = settings();
    assert_eq!(Config::read(&s), Config::DEFAULT);
    assert_eq!(Config::DEFAULT.idle_minutes, None, "auto-start is off by default");
    assert_eq!(Config::DEFAULT.fps, 30, "30 frames a second at first");
    assert!(SCENE.accepts(&"waves".to_owned()) && SCENE.accepts(&ROTATE.to_owned()));
    assert!(!SCENE.accepts(&"aquarium".to_owned()));
    assert!(SPEED.accepts(&150) && !SPEED.accepts(&120));
    assert!(FPS.accepts(&30) && !FPS.accepts(&144));
    assert!(IDLE_MINUTES.accepts(&0) && !IDLE_MINUTES.accepts(&7));
    assert!(MOTION.accepts(&"reduced".to_owned()) && !MOTION.accepts(&"slow".to_owned()));
}

#[test]
fn the_page_saves_each_choice_and_the_views_read_them() {
    let s = settings();
    let page = RelaxSettings;
    let info = page.page_info().unwrap();
    assert_eq!((info.id, info.group), ("relax", SettingsGroup::Interface));
    // Rotating shows how often.
    let keys = |s: &SettingsService| -> Vec<&'static str> {
        page.block(scope(s)).rows.iter().filter_map(|r| r.searchable().map(|(k, _, _)| k)).collect()
    };
    assert!(!keys(&s).contains(&"rotate_minutes"));
    page.choose(scope(&s), "scene", SCENES.len() as i64);
    assert!(keys(&s).contains(&"rotate_minutes"));
    page.choose(scope(&s), "rotate_minutes", 15);
    page.choose(scope(&s), "speed", 50);
    page.choose(scope(&s), "density", 150);
    page.choose(scope(&s), "theme_colors", 0);
    page.choose(scope(&s), "clock", 1);
    page.choose(scope(&s), "motion", 1);
    page.choose(scope(&s), "fps", 60);
    page.choose(scope(&s), "idle_minutes", 10);
    // A value not offered is left out.
    page.choose(scope(&s), "fps", 144);
    let c = Config::read(&s);
    assert_eq!(c.scene, SceneChoice::Rotate);
    assert_eq!((c.rotate_minutes, c.speed, c.density), (15, 0.5, 1.5));
    assert!(!c.theme_colors && c.clock);
    assert_eq!((c.motion, c.fps, c.idle_minutes), (Motion::Reduced, 60, Some(10)));
    page.choose(scope(&s), "scene", 1);
    assert_eq!(Config::read(&s).scene, SceneChoice::Fixed("waves"));
    page.choose(scope(&s), "idle_minutes", 0);
    assert_eq!(Config::read(&s).idle_minutes, None);
}

#[test]
fn the_scene_is_chosen_on_cards_with_their_pictures_and_previewed() {
    let s = settings();
    let block = RelaxSettings.block(scope(&s));
    let Some(SettingsRow::Cards { key, cards, current, preview, .. }) = block.rows.first() else {
        panic!("the scenes' cards first");
    };
    assert_eq!((*key, *current, *preview), ("scene", 0, crate::preview::PREVIEW));
    assert_eq!(cards.len(), SCENES.len() + 1, "each scene, then in turn");
    assert!(cards[0].tag.is_some() && cards.last().unwrap().tag.is_none(), "3D tagged");
    assert!(
        block.rows.iter().any(
            |r| matches!(r, SettingsRow::Actions { buttons, .. } if buttons[0].0 == "preview")
        )
    );
    assert_eq!(RelaxSettings.command("preview"), Some(crate::START));
    assert_eq!(RelaxSettings.command("other"), None);
}
