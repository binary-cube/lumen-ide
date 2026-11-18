//! What's checked: in a `.env` file on its own, a key defined twice (the loaders keep one of
//! them — the last for Symfony's and phpdotenv, the first for Node's dotenv); in code, a
//! variable read that no `.env` file defines; in a `.env` file, a key no code reads. Pure:
//! what's defined and what's read are given.

use std::collections::HashSet;
use std::ops::Range;

use crate::syntax::EnvFile;

/// The keys defined again after their first definition (their names' ranges), in order.
pub fn duplicates(text: &str, file: &EnvFile) -> Vec<Range<usize>> {
    let mut seen = HashSet::new();
    file.entries()
        .filter(|(_, entry)| !seen.insert(&text[entry.key.clone()]))
        .map(|(_, entry)| entry.key.clone())
        .collect()
}

/// The names among `reads` (ranges of `text`) that aren't `defined` (by a `.env` file, its
/// documentation, the environment) nor `provided` by the runtime, in order.
pub fn undefined(
    text: &str,
    reads: Vec<Range<usize>>,
    defined: &dyn Fn(&str) -> bool,
    provided: &[&str],
) -> Vec<Range<usize>> {
    reads
        .into_iter()
        .filter(|r| !provided.contains(&&text[r.clone()]) && !defined(&text[r.clone()]))
        .collect()
}

/// The keys `text` (read as `file`) defines that nothing `used` reads nor another of its
/// values interpolates (their names' ranges), in order.
pub fn unused(text: &str, file: &EnvFile, used: &dyn Fn(&str) -> bool) -> Vec<Range<usize>> {
    let interpolated: HashSet<&str> = file
        .entries()
        .flat_map(|(_, e)| e.value.interpolations.iter().map(|i| &text[i.name.clone()]))
        .collect();
    file.entries()
        .map(|(_, e)| e.key.clone())
        .filter(|key| !interpolated.contains(&text[key.clone()]) && !used(&text[key.clone()]))
        .collect()
}

/// Where byte `offset` of `text` is: its line and its column in characters (0-based).
pub fn position(text: &str, offset: usize) -> (u32, u32) {
    let before = &text[..offset.min(text.len())];
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    (before.matches('\n').count() as u32, before[line_start..].chars().count() as u32)
}

#[cfg(test)]
#[path = "../tests/unit/checks.rs"]
mod tests;
