//! Relax mode's view: opened bare over the whole window (a modal: nothing behind it gets
//! anything, nothing behind it is drawn), it draws its scene at the pace chosen, a large
//! clock if asked, the widgets offered to calm screens along its bottom (panes of glass:
//! the app's mark, its resources… — [`ui_components::screen_widgets`], whoever offers them:
//! it names none) and the `esc` key in the top right corner — always shown (how to leave
//! is never hidden). `esc` (the key or its cap) closes it; ← / → change the scene.
//!
//! The widgets are made when the view first paints and go with it.

use std::sync::Arc;
use std::time::{Duration, Instant};

use kernel::{Point, Rect, Rgba};
use render_api::Quad;
use text_layout::{Font, ShapedLine};
use ui_components::ds::{self, Ctx};
use ui_components::screen_widgets::{
    GlassLook, ScreenPen, ScreenWidgets, WidgetContext, screen_palette,
};
use ui_components::{Viewer, ViewerAction, ViewerCursor, ViewerKey, ViewerPaint};

use crate::scene::{self, RelaxScene, SceneColors, SceneInfo};
use crate::settings::{Config, Motion, SceneChoice};

/// The longest step the scene's clock takes between two frames: back from a pause (the
/// window hidden), it goes on from where it was.
pub const MAX_STEP: Duration = Duration::from_millis(100);
/// The fade from the window's background when it opens, and the dip between two scenes.
const OPENING: f32 = 0.9;
const SWITCH: f32 = 1.4;
/// A frame asked this much before it's due (the display's next refresh takes it).
const SLACK: Duration = Duration::from_millis(3);

/// The scene's own clock: seconds, advanced by the time between frames (at most
/// [`MAX_STEP`]) times the pace.
#[derive(Debug, Clone, Copy, Default)]
pub struct SceneClock {
    time: f64,
    last: Option<Instant>,
}

impl SceneClock {
    /// The time at `now`, gone on by `rate` × the time since the last call.
    pub fn advance(&mut self, now: Instant, rate: f32) -> f32 {
        if let Some(last) = self.last {
            let step = now.saturating_duration_since(last).min(MAX_STEP);
            self.time += step.as_secs_f64() * f64::from(rate);
        }
        self.last = Some(now);
        self.time as f32
    }

    pub fn time(&self) -> f32 {
        self.time as f32
    }
}

/// Less motion with `motion` chosen: always, never, or when the system asks.
pub fn reduced(motion: Motion) -> bool {
    match motion {
        Motion::Reduced => true,
        Motion::Full => false,
        Motion::System => ui_components::decorations::reduce_motion(),
    }
}

/// A change of scene under way: when it began, what comes, and whether it has come.
#[derive(Debug, Clone, Copy)]
struct Switch {
    began: Instant,
    to: &'static SceneInfo,
    done: bool,
}

pub struct RelaxView {
    config: Config,
    /// It follows the settings as they change (a test's view keeps its own).
    follows: bool,
    scene: Box<dyn RelaxScene>,
    bounds: Rect,
    scale: f32,
    clock: SceneClock,
    opened: Instant,
    /// The key's cap (from the last paint) and its label.
    esc: Rect,
    esc_label: Option<(f32, Arc<ShapedLine>, Font)>,
    /// The window has the keyboard (unfocused: at most 30 frames a second).
    focused: bool,
    closing: bool,
    next_frame: Option<Instant>,
    /// The scene came (the rotation counts from here).
    since_scene: Instant,
    switch: Option<Switch>,
    /// The clock's text: its minute, shaped, and when it's next looked at.
    time: Option<(i64, f32, Arc<ShapedLine>)>,
    time_checked: Option<Instant>,
    /// The widgets on it (made at the first paint), and what they're given.
    widgets: Option<ScreenWidgets>,
    /// The widgets' palette (the theme's, on glass), with the theme it was made from.
    screen_palette: Option<(u64, ui_components::Palette)>,
    widget_context: WidgetContext,
}

impl std::fmt::Debug for RelaxView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RelaxView").field("scene", &self.scene.id()).finish_non_exhaustive()
    }
}

/// The scene `config` asks for first.
fn first_scene(config: &Config) -> &'static SceneInfo {
    match config.scene {
        SceneChoice::Fixed(id) => scene::find(id).unwrap_or(&scene::SCENES[0]),
        SceneChoice::Rotate => &scene::SCENES[0],
    }
}

impl RelaxView {
    pub fn new(config: Config) -> Self {
        Self { follows: true, ..Self::at(config, Instant::now()) }
    }

    /// Opened at `now` with `config` kept as it is (a test's own clock and settings).
    pub fn at(config: Config, now: Instant) -> Self {
        let info = first_scene(&config);
        let mut scene = (info.make)();
        scene.set_density(config.density);
        Self {
            config,
            follows: false,
            scene,
            bounds: Rect::default(),
            scale: 0.0,
            clock: SceneClock::default(),
            opened: now,
            esc: Rect::default(),
            esc_label: None,
            focused: true,
            closing: false,
            next_frame: None,
            since_scene: now,
            switch: None,
            time: None,
            time_checked: None,
            widgets: None,
            screen_palette: None,
            widget_context: WidgetContext::default(),
        }
    }

