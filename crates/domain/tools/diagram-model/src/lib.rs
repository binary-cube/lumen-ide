//! Diagrams as draw.io writes them (plan 80): a `.drawio` file read into pages and cells —
//! shapes, edges, the layers holding them — each cell's style as its ordered keys and its
//! geometry as numbers, and written back as it was read but for what changed. The format is
//! the world's: what's drawn here opens in draw.io (and its editor extensions) and back; what
//! isn't understood here (another library's shape, an unknown style key, a wrapper's own
//! properties) is kept untouched. A file is input like any other: read with limits (size,
//! elements, nesting, a compressed page's size), each page's ids unique.
//!
//! Edits are transactions ([`ops`] makes them from the document; [`Diagram`] applies and
//! keeps their history, a drag's steps as one); copy and paste are draw.io's XML
//! ([`clipboard`]).
//!
//! Pure: no UI, no disk, no threads.

mod cell;
pub mod clipboard;
mod codec;
mod document;
mod edit;
mod geometry;
pub mod ops;
mod style;
mod xml;

pub use cell::{Cell, CellKind};
pub use document::{Document, MAX_ELEMENTS, MAX_FILE, MAX_PAGE, Page};
pub use edit::{Change, Diagram, HISTORY_BYTES, HISTORY_STEPS, History, Transaction};
pub use geometry::{Geometry, Point};
pub use style::Style;
pub use xml::number;

/// The most cells a file holds (all its pages).
pub const MAX_CELLS: usize = 200_000;

/// Why a file can't be read as a diagram.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("the file is too large to read as a diagram")]
    TooLarge,
    #[error("the file isn't a draw.io diagram")]
    NotDrawio,
    #[error("the diagram's XML can't be read: {0}")]
    Xml(String),
    #[error("a compressed page can't be read: {0}")]
    Compressed(String),
    #[error("the diagram has too many cells")]
    TooManyCells,
    #[error("the diagram's elements nest too deep")]
    TooDeep,
    #[error("two cells of a page have the same id: {0}")]
    DuplicateId(String),
}
