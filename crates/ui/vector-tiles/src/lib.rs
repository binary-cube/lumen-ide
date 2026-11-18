//! Vector shapes for diagrams: outlines ([`path`]), the shapes diagrams are drawn with
//! ([`shapes`]), and their rasterization into tiles of alpha masks ([`tiles`]) — pure work
//! for a background thread, its result an image the renderer draws. Shared by the Markdown
//! preview's Mermaid diagrams and the diagram editor (plan 80).

pub mod path;
pub mod shapes;
pub mod tiles;

pub use path::{Line, Seg, ellipse, flowing, polygon, rounded};
pub use shapes::{Outline, ShapeKind, outline};
pub use tiles::{Mask, Shape, TILE, grid, shape_bounds, tile};
