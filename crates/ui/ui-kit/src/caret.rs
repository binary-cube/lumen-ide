//! Every caret (the editor's, the text fields', the terminal's, the canvas's fields), drawn
//! and animated in one place, as the theme (or the user) describes it
//! ([`theming::caret::CaretLook`], set with [`configure`]): its shape and thickness, its
//! blinking (on / off, or fading), a halo, gliding to a new place, a trail that fades.
//!
//! The caret stays solid while the user types or clicks ([`reset`]) and then blinks, until
//! nothing happened for [`REST`]: then it rests, shown solid, until input ([`wake`]).
//! Painters draw through [`paint`] (or ask [`visible`] / [`opacity`]), which records that a
//! caret is on screen; after the frame the app reads [`take_next_frame`] and asks the
//! platform for one redraw then: right away while something moves (a fade, a glide, a
//! trail), else at the next blink — no thread, no frames in between.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::{Duration, Instant};

use kernel::{Rect, Rgba};
use render_api::{Layer, Quad};
use theming::caret::{Blink, CaretLook, CaretShape, Motion};

/// Half a blink: how long the caret stays on (or off).
const HALF: Duration = Duration::from_millis(530);
/// A smooth blink's fade (at the end of each half).
const FADE: Duration = Duration::from_millis(160);
/// How long gliding to a new place takes.
const GLIDE: Duration = Duration::from_millis(90);
/// A frame, while something moves.
const FRAME: Duration = Duration::from_millis(16);
/// A caret not drawn for this long is forgotten (its glide, its trail).
const FORGET: Duration = Duration::from_secs(2);
/// After this long without input the caret stops blinking, shown solid: no frames for it
/// while the user is away (and the window's spare image goes back, ~15 MB on a Retina one).
/// The next key, click or the pointer moving makes it blink again.
pub const REST: Duration = Duration::from_secs(15);

thread_local! {
    /// The last input: blinking restarts from it (caret visible first).
    static EPOCH: Cell<Option<Instant>> = const { Cell::new(None) };
    /// A caret was painted since the last `take_next_frame`.
    static SHOWN: Cell<bool> = const { Cell::new(false) };
    /// Something is moving (a glide, a trail): frames until it ends.
    static MOVING: Cell<bool> = const { Cell::new(false) };
    static LOOK: Cell<CaretLook> = Cell::new(CaretLook::default());
    /// Each caret's motion, by its painter's id.
    static TRACKS: RefCell<HashMap<u64, Track>> = RefCell::new(HashMap::new());
}

/// A caret's motion: where it glides from and to, the places it left (its trail).
#[derive(Debug, Clone)]
struct Track {
    from: Rect,
    to: Rect,
    start: Instant,
    /// Where it was, and when it left (newest last).
    trail: Vec<(Rect, Instant)>,
    seen: Instant,
}

/// How carets look from now on (the theme's, or the user's choice).
pub fn configure(look: CaretLook) {
    LOOK.with(|l| l.set(look));
}

/// How carets look.
pub fn look() -> CaretLook {
    LOOK.with(Cell::get)
}

/// Input happened (a key, a click): the caret shows solid and the blink restarts.
pub fn reset() {
    EPOCH.with(|e| e.set(Some(Instant::now())));
}

/// The pointer moved or the view scrolled: a caret at rest blinks again (`true`: it was at
/// rest, a frame is due); a blinking one goes on as it was.
pub fn wake() -> bool {
    let resting = EPOCH.with(Cell::get).is_some_and(|t| t.elapsed() >= REST);
    if resting {
        reset();
    }
    resting
}

/// How visible a caret is in this frame (0–1: blinking; records that a caret is on screen).
pub fn opacity() -> f32 {
    SHOWN.with(|s| s.set(true));
    let elapsed = EPOCH.with(Cell::get).map_or(Duration::ZERO, |t| t.elapsed());
    blink_opacity(look().blink, elapsed)
}

