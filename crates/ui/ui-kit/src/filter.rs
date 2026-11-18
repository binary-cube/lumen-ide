//! A list filtered by the text typed above it: an item shows when one of its texts contains
//! what was typed, regardless of case and diacritics ("Proiecte" finds "proiecte", "setari"
//! finds "Setări"). The one filter of the lists; the searches that rank (files, symbols,
//! commands) match fuzzily instead (`kernel::fuzzy_match`).

/// Text as a search compares it: lowercased, without diacritics, one character for each of
/// the text's.
pub fn fold(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            'ă' | 'â' | 'Ă' | 'Â' => 'a',
            'î' | 'Î' => 'i',
            'ș' | 'ş' | 'Ș' | 'Ş' => 's',
            'ț' | 'ţ' | 'Ț' | 'Ţ' => 't',
            c => c.to_lowercase().next().unwrap_or(c),
        })
        .collect()
}

/// What was typed to filter a list (trimmed and folded once), matched against the items'
/// texts: an empty one lets everything through.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListFilter {
    query: String,
}

impl ListFilter {
    pub fn new(typed: &str) -> Self {
        Self { query: fold(typed.trim()) }
    }

    pub fn is_empty(&self) -> bool {
        self.query.is_empty()
    }

    /// `text` contains what was typed.
    pub fn matches(&self, text: &str) -> bool {
        self.query.is_empty() || fold(text).contains(&self.query)
    }

    /// One of `texts` contains what was typed.
    pub fn matches_any<'a>(&self, texts: impl IntoIterator<Item = &'a str>) -> bool {
        self.query.is_empty() || texts.into_iter().any(|t| fold(t).contains(&self.query))
    }
}

#[cfg(test)]
#[path = "../tests/unit/filter.rs"]
mod tests;
