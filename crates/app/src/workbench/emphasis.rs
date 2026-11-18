//! The region emphasis ([`workbench_model::RegionEmphasis`], Focus mode's): while a module
//! asks for it, the window's content is muted but the region being worked in — listed from
//! the layout each frame ([`workbench_model::Region`]), chosen by
//! [`workbench_model::RegionFocus`] — and the editor's own popups; the effect goes in the
//! scene ([`render_api::Scene::emphasize`]) after the content, before the overlays, which
//! are never muted. Nothing of it runs while no module asks (the regions aren't listed).

use super::*;
use workbench_model::{EmphasisRequest, Follow, Region, RegionFocus, RegionKind};

/// The kept region's soft edge, logical pixels.
const FEATHER: f32 = 6.0;

/// The window's emphasis: the regions listed last (kept: no allocation once grown), which
/// had the keyboard, and what chooses among them.
#[derive(Debug, Default)]
pub(super) struct WindowEmphasis {
    regions: Vec<Region>,
    keyboard: Option<usize>,
    follow: Follow,
    focus: RegionFocus,
}

impl WindowEmphasis {
    /// Something asks for it now (regions were listed at the last frame).
    fn active(&self) -> bool {
        !self.regions.is_empty()
    }
}

impl Workbench {
    /// What the modules ask for now (the first that asks).
    pub(super) fn emphasis_request(&self) -> Option<EmphasisRequest> {
        self.support.region_emphasis.iter().find_map(|e| e.request())
    }

    /// The pointer moved to `p`: `true` if another region is emphasized now.
    pub(super) fn emphasis_pointer(&mut self, p: Point) -> bool {
        let open = self.overlay_open();
        let e = &mut self.emphasis;
        let another =
            e.active() && !open && e.focus.moves_to_another(&e.regions, e.keyboard, e.follow, p);
        e.focus.pointer_moved(p);
        another
    }

    /// The pointer left the window: `true` if a frame is needed (the keyboard's region).
    pub fn pointer_left(&mut self) -> bool {
        self.emphasis.focus.pointer_left();
        self.emphasis.active()
    }

    /// A key was pressed (with [`Follow::Both`], the keyboard's region comes back).
    pub fn emphasis_key(&mut self) {
        self.emphasis.focus.key_pressed();
    }

    /// The content drawn so far muted as `request` asks, but the region chosen and the
    /// editor's popups; or, asked nothing, nothing (the regions let go).
    pub(super) fn emphasize(&mut self, scene: &mut Scene, request: Option<EmphasisRequest>) {
        let Some(request) = request else {
            if self.emphasis.active() {
                self.emphasis.regions.clear();
                self.emphasis.focus.reset();
            }
            return;
        };
        self.list_regions();
        // Anything open over the window (a modal, a popover): what's behind stays as it was.
        let frozen = self.overlay_open();
        let e = &mut self.emphasis;
        e.follow = request.follow;
        let chosen = e.focus.choose(&e.regions, e.keyboard, request.follow, frozen);
        let mut emphasis =
            render_api::Emphasis::new(request.gray, request.dim, (FEATHER * self.scale).round());
        if let Some(region) = chosen {
            emphasis.keep(render_api::Keep::new(region.rect, region.radius));
        }
        // The editor's popups: over its content, never muted.
        let popups = [self.completion.bounds(), self.code_hover.bounds()];
        for rect in popups.into_iter().flatten() {
            emphasis.keep(render_api::Keep::new(rect, 0.0));
        }
        scene.emphasize(emphasis);
    }

    /// The layout's regions, in drawing order, and which has the keyboard.
    fn list_regions(&mut self) {
        let card = (CARD_RADIUS * self.scale).round();
        let mut regions = std::mem::take(&mut self.emphasis.regions);
        regions.clear();
        let mut keyboard = None;
        let mut push = |kind, rect: Rect, radius, focused: bool| {
            if rect.size.width > 0.0 && rect.size.height > 0.0 {
                if focused {
                    keyboard = Some(regions.len());
                }
                regions.push(Region { kind, rect, radius });
            }
        };
        let strips = [self.tools.strip.bounds(), self.tools.right_strip.bounds()];
        for rect in strips {
            push(RegionKind::ToolStrip, rect, 0.0, false);
        }
        let editor = matches!(self.focus, Focus::Editor | Focus::Find);
        if self.editors.groups.len() <= 1 {
            push(RegionKind::EditorGroup, self.card, card, editor);
        } else {
            for (g, group) in self.editors.groups.iter().enumerate() {
                push(
                    RegionKind::EditorGroup,
                    group.bounds,
                    card,
                    editor && g == self.editors.current,
                );
            }
        }
        for side in &self.tools.sides {
            push(RegionKind::SidePanel, side.bounds, card, self.focus == Focus::Side(side.dock));
        }
        if let Some(window) = self.tools.window() {
            for (g, group) in window.groups.iter().enumerate() {
                let focused = self.focus == Focus::Panel && g == window.current;
                push(RegionKind::BottomGroup, group.bounds, card, focused);
            }
        }
        push(RegionKind::StatusBar, self.status.bounds(), 0.0, false);
        if !self.chrome.top_bar_hides() || self.chrome.top_bar_reveal.value() > 0.0 {
            push(RegionKind::TopBar, self.chrome.top_bar.bounds(), 0.0, false);
        }
        self.emphasis.keyboard = keyboard;
        self.emphasis.regions = regions;
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/emphasis.rs"]
mod tests;
