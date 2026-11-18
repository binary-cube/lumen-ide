//! Canvas search (⌘K): jump to a file or a declaration in the project.

use code_intelligence::{CatalogEntry, fuzzy_score};

/// How many results are shown.
pub const RESULTS: usize = 8;

#[derive(Debug, Default)]
pub struct Search {
    pub query: String,
    /// Indices into the catalog, best matches first.
    pub results: Vec<usize>,
    pub selected: usize,
    /// All text is selected (⌘A): the next key replaces / deletes it.
    pub all: bool,
}

impl Search {
    /// Typed text (replaces the selection if all text is selected).
    pub fn insert(&mut self, typed: &str, catalog: &[CatalogEntry]) {
        if std::mem::take(&mut self.all) {
            self.query.clear();
        }
        self.query.push_str(typed);
        self.refresh(catalog);
    }

    /// Backspace: one character, or all text if selected.
    pub fn backspace(&mut self, catalog: &[CatalogEntry]) {
        if std::mem::take(&mut self.all) {
            self.query.clear();
        } else {
            self.query.pop();
        }
        self.refresh(catalog);
    }

    pub fn clear(&mut self) {
        self.query.clear();
        self.all = false;
        self.results.clear();
        self.selected = 0;
    }

    pub fn refresh(&mut self, catalog: &[CatalogEntry]) {
        self.selected = 0;
        if self.query.trim().is_empty() {
            self.results.clear();
            return;
        }
        let mut scored: Vec<(i32, usize)> = catalog
            .iter()
            .enumerate()
            .filter_map(|(i, e)| {
                let label = fuzzy_score(&self.query, &e.label).map(|s| s + 20);
                let detail = fuzzy_score(&self.query, &e.detail);
                label.max(detail).map(|s| (s, i))
            })
            .collect();
        scored.sort_by(|a, b| {
            b.0.cmp(&a.0).then(catalog[a.1].label.len().cmp(&catalog[b.1].label.len()))
        });
        self.results = scored.into_iter().take(RESULTS).map(|(_, i)| i).collect();
    }

    pub fn step(&mut self, delta: isize) {
        if self.results.is_empty() {
            return;
        }
        self.selected = ui_kit::step_index(self.selected, delta, self.results.len());
    }
}
