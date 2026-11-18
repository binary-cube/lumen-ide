//! What the log shows: its filters (combined: a commit must match them all) and its order —
//! the references whose history is read, a text or a hash, the authors, a time window and
//! the paths changed. The text, the authors and the time are matched here ([`LogFilter::
//! matches`]); the references, the paths and the order the repository applies as it walks.

use crate::Commit;

/// How the log is ordered.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum LogOrder {
    /// Newest first, by commit time.
    #[default]
    Date,
    /// No parent before its children, each line of history kept together
    /// (`git log --topo-order`).
    Topological,
}

/// The text a commit's message (or its hash) must have.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct TextFilter {
    pub text: String,
    /// The text is a regular expression.
    pub regex: bool,
    pub match_case: bool,
}

/// The log's filters and order. Empty (the default): every branch's whole history.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct LogFilter {
    /// Only these references' history (`main`, `origin/main`, a tag, `HEAD`); none: every
    /// branch's, every tag's and `HEAD`'s.
    pub references: Vec<String>,
    pub text: Option<TextFilter>,
    /// Commits by one of these authors (their names or emails); none: anyone's.
    pub authors: Vec<String>,
    /// Committed at or after `since`, before `until` (seconds since the epoch).
    pub since: Option<i64>,
    pub until: Option<i64>,
    /// Commits changing one of these (relative to the root, with `/`; a folder: anything
    /// in it); none: any.
    pub paths: Vec<String>,
    pub order: LogOrder,
}

impl LogFilter {
    /// Only the references set (or nothing): every commit of their history shows, the
    /// graph is the repository's own.
    pub fn keeps_every_commit(&self) -> bool {
        self.text.as_ref().is_none_or(|t| t.text.is_empty())
            && self.authors.is_empty()
            && self.since.is_none()
            && self.until.is_none()
            && self.paths.is_empty()
    }

    /// Nothing filtered at all (the order aside).
    pub fn is_empty(&self) -> bool {
        self.references.is_empty() && self.keeps_every_commit()
    }

    /// Whether `commit` (by `email`, with its whole `message`) passes the text, author and
    /// time filters; `text` is the text filter compiled ([`TextMatcher::new`]).
    pub fn matches(
        &self,
        text: Option<&TextMatcher>,
        commit: &Commit,
        email: &str,
        message: &str,
    ) -> bool {
        if self.since.is_some_and(|s| commit.time < s)
            || self.until.is_some_and(|u| commit.time >= u)
        {
            return false;
        }
        if !self.authors.is_empty()
            && !self.authors.iter().any(|a| {
                a.eq_ignore_ascii_case(&commit.author)
                    || (!email.is_empty() && a.eq_ignore_ascii_case(email))
            })
        {
            return false;
        }
        text.is_none_or(|t| t.matches(&commit.id, message))
    }
}

/// A text filter ready to match: a regular expression, or plain text (a hash's beginning
/// too).
#[derive(Debug, Clone)]
pub struct TextMatcher {
    pattern: regex::Regex,
    /// What's typed, lowercase, if it may be a hash (hexadecimal, 4 characters or more).
    hash: Option<String>,
}

impl TextMatcher {
    /// `None` when there's nothing to match (no text); an error when the regular expression
    /// isn't one.
    pub fn new(filter: &TextFilter) -> Result<Option<Self>, String> {
        let text = filter.text.trim();
        if text.is_empty() {
            return Ok(None);
        }
        let source = if filter.regex { text.to_owned() } else { regex::escape(text) };
        let pattern = regex::RegexBuilder::new(&source)
            .case_insensitive(!filter.match_case)
            .size_limit(1 << 20)
            .build()
            .map_err(|e| e.to_string())?;
        let hash = (text.len() >= 4 && text.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| text.to_ascii_lowercase());
        Ok(Some(Self { pattern, hash }))
    }

    /// The commit `id`'s `message` has the text, or its hash begins with it.
    pub fn matches(&self, id: &str, message: &str) -> bool {
        self.hash.as_ref().is_some_and(|h| id.starts_with(h.as_str()))
            || self.pattern.is_match(message)
    }
}

/// The first second of day `day` of `month` of `year` (UTC), as seconds since the epoch: the
/// inverse of [`crate::date`].
pub fn day_start(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe - 719_468) * 86_400
}

/// A date typed as `YYYY-MM-DD` (`YYYY-M-D` too), as its day's first second (UTC).
pub fn parse_day(text: &str) -> Option<i64> {
    let mut parts = text.trim().split('-');
    let year: i64 = parts.next()?.trim().parse().ok()?;
    let month: u32 = parts.next()?.trim().parse().ok()?;
    let day: u32 = parts.next()?.trim().parse().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let start = day_start(year, month, day);
    // Not a real day (the 31st of a 30-day month): its date comes back different.
    (crate::date(start) == (year, month, day)).then_some(start)
}

#[cfg(test)]
#[path = "../tests/unit/filter.rs"]
mod tests;
