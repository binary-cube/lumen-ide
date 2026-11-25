//! Which package of the list is worth installing, and what a package must be to count.

use crate::Version;

/// A package as the list offers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Offer<'a> {
    /// Its version, as listed (one that isn't a [`Version`] is never taken).
    pub version: &'a str,
    /// The protocol its host speaks.
    pub protocol: u32,
    /// The system it's made for ([`crate::Platform::name`]).
    pub platform: &'a str,
}

/// The list as signed: its packages and its safe minimum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Listed<'a> {
    /// Its packages, in the list's order (a [`Choice::Take`] names one by its index).
    pub offers: &'a [Offer<'a>],
    /// The oldest version still safe to run (`None`: any).
    pub minimum: Option<&'a str>,
}

/// What this install looks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wanted<'a> {
    /// This system's name in the list ([`crate::platform`]).
    pub platform: &'a str,
    /// The protocol the app speaks.
    pub protocol: u32,
    /// Versions that failed to start here: never taken again.
    pub refused: &'a [String],
    /// The version installed and chosen (`None`: none).
    pub current: Option<&'a str>,
}

/// What the list says to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// Install the offer at this index (newer than the one installed, or none is).
    Take(usize),
    /// Keep the one installed: nothing newer fits (an older one never replaces it).
    /// `below_minimum`: it is older than the list's safe minimum and nothing replaces it.
    Current { below_minimum: bool },
    /// Nothing fits and nothing is installed.
    Nothing,
}

/// The package worth installing for `wanted` from `listed`: the newest for this system and
/// protocol, at least the list's minimum, none refused; taken only when newer than the one
/// installed — an update never goes back to an older version.
pub fn choose(listed: Listed<'_>, wanted: Wanted<'_>) -> Choice {
    let minimum = listed.minimum.and_then(Version::parse);
    let best = listed
        .offers
        .iter()
        .enumerate()
        .filter(|(_, o)| o.platform == wanted.platform && o.protocol == wanted.protocol)
        .filter(|(_, o)| !wanted.refused.iter().any(|r| r == o.version))
        .filter_map(|(i, o)| Some((Version::parse(o.version)?, i)))
        .filter(|(version, _)| minimum.is_none_or(|minimum| *version >= minimum))
        .max_by_key(|(version, _)| *version);
    let current = wanted.current.and_then(Version::parse);
    match (best, current) {
        (Some((best, _)), Some(current)) if best <= current => {
            Choice::Current { below_minimum: minimum.is_some_and(|m| current < m) }
        }
        (Some((_, index)), _) => Choice::Take(index),
        (None, Some(current)) => {
            Choice::Current { below_minimum: minimum.is_some_and(|m| current < m) }
        }
        (None, None) => Choice::Nothing,
    }
}

/// A SHA-256 in hex: its length.
const SHA256_HEX: usize = 64;

/// Whether a listed package can be considered at all: a version written its one way
/// ([`Version::parse`]), a SHA-256 in hex, a size, an archive's name in its address
/// ([`archive_name`]).
pub fn well_formed(version: &str, sha256: &str, size: u64, url: &str) -> bool {
    let hex = sha256.len() == SHA256_HEX && sha256.bytes().all(|b| b.is_ascii_hexdigit());
    let version = Version::parse(version).is_some_and(|v| v.to_string() == version);
    version && hex && size > 0 && archive_name(url).is_some()
}

/// The archive's file name from its address (its last part, its format known by it):
/// `None` unless it's a plain name — nothing that could leave its folder or isn't a file's
/// name on any system (`\`, `?`, `#`, `:`, `*`, `<`, `>`, `|`, `"`, `..`, a leading dot,
/// control characters).
pub fn archive_name(url: &str) -> Option<&str> {
    let (_, name) = url.rsplit_once('/')?;
    let plain = !name.is_empty()
        && !name.starts_with('.')
        && !name.contains("..")
        && !name.contains(['\\', '?', '#', ':', '*', '<', '>', '|', '"'])
        && !name.chars().any(char::is_control);
    plain.then_some(name)
}

#[cfg(test)]
#[path = "../tests/unit/choice.rs"]
mod tests;
