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
fn the_defaults_gray_fully_without_dimming_following_both() {
    let s = settings();
    assert_eq!(Config::read(&s), Config::DEFAULT);
    assert!(STRENGTH.accepts(&75) && !STRENGTH.accepts(&60));
    assert!(DIM.accepts(&20) && !DIM.accepts(&30));
    assert!(FOLLOW.accepts(&"mouse".to_owned()) && !FOLLOW.accepts(&"eyes".to_owned()));
}

#[test]
fn the_page_saves_each_choice_and_the_policy_reads_them() {
    let s = settings();
    let page = FocusSettings;
    let info = page.page_info().unwrap();
    assert_eq!((info.id, info.group), ("focus", SettingsGroup::Interface));
    let block = page.block(scope(&s));
    let keys: Vec<_> = block.rows.iter().filter_map(|r| r.searchable()).map(|r| r.0).collect();
    assert_eq!(keys, ["strength", "dim", "follow"]);
    page.choose(scope(&s), "strength", 50);
    page.choose(scope(&s), "dim", 20);
    page.choose(scope(&s), "follow", 0);
    // Not offered: ignored.
    page.choose(scope(&s), "strength", 33);
    let config = Config::read(&s);
    assert_eq!(config, Config { gray: 0.5, dim: 0.2, follow: Follow::Keyboard });
    page.apply(&s);
    assert_eq!(super::config(), config);
    let block = page.block(scope(&s));
    let current = |key| {
        block.rows.iter().find_map(|r| match r {
            SettingsRow::Segmented { key: k, current, .. } if *k == key => Some(*current),
            _ => None,
        })
    };
    assert_eq!(
        (current("strength"), current("dim"), current("follow")),
        (Some(50), Some(20), Some(0))
    );
}
