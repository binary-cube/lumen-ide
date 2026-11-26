//! A tab tinted to draw the eye to it (a hint is about it, something there wants a look): the
//! tab itself takes a faint color of the theme, fading in and pulsing a moment, then holding
//! still until it's let go. Asked of the app (`WorkbenchHost::tint_tab`): tabs are its own,
//! nobody draws over another's. A tab is named exactly ([`ViewTab`]: what opened it), never
//! by a name two tabs could share.

/// The theme's color a tab is tinted in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TabTone {
    /// The theme's accent: look here.
    Accent,
    /// Something to keep in mind.
    Warning,
    /// Something went wrong.
    Danger,
    /// Done, all good.
    Success,
}

/// A tab named exactly, as a module opened it (`WorkbenchHost::open_view`): its title and
/// its view's extension — one tab in a window, never two (two files called alike are
/// different tabs).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ViewTab {
    pub title: String,
    pub extension: String,
}

impl ViewTab {
    pub fn new(title: impl Into<String>, extension: impl Into<String>) -> Self {
        Self { title: title.into(), extension: extension.into() }
    }
}
