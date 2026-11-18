//! The scenes' pictures on Relax mode's settings page (its cards, `SettingsRow::Cards`):
//! the live card plays its scene, small, at the settings' speed, density, colors and clock
//! (a change shows at once), "In turn" going through them; the others show a still frame,
//! placed once and kept until something it depends on changes. All of it goes when the
//! page closes (`release`).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kernel::{Rect, Rgba};
use text_layout::{Font, ShapedLine};
use ui_components::{CardPaint, CardPreview};

use crate::scene::{RelaxScene, SCENES, SceneColors, SceneInfo};
use crate::settings::{Config, config};
use crate::view::{SceneClock, reduced};

/// The name the settings' cards give.
pub const PREVIEW: &str = "relax-scenes";
/// A small picture shows less (its density times this) and smaller (its scale).
const SMALL_DENSITY: f32 = 0.35;
const SMALL_SCALE: f32 = 0.6;
/// The moment a still frame shows.
const STILL_TIME: f32 = 37.0;
/// "In turn" in a card: the next scene after this long.
const TURN: f32 = 6.0;
/// Where "In turn"'s still bands are kept among the stills (past any card's value).
const TURN_STILLS: i64 = 1000;
/// The space between "In turn"'s bands (logical pixels).
const TURN_GAP: f32 = 2.0;

/// What a picture was made for: when any of it changes, it's made again.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Made {
    bounds: Rect,
    scale: f32,
    density: f32,
    colors: SceneColors,
}

struct Picture {
    value: i64,
    scene: Box<dyn RelaxScene>,
    made: Option<Made>,
}

#[derive(Default)]
struct State {
    live: Option<(Picture, SceneClock)>,
    stills: Vec<Picture>,
    /// Still frames placed so far (each once while nothing changes).
    stills_made: u64,
    /// The clock's text: its minute and size, shaped.
    clock: Option<(i64, f32, Arc<ShapedLine>)>,
}

/// Relax mode's card preview.
#[derive(Default)]
pub struct ScenePreviews {
    state: Mutex<State>,
}

impl std::fmt::Debug for ScenePreviews {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScenePreviews").finish_non_exhaustive()
    }
}

/// The scene a card shows (the last value: "In turn", at `t`).
fn scene_of(value: i64, t: f32) -> &'static SceneInfo {
    match SCENES.get(value as usize) {
        Some(info) => info,
        None => &SCENES[(t / TURN) as usize % SCENES.len()],
    }
}

/// Places `picture` for `made` (made again if its scene changed or it was made for
/// something else). `true` if it was placed anew.
fn prepare(
    picture: &mut Picture,
    info: &SceneInfo,
    made: Made,
    config: &Config,
    palette: &ui_components::Palette,
) -> bool {
    if picture.scene.id() != info.id {
        picture.scene = (info.make)();
        picture.made = None;
    }
    let colors = if config.theme_colors {
        SceneColors::from_palette(palette)
    } else {
        picture.scene.own_colors()
    };
    let made = Made { colors, ..made };
    if picture.made == Some(made) {
        return false;
    }
    picture.scene.resize(made.bounds, made.scale);
    picture.scene.set_density(made.density);
    picture.scene.set_colors(&made.colors);
    picture.made = Some(made);
    true
}

impl ScenePreviews {
    /// Still frames placed so far.
    pub fn stills_made(&self) -> u64 {
        self.state.lock().map_or(0, |s| s.stills_made)
    }

    /// What it holds now (bytes).
    pub fn memory(&self) -> usize {
        self.state.lock().map_or(0, |s| {
            s.stills.iter().map(|p| p.scene.memory()).sum::<usize>()
                + s.live.as_ref().map_or(0, |(p, _)| p.scene.memory())
        })
    }

