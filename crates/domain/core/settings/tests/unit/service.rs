use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

/// Settings kept in memory; counts the reads (the service must cache them).
#[derive(Debug, Default)]
struct Memory {
    sections: Mutex<HashMap<String, Section>>,
    reads: AtomicUsize,
}

impl SettingsStore for Memory {
    fn section(&self, name: &str) -> Option<Section> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        self.sections.lock().unwrap().get(name).cloned()
    }
    fn update(&self, name: &str, change: &mut dyn FnMut(&mut Section)) -> std::io::Result<()> {
        change(self.sections.lock().unwrap().entry(name.to_owned()).or_default());
        Ok(())
    }
}

const AUTOSAVE: Setting<bool> = Setting::new("editor", "autosave", || true);
const KEYMAP: Setting<String> = Setting::new("general", "keymap", || "default".to_owned())
    .valid(|id| ["default", "vscode"].contains(&id.as_str()));

#[test]
fn defaults_typed_values_and_validation() {
    let store = Arc::new(Memory::default());
    let service = SettingsService::new(store.clone());
    assert!(service.get(&AUTOSAVE), "the default");
    assert!(!service.is_set(&AUTOSAVE));
    service.set(&AUTOSAVE, false).unwrap();
    assert!(!service.get(&AUTOSAVE) && service.is_set(&AUTOSAVE));
    // A hand-edited file: another type, or a value that isn't valid → the default.
    store
        .update("general", &mut |s| {
            s.insert("keymap".into(), SettingValue::Text("emacs".into()));
        })
        .unwrap();
    service.reload();
    assert_eq!(service.get(&KEYMAP), "default");
    store
        .update("editor", &mut |s| {
            s.insert("autosave".into(), SettingValue::Text("yes".into()));
        })
        .unwrap();
    service.reload();
    assert!(service.get(&AUTOSAVE), "wrong type: the default");
}

#[test]
fn reads_are_cached_and_changes_announced() {
    let store = Arc::new(Memory::default());
    let service = SettingsService::new(store.clone());
    for _ in 0..10 {
        service.get(&AUTOSAVE);
    }
    assert_eq!(store.reads.load(Ordering::Relaxed), 1, "one read for the section");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    let _subscription = service.subscribe(move |e: &SettingChanged| {
        sink.lock().unwrap().push(format!("{}.{}", e.section, e.key));
    });
    service.set(&AUTOSAVE, false).unwrap();
    service
        .update("appearance", &mut |s| {
            s.insert("theme".into(), SettingValue::Text("x".into()));
        })
        .unwrap();
    assert!(!service.get(&AUTOSAVE), "the cache follows what was set");
    assert_eq!(*seen.lock().unwrap(), ["editor.autosave", "appearance.*"]);
}

#[test]
fn a_refresh_keeps_and_announces_only_what_changed_on_disk() {
    let store = Arc::new(Memory::default());
    let service = SettingsService::new(store.clone());
    assert!(service.get(&AUTOSAVE));
    service.get(&KEYMAP);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    let _subscription = service.subscribe(move |e: &SettingChanged| {
        sink.lock().unwrap().push(format!("{}.{}", e.section, e.key));
    });
    assert!(service.refresh().is_empty(), "nothing changed");
    // Edited by hand (straight in the store, not through the service).
    store
        .update("editor", &mut |s| {
            s.insert("autosave".into(), SettingValue::Bool(false));
        })
        .unwrap();
    assert!(service.get(&AUTOSAVE), "the cache answers until refreshed");
    assert_eq!(service.refresh(), ["editor"]);
    assert!(!service.get(&AUTOSAVE));
    assert_eq!(*seen.lock().unwrap(), ["editor.*"]);
}

/// A store whose reads let a write through the service happen in the middle of a refresh.
#[derive(Debug)]
struct Racing {
    inner: Memory,
    service: std::sync::OnceLock<std::sync::Weak<SettingsService>>,
    armed: std::sync::atomic::AtomicBool,
}

impl SettingsStore for Racing {
    fn section(&self, name: &str) -> Option<Section> {
        let read = self.inner.section(name);
        if self.armed.swap(false, Ordering::AcqRel)
            && let Some(service) = self.service.get().and_then(std::sync::Weak::upgrade)
        {
            service.set(&AUTOSAVE, true).unwrap();
        }
        read
    }
    fn update(&self, name: &str, change: &mut dyn FnMut(&mut Section)) -> std::io::Result<()> {
        self.inner.update(name, change)
    }
}

#[test]
fn a_write_during_a_refresh_wins() {
    let store = Arc::new(Racing {
        inner: Memory::default(),
        service: std::sync::OnceLock::new(),
        armed: std::sync::atomic::AtomicBool::new(false),
    });
    let service = Arc::new(SettingsService::new(store.clone()));
    store.service.set(Arc::downgrade(&service)).unwrap();
    service.set(&AUTOSAVE, false).unwrap();
    assert!(!service.get(&AUTOSAVE));
    store.armed.store(true, Ordering::Release);
    assert!(service.refresh().is_empty(), "what was read is older than the write");
    assert!(service.get(&AUTOSAVE), "the write is kept");
}
