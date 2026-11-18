//! The secrets hidden in the editor (`[dotenv] mask_secrets`): the part of each
//! secret-looking value to hide ([`values::hidden_part`]), as a [`Concealment`].

use std::ops::Range;

use workbench_model::Concealment;

use crate::syntax::EnvFile;
use crate::{settings, values};

/// A file larger than this isn't looked through (not a `.env` file anyone writes by hand).
const MAX_TEXT: usize = 1 << 20;

#[derive(Debug, Default)]
pub struct Secrets;

impl Concealment for Secrets {
    fn languages(&self) -> &'static [&'static str] {
        &[crate::ID]
    }

    fn generation(&self) -> u64 {
        settings::generation()
    }

    fn concealed(&self, text: &str) -> Vec<Range<usize>> {
        if !settings::mask_secrets() || text.len() > MAX_TEXT {
            return Vec::new();
        }
        secrets(text)
    }
}

/// The byte ranges of `text` holding secrets, in order.
pub fn secrets(text: &str) -> Vec<Range<usize>> {
    EnvFile::parse(text)
        .entries()
        .filter_map(|(_, entry)| {
            let content = entry.value.content.clone();
            let hidden = values::hidden_part(&text[entry.key.clone()], &text[content.clone()])?;
            Some(content.start + hidden.start..content.start + hidden.end)
        })
        .collect()
}

#[cfg(test)]
#[path = "../tests/unit/conceal.rs"]
mod tests;
