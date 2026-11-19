//! The window's frame around the card: the top bar, the project's tint, the window's own
//! buttons and full screen.

use super::*;

/// The top bar, the tint and the window controls.
#[derive(Debug)]
pub(super) struct Chrome {
    pub top_bar: TopBar,
    /// The project's tint on the window background (a gradient in the badge color).
    pub tint: ui_workbench::ProjectTint,
    /// The project's label beside its name in the top bar (its branch), from its decorators.
    pub project_label: Option<String>,
    /// What a module put in the top bar after the branch (its click, its menu), as drawn.
    pub badge: Option<workbench_model::TopBadge>,
    /// The space for the window buttons in the top bar (macOS, logical).
    pub titlebar_inset: f32,
    /// The window's buttons drawn in the top bar (Windows), and whether it's maximized.
    pub window_controls: Option<bool>,
    /// The window is full screen (the system's bars gone).
    pub full_screen: bool,
    /// The mouse over the maximize button, as the system tells (Windows' snap layouts).
    pub maximize_hovered: bool,
    /// Full screen with the window's buttons ours, the top bar hides: how far it's shown
    /// (0 hidden, 1 over the content), when the mouse reaches the screen's top edge.
    pub top_bar_reveal: ui_workbench::Tween,
    /// The theme's decorations (plan 41).
    pub decorations: super::theme_decorations::ThemeDecorations,
    /// Whether the bar has the menu drawer's button, told to this window (the tests: no
    /// other test's setting shows it); `None`: as the setting says.
    pub menu_button: Option<bool>,
}

impl Default for Chrome {
    fn default() -> Self {
        Self {
            top_bar: TopBar::default(),
            tint: ui_workbench::ProjectTint::default(),
            project_label: None,
            badge: None,
            titlebar_inset: 0.0,
            window_controls: None,
            full_screen: false,
            maximize_hovered: false,
            top_bar_reveal: ui_workbench::Tween::new(0.0, TOP_BAR_REVEAL),
            decorations: Default::default(),
            menu_button: cfg!(test).then_some(false),
        }
    }
}

impl Chrome {
    /// The top bar hides in full screen and comes over the content at the screen's top
    /// edge. Never now: in full screen it stays, as on macOS (the window's buttons ours on
    /// Windows, their middle one leaving full screen); the reveal is kept for a setting.
    pub fn top_bar_hides(&self) -> bool {
        false
    }
}

impl Workbench {
    /// What a click on the top bar's branch runs, when a module gave it one (and there's a
    /// branch shown).
    pub(super) fn branch_action(&self) -> Option<workbench_model::TopBarLabelAction> {
        self.chrome.project_label.as_ref()?;
        self.support.top_bar_label_actions.first().copied()
    }
}
