use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use settings_model::{Section, SettingsStore};

use super::*;

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

#[test]
fn masking_secrets_is_off_by_default_and_saved() {
    let settings = SettingsService::new(Arc::new(Memory::default()));
    let scope = SettingsScope {
        settings: &settings,
        project: None,
        project_data: None,
        language: Some(crate::ID),
        wake: None,
    };
    let block = DotenvSettings.block(scope);
    assert!(matches!(
        block.rows.first(),
        Some(SettingsRow::Toggle { key: "mask_secrets", on: false, .. })
    ));
    DotenvSettings.choose(scope, "mask_secrets", 1);
    assert!(settings.get(&MASK_SECRETS));
    assert!(matches!(
        block.rows.get(1),
        Some(SettingsRow::Toggle { key: "unused_keys", on: false, .. })
    ));
    DotenvSettings.choose(scope, "unused_keys", 1);
    assert!(settings.get(&UNUSED_KEYS));
}

#[test]
fn what_follows_the_settings_is_told_while_kept() {
    let told = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = Arc::clone(&told);
    let follower: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        counter.fetch_add(1, Ordering::Relaxed);
    });
    // Its own list: the other tests running beside it change the settings too.
    let list = Mutex::new(vec![Arc::downgrade(&follower)]);
    let flag = AtomicBool::new(false);
    store_telling(&flag, true, &list);
    store_telling(&flag, true, &list);
    assert_eq!(told.load(Ordering::Relaxed), 1, "only a change is told");
    drop(follower);
    store_telling(&flag, false, &list);
    assert_eq!(told.load(Ordering::Relaxed), 1);
}
