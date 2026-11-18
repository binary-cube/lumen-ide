//! A file mask, as typed in a search: patterns split by commas or spaces (`*.php, *.twig`),
//! those starting with `!` leaving files out (`!*.min.js`). A pattern with a `/` matches the
//! whole path (relative, with `/`: `src/**/*.php`), one without the file's name. `*` is any
//! characters but `/`, `**` any at all, `?` one; letters match in any case.

/// A parsed mask (see the module's doc). Empty: every file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileMask {
    include: Vec<String>,
    exclude: Vec<String>,
}

impl FileMask {
    pub fn parse(text: &str) -> Self {
        let mut mask = Self::default();
        for part in text.split([',', ' ', ';']).map(str::trim).filter(|p| !p.is_empty()) {
            match part.strip_prefix('!') {
                Some(out) if !out.is_empty() => mask.exclude.push(out.to_lowercase()),
                Some(_) => {}
                None => mask.include.push(part.to_lowercase()),
            }
        }
        mask
    }

    /// Nothing is asked for: every file passes.
    pub fn is_empty(&self) -> bool {
        self.include.is_empty() && self.exclude.is_empty()
    }

    /// `path` (relative, with `/`) passes: one of the patterns kept matches it (or there's
    /// none) and none of those left out does.
    pub fn accepts(&self, path: &str) -> bool {
        let path = path.to_lowercase();
        let hits = |pattern: &String| {
            let subject = if pattern.contains('/') {
                path.as_str()
            } else {
                path.rsplit('/').next().unwrap_or(&path)
            };
            glob(pattern.as_bytes(), subject.as_bytes())
        };
        (self.include.is_empty() || self.include.iter().any(hits)) && !self.exclude.iter().any(hits)
    }
}

/// `pattern` matches all of `text` (`*`: any but `/`, `**`: any, `?`: one but `/`).
fn glob(pattern: &[u8], text: &[u8]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some((b'*', rest)) if rest.first() == Some(&b'*') => {
            // `**/` also matches nothing at all (`src/**/a.php` takes `src/a.php`).
            let rest = &rest[1..];
            let after_slash = rest.strip_prefix(b"/").unwrap_or(rest);
            (rest.first() == Some(&b'/') && glob(after_slash, text))
                || (0..=text.len()).any(|i| glob(rest, &text[i..]))
        }
        Some((b'*', rest)) => (0..=text.len())
            .take_while(|&i| i == 0 || text[i - 1] != b'/')
            .any(|i| glob(rest, &text[i..])),
        Some((b'?', rest)) => text.first().is_some_and(|c| *c != b'/') && glob(rest, &text[1..]),
        Some((c, rest)) => text.first() == Some(c) && glob(rest, &text[1..]),
    }
}

#[cfg(test)]
#[path = "../tests/unit/mask.rs"]
mod tests;
