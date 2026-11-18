//! What's shown on a calm screen (Relax mode's scene): small panes of glass — the app's mark,
//! the resources it uses — each offered by whoever owns it ([`ScreenWidget`], contributed:
//! the app puts them here), the screen knowing none of them. A widget is made when the
//! screen opens ([`ScreenWidget::open`]: what it needs from the app given) and goes when it
//! closes: nothing of it runs otherwise.
//!
//! They're drawn with the theme's palette, its words and edges in the glass's tint
//! ([`screen_palette`]): light over a dark sky, deep over a pale one, the theme's colors for
//! the rest (a chart's series). [`ScreenWidgets`] lays them out by their place.

use std::fmt::Debug;
use std::sync::{Arc, PoisonError, RwLock};

use icons::Icons;
use kernel::{Rect, Rgba};
use render_api::{Layer, Quad, Scene};
use text_layout::TextSystem;

use crate::Palette;
use crate::ds::{Ctx, radius};

/// Where a widget sits: along the bottom edge, from a corner (several in one place: side by
/// side, the first in the corner).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetPlace {
    BottomLeft,
    BottomRight,
}

/// What a widget is given when its screen opens.
#[derive(Clone, Default)]
pub struct WidgetContext {
    /// What the app's parts cost (`None`: a host without them).
    pub resources: Option<Arc<dyn workbench_model::ResourceSource>>,
    /// Wakes the window for a new frame (what a widget waits for arrived).
    pub wake: Option<Arc<dyn Fn() + Send + Sync>>,
}

/// Something offered to calm screens (a module's contribution, or the app's own).
pub trait ScreenWidget: Send + Sync + Debug {
    fn id(&self) -> &'static str;
    fn place(&self) -> WidgetPlace;
    /// Its order in its place (lower: nearer the corner).
    fn order(&self) -> i32 {
        0
    }
    /// Made for a screen that opens; `None`: nothing to show (what it needs is missing).
    fn open(&self, ctx: &WidgetContext) -> Option<Box<dyn WidgetView>>;
}

/// A widget on an open screen.
pub trait WidgetView: Send {
    /// Its size (physical pixels) with at most `room` across; `None`: nothing to show yet
    /// (what it waits for hasn't come).
    fn size(&mut self, text: &mut TextSystem, ds: &Ctx<'_>, room: f32) -> Option<(f32, f32)>;
    /// Draws itself in `p.rect` (its size, placed).
    fn paint(&mut self, p: WidgetPaint<'_, '_>);
}

/// What a widget draws with.
pub struct WidgetPaint<'a, 'c> {
    pub layer: &'a mut Layer,
    pub text: &'a mut TextSystem,
    pub icons: &'a Icons,
    /// With the screen's palette ([`screen_palette`]).
    pub ds: &'a Ctx<'c>,
    pub rect: Rect,
    pub look: GlassLook,
}

/// The pane's tint (light over a dark screen), how much of it shows (0..1: it fades in with
/// the screen), and whether it's the app's own night card (as its "About" card: deep,
/// with a soft shadow) or a faint frost (a pane up for hours over a dimmed screen).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlassLook {
    pub tint: Rgba,
    pub opacity: f32,
    pub night: bool,
}

/// The height every widget has (logical pixels): side by side, their tops line up.
pub const WIDGET_HEIGHT: f32 = 96.0;

static WIDGETS: RwLock<Vec<Arc<dyn ScreenWidget>>> = RwLock::new(Vec::new());

/// Registers a widget (the app, at startup; one with the same id is replaced).
pub fn register(widget: Arc<dyn ScreenWidget>) {
    let mut widgets = WIDGETS.write().unwrap_or_else(PoisonError::into_inner);
    widgets.retain(|w| w.id() != widget.id());
    widgets.push(widget);
    widgets.sort_by_key(|w| w.order());
}

/// Removes widget `id` (tests).
pub fn unregister(id: &str) {
    WIDGETS.write().unwrap_or_else(PoisonError::into_inner).retain(|w| w.id() != id);
}

/// The widgets offered, by their order.
pub fn widgets() -> Vec<Arc<dyn ScreenWidget>> {
    WIDGETS.read().unwrap_or_else(PoisonError::into_inner).clone()
}

/// The theme's palette as widgets draw on a screen: its words, muted words and edges in
/// `tint` (light over a dark sky), the rest (the accent, a chart's series) the theme's.
pub fn screen_palette(palette: &Palette, tint: Rgba) -> Palette {
    Palette {
        text: tint.with_alpha(0.94),
        text_muted: tint.with_alpha(0.62),
        card_border: tint.with_alpha(0.16),
        border: tint.with_alpha(0.16),
        ..palette.clone()
    }
}