/// Whether a caret should be drawn in this frame (half visible or more).
pub fn visible() -> bool {
    opacity() >= 0.5
}

/// The blink's opacity `elapsed` after the last input (solid once at rest).
fn blink_opacity(blink: Blink, elapsed: Duration) -> f32 {
    if elapsed >= REST {
        return 1.0;
    }
    let half = HALF.as_secs_f32();
    let t = elapsed.as_secs_f32() % (2.0 * half);
    let on = t < half;
    match blink {
        Blink::Off => 1.0,
        Blink::Classic => f32::from(u8::from(on)),
        Blink::Smooth => {
            // Holds, then fades during the half's last part (out when on, in when off).
            let into = if on { t } else { t - half };
            let fade = FADE.as_secs_f32();
            let k = ((into - (half - fade)) / fade).clamp(0.0, 1.0);
            let k = k * k * (3.0 - 2.0 * k);
            if on { 1.0 - k } else { k }
        }
    }
}

/// A caret to draw: where (the character cell it's at), in what color, how strong a halo
/// the painter allows (the terminal's outline when unfocused: none).
#[derive(Debug, Clone, Copy)]
pub struct CaretPaint {
    /// Tells this caret apart from the others (its glide and trail): the painter's id and
    /// the caret's index.
    pub id: u64,
    /// The character cell (its width: the character's, or a space's).
    pub cell: Rect,
    pub color: Rgba,
    /// Logical to physical pixels.
    pub scale: f32,
    /// A shape of its own (the terminal's, chosen there or by the program; a field's bar).
    pub shape: Option<CaretShape>,
    /// A thickness of its own, in physical pixels (a field's thin bar).
    pub thickness: Option<f32>,
    /// It doesn't blink (the terminal's choice), whatever the look says.
    pub steady: bool,
}

/// Draws a caret as the look says: its shape, blinking, glow, glide and trail. Returns the
/// rectangle drawn (for what goes over it: a block's character), `None` when hidden.
pub fn paint(layer: &mut Layer, caret: CaretPaint) -> Option<Rect> {
    let look = look();
    let alpha = if caret.steady {
        SHOWN.with(|s| s.set(true));
        1.0
    } else {
        opacity()
    };
    let thickness = caret.thickness.unwrap_or(look.width * caret.scale);
    let target = shape_rect(caret.cell, caret.shape.unwrap_or(look.shape), thickness);
    let now = Instant::now();
    let (rect, trail) = TRACKS.with(|tracks| {
        let mut tracks = tracks.borrow_mut();
        let track = tracks.entry(caret.id).or_insert_with(|| Track {
            from: target,
            to: target,
            start: now,
            trail: Vec::new(),
            seen: now,
        });
        track.seen = now;
        if track.to != target {
            // Moved: it glides from where it is now; the place it left joins the trail.
            let here = glide_at(track, now, look.motion);
            if look.trail_ms > 0 {
                track.trail.push((here, now));
            }
            track.from = here;
            track.to = target;
            track.start = now;
        }
        let keep = Duration::from_millis(u64::from(look.trail_ms));
        track.trail.retain(|(_, at)| now.duration_since(*at) < keep);
        track.trail.truncate(12);
        let rect = glide_at(track, now, look.motion);
        if rect != track.to || !track.trail.is_empty() {
            MOVING.with(|m| m.set(true));
        }
        let trail: Vec<(Rect, f32)> = track
            .trail
            .iter()
            .map(|(r, at)| {
                let age = now.duration_since(*at).as_secs_f32() / keep.as_secs_f32().max(0.001);
                (*r, (1.0 - age).clamp(0.0, 1.0))
            })
            .collect();
        (rect, trail)
    });
    // The trail: the places it left, fading (shown even while the blink hides the caret).
    for (r, strength) in trail {
        layer.quad(
            Quad::filled(r, fade(caret.color, 0.35 * strength))
                .rounded(r.size.width.min(r.size.height) / 2.0),
        );
    }
    if alpha <= 0.01 {
        return None;
    }
    if look.glow > 0.0 {
        // As wide as the caret is tall (whatever the screen's density).
        let tall = rect.size.height.max(rect.size.width);
        for (grow, strength) in [(0.38, 0.07), (0.22, 0.13), (0.11, 0.22)] {
            let g = (grow * tall).round().max(1.0);
            let halo = Rect::new(
                rect.min_x() - g,
                rect.min_y() - g,
                rect.size.width + 2.0 * g,
                rect.size.height + 2.0 * g,
            );
            layer.quad(
                Quad::filled(halo, fade(caret.color, look.glow * strength * alpha))
                    .rounded(g * 1.5),
            );
        }
    }
    layer.quad(Quad::filled(rect, fade(caret.color, alpha)));
    Some(rect)
}

