//! A module's view over every window at once ([`workbench_model`]'s
//! `open_overlay_everywhere`: a dimmed screen, a calm scene — what stands for the whole
//! app): opened in one window, the app opens it in the others; closed in any (input there),
//! the app closes it in all. Each window says what changed in its own
//! ([`Workbench::everywhere_changed`]) and follows what the app says ([`Workbench::follow_everywhere`]).
//!
//! One on the whole screen (a calm scene: `open_full_screen_everywhere`) is shown by one
//! window a screen — the app chooses which — and the others leave theirs as they are
//! ([`Workbench::follow_elsewhere`]): nothing over them, nothing drawn for it; a key or a
//! click in one of them closes it everywhere, as in the view itself.

use super::*;

/// The view over every window this one shows (its extension), and a change not told yet.
#[derive(Debug, Default)]
pub(super) struct Everywhere {
    shown: Option<String>,
    /// Opened here, for every window.
    opened: bool,
    /// Opened here on the whole screen (the app takes the screen for it).
    full: bool,
    /// Shown on the whole screen by another window: nothing over this one.
    elsewhere: bool,
    /// Input here while it's shown elsewhere: closed for all.
    closed_here: bool,
}

impl Workbench {
    /// Opens `extension` bare here with `data`, for every window.
    pub(super) fn open_everywhere(&mut self, extension: &str, data: &dyn std::any::Any) -> bool {
        if !self.open_bare_view(extension, data, false) {
            return false;
        }
        self.everywhere =
            Everywhere { shown: Some(extension.to_owned()), opened: true, ..Everywhere::default() };
        true
    }

    /// Opens `extension` bare here, for every window, on the whole screen.
    pub(super) fn open_full_everywhere(&mut self, extension: &str) -> bool {
        if !self.open_bare_view(extension, &(), false) {
            return false;
        }
        self.everywhere = Everywhere {
            shown: Some(extension.to_owned()),
            opened: true,
            full: true,
            ..Everywhere::default()
        };
        true
    }

    /// This window shows the view over every window on the whole screen, opened here.
    pub fn everywhere_full(&self) -> bool {
        self.everywhere.full && self.everywhere.shown.is_some()
    }

    /// A key or a click here while the view is shown by another window: closed for all
    /// (told at the next [`Self::everywhere_changed`]). `true` if it was.
    pub fn everywhere_input(&mut self) -> bool {
        if !self.everywhere.elsewhere || self.everywhere.shown.is_none() {
            return false;
        }
        self.everywhere.closed_here = true;
        true
    }

    /// The view over every window is shown on the whole screen by another window: this one
    /// leaves its own as it is (its own showing it, given way: closed). `true` if what it
    /// shows changed.
    pub fn follow_elsewhere(&mut self, extension: &str) -> bool {
        let showing = self.modal_view.as_ref().is_some_and(|m| m.extension == extension)
            && self.everywhere.shown.as_deref() == Some(extension);
        if showing {
            self.modal_view = None;
        }
        self.everywhere = Everywhere {
            shown: Some(extension.to_owned()),
            elsewhere: true,
            ..Everywhere::default()
        };
        showing
    }

    /// What every window should show now, if this one changed it since it was last asked:
    /// `Some(Some(extension))` opened here, `Some(None)` closed here.
    pub fn everywhere_changed(&mut self) -> Option<Option<String>> {
        if std::mem::take(&mut self.everywhere.opened) {
            return Some(self.everywhere.shown.clone());
        }
        if std::mem::take(&mut self.everywhere.closed_here) {
            self.everywhere = Everywhere::default();
            return Some(None);
        }
        let shown = self.everywhere.shown.as_deref()?;
        if self.everywhere.elsewhere {
            return None;
        }
        if self.modal_view.as_ref().is_some_and(|m| m.extension == shown) {
            return None;
        }
        // Closed here (input, its own `Close`): closed in all.
        self.everywhere = Everywhere::default();
        Some(None)
    }

    /// Shows what every window shows (`None`: nothing): opened here without data, or
    /// closed. A window with another view over it (a wizard) keeps that one. `true` if what
    /// it shows changed.
    pub fn follow_everywhere(&mut self, wanted: Option<&str>) -> bool {
        if self.everywhere.elsewhere {
            // Shown elsewhere until now: nothing of it here to close.
            self.everywhere.elsewhere = false;
            if wanted.is_none() {
                self.everywhere.shown = None;
                return false;
            }
            self.everywhere.shown = None;
        }
        if self.everywhere.shown.as_deref() == wanted {
            return false;
        }
        self.everywhere.full = false;
        let mut changed = false;
        if let Some(shown) = self.everywhere.shown.take()
            && self.modal_view.as_ref().is_some_and(|m| m.extension == shown)
        {
            self.modal_view = None;
            changed = true;
        }
        if let Some(extension) = wanted
            && self.modal_view.is_none()
            && self.open_bare_view(extension, &(), false)
        {
            self.everywhere.shown = Some(extension.to_owned());
            changed = true;
        }
        changed
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/everywhere.rs"]
mod tests;
