//! Where code reads environment variables: the ways other modules contributed
//! ([`EnvAccess`], in [`EnvReads`]) matched in a text — `getenv('X')`, `$_ENV['X']`,
//! `process.env.X`, `%env(int:X)%`, `${X:-default}`, `$X`. Pure: the ways are given.

use std::ops::Range;

use code_intelligence::env::{EnvAccess, EnvReads, Quoting};

/// The ways a file reads variables: those of the contributions covering its language and
/// its name, and whether its undefined reads are flagged (any of them says so), with the
/// names its runtime provides.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileReads {
    pub accesses: Vec<EnvAccess>,
    pub flagged: Vec<EnvAccess>,
    pub provided: Vec<&'static str>,
}

impl FileReads {
    /// What `registered` says of a file named `name`, in `language`.
    pub fn of(registered: &[EnvReads], language: &str, name: &str) -> Self {
        let mut out = Self::default();
        for reads in registered.iter().filter(|r| r.covers(language, name)) {
            out.accesses.extend_from_slice(reads.accesses);
            if reads.flag_undefined {
                out.flagged.extend_from_slice(reads.accesses);
            }
            out.provided.extend_from_slice(reads.provided);
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.accesses.is_empty()
    }
}

/// The names of the variables `text` reads by `accesses` (their ranges), in order, once
/// each place.
pub fn reads(text: &str, accesses: &[EnvAccess]) -> Vec<Range<usize>> {
    let mut out: Vec<Range<usize>> =
        accesses.iter().flat_map(|access| matches(text, access)).collect();
    out.sort_by_key(|r| r.start);
    out.dedup();
    out
}

/// The name of the variable read at `offset` (the cursor on it or right after it), looked
/// for on its line only.
pub fn read_at(text: &str, accesses: &[EnvAccess], offset: usize) -> Option<Range<usize>> {
    let offset = offset.min(text.len());
    let start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
    let end = text[offset..].find('\n').map_or(text.len(), |i| offset + i);
    reads(&text[start..end], accesses)
        .into_iter()
        .map(|r| start + r.start..start + r.end)
        .find(|r| r.start <= offset && offset <= r.end)
}

fn is_name_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_name(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Past the spaces from `at`.
fn spaces(bytes: &[u8], mut at: usize) -> usize {
    while bytes.get(at).is_some_and(|b| *b == b' ' || *b == b'\t') {
        at += 1;
    }
    at
}

/// The name starting at `at`: its end.
fn name_end(bytes: &[u8], at: usize) -> Option<usize> {
    if !bytes.get(at).copied().is_some_and(is_name_start) {
        return None;
    }
    let mut end = at + 1;
    while bytes.get(end).copied().is_some_and(is_name) {
        end += 1;
    }
    Some(end)
}

/// Whether the prefix at `at` starts a word: nothing of a name, no member access
/// (`->`, `::`) right before it.
fn starts_word(text: &str, at: usize) -> bool {
    let before = &text[..at];
    let joined = before.bytes().next_back().is_some_and(|b| is_name(b) || b == b'$');
    !joined && !before.ends_with("->") && !before.ends_with("::")
}

/// A `$` escaped where it stands (`$$X` in Docker Compose, `\$X` in a shell's quotes).
fn escaped(text: &str, at: usize, access: &EnvAccess) -> bool {
    access.prefix.starts_with('$') && text[..at].ends_with(['$', '\\'])
}

/// The variables `access` reads in `text`.
fn matches(text: &str, access: &EnvAccess) -> Vec<Range<usize>> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    for (at, _) in text.match_indices(access.prefix) {
        if (access.word && !starts_word(text, at)) || escaped(text, at, access) {
            continue;
        }
        if let Some(name) = name_after(text, bytes, at + access.prefix.len(), access) {
            out.push(name);
        }
    }
    out
}

/// The name `access` reads right after its prefix (ending at `from`).
fn name_after(text: &str, bytes: &[u8], from: usize, access: &EnvAccess) -> Option<Range<usize>> {
    let quoted = access.quoting == Quoting::Quoted;
    let mut at = from;
    if quoted {
        at = spaces(bytes, at);
    }
    if !text[at..].starts_with(access.opening) {
        return None;
    }
    at += access.opening.len();
    if access.processors {
        return processed_name(text, at, access.after);
    }
    let quote = if quoted {
        at = spaces(bytes, at);
        let quote = *bytes.get(at).filter(|b| matches!(b, b'\'' | b'"'))?;
        at += 1;
        Some(quote)
    } else {
        None
    };
    let end = name_end(bytes, at)?;
    let mut rest = end;
    if let Some(quote) = quote {
        rest = (bytes.get(rest) == Some(&quote)).then_some(rest + 1)?;
        rest = spaces(bytes, rest);
    }
    let follows =
        access.after.is_empty() || access.after.iter().any(|a| text[rest..].starts_with(a));
    follows.then_some(at..end)
}

/// The name in `%env(int:default::NAME)%`: the last `:`-separated part before the closing.
fn processed_name(text: &str, at: usize, closes: &[&str]) -> Option<Range<usize>> {
    let line_end = text[at..].find('\n').map_or(text.len(), |i| at + i);
    let close = closes.iter().filter_map(|c| text[at..line_end].find(c)).min()?;
    let inside = &text[at..at + close];
    let start = at + inside.rfind(':').map_or(0, |i| i + 1);
    let end = at + close;
    let bytes = text.as_bytes();
    (name_end(bytes, start) == Some(end)).then_some(start..end)
}

#[cfg(test)]
#[path = "../tests/unit/reads.rs"]
mod tests;
