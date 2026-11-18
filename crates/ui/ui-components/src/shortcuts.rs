//! The commands' keys as the user has them now, for what shows them (a tooltip's shortcut):
//! the app tells how to read them ([`set_reader`]); modules ask by command id ([`label`]),
//! so a remapped command shows its new keys.

use std::sync::RwLock;

type Reader = fn(&str) -> Option<String>;

static READER: RwLock<Option<Reader>> = RwLock::new(None);

/// How a command's keys are read (the app's keymap: its first shortcut, as the system
/// shows it).
pub fn set_reader(reader: Reader) {
    if let Ok(mut slot) = READER.write() {
        *slot = Some(reader);
    }
}

/// Command `id`'s keys as shown (`⌘S`), if it has any.
pub fn label(id: &str) -> Option<String> {
    let reader = READER.read().ok().and_then(|r| *r)?;
    reader(id)
}
