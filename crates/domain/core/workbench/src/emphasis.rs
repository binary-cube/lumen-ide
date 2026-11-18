//! Region emphasis: a module asks for the region being worked in — an editor group, a
//! panel, a bottom window's group, the top bar, the status bar, a tool strip — to stand out,
//! the rest of the window muted (Focus mode). It contributes a [`RegionEmphasis`]; the app
//! asks it at each frame (cheap) and, while it answers, mutes the window's content but the
//! region chosen (and the editor's own popups), never what's over it (modals, popovers,
//! menus, tooltips). The app knows no module: only this contract.
//!
//! Which region is pure logic, here: the app lists its layout's [`Region`]s each frame and
//! [`RegionFocus`] picks one — the keyboard's, the pointer's, or both ([`Follow`]) — kept
//! as it was while something is open over the window.

use std::fmt::Debug;

use kernel::{Point, Rect};

/// What decides the region emphasized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Follow {
    /// The one with the keyboard.
    Keyboard,
    /// The one under the pointer (the keyboard's while it's over none).
    Pointer,
    /// The pointer's while it's over one and moved since the last key; the keyboard's
    /// otherwise (typing brings it back to where the keys go).
    #[default]
    Both,
}

/// How a contributor wants the window now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EmphasisRequest {
    /// How far the rest goes to gray (0..1).
    pub gray: f32,
    /// How much darker the rest is (0..1).
    pub dim: f32,
    pub follow: Follow,
    /// The theme's animated effects stand still meanwhile (drawn as they were; they go on
    /// from there after).
    pub pause_effects: bool,
}

/// A module's wish for the region being worked in to stand out.
pub trait RegionEmphasis: Send + Sync + Debug {
    /// What it wants now (`None`: nothing — the window as usual). Asked at each frame:
    /// cheap.
    fn request(&self) -> Option<EmphasisRequest>;
}

/// What a region of the window's layout is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionKind {
    /// An editor group (its tabs, its breadcrumb, its editor or view).
    EditorGroup,
    /// A side panel (left, right, or at the bottom).
    SidePanel,
    /// A group of the bottom tool window.
    BottomGroup,
    TopBar,
    StatusBar,
    /// The tool windows' buttons along a side.
    ToolStrip,
}

/// A rectangle of the window's layout, as it's drawn (`radius`: its corners).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Region {
    pub kind: RegionKind,
    pub rect: Rect,
    pub radius: f32,
}

/// Where the pointer is and whether it moved since the last key; which region was chosen
/// last. Told the pointer's moves and the keys; asked [`RegionFocus::choose`] each frame.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RegionFocus {
    /// The pointer, while it's over the window.
    pointer: Option<Point>,
    /// A key came after its last move (it then follows the keyboard, [`Follow::Both`]).
    keys_since: bool,
    /// The last region chosen (its place in the list, and itself).
    chosen: Option<(usize, Region)>,
}

impl RegionFocus {
    /// The pointer moved to `p`.
    pub fn pointer_moved(&mut self, p: Point) {
        self.pointer = Some(p);
        self.keys_since = false;
    }

    /// The pointer left the window.
    pub fn pointer_left(&mut self) {
        self.pointer = None;
    }

    /// A key was pressed.
    pub fn key_pressed(&mut self) {
        self.keys_since = true;
    }

    /// The region `follow` gives among `regions` — `keyboard` the one with the keyboard
    /// (its index). `frozen` (something open over the window): the one chosen last, where
    /// it is now (the same place in the list, if it's still that kind of region).
    pub fn choose(
        &mut self,
        regions: &[Region],
        keyboard: Option<usize>,
        follow: Follow,
        frozen: bool,
    ) -> Option<Region> {
        if frozen && let Some((i, last)) = self.chosen {
            let now = regions.get(i).filter(|r| r.kind == last.kind).copied().unwrap_or(last);
            self.chosen = Some((i, now));
            return Some(now);
        }
        let hovered = || self.pointer.and_then(|p| region_at(regions, p));
        let index = match follow {
            Follow::Keyboard => keyboard,
            Follow::Pointer => hovered().or(keyboard),
            Follow::Both if self.keys_since => keyboard,
            Follow::Both => hovered().or(keyboard),
        };
        self.chosen = index.and_then(|i| regions.get(i).map(|r| (i, *r)));
        self.chosen.map(|(_, r)| r)
    }

    /// The region chosen last.
    pub fn chosen(&self) -> Option<Region> {
        self.chosen.map(|(_, r)| r)
    }

    /// Whether moving the pointer to `p` chooses another region than the last (a frame is
    /// needed), the rest as then.
    pub fn moves_to_another(
        &self,
        regions: &[Region],
        keyboard: Option<usize>,
        follow: Follow,
        p: Point,
    ) -> bool {
        let mut next = *self;
        next.pointer_moved(p);
        next.choose(regions, keyboard, follow, false) != self.chosen()
    }

    /// Forgets the last region chosen (the emphasis ended).
    pub fn reset(&mut self) {
        self.chosen = None;
    }
}

/// The region under `p` (the last listed of those holding it: drawn over the others).
pub fn region_at(regions: &[Region], p: Point) -> Option<usize> {
    regions.iter().rposition(|r| r.rect.contains(p))
}

#[cfg(test)]
#[path = "../tests/unit/emphasis.rs"]
mod tests;
