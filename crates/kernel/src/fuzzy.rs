//! Fuzzy matching, shared by the file search and completion: a case-insensitive
//! subsequence, scored with bonuses for consecutive characters and word starts. Each use
//! tunes it with a [`FuzzyStyle`] (paths and identifiers break words differently).

/// How a fuzzy match is scored.
#[derive(Debug, Clone, Copy)]
pub struct FuzzyStyle {
    /// Characters after which a word starts (an uppercase letter after a lowercase one
    /// always does).
    pub separators: &'static [char],
    pub consecutive_bonus: i32,
    pub boundary_bonus: i32,
    /// Characters skipped between two matched ones cost a point each (up to 10).
    pub gap_penalty: bool,
    /// When a character's first occurrence isn't a word start, a later word start with
    /// the same character is preferred.
    pub prefer_boundaries: bool,
}

/// `c` lowercase (its first character): ASCII without going through Unicode's tables (a
/// completion lowers every character of hundreds of thousands of names at each key).
#[inline]
fn lower(c: char) -> char {
    if c.is_ascii() { c.to_ascii_lowercase() } else { c.to_lowercase().next().unwrap_or(c) }
}

/// The score of `text` for `query` (lowercase characters) and the byte positions of the
/// matched characters, or `None` if it doesn't match.
pub fn fuzzy_match(text: &str, query: &[char], style: &FuzzyStyle) -> Option<(i32, Vec<usize>)> {
    // Most candidates don't match (a completion looks at a whole project's names): that is
    // found without allocating — the earliest occurrences, in order, are what the scoring
    // below starts from, so when they don't all exist nothing does.
    let mut wanted = query.iter();
    let mut next = wanted.next();
    for c in text.chars() {
        let Some(&q) = next else { break };
        if lower(c) == q {
            next = wanted.next();
        }
    }
    if next.is_some() {
        return None;
    }
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let boundary = |i: usize| -> bool {
        if i == 0 {
            return true;
        }
        let (prev, cur) = (chars[i - 1].1, chars[i].1);
        style.separators.contains(&prev) || (prev.is_lowercase() && cur.is_uppercase())
    };
    let lower: Vec<char> = chars.iter().map(|(_, c)| lower(*c)).collect();
    let mut positions = Vec::with_capacity(query.len());
    let mut score = 0;
    let mut i = 0;
    let mut last: Option<usize> = None;
    for &q in query {
        let first = (i..lower.len()).find(|&j| lower[j] == q)?;
        let consecutive = last.is_some_and(|l| l + 1 == first);
        let chosen = if !style.prefer_boundaries || consecutive || boundary(first) {
            first
        } else {
            (first..lower.len()).find(|&j| lower[j] == q && boundary(j)).unwrap_or(first)
        };
        score += 1;
        if last.is_some_and(|l| l + 1 == chosen) {
            score += style.consecutive_bonus;
        }
        if boundary(chosen) {
            score += style.boundary_bonus;
        }
        if style.gap_penalty
            && let Some(l) = last
        {
            score -= (chosen - l - 1).min(10) as i32;
        }
        positions.push(chars[chosen].0);
        last = Some(chosen);
        i = chosen + 1;
    }
    Some((score, positions))
}

/// [`fuzzy_match`]'s score alone, worked out without allocating when the style takes each
/// character's first occurrence (`prefer_boundaries` off: completion scores a whole
/// project's names on every key).
pub fn fuzzy_score(text: &str, query: &[char], style: &FuzzyStyle) -> Option<i32> {
    if style.prefer_boundaries {
        return fuzzy_match(text, query, style).map(|(score, _)| score);
    }
    let mut wanted = query.iter().peekable();
    let (mut score, mut last, mut prev) = (0, None::<usize>, None::<char>);
    for (j, c) in text.chars().enumerate() {
        let Some(&&q) = wanted.peek() else { break };
        if lower(c) == q {
            score += 1;
            if last.is_some_and(|l| l + 1 == j) {
                score += style.consecutive_bonus;
            }
            let boundary = prev.is_none_or(|p| {
                style.separators.contains(&p) || (p.is_lowercase() && c.is_uppercase())
            });
            if boundary {
                score += style.boundary_bonus;
            }
            if style.gap_penalty
                && let Some(l) = last
            {
                score -= (j - l - 1).min(10) as i32;
            }
            last = Some(j);
            wanted.next();
        }
        prev = Some(c);
    }
    wanted.peek().is_none().then_some(score)
}

#[cfg(test)]
#[path = "../tests/unit/fuzzy.rs"]
mod tests;
