//! A drawing's tiles, rasterized on a worker of their own: asked for as they come into view,
//! kept by (drawing, scale, place), the least recently drawn let go past a budget. While the
//! tiles in view aren't all made, the last whole set is drawn instead — the previous scale
//! stretched under the new one (a zoom never shows holes), or the drawing before an edit
//! (it never blinks); the scales are steps (half an octave apart), so a zoom makes few.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};

use kernel::{Point, Rect};
use vector_tiles::{Mask, TILE};

use crate::drawing::Drawing;

/// The most tiles kept (a few screens' worth at the scales in use).
const KEPT: usize = 96;

/// A tile: its drawing's generation, its scale's step, its column and row.
type Key = (u64, i32, u32, u32);

/// A tile made on the worker.
type Made = (Key, Vec<Mask>);

/// A tile to draw: its masks, how much they're stretched, the drawing's corner.
pub type Drawn = (Arc<Vec<Mask>>, f32, Point);

type Waker = Arc<dyn Fn() + Send + Sync>;

/// The scale `k` (physical pixels per diagram unit) rounded to its step: (the step's number,
/// the step's scale).
pub fn step(k: f32) -> (i32, f32) {
    let n = (k.max(1e-3).log2() * 2.0).round() as i32;
    (n, 2f32.powf(n as f32 / 2.0))
}

/// The tiles made and being made.
pub struct Tiles {
    made: HashMap<Key, (Arc<Vec<Mask>>, u64)>,
    asked: std::collections::HashSet<Key>,
    done: (Sender<Made>, Receiver<Made>),
    pool: Option<kernel::work::Pool>,
    /// Counted per paint: what was drawn last.
    frame: u64,
    /// The last drawing, generation and step every tile in view was there for (drawn while
    /// another is made).
    complete: Option<(Arc<Drawing>, u64, i32)>,
    /// The generation and step the last paint wanted.
    shown: Option<(u64, i32)>,
    /// The newest generation asked for: a job for an older one, still queued, isn't made.
    latest: Arc<AtomicU64>,
    waker: Arc<Mutex<Option<Waker>>>,
}

/// `masks` given up: the renderer frees their textures now, not when they've been unused
/// long enough.
fn release(masks: &[Mask]) {
    for (image, _) in masks {
        image.release();
    }
}

impl std::fmt::Debug for Tiles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tiles").field("made", &self.made.len()).finish_non_exhaustive()
    }
}

impl Default for Tiles {
    fn default() -> Self {
        Self {
            made: HashMap::new(),
            asked: std::collections::HashSet::new(),
            done: channel(),
            pool: None,
            frame: 0,
            complete: None,
            shown: None,
            latest: Arc::default(),
            waker: Arc::default(),
        }
    }
}

/// What a paint draws of a drawing: where its corner is, at which scale.
#[derive(Debug, Clone, Copy)]
pub struct Place {
    /// The drawing's corner on the screen.
    pub corner: Point,
    /// Physical pixels per diagram unit.
    pub k: f32,
    /// The view (physical pixels).
    pub view: Rect,
}

impl Tiles {
    pub fn set_waker(&mut self, waker: Option<&Waker>) {
        if let (Some(w), Ok(mut slot)) = (waker, self.waker.lock()) {
            *slot = Some(Arc::clone(w));
        }
    }

    /// The tiles made since, taken. `true`: something new to draw — a tile shown as it comes
    /// (the drawing shown, at another scale or place), or the last one of a drawing's set
    /// (an edit's tiles show together: before, nothing would change on the screen).
    pub fn poll(&mut self) -> bool {
        let mut any = false;
        while let Ok((key, masks)) = self.done.1.try_recv() {
            self.asked.remove(&key);
            // A drawing replaced since: never drawn.
            if self.shown.is_some_and(|(g, _)| key.0 < g) {
                release(&masks);
                continue;
            }
            self.made.insert(key, (Arc::new(masks), self.frame));
            let Some((g, n)) = self.shown else {
                any = true;
                continue;
            };
            if (key.0, key.1) == (g, n) {
                let as_they_come = self.complete.as_ref().is_none_or(|(_, cg, _)| *cg == g);
                let last = !self.asked.iter().any(|k| (k.0, k.1) == (g, n));
                any |= as_they_come || last;
            }
        }
        any
    }

