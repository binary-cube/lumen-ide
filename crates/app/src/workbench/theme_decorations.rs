//! The theme's decorations (plan 41): what a theme draws besides its colors, made by the
//! module contributing a `DecorationsProvider` only while the theme has some (and the user
//! hasn't turned them off), let go when it doesn't. Drawn in their own layer right above
//! the window's background — behind the strips and the cards, which cover them — at the
//! time their clock says; the window asks for their frames (a low rate, only while it's
//! shown, focused and in use) and may draw a frame of only them (`repaint_decorations`).

use std::time::Instant;

use super::*;
use theming::decorations::Decorations;
use ui_workbench::decorations::{
    DecorationClock, DecorationFrame, DecorationMode, Geometry, Moving, WindowDecorations,
};

/// The window's decorations and what they need.
pub(super) struct ThemeDecorations {
    painter: Option<Box<dyn WindowDecorations>>,
    /// What it was made for: the theme's decorations and the provider's version.
    made_for: Option<(Arc<Decorations>, u64)>,
    clock: DecorationClock,
    /// The window has the keyboard, is shown; the system asks for still (reduced motion,
    /// saving power) or the user chose static.
    focused: bool,
    visible: bool,
    still: bool,
    /// A view drawing the whole window is open (Relax mode): nothing of them is drawn, no
    /// frame asked, their clock stopped where it was.
    suspended: bool,
    /// Asked to stand still (Focus mode: `EmphasisRequest::pause_effects`): drawn as they
    /// were, no frame asked, their clock stopped — as in a window without the keyboard.
    pub paused: bool,
    /// The user's frame rate and idle time (the provider's settings).
    fps: u32,
    idle: Option<std::time::Duration>,
    /// Where they went in the last whole frame's scene, and what that frame's geometry was.
    layer: Option<usize>,
    /// What covers them (the cards) and what the top bar shows, gathered each frame
    /// (kept: no allocation once grown).
    covered: Vec<Rect>,
    editor: Vec<Rect>,
    clear: Vec<Rect>,
    /// How opaque the cards are while effects show through them (worked out when the
    /// decorations are made: the effects' glass, raised to keep the text's contrast).
    pub glass: Option<Glass>,
    /// The palette the side panels draw their content with meanwhile: their background
    /// left to the card (translucent), the rest the theme's (made with the glass).
    pub glass_palette: Option<ui_workbench::Palette>,
}

/// The cards' backgrounds while effects show through them: the side panels' and the bottom
/// window's opacity, and the editor's (when it lets them show too).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Glass {
    pub panels: f32,
    pub editor: Option<f32>,
}

impl std::fmt::Debug for ThemeDecorations {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThemeDecorations")
            .field("painter", &self.painter)
            .field("focused", &self.focused)
            .field("visible", &self.visible)
            .field("still", &self.still)
            .finish_non_exhaustive()
    }
}

impl Default for ThemeDecorations {
    fn default() -> Self {
        Self {
            painter: None,
            made_for: None,
            clock: DecorationClock::new(Instant::now()),
            // A window opens in front; a test's doesn't, until told (tests run side by side in
            // one process: none claims to be the window worked in, as one shared server asks).
            focused: !cfg!(test),
            visible: true,
            still: false,
            suspended: false,
            paused: false,
            fps: ui_workbench::decorations::FPS,
            idle: Some(ui_workbench::decorations::IDLE_STOP),
            layer: None,
            covered: Vec::new(),
            editor: Vec::new(),
            clear: Vec::new(),
            glass: None,
            glass_palette: None,
        }
    }
}

impl ThemeDecorations {
    fn moving(&self, moves: bool) -> Moving {
        Moving {
            allowed: !self.still,
            moves,
            focused: self.focused && !self.paused,
            visible: self.visible && !self.suspended,
            fps: self.fps,
            idle: self.idle,
        }
    }

    /// Lets go of what's made (the caches' purge; made again at the next frame).
    pub fn purge(&mut self) {
        self.painter = None;
        self.made_for = None;
        self.layer = None;
        self.glass = None;
        self.glass_palette = None;
    }
}

