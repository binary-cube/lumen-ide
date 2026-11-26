//! Hints (coach marks): a short card pointing at a part of the window that tells the user
//! of something they may not know, with a quick action — or a tour of several steps ("click
//! here, then there"). Modules contribute them at start ([`HintContribution`]) and ask for
//! one at the moment it fits (`host.show_hint(id)`); the app shows it next to its anchor
//! (a rect a view or the app reported by its id, see `ui_components::anchors`), floating:
//! it takes no key from the editor, blocks nothing, and a click elsewhere leaves it be.
//!
//! What it says is i18n keys (the module's texts); what an action does is a command (the
//! module's). Once acknowledged — an action that says so, or "Don't show again" — it's
//! remembered in the app's settings and never shown again; closed (× / Esc) it waits for
//! the next run. [`HintTour`] is the state of the one shown, pure (the app draws it).

use command_model::CommandId;

/// The anchor of the editor's active tab (the app reports it as it draws the tabs).
pub const ANCHOR_ACTIVE_TAB: &str = "editor.active-tab";
/// The anchors of the editor's tabs, each known by its name (the name a module opened its
/// view by): the app reports every tab it draws.
pub const ANCHOR_TAB: &str = "editor.tab";

/// What a hint points at instead of its step's anchor, said when it's asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HintAnchor {
    /// The editor's tab of a module's view (as it opened it), wherever it is.
    Tab(crate::ViewTab),
}

/// What a hint's button looks like: the main one (in the accent, with ↵) or beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintActionKind {
    Primary,
    Secondary,
}

/// A button of a hint's step: its label (an i18n key), the command it runs (`None`: it only
/// closes), and whether pressing it means the hint was seen for good.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HintAction {
    pub label: &'static str,
    pub command: Option<CommandId>,
    pub kind: HintActionKind,
    pub acknowledges: bool,
}

impl HintAction {
    /// The main action: runs `command`, the hint seen for good.
    pub const fn primary(label: &'static str, command: CommandId) -> Self {
        Self { label, command: Some(command), kind: HintActionKind::Primary, acknowledges: true }
    }

    /// An action beside it: runs `command`, the hint seen for good.
    pub const fn secondary(label: &'static str, command: CommandId) -> Self {
        Self { label, command: Some(command), kind: HintActionKind::Secondary, acknowledges: true }
    }
}

/// One step: its title and text (i18n keys), what it points at (an anchor's id; `None`:
/// the window's top right, under its bar), its own buttons (a tour adds Back / Next).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HintStep {
    /// Its header's icon (an icon's name; `None`: none).
    pub icon: Option<&'static str>,
    /// The small label over its title (an i18n key; `None`: "Tip").
    pub eyebrow: Option<&'static str>,
    pub title: &'static str,
    pub body: &'static str,
    /// Where what it tells of lives in the settings (an i18n key: "Settings › …"), said on a
    /// line of its own; `None`: nowhere.
    pub place: Option<&'static str>,
    pub anchor: Option<&'static str>,
    pub actions: Vec<HintAction>,
}

/// A hint: its id (what's remembered once seen: never a name that could change) and its
/// steps (one for a hint, several for a tour).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    pub id: &'static str,
    pub steps: Vec<HintStep>,
}

/// A hint a module offers, known at start; shown when it asks (`host.show_hint(id)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HintContribution {
    pub hint: Hint,
}

/// What pressing something on the card comes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintOutcome {
    /// Nothing more: it moved to another step.
    Stay,
    /// It closes; `command` runs; `seen`: never shown again.
    Close { command: Option<CommandId>, seen: bool },
}

/// What can be pressed on a step, in its order (the keys move through it): the step's own
/// buttons, then the tour's, "Don't show again" first of all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintButton {
    /// "Don't show again": closes, seen for good.
    Dismiss,
    /// The step's action `i`.
    Action(usize),
    Back,
    Next,
    /// The tour's last step: closes, seen for good.
    Done,
}

/// The hint shown now: which step, which button has the keyboard's focus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HintTour {
    pub hint: Hint,
    /// What it points at instead of its steps' anchors (`None`: theirs).
    pub anchor: Option<HintAnchor>,
    pub step: usize,
    /// The button the keys are on (the card focused), by its place in [`Self::buttons`].
    pub focus: Option<usize>,
}

