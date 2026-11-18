//! The dimmed screen: while the computer is kept awake and nobody touches anything, a veil
//! darkens the whole window gently (in [`power::policy::DIM_FADE`] seconds) to the level
//! chosen, and — if asked — a small mark (the module's icon and "Kept awake") drifts slowly
//! over it, so nothing stays hours in one place. What's under it is still drawn (dimmed),
//! nothing of it moves. A few frames a second while the veil comes, then two (the mark) or
//! none. Any input — a move of the mouse, a click, a key — closes it.

use std::time::{Duration, Instant};

use kernel::{Point, Rect, Rgba};
use power::ScreenCare;
use power::policy::DIM_FADE;
use render_api::Quad;
use ui_components::ds::{self, Ctx};
use ui_components::{Viewer, ViewerAction, ViewerKey, ViewerPaint};

/// Frames while the veil comes, and after (the mark drifting). The mark moves at most about
/// 0.09 logical pixels a second ([`wander`]), drawn on whole physical pixels: a frame every
/// five seconds moves it as often as it moves at all (at 0.5 s, nineteen frames in twenty
/// were the same, all night).
const FADE_FRAME: Duration = Duration::from_millis(33);
const DRIFT_FRAME: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub struct ScreenView {
    care: ScreenCare,
    opened: Instant,
    next_frame: Option<Instant>,
    closing: bool,
}

/// How far (logical pixels) the corner's sign has moved by `t` seconds: a slow few pixels, so
/// a screen that keeps images doesn't keep this one.
fn wander(t: f32) -> f32 {
    8.0 * (t / 90.0).sin()
}

impl ScreenView {
    pub fn new(care: ScreenCare) -> Self {
        Self::at(care, Instant::now())
    }

    pub fn at(care: ScreenCare, opened: Instant) -> Self {
        Self { care, opened, next_frame: None, closing: false }
    }

    /// How dark it is at `now`.
    pub fn dim(&self, now: Instant) -> f32 {
        self.care.dim_at(now.saturating_duration_since(self.opened))
    }

    /// Drawn at `now`: the veil, the mark; the next frame asked for.
    pub fn paint_at(&mut self, ctx: ViewerPaint<'_>, now: Instant) {
        let b = ctx.bounds;
        let since = now.saturating_duration_since(self.opened);
        let dim = self.dim(now);
        let layer = ctx.scene.push_layer(Some(b));
        layer.quad(Quad::filled(b, Rgba::BLACK.with_alpha(dim)));
        if dim > 0.0 {
            let base = Ctx::new(ctx.palette, ctx.scale);
            let label = i18n::t("keep_awake.screen_label");
            // As bright as a sign on a dark screen may be; slowly moving when it should.
            let opacity = (0.8 * dim / self.care.level.max(0.01)).min(0.8);
            let wander =
                if self.care.drift { base.px(wander(since.as_secs_f32())).round() } else { 0.0 };
            let area =
                Rect::new(b.min_x() - wander, b.min_y() - wander, b.size.width, b.size.height);
            let badge = ds::GlassBadge {
                detail: Some(&label),
                opacity,
                ..ds::GlassBadge::new(crate::ICON, kernel::brand::NAME)
            };
            ds::glass_badge(layer, ctx.text, ctx.icons, &base, area, &badge);
        }
        let fading = since.as_secs_f32() < DIM_FADE;
        self.next_frame = if fading {
            Some(now + FADE_FRAME)
        } else if self.care.drift {
            Some(now + DRIFT_FRAME)
        } else {
            None
        };
    }

    /// Any input: it closes.
    fn wake(&mut self) -> bool {
        self.closing = true;
        true
    }
}

impl Viewer for ScreenView {
    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        self.paint_at(ctx, Instant::now());
    }

    fn frame_deadline(&self) -> Option<Instant> {
        self.next_frame
    }

    fn hover(&mut self, _p: Point) -> bool {
        self.wake()
    }

    fn click(&mut self, _p: Point) -> Option<ViewerAction> {
        Some(ViewerAction::Close)
    }

    fn edit_key(&mut self, _key: ViewerKey) -> bool {
        self.wake()
    }

    fn text_input(&mut self, _text: &str) -> bool {
        self.wake()
    }

    fn scroll(&mut self, _dx: f32, _dy: f32) -> bool {
        self.wake()
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        std::mem::take(&mut self.closing).then_some(ViewerAction::Close)
    }

    /// The app's `--snapshot`: as after its fade, its mark a while into its path.
    #[cfg(feature = "snapshot")]
    fn command(&mut self, id: &str) -> bool {
        if id != "snapshot:dimmed" {
            return false;
        }
        self.opened = Instant::now().checked_sub(Duration::from_secs(40)).unwrap_or(self.opened);
        true
    }
}

#[cfg(test)]
#[path = "../tests/unit/screen.rs"]
mod tests;
