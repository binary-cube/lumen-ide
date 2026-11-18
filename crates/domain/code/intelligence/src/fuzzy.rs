//! Fuzzy matching for completion: case-insensitive subsequence, with
//! bonuses for prefix, consecutive characters and word starts (camelCase, `_`).

/// The score of `candidate` for `query` (higher = better), or `None` if it doesn't match.
/// An empty `query` matches anything (score 0).
pub fn fuzzy_score(query: &str, candidate: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    /// Identifiers: words start after `_ - \ $` (and in camelCase).
    const IDENTIFIERS: kernel::FuzzyStyle = kernel::FuzzyStyle {
        separators: &['_', '-', '\\', '$'],
        consecutive_bonus: 8,
        boundary_bonus: 6,
        gap_penalty: false,
        prefer_boundaries: false,
    };
    // Lowered once per query, not per candidate (a completion scores hundreds of thousands).
    thread_local! {
        static LOWERED: std::cell::RefCell<(String, Vec<char>)> = std::cell::RefCell::default();
    }
    let mut score = LOWERED.with_borrow_mut(|(last, lowered)| {
        if last != query {
            last.clear();
            last.push_str(query);
            *lowered = query.chars().flat_map(char::to_lowercase).collect();
        }
        kernel::fuzzy_score(candidate, lowered, &IDENTIFIERS)
    })?;
    let chars = candidate.chars().count();
    let prefix = candidate.len() >= query.len()
        && candidate.get(..query.len()).is_some_and(|p| p.eq_ignore_ascii_case(query));
    if prefix {
        score += 40;
    }
    if candidate.eq_ignore_ascii_case(query) {
        score += 20;
    }
    Some(score - (chars as i32 - query.len() as i32).max(0) / 4)
}

#[cfg(test)]
#[path = "../tests/unit/fuzzy.rs"]
mod tests;
