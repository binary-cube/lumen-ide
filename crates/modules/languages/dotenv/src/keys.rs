//! Where a key's name is written, whatever the file: in a `.env` file (its definition, a
//! value interpolating it) or where code reads it ([`crate::reads`], by the ways the file's
//! language reads variables). Pure: the ways are given.

use std::ops::Range;

use code_intelligence::env::EnvReads;

use crate::reads::{FileReads, read_at, reads};
use crate::syntax::EnvFile;

/// The key's name at `offset` (the cursor on it or right after it) of `text`, a file named
/// `name` of `language`.
pub fn key_at(
    registered: &[EnvReads],
    language: &str,
    name: &str,
    text: &str,
    offset: usize,
) -> Option<Range<usize>> {
    if language == crate::ID {
        return EnvFile::parse(text).name_at(offset).map(|(r, _)| r);
    }
    let file = FileReads::of(registered, language, name);
    if file.is_empty() {
        return None;
    }
    read_at(text, &file.accesses, offset)
}

/// Every place a key's name is written in `text` (a file named `name` of `language`), in
/// order.
pub fn places(
    registered: &[EnvReads],
    language: &str,
    name: &str,
    text: &str,
) -> Vec<Range<usize>> {
    if language == crate::ID {
        return EnvFile::parse(text).names().map(|(r, _)| r).collect();
    }
    let file = FileReads::of(registered, language, name);
    if file.is_empty() {
        return Vec::new();
    }
    reads(text, &file.accesses)
}

#[cfg(test)]
#[path = "../tests/unit/keys.rs"]
mod tests;
