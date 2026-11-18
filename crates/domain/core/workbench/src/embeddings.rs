//! Finding texts by meaning: a module that has a model turning texts into vectors (the
//! indexing module: semantic search's) contributes it as an [`EmbeddingSource`]; whoever
//! searches by meaning outside that module (the settings' search) asks it for the model.

use std::sync::Arc;

/// A model for finding texts by meaning, contributed by a module.
pub trait EmbeddingSource: std::fmt::Debug + Send + Sync {
    /// The model, if it's on and loaded now (`None`: find by words only). Never loads it.
    fn embedder(&self) -> Option<Arc<dyn text_search::Embedder>>;
}
