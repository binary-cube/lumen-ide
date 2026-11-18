//! The files' icons (by name, through the modules' rules and the chosen set), cached for the tabs
//! and the tree.

use super::*;

/// A file's icon and color, by name: the icons' registry decides (the chosen file icon
/// set's, else ours — the modules' rules, then the language).
pub(super) fn file_icon(
    support: &LanguageSupport,
    icons: &Icons,
    palette: &Palette,
    name: &str,
) -> (IconId, Rgba) {
    let (icon, language) = support.languages.icon_for(name);
    let path = std::path::Path::new(name);
    if is_virtual(path) {
        // A view's tab its module gave an icon (a rule for its `view-*` extension): a file
        // icon set can't know it, so ours. Else as its title (without the extension) reads.
        if icon != "file" {
            return (icons.id_or_file(icon), palette.tint(&icons::Tint::Language(language)));
        }
        let title = path.file_stem().map(|s| s.to_string_lossy()).unwrap_or_default();
        return file_icon(support, icons, palette, &title);
    }
    let (id, tint) = icons.file(name, icon, &language);
    (id, palette.tint(&tint))
}

impl Default for FileIcons {
    fn default() -> Self {
        Self {
            stamp: (0, None, icons::IconLook::default()),
            icons: kernel::GenerationalCache::new(FILE_ICONS_LIMIT),
        }
    }
}

impl FileIcons {
    /// File `name`'s icon and color, found again only when the theme (`theme`: its
    /// generation), the file icon set or the look changed since.
    pub(super) fn get(
        &mut self,
        support: &LanguageSupport,
        icons: &Icons,
        palette: &Palette,
        theme: u64,
        name: &str,
    ) -> (IconId, Rgba) {
        let stamp = (theme, icons.file_set(), icons.look());
        if stamp != self.stamp {
            self.icons.clear();
            self.stamp = stamp;
        }
        if let Some(&found) = self.icons.get(name) {
            return found;
        }
        let found = file_icon(support, icons, palette, name);
        self.icons.insert(name.into(), found);
        found
    }
}

impl Purge for FileIcons {
    fn purge(&mut self) {
        self.icons.clear();
    }
}

/// The icons, parsed once and shared by every window (dropped with the last one).
pub(super) fn shared_icons(support: &LanguageSupport) -> Arc<Icons> {
    static SHARED: std::sync::Mutex<std::sync::Weak<Icons>> =
        std::sync::Mutex::new(std::sync::Weak::new());
    let Ok(mut shared) = SHARED.lock() else {
        return Arc::new(Icons::builtin());
    };
    if let Some(icons) = shared.upgrade() {
        return icons;
    }
    let mut icons = Icons::builtin();
    for icon in &support.icons {
        icons.register(icon.name, icon.svg);
    }
    for pack in &support.icon_packs {
        icons.register_pack(Arc::clone(pack));
    }
    for set in &support.file_icon_sets {
        icons.register_file_set(Arc::clone(set));
    }
    icons.set_look(crate::icon_style::current(&icons));
    icons.set_file_set(crate::icon_style::current_file_set(&icons));
    let icons = Arc::new(icons);
    *shared = Arc::downgrade(&icons);
    icons
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/tab_icons.rs"]
mod tests;
