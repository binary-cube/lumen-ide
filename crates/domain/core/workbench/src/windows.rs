//! The projects open in the app's windows (one each), as modules see them: their names,
//! where they are, the number each has while it's open (1–9: the lowest free when its
//! window got it, kept until it closes) and which is this window's. A module brings one to
//! the front through its host ([`crate::SystemHost::focus_project`]), and may say what the
//! top bar shows after the git branch for this window's ([`TopBarBadge`]).

use std::path::PathBuf;

/// A project open in one of the app's windows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectWindow {
    /// Its window (unique while the app runs).
    pub window: u64,
    pub name: String,
    pub path: PathBuf,
    /// Its number while it's open (1–9; `None` past the ninth).
    pub number: Option<u8>,
    /// It's this window's.
    pub current: bool,
}

/// The most projects numbered at once.
pub const NUMBERED: u8 = 9;

/// The numbers of `windows` (each one's window, and the number its project prefers — saved
/// with it), keeping those `given` already had; a new one gets the number it prefers if
/// it's free, else the lowest free, none past [`NUMBERED`]; a window gone frees its own.
/// `asked`: numbers the user just chose (once): the window takes it, the one that had it
/// gets the asker's old one (they swap) or, without one, the lowest free.
pub fn numbered(
    windows: &[(u64, Option<u8>)],
    given: &[(u64, u8)],
    asked: &[(u64, u8)],
) -> Vec<(u64, u8)> {
    let open = |w: &u64| windows.iter().any(|(o, _)| o == w);
    let mut kept: Vec<(u64, u8)> = given.iter().copied().filter(|(w, _)| open(w)).collect();
    for &(window, number) in asked.iter().filter(|(w, n)| open(w) && (1..=NUMBERED).contains(n)) {
        let old = kept.iter().position(|(w, _)| *w == window).map(|at| kept.remove(at).1);
        if let Some(holder) = kept.iter_mut().find(|(_, n)| *n == number) {
            match old {
                Some(old) => holder.1 = old,
                None => {
                    let holder_window = holder.0;
                    kept.retain(|(w, _)| *w != holder_window);
                }
            }
        }
        kept.push((window, number));
    }
    let free = |kept: &[(u64, u8)], n: u8| !kept.iter().any(|(_, k)| *k == n);
    for &(window, prefers) in windows {
        if kept.iter().any(|(w, _)| *w == window) {
            continue;
        }
        let wanted = prefers.filter(|n| (1..=NUMBERED).contains(n) && free(&kept, *n));
        if let Some(n) = wanted.or_else(|| (1..=NUMBERED).find(|n| free(&kept, *n))) {
            kept.push((window, n));
        }
    }
    kept.sort_by_key(|(_, n)| *n);
    kept
}

/// What the top bar shows after the git branch for this window's project: its text (the
/// project's shortcut), its tooltip, what a click runs and the menu a right click opens
/// (a menu contributed with that id).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopBadge {
    pub text: String,
    pub tooltip: String,
    pub command: Option<command_model::CommandId>,
    pub menu: Option<&'static str>,
}

/// What a module puts in the top bar for this window's project (contributed). Asked as the
/// bar is drawn: cheap.
pub trait TopBarBadge: Send + Sync + std::fmt::Debug {
    fn badge(&self, project: &ProjectWindow) -> Option<TopBadge>;
}

/// What a click on the top bar's label (the project's branch) runs, from the module giving
/// it; none contributed: the label is only shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TopBarLabelAction {
    pub command: command_model::CommandId,
    /// Its tooltip (an i18n key).
    pub tooltip: &'static str,
}

/// A place of the window something opens by (a popover under its button).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    /// The top bar's label (the branch).
    TopBarLabel,
}

#[cfg(test)]
#[path = "../tests/unit/windows.rs"]
mod tests;