    /// Paints at `now` (a test's own clock).
    pub fn paint_at(&self, value: i64, live: bool, mut p: CardPaint<'_>, now: Instant) {
        let Ok(mut state) = self.state.lock() else { return };
        let config = config();
        let made = Made {
            bounds: p.bounds,
            scale: p.scale * SMALL_SCALE,
            density: (config.density * SMALL_DENSITY).max(0.25),
            colors: SceneColors::from_palette(p.palette),
        };
        if live {
            let state = &mut *state;
            if state.live.as_ref().is_none_or(|(pic, _)| pic.value != value) {
                let picture = Picture { value, scene: (scene_of(value, 0.0).make)(), made: None };
                state.live = Some((picture, SceneClock::default()));
            }
            let Some((picture, clock)) = &mut state.live else { return };
            let rate = config.speed * if reduced(config.motion) { 0.35 } else { 1.0 };
            let t = clock.advance(now, rate);
            prepare(picture, scene_of(value, t), made, &config, p.palette);
            picture.scene.update(t);
            picture.scene.paint(p.scene, p.clip);
            if config.clock {
                let colors = picture.made.map(|m| m.colors);
                paint_clock(&mut state.clock, &mut p, colors);
            }
            return;
        }
        // "In turn" isn't one scene: each one whole in its own band, side by side, apart (a
        // slice of a picture made for the whole card would cut it where the next begins).
        if SCENES.get(value as usize).is_none() {
            let count = SCENES.len() as f32;
            let gap = (TURN_GAP * p.scale).round();
            let width = ((p.bounds.size.width - gap * (count - 1.0)) / count).max(0.0);
            for (i, _) in SCENES.iter().enumerate() {
                let x = (p.bounds.min_x() + (width + gap) * i as f32).round();
                let band = Rect::new(x, p.bounds.min_y(), width.round(), p.bounds.size.height);
                let Some(clip) = band.intersect(&p.clip) else { continue };
                let made = Made { bounds: band, ..made };
                paint_still(&mut state, TURN_STILLS + i as i64, i, made, &config, &mut p, clip);
            }
            return;
        }
        let clip = p.clip;
        paint_still(&mut state, value, value as usize, made, &config, &mut p, clip);
    }
}

/// Scene `index`'s still frame, kept under `key`, drawn within `clip`.
fn paint_still(
    state: &mut State,
    key: i64,
    index: usize,
    made: Made,
    config: &Config,
    p: &mut CardPaint<'_>,
    clip: Rect,
) {
    let info = &SCENES[index];
    {
        let at = match state.stills.iter().position(|s| s.value == key) {
            Some(at) => at,
            None => {
                state.stills.push(Picture { value: key, scene: (info.make)(), made: None });
                state.stills.len() - 1
            }
        };
        let picture = &mut state.stills[at];
        let fresh = prepare(picture, info, made, config, p.palette);
        if fresh {
            picture.scene.update(STILL_TIME);
        }
        picture.scene.paint(p.scene, clip);
        if fresh {
            state.stills_made += 1;
        }
    }
}

/// The clock over a live card, small.
fn paint_clock(
    cache: &mut Option<(i64, f32, Arc<ShapedLine>)>,
    p: &mut CardPaint<'_>,
    colors: Option<SceneColors>,
) {
    let size = (p.bounds.size.height * 0.2).round().max(8.0);
    let local = chrono::Local::now();
    let minute = local.timestamp().div_euclid(60);
    if cache.as_ref().is_none_or(|(m, s, _)| *m != minute || *s != size) {
        let font = Font::new(p.palette.ui_font.clone(), size).with_weight(200);
        *cache = Some((minute, size, p.text.shape_line(&local.format("%H:%M").to_string(), &font)));
    }
    let Some((_, size, shaped)) = cache.as_ref() else { return };
    let dark = colors.is_none_or(|c| c.dark);
    let color = if dark { Rgba::WHITE.with_alpha(0.6) } else { p.palette.text.with_alpha(0.55) };
    let b = p.bounds;
    let layer = p.scene.push_layer(Some(p.clip));
    let x = (b.min_x() + (b.size.width - shaped.width) / 2.0).round();
    let baseline = (b.min_y() + b.size.height * 0.15 + size * 0.36).round();
    ui_kit::draw_text(layer, shaped, x, baseline, color);
}

impl CardPreview for ScenePreviews {
    fn id(&self) -> &'static str {
        PREVIEW
    }

    fn paint(&self, value: i64, live: bool, p: CardPaint<'_>) {
        self.paint_at(value, live, p, Instant::now());
    }

    fn frame_interval(&self) -> Duration {
        Duration::from_millis(33)
    }

    fn release(&self) {
        if let Ok(mut state) = self.state.lock() {
            *state = State::default();
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/preview.rs"]
mod tests;
