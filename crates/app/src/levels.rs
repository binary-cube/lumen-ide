//! The project's language levels (e.g. PHP 8.2): detected by the language module (from
//! `composer.json`) or chosen manually and kept in `<project>/<project dir>/project.toml`:
//!
//! ```toml
//! [levels]
//! php = "8.3"
//!
//! # Per-file exceptions (e.g. a script to test on an older version).
//! [files."legacy/import.php"]
//! php = "7.4"
//! ```

use std::path::Path;
use std::sync::Arc;

use code_intelligence::{LanguageIntelligence, Project};
use settings_model::ProjectLevelsStore;

pub use code_intelligence::levels::LanguageLevels;

/// Bumped once the environment was looked at (`LanguageIntelligence::warm`): the open
/// projects take its levels then.
static WARMED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The environment was looked at (from the background thread).
pub fn warmed() {
    WARMED.fetch_add(1, std::sync::atomic::Ordering::Release);
}

/// How many times the environment was looked at (a window compares it with its own).
pub fn warm_generation() -> u64 {
    WARMED.load(std::sync::atomic::Ordering::Acquire)
}

/// The project's levels, from what `store` keeps in its folder.
pub fn load(
    store: &dyn ProjectLevelsStore,
    root: &Path,
    intelligence: &[Arc<dyn LanguageIntelligence>],
    project: &Project<'_>,
) -> LanguageLevels {
    let saved = store.load(root);
    let kept = code_intelligence::levels::KeptLevels { levels: saved.levels, files: saved.files };
    let levels = LanguageLevels::new(&kept, intelligence, project);
    for ((language, level), (_, source)) in levels.detected.iter().zip(&levels.sources) {
        tracing::info!(%language, %level, %source, "language level detected");
    }
    levels
}

/// Keeps the choices in the project's folder, with `store`.
pub fn save(store: &dyn ProjectLevelsStore, root: &Path, levels: &LanguageLevels) {
    let kept = levels.kept();
    let saved = settings_model::SavedLevels { levels: kept.levels, files: kept.files };
    if let Err(e) = store.save(root, &saved) {
        tracing::warn!(error = %e, "cannot save the language levels");
    }
}
