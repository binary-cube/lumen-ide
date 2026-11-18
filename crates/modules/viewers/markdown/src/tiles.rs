//! A Mermaid scene's shapes as tiles of alpha masks (`vector-tiles`: shared with the diagram
//! editor), and their tiles drawn from the preview's cache: only those near the view, made
//! on its workers.

use kernel::Rect;

use crate::mermaid::scene::{Prim, Scene};

pub use vector_tiles::{Mask, TILE};

/// The shapes of `scene` to rasterize (its texts are the text system's).
fn shapes(scene: &Scene) -> impl Iterator<Item = vector_tiles::Shape<'_>> {
    scene.prims.iter().filter_map(|prim| match prim {
        Prim::Path { segs, fill, line } => {
            Some(vector_tiles::Shape { segs, fill: *fill, line: line.as_ref() })
        }
        Prim::Text(_) => None,
    })
}

/// The tiles a scene at scale `k` has across and down.
pub fn grid(scene: &Scene, k: f32) -> (u32, u32) {
    vector_tiles::grid((scene.width, scene.height), k)
}

/// Tile `(tx, ty)` of `scene`'s shapes at scale `k`: a mask per color it shows.
pub fn tile(scene: &Scene, k: f32, tx: u32, ty: u32) -> Vec<Mask> {
    vector_tiles::tile((scene.width, scene.height), shapes(scene), k, tx, ty)
}

/// A whole scene's shapes as masks at scale `k`.
#[cfg(test)]
pub fn all(scene: &Scene, k: f32) -> Vec<Mask> {
    let (cols, rows) = grid(scene, k);
    let mut out = Vec::new();
    for ty in 0..rows {
        for tx in 0..cols {
            out.extend(tile(scene, k, tx, ty));
        }
    }
    out
}

/// How [`draw`] places a scene's tiles: the scene's corner on the screen, how much they're
/// stretched (a zoom between two scales they're made at: 1 in the preview), what's in view
/// and how far around it they're made ahead, whether missing ones are asked for.
#[derive(Debug, Clone, Copy)]
pub struct Placement {
    pub origin: kernel::Point,
    pub stretch: f32,
    pub view: Rect,
    pub margin: f32,
    pub fetch: bool,
}

/// Draws the tiles of `scene` at scale `k` that meet the view, asking `cache` for those
/// near it too; a tile not made yet is left out until it is. `true` if every one in view
/// was there.
pub fn draw(
    layer: &mut render_api::Layer,
    cache: &mut crate::jobs::Jobs<std::sync::Arc<Vec<Mask>>>,
    scene: &std::sync::Arc<Scene>,
    k: f32,
    key: u64,
    at: Placement,
) -> bool {
    let Placement { origin, stretch, view, margin, fetch } = at;
    let (cols, rows) = grid(scene, k);
    let origin = kernel::Point::new(origin.x.round(), origin.y.round());
    let side = TILE as f32 * stretch;
    let near = Rect::new(
        view.min_x() - margin,
        view.min_y() - margin,
        view.size.width + 2.0 * margin,
        view.size.height + 2.0 * margin,
    );
    // The tiles' range near the view.
    let range = |lo: f32, hi: f32, o: f32, count: u32| -> (u32, u32) {
        let a = ((lo - o) / side).floor().max(0.0) as u32;
        let b = (((hi - o) / side).ceil().max(0.0) as u32).min(count);
        (a.min(count), b)
    };
    let (cx0, cx1) = range(near.min_x(), near.max_x(), origin.x, cols);
    let (cy0, cy1) = range(near.min_y(), near.max_y(), origin.y, rows);
    let mut complete = true;
    for ty in cy0..cy1 {
        for tx in cx0..cx1 {
            let place =
                Rect::new(origin.x + tx as f32 * side, origin.y + ty as f32 * side, side, side);
            let shown = place.intersect(&view).is_some();
            let tile_key = crate::jobs::key(&(key, tx, ty));
            let made = if fetch {
                let scene = std::sync::Arc::clone(scene);
                cache.get(tile_key, move || std::sync::Arc::new(tile(&scene, k, tx, ty)))
            } else {
                cache.peek(tile_key)
            };
            let Some(masks) = made else {
                complete &= !shown;
                continue;
            };
            if masks.iter().any(|(i, _)| i.is_lost()) {
                // Freed out of sight (the budget, the renderer): made again.
                cache.remove(tile_key);
                complete &= !shown;
                continue;
            }
            if !shown {
                continue;
            }
            for (image, r) in masks.iter() {
                let place = Rect::new(
                    origin.x + r.min_x() * stretch,
                    origin.y + r.min_y() * stretch,
                    r.size.width * stretch,
                    r.size.height * stretch,
                );
                layer.image(image, place);
                crate::budget::drawn(image);
            }
        }
    }
    complete
}

#[cfg(test)]
#[path = "../tests/unit/tiles.rs"]
mod tests;
