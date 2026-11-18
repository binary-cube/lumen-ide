//! A module's sign in the status bar (contributed): something the user must always see while
//! it lasts — the computer kept awake —, its icon and short label, what its tooltip says, and
//! what a click on it does. Asked as the bar is drawn (cheap: from what the module knows), in
//! every window.

use command_model::CommandId;

/// What a module shows in the status bar now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Indicator {
    /// An icon of the app's set (or one the module contributes).
    pub icon: &'static str,
    pub label: String,
    pub tooltip: String,
    /// What a click on it runs.
    pub command: Option<CommandId>,
}

/// A module's sign in the status bar.
pub trait StatusIndicator: Send + Sync + std::fmt::Debug {
    /// Shown now (`None`: nothing to show). Cheap: asked as the bar is drawn.
    fn indicator(&self) -> Option<Indicator>;
}

/// What a module shows outside the app's windows now, while something it holds lasts (the
/// computer kept awake): an icon by the system's clock (macOS' menu bar, Windows'
/// notification area) with a short menu, and the same icon marking the app's (the Dock, the
/// taskbar) — seen with every window hidden or behind others.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemSign {
    /// An icon of the app's set (or one the module contributes).
    pub icon: &'static str,
    /// Why it's shown (the icon's tooltip, the menu's first row).
    pub tooltip: String,
    /// What its menu offers besides bringing the app to the front (a label and the command
    /// it runs: turning it off).
    pub action: Option<(String, CommandId)>,
}

/// A module's sign outside the app's windows. The app shows the first one there is.
pub trait SystemIndicator: Send + Sync + std::fmt::Debug {
    /// Shown now (`None`: nothing to show). Cheap: asked as the windows wake.
    fn sign(&self) -> Option<SystemSign>;
}