    /// The scene shown.
    pub fn scene_id(&self) -> &'static str {
        self.scene.id()
    }

    /// Less motion: asked in the settings, or by the system.
    pub fn reduced(&self) -> bool {
        reduced(self.config.motion)
    }

    /// The scenes' pace now.
    fn rate(&self) -> f32 {
        self.config.speed * if self.reduced() { 0.35 } else { 1.0 }
    }

    /// The time between two frames: at most 30 a second with less motion or while the
    /// window is in the background.
    pub fn frame_interval(&self) -> Duration {
        let slow = self.reduced() || !self.focused;
        let fps = if slow { self.config.fps.min(30) } else { self.config.fps };
        Duration::from_secs_f32(1.0 / fps.clamp(10, 120) as f32)
    }

    /// Changes to `to` (faded through the sky's color, at once with less motion).
    pub fn switch_to(&mut self, to: &'static SceneInfo, now: Instant) {
        if to.id == self.scene.id() && self.switch.is_none() {
            return;
        }
        if self.reduced() {
            self.install(to, now);
            return;
        }
        self.switch = Some(Switch { began: now, to, done: false });
    }

    fn install(&mut self, to: &'static SceneInfo, now: Instant) {
        let mut scene = (to.make)();
        scene.set_density(self.config.density);
        if self.scale > 0.0 {
            scene.resize(self.bounds, self.scale);
        }
        self.scene = scene;
        self.since_scene = now;
    }

    /// The veil over the scene at `now` (0: none): the opening's fade, a change of scene.
    fn veil(&mut self, now: Instant) -> f32 {
        let opening = if self.reduced() {
            0.0
        } else {
            1.0 - (now.saturating_duration_since(self.opened).as_secs_f32() / OPENING).min(1.0)
        };
        let mut switching = 0.0;
        if let Some(mut s) = self.switch {
            let p = now.saturating_duration_since(s.began).as_secs_f32() / SWITCH;
            if p >= 0.5 && !s.done {
                self.install(s.to, now);
                s.done = true;
            }
            switching = 1.0 - (p * 2.0 - 1.0).abs();
            self.switch = (p < 1.0).then_some(s);
        }
        opening.max(switching.clamp(0.0, 1.0))
    }

    /// Rotating: the next scene once the minutes are up.
    fn rotate(&mut self, now: Instant) {
        if self.config.scene != SceneChoice::Rotate || self.switch.is_some() {
            return;
        }
        let every = Duration::from_secs(u64::from(self.config.rotate_minutes.max(1)) * 60);
        if now.saturating_duration_since(self.since_scene) >= every {
            self.switch_to(scene::next_after(self.scene.id()), now);
        }
    }

    /// Paints at `now` (a test's own clock).
    pub fn paint_at(&mut self, ctx: ViewerPaint<'_>, now: Instant) {
        if self.follows {
            self.config = crate::settings::config();
        }
        self.focused = ctx.focused;
        let bounds = ctx.bounds;
        if bounds != self.bounds || ctx.scale != self.scale {
            self.bounds = bounds;
            self.scale = ctx.scale;
            self.scene.resize(bounds, ctx.scale);
            self.esc_label = None;
            self.time = None;
        }
        self.rotate(now);
        let veil = self.veil(now);
        self.scene.set_density(self.config.density);
        let colors = if self.config.theme_colors {
            SceneColors::from_palette(ctx.palette)
        } else {
            self.scene.own_colors()
        };
        self.scene.set_colors(&colors);
        let t = self.clock.advance(now, self.rate());
        self.scene.update(t);
        self.scene.paint(ctx.scene, bounds);
        let layer = ctx.scene.push_layer(Some(bounds));
        if veil > 0.0 {
            // From the window's background when it opens; through the sky between scenes.
            let under =
                if self.switch.is_some() { colors.sky_top } else { ctx.palette.window_background };
            layer.quad(Quad::filled(bounds, under.with_alpha(veil)));
        }
        if self.config.clock {
            self.paint_clock(ctx.text, ctx.palette, &colors, layer, now);
        }
        let base = Ctx::new(ctx.palette, ctx.scale);
        // The `esc` key, always there: how to leave is never hidden.
        let size = base.px(11.0);
        if self.esc_label.as_ref().is_none_or(|(s, _, _)| *s != size) {
            let font = ds::overlay_key_font(&base);
            self.esc_label = Some((size, ctx.text.shape_line("esc", &font), font));
        }
        if let Some((_, label, font)) = &self.esc_label {
            let corner = Point::new(bounds.max_x() - base.px(16.0), bounds.min_y() + base.px(16.0));
            self.esc = ds::overlay_key(layer, &base, label, font, corner);
        }
        // The widgets along the bottom, on glass tinted for the sky (light over a dark one,
        // deep over a pale one), fading in with the scene.
        let context = &mut self.widget_context;
        if context.wake.is_none() {
            context.wake.clone_from(&ctx.waker);
        }
        let widgets = self.widgets.get_or_insert_with(|| ScreenWidgets::open(context));
        if !widgets.is_empty() {
            // On the app's dark glass, whatever the sky: its words white, as on its card.
            let tint = Rgba::WHITE;
            // The widgets' palette, made again only for another theme (it's a whole theme:
            // never cloned each frame of a scene that moves all the time).
            let stamp = ui_components::screen_widgets::palette_stamp(ctx.palette, tint);
            if self.screen_palette.as_ref().is_none_or(|(s, _)| *s != stamp) {
                self.screen_palette = Some((stamp, screen_palette(ctx.palette, tint)));
            }
            let Some((_, palette)) = &self.screen_palette else { return };
            let ds = Ctx { opacity: 1.0 - veil, ..Ctx::new(palette, ctx.scale) };
            let look = GlassLook { tint, opacity: 1.0 - veil, night: true };
            let pen =
                ScreenPen { scene: ctx.scene, text: ctx.text, icons: ctx.icons, ds: &ds, look };
            widgets.paint(pen, bounds, 28.0);
        }
        self.next_frame = Some((now + self.frame_interval()).checked_sub(SLACK).unwrap_or(now));
    }

    /// The large clock, its text shaped again only when the minute changes.
    fn paint_clock(
        &mut self,
        text: &mut text_layout::TextSystem,
        palette: &ui_components::Palette,
        colors: &SceneColors,
        layer: &mut render_api::Layer,
        now: Instant,
    ) {
        let size =
            (self.bounds.size.height * 0.11).clamp(48.0 * self.scale, 160.0 * self.scale).round();
        let due = self.time_checked.is_none_or(|at| now >= at);
        if due || self.time.as_ref().is_none_or(|(_, s, _)| *s != size) {
            self.time_checked = Some(now + Duration::from_secs(1));
            let local = chrono::Local::now();
            let minute = local.timestamp().div_euclid(60);
            if self.time.as_ref().is_none_or(|(m, s, _)| *m != minute || *s != size) {
                let font = Font::new(palette.ui_font.clone(), size).with_weight(200);
                let shaped = text.shape_line(&local.format("%H:%M").to_string(), &font);
                self.time = Some((minute, size, shaped));
            }
        }
        let Some((_, size, shaped)) = &self.time else { return };
        let b = self.bounds;
        let color =
            if colors.dark { Rgba::WHITE.with_alpha(0.6) } else { palette.text.with_alpha(0.55) };
        let x = (b.min_x() + (b.size.width - shaped.width) / 2.0).round();
        let baseline = (b.min_y() + b.size.height * 0.15 + size * 0.36).round();
        ui_kit::draw_text(layer, shaped, x, baseline, color);
    }

    /// A click at `p`: the key's cap closes it (elsewhere, nothing: a stray click loses
    /// nothing).
    pub fn click_at(&self, p: Point) -> Option<ViewerAction> {
        self.esc.contains(p).then_some(ViewerAction::Close)
    }

    /// When the next frame is due.
    pub fn next_frame(&self) -> Option<Instant> {
        self.next_frame
    }

    /// The scene's clock now.
    pub fn time(&self) -> f32 {
        self.clock.time()
    }
}

