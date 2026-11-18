//! The project's own folder's `.gitignore`, shared by the modules keeping files there: it
//! ignores everything (`*`) but what a module lets through (its `!` lines). Each module adds
//! only its own lines, missing ones, after what's there: none overwrites another's.

use std::io;
use std::path::Path;

/// `dir`'s `.gitignore` lets `lines` through (e.g. `!diagrams/`, `!diagrams/**`): the ones
/// missing added at its end; made (ignoring everything else) if it isn't there. Written only
/// when something was missing.
pub fn let_through(dir: &Path, lines: &[&str]) -> io::Result<()> {
    let path = dir.join(".gitignore");
    let mut text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => "*\n".to_owned(),
        Err(e) => return Err(e),
    };
    let before = text.clone();
    for line in lines {
        if text.lines().any(|l| l.trim() == *line) {
            continue;
        }
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(line);
        text.push('\n');
    }
    if text == before && path.exists() {
        return Ok(());
    }
    crate::write_atomic(&path, text.as_bytes())
}

#[cfg(test)]
#[path = "../tests/unit/ignore.rs"]
mod tests;
