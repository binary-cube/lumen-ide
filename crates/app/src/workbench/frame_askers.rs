//! What on screen asks for frames, walked once by name: each part shown (the dividers, the
//! bottom window's views, the side panels open, the tabs on screen and their bars, the
//! tooltips, the windows over the editor…) told as an [`Asker`] — whether it moves now, and
//! a time of its own it wants a frame at. The one list behind "is something moving?"
//! ([`Workbench::animating`]), a profile's frame causes and the warning for a part that says
//! it moves while nothing on screen changes: a new part joins all three by being walked here,
//! none of them names it. A part not on screen isn't walked: it moves for nobody.

use std::time::Instant;

use super::*;

/// A part asking for frames: its name (our code's, static), whether it moves now, when it
/// wants its next frame on its own (a caret's blink, a fade's end).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Asker {
    pub name: &'static str,
    pub moving: bool,
    pub due: Option<Instant>,
}

impl Workbench {
    /// Each part on screen asking for frames, told to `visit` (the walk stops when it says
    /// [`std::ops::ControlFlow::Break`]).
    pub fn each_frame_asker(
        &self,
        mut visit: impl FnMut(Asker) -> std::ops::ControlFlow<()>,
    ) -> std::ops::ControlFlow<()> {
        let mut ask = |name, moving, due| visit(Asker { name, moving, due });
        // A view over the whole window (a scene, the screen kept awake) hides the rest: only
        // the windows over the editor (it among them) are asked.
        if !self.modal_view_covers() {
            self.each_window_part(&mut ask)?;
        }
        // Each window over the editor says itself whether it moves: one closing may still
        // slide or fade out (the menu drawer), so they're asked open or not.
        for o in Overlay::ALL {
            ask(o.name(), o.slot().animating(self), None)?;
        }
        std::ops::ControlFlow::Continue(())
    }

    /// The window's own parts on screen (under the windows over the editor), asked as
    /// [`Self::each_frame_asker`] does.
    fn each_window_part(
        &self,
        ask: &mut impl FnMut(&'static str, bool, Option<Instant>) -> std::ops::ControlFlow<()>,
    ) -> std::ops::ControlFlow<()> {
        ask("dividers", self.dividers_animating(), None)?;
        if let Some(w) = self.tools.window() {
            ask("bottom-split", w.split.animating(), None)?;
            for v in w.groups.iter().filter_map(|g| g.active_view()) {
                ask(v.provider, v.viewer.is_animating(), v.viewer.frame_deadline())?;
            }
        }
        for s in &self.tools.sides {
            ask(s.id, s.panel.is_animating(), s.panel.frame_deadline())?;
        }
        for t in self.shown_tabs() {
            ask("editor", t.view.is_animating(), t.view.scrollbar_deadline())?;
            if let Some(v) = &t.viewer {
                ask(v.provider, v.viewer.is_animating(), v.viewer.frame_deadline())?;
                ask("view-split", v.split.splitter.is_animating(), None)?;
            }
            if let Some(bar) = &t.toolbar {
                ask("toolbar", bar.bar.is_animating(), None)?;
            }
        }
        ask("tooltips", self.tooltips.is_animating(), None)?;
        ask("tab-tints", self.tints_moving_in_sight(), None)?;
        ask("top-bar", self.chrome.top_bar_reveal.animating(), None)?;
        ask("editor-split", self.editors.split.animating(), None)?;
        std::ops::ControlFlow::Continue(())
    }

    /// The names of the parts moving now (a warning's, a test's).
    pub fn moving_parts(&self) -> Vec<&'static str> {
        let mut names = Vec::new();
        let _ = self.each_frame_asker(|a| {
            if a.moving && !names.contains(&a.name) {
                names.push(a.name);
            }
            std::ops::ControlFlow::Continue(())
        });
        names
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/frame_askers.rs"]
mod tests;
