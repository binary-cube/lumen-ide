//! The settings service: every setting declared once ([`Setting`]: its section, key, type
//! and default), read and written through one service — cached (the file isn't read again
//! on every access), a change announced as a [`SettingChanged`] event.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use kernel::{DomainEvent, EventBus, Subscription};

use crate::store::{Section, SettingValue, SettingsStore};

/// A type a setting can have.
pub trait SettingType: Sized + Clone + Send + Sync + 'static {
    /// `None` if the stored value has another type (the default is used).
    fn from_value(value: &SettingValue) -> Option<Self>;
    fn to_value(&self) -> SettingValue;
}

impl SettingType for bool {
    fn from_value(value: &SettingValue) -> Option<Self> {
        value.as_bool()
    }
    fn to_value(&self) -> SettingValue {
        SettingValue::Bool(*self)
    }
}

impl SettingType for i64 {
    fn from_value(value: &SettingValue) -> Option<Self> {
        match value {
            SettingValue::Int(n) => Some(*n),
            _ => None,
        }
    }
    fn to_value(&self) -> SettingValue {
        SettingValue::Int(*self)
    }
}

impl SettingType for String {
    fn from_value(value: &SettingValue) -> Option<Self> {
        value.as_str().map(str::to_owned)
    }
    fn to_value(&self) -> SettingValue {
        SettingValue::Text(self.clone())
    }
}

impl SettingType for Vec<String> {
    fn from_value(value: &SettingValue) -> Option<Self> {
        value.as_list().map(<[String]>::to_vec)
    }
    fn to_value(&self) -> SettingValue {
        SettingValue::List(self.clone())
    }
}

/// A setting's declaration: `[section] key`, its default, and which values are valid.
#[derive(Debug)]
pub struct Setting<T: 'static> {
    pub section: &'static str,
    pub key: &'static str,
    default: fn() -> T,
    /// A stored value is used only if this accepts it (otherwise the default).
    valid: fn(&T) -> bool,
}

impl<T> Setting<T> {
    pub const fn new(section: &'static str, key: &'static str, default: fn() -> T) -> Self {
        Self { section, key, default, valid: |_| true }
    }

    /// Only values `valid` accepts are used (a hand-edited file can't break the app).
    pub const fn valid(self, valid: fn(&T) -> bool) -> Self {
        Self { valid, ..self }
    }

    pub fn default_value(&self) -> T {
        (self.default)()
    }

    /// `value` would be used (the choices a settings block offers are checked by it).
    pub fn accepts(&self, value: &T) -> bool {
        (self.valid)(value)
    }
}

/// A setting changed (through the service): `[section] key`; `key` is `*` when a whole
/// section was rewritten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingChanged {
    pub section: String,
    pub key: String,
}

impl DomainEvent for SettingChanged {}

/// The settings, for the whole app (one instance).
pub struct SettingsService {
    store: Arc<dyn SettingsStore>,
    /// The sections read so far (`None`: missing or unreadable).
    cache: RwLock<HashMap<String, Option<Section>>>,
    /// Counts the writes through the service, twice each (before and after it, odd while
    /// one goes on): a [`refresh`](Self::refresh) that saw one start or end while it read
    /// drops what it read (the write is newer).
    writes: AtomicU64,
    events: EventBus,
}

impl std::fmt::Debug for SettingsService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsService").field("store", &self.store).finish_non_exhaustive()
    }
}

impl SettingsService {
    pub fn new(store: Arc<dyn SettingsStore>) -> Self {
        Self { store, cache: RwLock::default(), writes: AtomicU64::new(0), events: EventBus::new() }
    }

    /// The setting's value: as stored if it has the right type and is valid, otherwise its
    /// default.
    pub fn get<T: SettingType>(&self, setting: &Setting<T>) -> T {
        self.value(setting.section, setting.key)
            .and_then(|v| T::from_value(&v))
            .filter(|v| (setting.valid)(v))
            .unwrap_or_else(|| setting.default_value())
    }