impl Workbench {
    /// Makes the window's decorations for its theme (once), or lets them go.
    fn sync_theme_decorations(&mut self) {
        let wanted = self.theme.as_ref().map(|t| Arc::clone(t.decorations()));
        let provider = self.support.decorations.first();
        let version = provider.map_or(0, |p| p.version());
        let d = &mut self.chrome.decorations;
        let same = match (&d.made_for, &wanted) {
            (Some((made, v)), Some(want)) => Arc::ptr_eq(made, want) && *v == version,
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        // The provider decides (the user's own may go over a theme without any).
        d.painter = match (&wanted, provider) {
            (Some(want), Some(provider)) => provider.create(want),
            _ => None,
        };
        d.made_for = wanted.map(|w| (w, version));
        d.layer = None;
        // The cards over effects shown through them: their opacity, keeping the text read.
        let see = d.painter.as_ref().and_then(|p| p.see_through());
        let p = &self.palette;
        d.glass = see.map(|see| {
            let behind: Vec<(kernel::Rgba, f32)> =
                see.colors.iter().map(|(c, o)| (*c, o.min(1.0))).collect();
            let texts = [
                (p.text, ui_workbench::decorations::TEXT_CONTRAST),
                (p.text_muted, ui_workbench::decorations::MUTED_CONTRAST),
            ];
            let under = p.window_background;
            let alpha = |card| {
                ui_workbench::decorations::glass_alpha(
                    card,
                    &texts,
                    under,
                    &behind,
                    see.card_opacity,
                )
            };
            Glass {
                panels: alpha(p.sidebar_background).max(alpha(p.editor_background)),
                editor: see.editor.then(|| alpha(p.editor_background)),
            }
        });
        d.glass_palette = d.glass.map(|_| ui_workbench::Palette {
            sidebar_background: kernel::Rgba::TRANSPARENT,
            ..p.clone()
        });
        if d.painter.is_some() {
            tracing::debug!("the theme's decorations are drawn");
        }
    }

    /// The window as the system shows it: focused, shown; whether the system asks for less
    /// motion or saves power.
    pub fn set_decorations_window(&mut self, focused: bool, visible: bool, system_still: bool) {
        let provider = self.support.decorations.first();
        let mode = provider.map(|p| p.mode()).unwrap_or_default();
        let respects = provider.is_none_or(|p| p.respects_system());
        let (fps, idle) = provider.map_or(
            (ui_workbench::decorations::FPS, Some(ui_workbench::decorations::IDLE_STOP)),
            |p| (p.fps(), p.idle()),
        );
        let d = &mut self.chrome.decorations;
        d.focused = focused;
        d.visible = visible;
        d.fps = fps;
        d.idle = idle;
        d.still = (system_still && respects) || mode == DecorationMode::Static;
    }

    /// This window came in front of the app's others: told on its project's own bus.
    pub fn project_brought_forward(&self) {
        self.data.events.publish(&workspace::events::ProjectBroughtForward);
    }

    /// This window lost the keyboard: told on its project's own bus.
    pub fn project_left(&self) {
        self.data.events.publish(&workspace::events::ProjectLeft);
    }

    /// The window has the keyboard (as last told).
    pub(crate) fn window_focused(&self) -> bool {
        self.chrome.decorations.focused
    }

    /// The window shown again or focused again (`focused`): the decorations move again, as
    /// after input — stopped while idle, nothing else would wake them; the focus coming back
    /// is the user's activity too (what waits for idleness waits again).
    pub fn window_returned(&mut self, now: Instant, focused: bool) {
        self.decorations_input(now);
        if focused {
            self.idle_input(now);
        }
    }

    /// Input came: the decorations move again (if they stopped, idle).
    pub fn decorations_input(&mut self, now: Instant) {
        self.chrome.decorations.clock.input(now);
    }

    /// When their next frame is due (`None`: still, or none drawn).
    pub fn decorations_deadline(&self, now: Instant) -> Option<Instant> {
        let d = &self.chrome.decorations;
        let painter = d.painter.as_ref()?;
        d.clock.next_frame(now, d.moving(painter.moves()))
    }

    /// Whether they're in the last whole frame's scene (a frame of only them can be drawn).
    pub fn decorations_drawn(&self) -> bool {
        let d = &self.chrome.decorations;
        d.painter.is_some() && d.layer.is_some()
    }

    /// What covers them (the cards) and what the top bar shows, gathered again.
    fn gather_decoration_bounds(&mut self) {
        let bottom = self.tool_window_shown().then_some(self.tools.bottom_bounds);
        let d = &mut self.chrome.decorations;
        d.covered.clear();
        d.covered.push(self.card);
        d.covered.extend(self.tools.sides.iter().map(|s| s.bounds));
        d.covered.extend(bottom);
        d.editor.clear();
        d.editor.push(self.card);
        d.clear.clear();
        d.clear.extend_from_slice(self.chrome.top_bar.occupied());
    }

    fn decorations_geometry(&self) -> Geometry<'_> {
        let inset =
            if self.chrome.full_screen { 0.0 } else { self.chrome.titlebar_inset * self.scale };
        Geometry {
            window: Rect::new(0.0, 0.0, self.size.width as f32, self.size.height as f32),
            scale: self.scale,
            top_inset: inset.round(),
            top_bar: ui_workbench::TopBar::height(self.scale),
            keep_clear: &self.chrome.decorations.clear,
        }
    }

