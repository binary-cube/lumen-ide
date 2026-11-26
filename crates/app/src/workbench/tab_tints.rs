//! Tabs tinted to draw the eye (`WorkbenchHost::tint_tab`: a hint is about one, a module
//! wants a look there): each by its tab's identity (a UUID), in a tone of the theme, with
//! its animation (`anim::Attention`: it fades in, pulses a moment, then holds still); let
//! go, it fades out and is dropped. Frames are asked only while one changes in sight.

use std::path::PathBuf;
use std::time::Instant;

use kernel::Rgba;
use ui_workbench::{Attention, AttentionLook};
use workbench_model::{TabTone, ViewTab};

use super::{Tab, Workbench};

/// The tabs tinted now (by identity), each with its tone and its animation.
#[derive(Debug, Default)]
pub(crate) struct TabTints(Vec<(u128, TabTone, Attention)>);

impl TabTints {
    /// Tab `tab` tinted in `tone` from `now` (`None`: let go, fading out). `true` if that
    /// changed anything.
    pub(crate) fn set(&mut self, tab: u128, tone: Option<TabTone>, now: Instant) -> bool {
        let held = self.0.iter_mut().find(|(t, _, a)| *t == tab && !a.released());
        match (tone, held) {
            (Some(tone), Some((_, held, _))) => {
                let changed = *held != tone;
                *held = tone;
                changed
            }
            (Some(tone), None) => {
                let attention = Attention::new(AttentionLook::default(), now);
                self.0.push((tab, tone, attention));
                true
            }
            (None, Some((_, _, attention))) => {
                attention.release(now);
                true
            }
            (None, None) => false,
        }
    }

    /// How tab `tab` is tinted at `now`: its tone and how strongly (the strongest of its
    /// tints, one fading out under a new one).
    pub(crate) fn of(&self, tab: u128, now: Instant) -> Option<(TabTone, f32)> {
        self.0
            .iter()
            .filter(|(t, _, a)| *t == tab && !a.gone(now))
            .map(|(_, tone, a)| (*tone, a.value(now)))
            .max_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Some tint changes at `now` (frames wanted, if its tab is in sight).
    pub(crate) fn animating(&self, now: Instant) -> bool {
        self.0.iter().any(|(_, _, a)| a.animating(now))
    }

    /// Tab `tab`'s tint changes at `now`.
    pub(crate) fn animating_of(&self, tab: u128, now: Instant) -> bool {
        self.0.iter().any(|(t, _, a)| *t == tab && a.animating(now))
    }

    /// Those faded out after they were let go, and those of tabs no longer open
    /// (`open`), dropped. Cheap when there are none.
    pub(crate) fn purge(&mut self, now: Instant, open: impl Fn(u128) -> bool) {
        if !self.0.is_empty() {
            self.0.retain(|(tab, _, a)| !a.gone(now) && open(*tab));
        }
    }
}

/// `tone` in the theme's colors (the components' tones).
pub(crate) fn tone_color(palette: &ui_workbench::Palette, tone: TabTone) -> Rgba {
    use ui_workbench::ds::Tone;
    let tone = match tone {
        TabTone::Accent => Tone::Accent,
        TabTone::Warning => Tone::Warning,
        TabTone::Danger => Tone::Danger,
        TabTone::Success => Tone::Success,
    };
    tone.color(palette)
}

impl Workbench {
    /// Where a module's view `tab` lives (what `open_view` opens it as): one tab's place.
    pub(crate) fn view_path(&self, tab: &ViewTab) -> PathBuf {
        let dir = self.project.as_ref().map(|p| p.root_path().to_path_buf()).unwrap_or_default();
        dir.join(format!("{}.{}", tab.title, tab.extension))
    }

    /// Every tab open in this window, the current group's and the others'.
    fn open_tabs(&self) -> impl Iterator<Item = &Tab> {
        self.tabs.iter().chain(self.editors.groups.iter().flat_map(|g| g.tabs.iter()))
    }

    /// The identity of the tab of module view `tab`, if it's open here.
    pub(crate) fn view_tab_id(&self, tab: &ViewTab) -> Option<u128> {
        let path = self.view_path(tab);
        self.open_tabs().find(|t| t.doc.path() == Some(path.as_path())).map(|t| t.id)
    }

    /// Whether the tab of identity `id` is open here.
    pub(crate) fn tab_open(&self, id: u128) -> bool {
        self.open_tabs().any(|t| t.id == id)
    }

    /// A tint changes on a tab in sight (frames wanted only then: one scrolled away, or in
    /// a group not shown, moves unseen and asks for nothing). Cheap when none moves.
    pub(crate) fn tints_moving_in_sight(&self) -> bool {
        let now = Instant::now();
        if !self.tab_tints.animating(now) {
            return false;
        }
        let moving = |tabs: &[Tab], bar: &ui_workbench::TabBar| {
            tabs.iter().enumerate().any(|(i, t)| {
                self.tab_tints.animating_of(t.id, now) && bar.visible_tab_rect(i).is_some()
            })
        };
        moving(&self.tabs, &self.tab_bar)
            || self.editors.groups.iter().any(|g| moving(&g.tabs, &g.tab_bar))
    }

    /// [`workbench_model::WorkbenchHost::tint_tab`]: module view `tab` (in any group of this
    /// window) tinted in `tone`, or let go. `false`: no such tab here, or nothing changed.
    pub(crate) fn tint_tab(&mut self, tab: &ViewTab, tone: Option<TabTone>) -> bool {
        let Some(id) = self.view_tab_id(tab) else { return false };
        self.tint_tab_id(id, tone)
    }

    /// Tab `id` tinted in `tone`, or let go (even once closed: nothing left behind).
    pub(crate) fn tint_tab_id(&mut self, id: u128, tone: Option<TabTone>) -> bool {
        if tone.is_some() && !self.tab_open(id) {
            return false;
        }
        self.tab_tints.set(id, tone, Instant::now())
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/tab_tints.rs"]
mod tests;
