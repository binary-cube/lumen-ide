//! Alt pressed and released alone (nothing else in between: no key, no click, no other
//! modifier) opens or closes the menu drawer, as Alt reaches the menu bar on Windows and
//! Linux. Not on macOS: Option alone types nothing there but is no menu key either.

use platform_api::Modifiers;

#[derive(Debug, Default)]
pub(super) struct AltTap {
    /// Alt alone is held down now.
    down: bool,
    /// The window's focus changed: what's held now began elsewhere (Alt + Tab), ignored
    /// until every modifier is released.
    blocked: bool,
}

impl AltTap {
    /// The modifiers changed. `true` when this releases a lone Alt.
    pub fn modifiers(&mut self, m: Modifiers) -> bool {
        let alt_only = m.alt && !m.ctrl && !m.shift && !m.meta;
        let none = !m.alt && !m.ctrl && !m.shift && !m.meta;
        if self.blocked {
            self.blocked = !none;
            self.down = false;
            return false;
        }
        if alt_only {
            self.down = true;
            return false;
        }
        let tapped = none && self.down;
        // Released, or another modifier joined (Ctrl + Alt is AltGr): over either way.
        self.down = false;
        tapped
    }

    /// A real key, a click or a scroll happened: not a lone Alt.
    pub fn cancel(&mut self) {
        self.down = false;
    }

    /// The window gained or lost the focus: the modifiers held now don't count.
    pub fn focus_changed(&mut self) {
        self.down = false;
        self.blocked = true;
    }
}

#[cfg(test)]
#[path = "../../tests/unit/app/alt_tap.rs"]
mod tests;