impl HintTour {
    pub fn new(hint: Hint) -> Self {
        Self { hint, anchor: None, step: 0, focus: None }
    }

    /// The step shown (a hint without steps shows nothing).
    pub fn current(&self) -> Option<&HintStep> {
        self.hint.steps.get(self.step)
    }

    /// "2 / 4" while it's a tour.
    pub fn counter(&self) -> Option<(usize, usize)> {
        (self.hint.steps.len() > 1).then_some((self.step + 1, self.hint.steps.len()))
    }

    /// Its buttons now, in their order: "Don't show again", Back (past the first step), the
    /// step's actions, then Next or Done in a tour.
    pub fn buttons(&self) -> Vec<HintButton> {
        let mut buttons = vec![HintButton::Dismiss];
        let tour = self.hint.steps.len() > 1;
        if tour && self.step > 0 {
            buttons.push(HintButton::Back);
        }
        let actions = self.current().map_or(0, |s| s.actions.len());
        buttons.extend((0..actions).map(HintButton::Action));
        if tour {
            let last = self.step + 1 == self.hint.steps.len();
            buttons.push(if last { HintButton::Done } else { HintButton::Next });
        }
        buttons
    }

    /// `button` pressed.
    pub fn press(&mut self, button: HintButton) -> HintOutcome {
        match button {
            HintButton::Dismiss | HintButton::Done => {
                HintOutcome::Close { command: None, seen: true }
            }
            HintButton::Back => {
                self.step = self.step.saturating_sub(1);
                self.focus = None;
                HintOutcome::Stay
            }
            HintButton::Next => {
                self.step = (self.step + 1).min(self.hint.steps.len().saturating_sub(1));
                self.focus = None;
                HintOutcome::Stay
            }
            HintButton::Action(i) => {
                let Some(action) = self.current().and_then(|s| s.actions.get(i)) else {
                    return HintOutcome::Stay;
                };
                HintOutcome::Close { command: action.command, seen: action.acknowledges }
            }
        }
    }

    /// The keys' focus moved by `by` among the buttons (wrapping); none yet: the main one.
    pub fn move_focus(&mut self, by: isize) {
        let count = self.buttons().len();
        if count == 0 {
            return;
        }
        let at = match self.focus {
            None => self.main(),
            Some(at) => (at as isize + by).rem_euclid(count as isize) as usize,
        };
        self.focus = Some(at);
    }

    /// The button ↵ presses: the focused one, else the main one.
    pub fn enter(&self) -> HintButton {
        let buttons = self.buttons();
        let at = self.focus.unwrap_or_else(|| self.main());
        buttons.get(at).copied().unwrap_or(HintButton::Dismiss)
    }

    /// The main button's place: the step's primary action, else Next / Done, else the last.
    fn main(&self) -> usize {
        let buttons = self.buttons();
        let primary = |b: &HintButton| match b {
            HintButton::Action(i) => self
                .current()
                .and_then(|s| s.actions.get(*i))
                .is_some_and(|a| a.kind == HintActionKind::Primary),
            HintButton::Next | HintButton::Done => true,
            _ => false,
        };
        buttons.iter().position(primary).unwrap_or(buttons.len().saturating_sub(1))
    }

    /// Whether `button` is the main one (drawn in the accent, with ↵).
    pub fn is_main(&self, button: HintButton) -> bool {
        self.buttons().get(self.main()) == Some(&button)
    }
}

/// Which hints may be shown: not those seen for good (`seen`, the app's settings), nor
/// those closed during this run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HintBook {
    pub seen: Vec<String>,
    pub closed: Vec<String>,
}

impl HintBook {
    /// Whether hint `id` may be shown now.
    pub fn may_show(&self, id: &str) -> bool {
        !self.seen.iter().chain(&self.closed).any(|s| s == id)
    }

    /// Hint `id` closed: for this run, or (`seen`) for good. `true` if `seen` changed (the
    /// settings to save).
    pub fn close(&mut self, id: &str, seen: bool) -> bool {
        if !self.closed.iter().any(|c| c == id) {
            self.closed.push(id.to_owned());
        }
        if seen && !self.seen.iter().any(|s| s == id) {
            self.seen.push(id.to_owned());
            return true;
        }
        false
    }
}

#[cfg(test)]
#[path = "../tests/unit/hints.rs"]
mod tests;
