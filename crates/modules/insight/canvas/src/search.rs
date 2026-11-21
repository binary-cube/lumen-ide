//! Canvas search (⌘K): jump to a file or a declaration in the project.

use code_intelligence::{CatalogEntry, fuzzy_score};
use ui_components::search_field::SearchField;

/// How many results are shown.
pub const RESULTS: usize = 8;

#[derive(Debug, Default)]
pub struct Search {
    /// What's typed: the app's search field (its caret, selection, ×, editing keys).
    pub field: SearchField,
    /// Indices into the catalog, best matches first.
    pub results: Vec<usize>,
    pub selected: usize,
}

impl Search {
    /// What's typed.
    pub fn query(&self) -> &str {
        self.field.text()
    }

    pub fn refresh(&mut self, catalog: &[CatalogEntry]) {
        self.selected = 0;
        let query = self.field.text();
        if query.trim().is_empty() {
            self.results.clear();
            return;
        }
        let mut scored: Vec<(i32, usize)> = catalog
            .iter()
            .enumerate()
            .filter_map(|(i, e)| {
                let label = fuzzy_score(query, &e.label).map(|s| s + 20);
                let detail = fuzzy_score(query, &e.detail);
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
