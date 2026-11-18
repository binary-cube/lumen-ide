//! A project's language levels (e.g. PHP 8.2): the one used per language — the user's
//! choice, else what the project says (detected by the language), else the newest — and
//! the files with a level of their own. Pure: what was saved comes in, what to save goes
//! out (the app keeps it in `<project dir>/project.toml`).

use std::sync::Arc;

use kernel::LanguageId;

use crate::{LanguageIntelligence, Project};

/// What's kept of the levels: the manual choices (language, level) and the files' own
/// (relative path with `/`, language, level).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeptLevels {
    pub levels: Vec<(String, String)>,
    pub files: Vec<(String, String, String)>,
}

#[derive(Debug, Clone, Default)]
pub struct LanguageLevels {
    /// The level used, per language.
    pub chosen: Vec<(LanguageId, String)>,
    /// What was detected from the project (for „Auto” in the menu).
    pub detected: Vec<(LanguageId, String)>,
    /// Where each detected level comes from (e.g. `composer.json`, `php 8.3.6`).
    pub sources: Vec<(LanguageId, String)>,
    /// The manual choices (saved).
    overrides: Vec<(LanguageId, String)>,
    /// Per-file exceptions: (relative path with `/`, language, level).
    files: Vec<(String, LanguageId, String)>,
}

impl LanguageLevels {
    /// The project's levels, for the languages that have versions: what was `kept` (those
    /// the languages still know), and what each detects in the `project`.
    pub fn new(
        kept: &KeptLevels,
        intelligence: &[Arc<dyn LanguageIntelligence>],
        project: &Project<'_>,
    ) -> Self {
        let mut levels = Self::default();
        for (path, language, level) in &kept.files {
            let Some(intel) = intelligence.iter().find(|i| i.language().as_str() == language)
            else {
                continue;
            };
            if intel.levels().contains(&level.as_str()) {
                levels.files.push((path.clone(), intel.language(), level.clone()));
            }
        }
        for intel in intelligence.iter().filter(|i| !i.levels().is_empty()) {
            let language = intel.language();
            if let Some(found) = intel.detect_level(project) {
                levels.detected.push((language.clone(), found.level));
                levels.sources.push((language.clone(), found.source));
            }
            if let Some((_, level)) = kept
                .levels
                .iter()
                .find(|(l, v)| l == language.as_str() && intel.levels().contains(&v.as_str()))
            {
                levels.overrides.push((language.clone(), level.clone()));
            }
        }
        levels.resolve(intelligence);
        levels
    }

    /// The manual choice for a language (`None` = automatic).
    pub fn set(
        &mut self,
        intelligence: &[Arc<dyn LanguageIntelligence>],
        language: &LanguageId,
        level: Option<&str>,
    ) {
        self.overrides.retain(|(l, _)| l != language);
        if let Some(level) = level {
            self.overrides.push((language.clone(), level.to_owned()));
        }
        self.resolve(intelligence);
    }

    /// The level of a single file (`None` = same as the project).
    pub fn set_file(&mut self, path: &str, language: &LanguageId, level: Option<&str>) {
        self.files.retain(|(p, l, _)| !(p == path && l == language));
        if let Some(level) = level {
            self.files.push((path.to_owned(), language.clone(), level.to_owned()));
        }
    }

    /// The file's exception for a language, if any.
    pub fn file_level(&self, path: &str, language: &LanguageId) -> Option<&str> {
        self.files.iter().find(|(p, l, _)| p == path && l == language).map(|(_, _, v)| v.as_str())
    }

    /// The levels used for a file: the project's, with the file's exceptions.
    pub fn for_file(&self, path: Option<&str>) -> Vec<(LanguageId, String)> {
        let mut levels = self.chosen.clone();
        if let Some(path) = path {
            for (language, level) in &mut levels {
                if let Some(own) = self.file_level(path, language) {
                    own.clone_into(level);
                }
            }
        }
        levels
    }

    /// What to keep: the manual choices and the files' own.
    pub fn kept(&self) -> KeptLevels {
        KeptLevels {
            levels: self
                .overrides
                .iter()
                .map(|(l, v)| (l.as_str().to_owned(), v.clone()))
                .collect(),
            files: self
                .files
                .iter()
                .map(|(p, l, v)| (p.clone(), l.as_str().to_owned(), v.clone()))
                .collect(),
        }
    }

    pub fn is_auto(&self, language: &LanguageId) -> bool {
        !self.overrides.iter().any(|(l, _)| l == language)
    }

    pub fn get<'a>(list: &'a [(LanguageId, String)], language: &LanguageId) -> Option<&'a str> {
        list.iter().find(|(l, _)| l == language).map(|(_, v)| v.as_str())
    }

    /// Chosen = manual, otherwise detected, otherwise the newest level.
    fn resolve(&mut self, intelligence: &[Arc<dyn LanguageIntelligence>]) {
        self.chosen = intelligence
            .iter()
            .filter(|i| !i.levels().is_empty())
            .filter_map(|intel| {
                let language = intel.language();
                let level = Self::get(&self.overrides, &language)
                    .or_else(|| Self::get(&self.detected, &language))
                    .or_else(|| intel.levels().last().copied())?
                    .to_owned();
                Some((language, level))
            })
            .collect();
    }
}

#[cfg(test)]
#[path = "../tests/unit/levels.rs"]
mod tests;
