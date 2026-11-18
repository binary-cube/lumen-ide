//! The caret's place told to the system's input methods. On macOS each telling makes the
//! system show its input source's popup ("US") at the caret again: the app's caret moving
//! (typing, a click, a field taking the keyboard, a panel over the editor) would bring it
//! up all the time. So there the place is kept, and told only when it's needed: while a
//! composition is under way (dead keys, CJK: the candidates follow the caret) and when the
//! keyboard's input source changes (the popup then shows once, where the caret is).
//! Elsewhere it's told at once, as before.

use std::sync::{Arc, Mutex, PoisonError};

use kernel::Rect;

/// A window's caret place for the input methods, shared by the window the app holds and the
/// event loop.
#[derive(Debug, Default)]
pub(crate) struct ImeCaret {
    /// The last place the app said.
    area: Option<Rect>,
    /// A composition is shown (its marked text).
    composing: bool,
    /// The place the system was last told.
    told: Option<Rect>,
}

/// What [`ImeCaret`] says to do: tell the system this place.
pub(crate) type Tell = Option<Rect>;

impl ImeCaret {
    pub(crate) fn shared() -> Arc<Mutex<Self>> {
        Arc::default()
    }

    /// The app's caret is at `area`: told now only while composing, or wherever the system
    /// doesn't show a popup for it (`defer`: macOS).
    pub(crate) fn moved(&mut self, area: Rect, defer: bool) -> Tell {
        self.area = Some(area);
        if defer && !self.composing { None } else { self.tell() }
    }

    /// A composition's text changed (empty: it ended): at its start, the place is told.
    pub(crate) fn composing(&mut self, composing: bool) -> Tell {
        let started = composing && !self.composing;
        self.composing = composing;
        if started { self.tell() } else { None }
    }

    /// The keyboard's input source changed: its popup shows where the caret is.
    pub(crate) fn source_changed(&mut self) -> Tell {
        self.told = None;
        self.tell()
    }

    fn tell(&mut self) -> Tell {
        let area = self.area?;
        if self.told == Some(area) {
            return None;
        }
        self.told = Some(area);
        Some(area)
    }
}

/// Locks `caret` (a poisoned lock is still usable: its fields are plain values).
pub(crate) fn lock(caret: &Mutex<ImeCaret>) -> std::sync::MutexGuard<'_, ImeCaret> {
    caret.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The window told of `area` (physical pixels).
pub(crate) fn tell(window: &winit::window::Window, area: Rect) {
    window.set_ime_cursor_area(
        winit::dpi::PhysicalPosition::new(area.min_x(), area.min_y()),
        winit::dpi::PhysicalSize::new(area.size.width, area.size.height),
    );
}

/// The keyboard's input source changing reaches the loop as `on_change` (macOS), observed
/// once, for the app's lifetime.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
pub(crate) fn observe_source(on_change: impl Fn() + 'static) {
    use std::ptr::NonNull;

    use objc2_app_kit::NSTextInputContextKeyboardSelectionDidChangeNotification;
    use objc2_foundation::{NSNotification, NSNotificationCenter};

    let block = block2::RcBlock::new(move |_: NonNull<NSNotification>| on_change());
    let center = NSNotificationCenter::defaultCenter();
    // SAFETY: a notification name AppKit defines; no object filter, posted on the main
    // thread (no queue: the posting thread).
    let token = unsafe {
        center.addObserverForName_object_queue_usingBlock(
            Some(NSTextInputContextKeyboardSelectionDidChangeNotification),
            None,
            None,
            &block,
        )
    };
    std::mem::forget(token);
}

#[cfg(test)]
#[path = "../tests/unit/ime_caret.rs"]
mod tests;
