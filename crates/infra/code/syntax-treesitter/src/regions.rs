//! Regions a language works out from its text itself, beyond its tree's — a `.env` file's
//! sections, from its comments and the markers frameworks put there: contributed by its
//! module ([`FoldProvider`]), added to the document's folding regions on the parse thread.

use languages::{FoldRange, LanguageId};

/// A language's own folding regions.
pub trait FoldProvider: Send + Sync + std::fmt::Debug {
    /// The language whose documents it folds (as the document's own language: not where
    /// it's embedded).
    fn language(&self) -> LanguageId;
    /// The regions of `text` (each starting on its first line; on a line the largest of all
    /// the document's regions is kept). Cheap: asked at every parse.
    fn folds(&self, text: &str) -> Vec<FoldRange>;
}