/// A pane in `rect`. The app's night card (the look says so), as its "About" card: the deep
/// night (tinted by the theme's accent) hiding what's behind, deeper toward its foot, a
/// sheen from its top, a fine rim — all painted: nothing made again per frame but its
/// quads. Otherwise a faint frost of the tint and its rim.
pub fn glass_pane(layer: &mut Layer, ds: &Ctx<'_>, rect: Rect, look: GlassLook) {
    let o = look.opacity;
    let round = ds.px(radius::XL);
    let rim = |alpha: f32| {
        Quad::filled(rect, Rgba::TRANSPARENT)
            .rounded(round)
            .bordered(ds.px(1.0), look.tint.with_alpha(alpha * o))
    };
    if !look.night {
        layer.quad(Quad::filled(rect, look.tint.with_alpha(0.07 * o)).rounded(round));
        layer.quad(rim(0.16));
        return;
    }
    let night = crate::ds::NIGHT.mix(ds.palette.accent, 0.08);
    layer.quad(Quad::filled(rect, night.with_alpha(0.9 * o)).rounded(round));
    // Deeper toward its foot, a sheen from its top: each a gradient of many faint bands
    // (each rounded as the pane), too fine apart to show as lines.
    const STEPS: usize = 12;
    for i in 1..=STEPS {
        let f = i as f32 / STEPS as f32;
        let h = (rect.size.height * 0.75 * f).round();
        let foot = Rect::new(rect.min_x(), rect.max_y() - h, rect.size.width, h);
        layer.quad(Quad::filled(foot, crate::ds::NIGHT.with_alpha(0.022 * o)).rounded(round));
        let h = (rect.size.height * 0.5 * f).round();
        let top = Rect::new(rect.min_x(), rect.min_y(), rect.size.width, h);
        layer.quad(Quad::filled(top, Rgba::WHITE.with_alpha(0.005 * o)).rounded(round));
    }
    layer.quad(rim(0.14));
}

/// The widgets of an open screen, laid out by place.
#[derive(Default)]
pub struct ScreenWidgets {
    open: Vec<(WidgetPlace, Box<dyn WidgetView>)>,
}

impl Debug for ScreenWidgets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScreenWidgets").field("open", &self.open.len()).finish()
    }
}

impl Debug for WidgetContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WidgetContext")
            .field("resources", &self.resources.is_some())
            .finish_non_exhaustive()
    }
}

impl Debug for WidgetPaint<'_, '_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WidgetPaint").field("rect", &self.rect).finish_non_exhaustive()
    }
}

impl Debug for ScreenPen<'_, '_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScreenPen").field("look", &self.look).finish_non_exhaustive()
    }
}

impl ScreenWidgets {
    /// Every widget offered, made for this screen (those with nothing to show left out).
    pub fn open(ctx: &WidgetContext) -> Self {
        Self::open_from(&widgets(), ctx)
    }

    /// The same, from `offered` (tests).
    pub fn open_from(offered: &[Arc<dyn ScreenWidget>], ctx: &WidgetContext) -> Self {
        Self { open: offered.iter().filter_map(|w| Some((w.place(), w.open(ctx)?))).collect() }
    }

    pub fn len(&self) -> usize {
        self.open.len()
    }

    pub fn is_empty(&self) -> bool {
        self.open.is_empty()
    }

    /// Draws them along `bounds`' bottom edge, `margin` (logical) from it: each place's
    /// side by side from its corner, each given what's left of the width.
    pub fn paint(&mut self, pen: ScreenPen<'_, '_>, bounds: Rect, margin: f32) -> Vec<Rect> {
        let ScreenPen { scene, text, icons, ds, look } = pen;
        let layer = scene.push_layer(Some(bounds));
        let (m, gap) = (ds.px(margin), ds.px(12.0));
        let bottom = bounds.max_y() - m;
        let (mut left, mut right) = (bounds.min_x() + m, bounds.max_x() - m);
        let mut drawn = Vec::new();
        for place in [WidgetPlace::BottomLeft, WidgetPlace::BottomRight] {
            for (_, view) in self.open.iter_mut().filter(|(p, _)| *p == place) {
                let room = right - left;
                let Some((w, h)) = view.size(text, ds, room).filter(|(w, _)| *w <= room) else {
                    continue;
                };
                let x = if place == WidgetPlace::BottomLeft { left } else { right - w };
                let rect = Rect::new(x.round(), (bottom - h).round(), w.round(), h.round());
                view.paint(WidgetPaint { layer, text, icons, ds, rect, look });
                drawn.push(rect);
                if place == WidgetPlace::BottomLeft {
                    left = rect.max_x() + gap;
                } else {
                    right = rect.min_x() - gap;
                }
            }
        }
        drawn
    }
}

/// What [`ScreenWidgets::paint`] draws with.
pub struct ScreenPen<'a, 'c> {
    pub scene: &'a mut Scene,
    pub text: &'a mut TextSystem,
    pub icons: &'a Icons,
    /// With the screen's palette ([`screen_palette`]).
    pub ds: &'a Ctx<'c>,
    pub look: GlassLook,
}

#[cfg(test)]
#[path = "../tests/unit/screen_widgets.rs"]
mod tests;
