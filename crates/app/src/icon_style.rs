//! The icons' look (Settings → Icons): one of our styles or a module's pack, saved as
//! `[appearance] icons` in `settings.toml` (a style's id, or a pack's). The sets are ours or
//! the modules' (licensed), not user files.

use icons::{IconLook, Icons};

/// The chosen look (Octicons by default; a pack no longer there: ours).
pub fn current(icons: &Icons) -> IconLook {
    icons
        .look_from_id(&crate::config::settings().get(&crate::settings::ICON_STYLE))
        .unwrap_or_default()
}

pub fn save(icons: &Icons, look: IconLook) {
    let id = icons.look_id(look);
    if let Err(e) = crate::config::settings().set(&crate::settings::ICON_STYLE, id) {
        tracing::warn!("can't save the icon set: {e}");
    }
}

/// The chosen file icon set (its place; `None`: ours, by language).
pub fn current_file_set(icons: &Icons) -> Option<u8> {
    let id = crate::config::settings().get(&crate::settings::FILE_ICONS);
    icons.file_sets().iter().position(|s| s.id() == id).map(|i| i as u8)
}

pub fn save_file_set(icons: &Icons, set: Option<u8>) {
    let id = set
        .and_then(|s| icons.file_sets().get(usize::from(s)))
        .map_or_else(|| "ours".to_owned(), |s| s.id().to_owned());
    if let Err(e) = crate::config::settings().set(&crate::settings::FILE_ICONS, id) {
        tracing::warn!("can't save the file icons: {e}");
    }
}
