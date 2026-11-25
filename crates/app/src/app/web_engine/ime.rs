//! What the input method composes in a page, as the engine is told it: a give-up held for
//! the text that follows, a cursor counted as Chromium counts.

use platform_api::{PageInput, PageMouseAction};

/// What the input method composes in a page. winit gives up a composition (an empty one)
/// right before committing its text: the give-up waits for what follows — the text replaces
/// the composition then — and is told only before anything else.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Composition {
    /// Something is composed in the page now.
    composing: bool,
    /// …given up (an empty composition), not told yet.
    cancel_pending: bool,
}

/// What a composition's change sends the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ImeStep {
    /// The composed text.
    Compose,
    /// Nothing now (a give-up waiting, or nothing composed).
    Nothing,
}

/// What the input method's text sends the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TextStep {
    /// In place of what was composed.
    Commit,
    /// Typed at the caret (nothing composed).
    Type,
}

impl Composition {
    /// The input method composes `empty` text or some.
    pub(super) fn compose(self, empty: bool) -> (Self, ImeStep) {
        match empty {
            true => (Self { cancel_pending: self.composing, ..self }, ImeStep::Nothing),
            false => (Self { composing: true, cancel_pending: false }, ImeStep::Compose),
        }
    }

    /// Its text came.
    pub(super) fn text(self) -> (Self, TextStep) {
        let step = if self.composing { TextStep::Commit } else { TextStep::Type };
        (Self::default(), step)
    }

    /// Whether `input` ends what's composed: the focus moving, a press.
    pub(super) fn ends_at(input: &PageInput) -> bool {
        match input {
            PageInput::Focus(_) => true,
            PageInput::Mouse(mouse) => matches!(mouse.action, PageMouseAction::Down(_)),
            _ => false,
        }
    }

    /// Anything else comes (a key, the mouse, the focus): `true` if a give-up waiting is
    /// told first.
    pub(super) fn settle(self) -> (Self, bool) {
        match self.cancel_pending {
            true => (Self::default(), true),
            false => (self, false),
        }
    }
}

/// The UTF-16 offset of byte `at` in `text` (an input method's cursor, as Chromium counts).
pub(super) fn utf16_at(text: &str, at: usize) -> u32 {
    let at = (0..=at.min(text.len())).rev().find(|i| text.is_char_boundary(*i)).unwrap_or(0);
    u32::try_from(text[..at].encode_utf16().count()).unwrap_or(u32::MAX)
}

#[cfg(test)]
#[path = "../../../tests/unit/app/web_engine/ime.rs"]
mod tests;
