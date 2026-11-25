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
    /// This window is kept above every other one (its own: each window has its state, for
    /// the run — the windows' size and place aren't kept between runs either).
    pub on_top: bool,
    /// The system can keep this window on top (not on Wayland): the pin and its command
    /// are offered only then.
    pub can_keep_on_top: bool,
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
            on_top: false,
            can_keep_on_top: cfg!(test),
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
    /// Whether the system can keep this window on top (told once, as it opens).
    pub fn set_keeps_on_top(&mut self, can: bool) {
        self.chrome.can_keep_on_top = can;
    }

    /// What the window itself can do for app command `id` (read from memory, cheap: asked
    /// as the top bar is drawn); the rest of what it needs is the menus' (`requires`).
    pub(super) fn window_allows(&self, id: command_model::CommandId) -> bool {
        crate::commands::requires(id).iter().all(|r| match r {
            command_model::Requires::KeepOnTop => self.chrome.can_keep_on_top,
            _ => true,
        })
    }

    /// This window kept above the others, or back among them: how it stands now (the app
    /// tells the system).
    pub fn toggle_on_top(&mut self) -> bool {
        self.chrome.on_top = !self.chrome.on_top;
        self.chrome.on_top
    }

    /// How the app's own command `id` that turns something on or off stands (`None`: a
    /// plain action, or a module's).
    pub(super) fn app_switched(&self, id: command_model::CommandId) -> Option<bool> {
        (id == crate::commands::KEEP_ON_TOP).then_some(self.chrome.on_top)
    }

    /// What a click on the top bar's branch runs, when a module gave it one (and there's a
    /// branch shown).
    pub(super) fn branch_action(&self) -> Option<workbench_model::TopBarLabelAction> {
        self.chrome.project_label.as_ref()?;
        self.support.top_bar_label_actions.first().copied()
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/chrome.rs"]
mod tests;
