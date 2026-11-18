//! A version as SemVer reads it (`1.0.0`, `1.0.0-beta.1`, a leading `v` allowed, build
//! metadata ignored): compared by its numbers, then a pre-release before the release, two
//! pre-releases by their identifiers (numbers as numbers, below words).

use std::cmp::Ordering;
use std::fmt;

/// A version: `major.minor.patch`, with its pre-release identifiers (`beta`, `1`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Vec<String>,
}

impl Version {
    /// `text` read (`v1.2.3`, `1.2.3-rc.1+build`); `None` if it isn't a version.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let text = text.strip_prefix('v').unwrap_or(text);
        // Build metadata never orders.
        let text = text.split('+').next()?;
        let (core, pre) = match text.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (text, None),
        };
        let mut numbers = core.split('.');
        let number = |part: Option<&str>| -> Option<u64> {
            let part = part?;
            // No sign, no leading zero but `0` itself (SemVer's rule).
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            if part.len() > 1 && part.starts_with('0') {
                return None;
            }
            part.parse().ok()
        };
        let (major, minor, patch) =
            (number(numbers.next())?, number(numbers.next())?, number(numbers.next())?);
        if numbers.next().is_some() {
            return None;
        }
        let pre = match pre {
            Some(pre) => {
                let ids: Vec<String> = pre.split('.').map(str::to_owned).collect();
                let valid = |id: &String| {
                    !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                };
                if !ids.iter().all(valid) {
                    return None;
                }
                ids
            }
            None => Vec::new(),
        };
        Some(Self { major, minor, patch, pre })
    }

    /// A pre-release (`-beta.1`): offered only to whoever takes them.
    pub fn is_pre(&self) -> bool {
        !self.pre.is_empty()
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if !self.pre.is_empty() {
            write!(f, "-{}", self.pre.join("."))?;
        }
        Ok(())
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                // The release comes after its pre-releases.
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => compare_pre(&self.pre, &other.pre),
            })
    }
}

/// Two pre-releases' identifiers, one by one: numbers as numbers and before words, words
/// as text; with all equal, fewer before more.
fn compare_pre(a: &[String], b: &[String]) -> Ordering {
    for (x, y) in a.iter().zip(b) {
        let numbers = (x.parse::<u64>().ok(), y.parse::<u64>().ok());
        let order = match numbers {
            (Some(x), Some(y)) => x.cmp(&y),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => x.cmp(y),
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    a.len().cmp(&b.len())
}

#[cfg(test)]
#[path = "../tests/unit/version.rs"]
mod tests;