    /// Drawing `now` (generation `now_gen`) replacing `before` (`before_gen`) after an edit:
    /// the tiles no change reaches are the new one's too (the edit makes only those it
    /// touches again). Nothing carried when the page's box changed (its tiles moved) or
    /// shapes came or went.
    pub fn carry(&mut self, before: &Drawing, before_gen: u64, now: &Drawing, now_gen: u64) {
        if before.bounds != now.bounds || before.items.len() != now.items.len() {
            return;
        }
        let shape = |i: &crate::drawing::Item| {
            vector_tiles::shape_bounds(&vector_tiles::Shape {
                segs: &i.segs,
                fill: i.fill,
                line: i.line.as_ref(),
            })
        };
        let mut dirty: Option<Rect> = None;
        for (a, b) in before.items.iter().zip(&now.items) {
            if a != b {
                for r in [shape(a), shape(b)] {
                    dirty = Some(dirty.map_or(r, |d| crate::edit::union(d, r)));
                }
            }
        }
        let old: Vec<(Key, Arc<Vec<Mask>>, u64)> = self
            .made
            .iter()
            .filter(|(k, _)| k.0 == before_gen)
            .map(|(k, (m, used))| (*k, Arc::clone(m), *used))
            .collect();
        for ((_, n, tx, ty), masks, used) in old {
            let kq = 2f32.powf(n as f32 / 2.0);
            let side = TILE as f32 / kq;
            // A pixel's antialiasing around what changed.
            let tile = Rect::new(tx as f32 * side, ty as f32 * side, side, side);
            let reached = dirty.is_some_and(|d| {
                let pad = 2.0 / kq;
                let d = Rect::new(
                    d.min_x() - pad,
                    d.min_y() - pad,
                    d.size.width + 2.0 * pad,
                    d.size.height + 2.0 * pad,
                );
                d.intersect(&tile).is_some()
            });
            if !reached {
                self.made.insert((now_gen, n, tx, ty), (masks, used));
            }
        }
    }

    /// Everything let go (another drawing: its tiles made again as it's drawn).
    pub fn clear(&mut self) {
        // A tile carried across an edit is under two keys: the last of them to go frees it.
        for (_, (masks, _)) in self.made.drain() {
            if Arc::strong_count(&masks) == 1 {
                release(&masks);
            }
        }
        self.asked.clear();
        self.complete = None;
    }

    /// What the tiles hold (bytes): in memory (pixels not on the GPU yet) and on the GPU.
    /// A tile carried across an edit counts once.
    pub fn memory(&self) -> (u64, u64) {
        let mut seen = std::collections::HashSet::new();
        self.made
            .values()
            .filter(|(m, _)| seen.insert(Arc::as_ptr(m)))
            .flat_map(|(m, _)| m.iter())
            .fold((0, 0), |(ram, gpu), (image, _)| {
                (ram + image.memory() as u64, gpu + image.gpu_memory())
            })
    }

    /// What the tiles take (bytes), wherever they are.
    #[cfg(test)]
    pub fn bytes(&self) -> u64 {
        self.made
            .values()
            .flat_map(|(m, _)| m.iter())
            .map(|(image, _)| {
                u64::from(image.width) * u64::from(image.height) * image.bytes_per_pixel()
            })
            .sum()
    }

    /// The masks to draw for `drawing` (its generation `generation`) at `at`, each with its
    /// place on the screen; the ones missing asked for. `true` (second): all were there.
    pub fn visible(
        &mut self,
        drawing: &Arc<Drawing>,
        generation: u64,
        at: Place,
    ) -> (Vec<Drawn>, bool) {
        self.frame += 1;
        let (n, kq) = step(at.k);
        let wanted = self.cover(drawing, generation, n, kq, at, true);
        let complete = wanted.iter().all(Option::is_some);
        let mut out: Vec<_> = wanted.into_iter().flatten().collect();
        self.shown = Some((generation, n));
        self.latest.fetch_max(generation, Ordering::Relaxed);
        if complete {
            // An older drawing's tiles are never drawn again: let go (their textures too).
            if self.complete.as_ref().is_some_and(|(_, g, _)| *g < generation) {
                let frame = self.frame;
                self.made.retain(|k, (masks, used)| {
                    let keep = k.0 >= generation || *used == frame;
                    if !keep && Arc::strong_count(masks) == 1 {
                        release(masks);
                    }
                    keep
                });
            }
            self.complete = Some((Arc::clone(drawing), generation, n));
        } else if let Some((before, g, prev)) =
            self.complete.clone().filter(|(_, g, p)| (*g, *p) != (generation, n))
        {
            // The last whole set: its corner where its drawing's corner is now.
            let kp = 2f32.powf(prev as f32 / 2.0);
            let corner = Point::new(
                at.corner.x + (before.bounds.min_x() - drawing.bounds.min_x()) * at.k,
                at.corner.y + (before.bounds.min_y() - drawing.bounds.min_y()) * at.k,
            );
            let then = Place { corner, ..at };
            let under: Vec<_> =
                self.cover(&before, g, prev, kp, then, false).into_iter().flatten().collect();
            // Another drawing (an edit): only the whole one, not a mix of both; another
            // scale of the same: the old stretched under what's there of the new.
            out = if g == generation { under.into_iter().chain(out).collect() } else { under };
        }
        self.trim();
        (out, complete)
    }