/// The caret's rectangle in a character cell (`thickness` in physical pixels).
fn shape_rect(cell: Rect, shape: CaretShape, thickness: f32) -> Rect {
    let thick = thickness.round().max(1.0);
    match shape {
        CaretShape::Bar => Rect::new(cell.min_x().round(), cell.min_y(), thick, cell.size.height),
        CaretShape::Block => cell,
        CaretShape::Underline => {
            Rect::new(cell.min_x(), cell.max_y() - thick, cell.size.width.max(thick), thick)
        }
    }
}

/// Where a gliding caret is at `now` (eased out), or its place (no gliding).
fn glide_at(track: &Track, now: Instant, motion: Motion) -> Rect {
    let t = now.duration_since(track.start).as_secs_f32() / GLIDE.as_secs_f32();
    if motion == Motion::Jump || t >= 1.0 {
        return track.to;
    }
    let k = 1.0 - (1.0 - t).powi(3);
    let lerp = |a: f32, b: f32| a + (b - a) * k;
    Rect::new(
        lerp(track.from.min_x(), track.to.min_x()),
        lerp(track.from.min_y(), track.to.min_y()),
        lerp(track.from.size.width, track.to.size.width),
        lerp(track.from.size.height, track.to.size.height),
    )
}

/// `color` at `k` of its opacity.
fn fade(color: Rgba, k: f32) -> Rgba {
    color.with_alpha(color.a * k)
}

/// When the window needs its next frame for the carets, if one was painted since the last
/// call: soon while something moves (a glide, a trail, a fade), else at the blink's next
/// change (none when it doesn't blink). Clears the record.
pub fn take_next_frame() -> Option<Instant> {
    if !SHOWN.with(|s| s.replace(false)) {
        return None;
    }
    let now = Instant::now();
    TRACKS.with(|t| t.borrow_mut().retain(|_, track| now.duration_since(track.seen) < FORGET));
    if MOVING.with(|m| m.replace(false)) {
        return Some(now + FRAME);
    }
    let epoch = EPOCH.with(Cell::get).unwrap_or_else(|| {
        EPOCH.with(|e| e.set(Some(now)));
        now
    });
    let half = HALF.as_millis();
    let elapsed = epoch.elapsed().as_millis();
    // At rest: nothing more (its last frame showed it solid).
    let rest = epoch + REST;
    if now >= rest {
        return None;
    }
    let next = match look().blink {
        Blink::Off => None,
        Blink::Classic => Some(epoch + Duration::from_millis(((elapsed / half + 1) * half) as u64)),
        Blink::Smooth => {
            // Fading: frame by frame; holding: until the next fade starts.
            let into = elapsed % half;
            let fade_from = half - FADE.as_millis();
            if into >= fade_from {
                Some(now + FRAME)
            } else {
                Some(epoch + Duration::from_millis((elapsed - into + fade_from) as u64))
            }
        }
    };
    // The blink's last frame is the one at rest (shown solid).
    next.map(|at| at.min(rest))
}

#[cfg(test)]
#[path = "../tests/unit/caret.rs"]
mod tests;