impl Viewer for RelaxView {
    fn memory(&self) -> ui_components::ViewMemory {
        ui_components::ViewMemory { ram: self.scene.memory() as u64, gpu: 0 }
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        self.paint_at(ctx, Instant::now());
    }

    fn covers_window(&self) -> bool {
        true
    }

    fn frame_deadline(&self) -> Option<Instant> {
        self.next_frame
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        self.click_at(p)
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        self.esc.contains(p).then_some(ViewerCursor::Pointer)
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        let now = Instant::now();
        match key {
            ViewerKey::Escape => self.closing = true,
            ViewerKey::Right => self.switch_to(scene::next_after(self.scene.id()), now),
            ViewerKey::Left => self.switch_to(scene::previous_before(self.scene.id()), now),
            _ => return false,
        }
        true
    }

    /// The app's `--snapshot`: as after its fade, a moment given to its widgets (a logo
    /// decoded, a first look at the resources).
    #[cfg(feature = "snapshot")]
    fn command(&mut self, id: &str) -> bool {
        if id != "snapshot:settled" {
            return false;
        }
        self.opened = Instant::now().checked_sub(Duration::from_secs(30)).unwrap_or(self.opened);
        std::thread::sleep(Duration::from_millis(1500));
        true
    }

    fn set_data(&mut self, data: &dyn std::any::Any) {
        // What its widgets may need from the app (the resources).
        if let Some(workbench_model::ResourceData(source)) = data.downcast_ref() {
            self.widget_context.resources = Some(Arc::clone(source));
        }
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        std::mem::take(&mut self.closing).then_some(ViewerAction::Close)
    }

    fn purge(&mut self) {
        // Its images are made again at the next frame.
        let info = scene::find(self.scene.id()).unwrap_or(&scene::SCENES[0]);
        self.install(info, Instant::now());
    }
}

#[cfg(test)]
#[path = "../tests/unit/view.rs"]
mod tests;
