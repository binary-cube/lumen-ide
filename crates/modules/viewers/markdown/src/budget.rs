//! One GPU budget for every open preview's textures (images, diagrams' tiles, formulas):
//! each view says what it drew; when they hold more than the budget, the ones not drawn for
//! the longest are given up ([`ImageData::release`]: the renderer frees them at its next
//! frame) — what's on screen (drawn in the last moment, by any window) is never. A given-up
//! one is made again if it's shown again.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use render_api::ImageData;

/// How much the previews may hold on the GPU together.
pub const GPU_BUDGET: u64 = 48 * 1024 * 1024;

#[derive(Default)]
struct Registry {
    /// Each texture drawn and when it was last drawn.
    drawn: HashMap<u64, (ImageData, Instant)>,
}

/// Drawn this recently: on screen in some window (never given up).
const SHOWN: Duration = Duration::from_millis(500);

static REGISTRY: Mutex<Option<Registry>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Registry) -> R) -> R {
    let mut guard = REGISTRY.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    f(guard.get_or_insert_with(Registry::default))
}

/// `image` is drawn in this frame.
pub fn drawn(image: &ImageData) {
    with(|r| {
        let now = Instant::now();
        r.drawn.entry(image.id).and_modify(|e| e.1 = now).or_insert_with(|| (image.clone(), now));
    });
}

/// A view finished its frame: over the budget (`limit` bytes), the textures drawn longest
/// ago are given up.
pub fn frame_done(limit: u64) {
    with(|r| {
        let now = Instant::now();
        // Freed by the renderer (unused a while), or lost: forgotten.
        r.drawn.retain(|_, (image, _)| !image.is_lost() && !image.released());
        let mut total: u64 = r.drawn.values().map(|(i, _)| i.gpu_memory().max(pending(i))).sum();
        if total <= limit {
            return;
        }
        let mut old: Vec<(u64, Instant)> = r
            .drawn
            .iter()
            .filter(|(_, (_, at))| now.duration_since(*at) > SHOWN)
            .map(|(id, (_, at))| (*id, *at))
            .collect();
        old.sort_by_key(|(_, f)| *f);
        for (id, _) in old {
            if total <= limit {
                break;
            }
            if let Some((image, _)) = r.drawn.remove(&id) {
                total = total.saturating_sub(image.gpu_memory().max(pending(&image)));
                image.release();
            }
        }
    });
}

/// What an image not uploaded yet will take.
fn pending(image: &ImageData) -> u64 {
    if image.memory() > 0 {
        u64::from(image.width) * u64::from(image.height) * image.bytes_per_pixel()
    } else {
        0
    }
}

/// What the previews hold on the GPU now (bytes).
#[cfg(test)]
pub fn live() -> u64 {
    with(|r| r.drawn.values().map(|(i, _)| i.gpu_memory()).sum())
}

#[cfg(test)]
#[path = "../tests/unit/budget.rs"]
mod tests;