    /// A view drawing the whole window opened: they're suspended — not drawn (no frame of
    /// only them either), asking no frames, their clock stopped (they go on from there).
    pub(super) fn suspend_theme_decorations(&mut self) {
        let d = &mut self.chrome.decorations;
        d.suspended = true;
        d.layer = None;
        let moves = d.painter.as_ref().is_some_and(|p| p.moves());
        let moving = d.moving(moves);
        // Not running now: the clock forgets its last frame (no jump when they resume).
        d.clock.advance(Instant::now(), moving);
    }

    /// Draws them into a new layer (a whole frame, right after the window's background).
    pub(super) fn paint_theme_decorations(&mut self, scene: &mut Scene) {
        self.chrome.decorations.suspended = false;
        self.sync_theme_decorations();
        if self.chrome.decorations.painter.is_none() {
            self.chrome.decorations.layer = None;
            return;
        }
        self.gather_decoration_bounds();
        let index = scene.layers().len();
        let layer = scene.push_layer(None);
        self.paint_decorations_into(layer);
        self.chrome.decorations.layer = Some(index);
    }

    fn paint_decorations_into(&mut self, layer: &mut render_api::Layer) {
        // Taken out while it draws (the frame borrows the rest of the window).
        let Some(mut painter) = self.chrome.decorations.painter.take() else { return };
        let moving = self.chrome.decorations.moving(painter.moves());
        let time = self.chrome.decorations.clock.advance(Instant::now(), moving);
        let frame = DecorationFrame {
            geometry: self.decorations_geometry(),
            covered: &self.chrome.decorations.covered,
            editor: &self.chrome.decorations.editor,
            time,
            icons: Some(&self.icons),
        };
        painter.paint(&frame, layer);
        self.chrome.decorations.painter = Some(painter);
    }

    /// A frame of only the decorations: their layer in the kept scene drawn again at the
    /// new time (nothing else changed since the last whole frame). `false` when it can't be
    /// done (then the whole frame is).
    pub fn repaint_decorations(&mut self, scene: &mut Scene) -> bool {
        let Some(index) = self.chrome.decorations.layer else { return false };
        if self.chrome.decorations.painter.is_none() {
            return false;
        }
        let Some(layer) = scene.layer_mut(index) else { return false };
        layer.quads.clear();
        layer.images.clear();
        layer.glyphs.clear();
        self.paint_decorations_into(layer);
        true
    }

    /// The time they're drawn at (the tests: frozen while paused).
    #[cfg(test)]
    pub(super) fn decorations_time(&self) -> f32 {
        self.chrome.decorations.clock.time()
    }

    /// What they hold (the Task Manager's "Interface"): in memory, on the GPU.
    pub fn decorations_memory(&self) -> (usize, u64) {
        self.chrome.decorations.painter.as_ref().map_or((0, 0), |p| (p.memory(), p.gpu_memory()))
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/theme_decorations.rs"]
mod tests;