    /// The tiles of step `n` (scale `kq`) in view: made ones with their place, missing ones
    /// `None` (asked for when `ask`).
    fn cover(
        &mut self,
        drawing: &Arc<Drawing>,
        generation: u64,
        n: i32,
        kq: f32,
        at: Place,
        ask: bool,
    ) -> Vec<Option<Drawn>> {
        let stretch = at.k / kq;
        let size = (drawing.bounds.size.width, drawing.bounds.size.height);
        let (cols, rows) = vector_tiles::grid(size, kq);
        let side = TILE as f32 * stretch;
        let range = |lo: f32, hi: f32, o: f32, count: u32| -> (u32, u32) {
            let a = ((lo - o) / side).floor().max(0.0) as u32;
            let b = (((hi - o) / side).ceil().max(0.0) as u32).min(count);
            (a.min(count), b)
        };
        let (x0, x1) = range(at.view.min_x(), at.view.max_x(), at.corner.x, cols);
        let (y0, y1) = range(at.view.min_y(), at.view.max_y(), at.corner.y, rows);
        let mut out = Vec::new();
        for ty in y0..y1 {
            for tx in x0..x1 {
                let key = (generation, n, tx, ty);
                if ask && !self.made.contains_key(&key) {
                    self.ask(drawing, key, kq);
                }
                match self.made.get_mut(&key) {
                    Some((masks, used)) if !masks.iter().any(|(i, _)| i.is_lost()) => {
                        *used = self.frame;
                        out.push(Some((Arc::clone(masks), stretch, at.corner)));
                    }
                    _ => {
                        // Lost (freed by the renderer): made again next time.
                        self.made.remove(&key);
                        out.push(None);
                    }
                }
            }
        }
        out
    }

    /// Tile `key` made on the worker; without a waker (tests, snapshots: nothing would draw
    /// it when it's done) here and now.
    fn ask(&mut self, drawing: &Arc<Drawing>, key: Key, kq: f32) {
        if self.waker.lock().unwrap_or_else(PoisonError::into_inner).is_none() {
            let size = (drawing.bounds.size.width, drawing.bounds.size.height);
            let masks = vector_tiles::tile(size, drawing.shapes(), kq, key.2, key.3);
            self.made.insert(key, (Arc::new(masks), self.frame));
            return;
        }
        if !self.asked.insert(key) {
            return;
        }
        let pool = self
            .pool
            .get_or_insert_with(|| kernel::work::Pool::new("diagram-tiles", 2, kernel::work::IDLE));
        let (drawing, tx, waker, latest) = (
            Arc::clone(drawing),
            self.done.0.clone(),
            Arc::clone(&self.waker),
            Arc::clone(&self.latest),
        );
        pool.spawn(kernel::work::Priority::Interactive, None, move || {
            // Edited since (typing a number in the panel): nothing made, its key let go.
            if key.0 < latest.load(Ordering::Relaxed) {
                let _ = tx.send((key, Vec::new()));
                return;
            }
            let size = (drawing.bounds.size.width, drawing.bounds.size.height);
            let masks = vector_tiles::tile(size, drawing.shapes(), kq, key.2, key.3);
            if tx.send((key, masks)).is_ok()
                && let Some(wake) = waker.lock().unwrap_or_else(PoisonError::into_inner).clone()
            {
                wake();
            }
        });
    }

    /// The least recently drawn let go past the budget.
    fn trim(&mut self) {
        if self.made.len() <= KEPT {
            return;
        }
        let mut by_use: Vec<(Key, u64)> =
            self.made.iter().map(|(k, (_, used))| (*k, *used)).collect();
        // The least recently drawn first; drawn as long ago, an older drawing's first.
        by_use.sort_by_key(|(key, used)| (*used, key.0));
        let frame = self.frame;
        for (key, used) in by_use.into_iter().take(self.made.len() - KEPT) {
            if let Some((masks, _)) = self.made.remove(&key)
                && used != frame
                && Arc::strong_count(&masks) == 1
            {
                release(&masks);
            }
        }
    }
}

/// A closed tab's tiles freed now (their textures), not when the renderer finds them unused.
impl Drop for Tiles {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
#[path = "../tests/unit/tiles.rs"]
mod tests;