    /// Whether the setting was chosen (stored), as opposed to its default being used.
    pub fn is_set<T: SettingType>(&self, setting: &Setting<T>) -> bool {
        self.value(setting.section, setting.key).and_then(|v| T::from_value(&v)).is_some()
    }

    /// Stores the setting's new value and announces it.
    pub fn set<T: SettingType>(&self, setting: &Setting<T>, value: T) -> std::io::Result<()> {
        let stored = value.to_value();
        self.writes.fetch_add(1, Ordering::AcqRel);
        let written = self.store.set(setting.section, setting.key, stored.clone());
        if written.is_ok()
            && let Ok(mut cache) = self.cache.write()
        {
            let section = cache.entry(setting.section.to_owned()).or_default();
            section.get_or_insert_with(Section::new).insert(setting.key.to_owned(), stored);
        }
        self.writes.fetch_add(1, Ordering::AcqRel);
        written?;
        self.events.publish(&SettingChanged {
            section: setting.section.to_owned(),
            key: setting.key.to_owned(),
        });
        Ok(())
    }

    /// A whole section (settings with their own structure: the themes, the modules).
    pub fn section(&self, name: &str) -> Option<Section> {
        if let Some(cached) = self.cache.read().ok().and_then(|c| c.get(name).cloned()) {
            return cached;
        }
        let section = self.store.section(name);
        if let Ok(mut cache) = self.cache.write() {
            cache.insert(name.to_owned(), section.clone());
        }
        section
    }

    /// Changes a whole section and announces it (key `*`).
    pub fn update(&self, name: &str, change: &mut dyn FnMut(&mut Section)) -> std::io::Result<()> {
        self.writes.fetch_add(1, Ordering::AcqRel);
        let written = self.store.update(name, change);
        if let Ok(mut cache) = self.cache.write() {
            cache.remove(name);
        }
        self.writes.fetch_add(1, Ordering::AcqRel);
        written?;
        self.events.publish(&SettingChanged { section: name.to_owned(), key: "*".to_owned() });
        Ok(())
    }

    /// Forgets what was read (the file was edited by hand): the next reads go to the store.
    pub fn reload(&self) {
        if let Ok(mut cache) = self.cache.write() {
            cache.clear();
        }
    }

    /// Reads again the sections read so far (the file may have been edited by hand) and
    /// keeps the ones that changed, announcing each (key `*`). Blocking: meant for a
    /// background thread, so the UI keeps answering from the cache meanwhile. Returns the
    /// sections that changed (none if a write through the service happened meanwhile).
    pub fn refresh(&self) -> Vec<String> {
        let started = self.writes.load(Ordering::Acquire);
        let mut changed = Vec::new();
        if started % 2 == 1 {
            return changed;
        }
        let names: Vec<String> =
            self.cache.read().map(|c| c.keys().cloned().collect()).unwrap_or_default();
        // Unreadable now (a file half-edited by hand): what was read stays.
        let Some(sections) = self.store.sections(&names) else { return changed };
        let fresh: Vec<(String, Option<Section>)> = names.into_iter().zip(sections).collect();
        {
            let Ok(mut cache) = self.cache.write() else { return changed };
            if self.writes.load(Ordering::Acquire) != started {
                return changed;
            }
            for (name, section) in fresh {
                if cache.get(&name) != Some(&section) {
                    cache.insert(name.clone(), section);
                    changed.push(name);
                }
            }
        }
        for name in &changed {
            self.events.publish(&SettingChanged { section: name.clone(), key: "*".to_owned() });
        }
        changed
    }

    /// Calls `handler` on every change (while the subscription is kept).
    pub fn subscribe(
        &self,
        handler: impl Fn(&SettingChanged) + Send + Sync + 'static,
    ) -> Subscription {
        self.events.subscribe(handler)
    }

    /// One value (only it is copied; the section is read once).
    fn value(&self, section: &str, key: &str) -> Option<SettingValue> {
        if let Some(cached) = self.cache.read().ok()?.get(section) {
            return cached.as_ref()?.get(key).cloned();
        }
        self.section(section)?.remove(key)
    }
}

#[cfg(test)]
#[path = "../tests/unit/service.rs"]
mod tests;
