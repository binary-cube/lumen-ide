//! The conventions `check-deps` reads in every crate's sources (CLAUDE.md), beyond the
//! dependencies and the app's own rules (`app_sources`):
//!
//! - tests live in their own files: a `#[cfg(test)]` module is `#[path = "…"] mod tests;`,
//!   never written inline (`mod tests { … }`);
//! - no app name in the code's text: a string literal holding the brand's id (`"lumen"`,
//!   `"lumen-dark"`) is only an alias for something saved before a rename, said so in a
//!   comment just above it (`alias`, `before it was renamed`, `before a rename`);
//! - settings are declared once, in a crate's `settings.rs` (`Setting::new`): nowhere else;
//! - every locale key the code names (`t("tasks.title")`, `tr!("console.error_at", …)`) is
//!   in the locales (both have the same keys: the i18n crate's own test).
//!
//! Test code (`tests/` folders) and the build-only tools (`xtask`) are left out.

use std::collections::BTreeSet;
use std::path::Path;

use crate::app_sources::{Source, code_of, read_sources};

/// What a comment above a brand literal says to make it an alias (lowercase).
const ALIAS_MARKS: &[&str] = &["alias", "before it was renamed", "before a rename"];

/// How many lines above a brand literal its comment may be.
const ALIAS_REACH: usize = 3;

/// The violations of the rules above in `root`'s crates (`crates/**/src`), `brand` the app's
/// id and `locale_keys` the keys the locales define (`section.key`).
pub fn violations(root: &Path, brand: &str, locale_keys: &BTreeSet<String>) -> Vec<String> {
    let mut found = Vec::new();
    for source in crate_sources(&root.join("crates")) {
        found.extend(inline_tests(&source));
        found.extend(brand_literals(&source, brand));
        found.extend(settings_outside(&source));
        found.extend(unknown_keys(&source, locale_keys));
    }
    found
}

/// The `.rs` files under each crate's `src` (not its `tests`), paths from `crates/`.
fn crate_sources(crates: &Path) -> Vec<Source> {
    read_sources(crates)
        .into_iter()
        .filter(|s| s.path.contains("/src/") && !s.path.contains("/tests/"))
        .collect()
}

/// A test module written inline (`#[cfg(test)]` then `mod x {`).
pub fn inline_tests(source: &Source) -> Vec<String> {
    let mut found = Vec::new();
    let mut gated = false;
    for (n, line) in source.text.lines().enumerate() {
        let line = line.trim();
        if line == "#[cfg(test)]" {
            gated = true;
            continue;
        }
        if gated {
            let rest = line
                .strip_prefix("pub(crate) ")
                .or_else(|| line.strip_prefix("pub "))
                .unwrap_or(line);
            if let Some(module) = rest.strip_prefix("mod ")
                && module.contains('{')
            {
                found.push(format!(
                    "{}:{}: an inline test module: move it to tests/unit (`#[path = \"…\"] mod tests;`)",
                    source.path,
                    n + 1
                ));
            }
            if !line.starts_with("#[") {
                gated = false;
            }
        }
    }
    found
}

/// A string literal holding the brand's id without an alias comment above it.
pub fn brand_literals(source: &Source, brand: &str) -> Vec<String> {
    let brand = brand.to_lowercase();
    let lines: Vec<&str> = source.text.lines().collect();
    let mut found = Vec::new();
    for (n, line) in lines.iter().enumerate() {
        let code = code_of(line);
        if !literals(code).any(|l| l.to_lowercase().contains(&brand)) {
            continue;
        }
        let above = &lines[n.saturating_sub(ALIAS_REACH)..=n];
        let alias = above.iter().any(|l| {
            let l = l.to_lowercase();
            l.contains("//") && ALIAS_MARKS.iter().any(|m| l.contains(m))
        });
        if !alias {
            found.push(format!(
                "{}:{}: the app's name in the code's text (`kernel::brand` for what's shown, a \
                 technical name for ids; an alias for a saved id says so in a comment above)",
                source.path,
                n + 1
            ));
        }
    }
    found
}

/// A setting declared outside a `settings.rs`.
pub fn settings_outside(source: &Source) -> Vec<String> {
    if source.path.ends_with("/settings.rs") {
        return Vec::new();
    }
    source
        .text
        .lines()
        .enumerate()
        .filter(|(_, line)| code_of(line).contains("Setting::new("))
        .map(|(n, _)| {
            format!("{}:{}: a setting declared outside the crate's settings.rs", source.path, n + 1)
        })
        .collect()
}

/// A locale key named in the code (`t("a.b")`, `tr!("a.b", …)`) that the locales don't have.
pub fn unknown_keys(source: &Source, keys: &BTreeSet<String>) -> Vec<String> {
    let mut found = Vec::new();
    for (n, line) in source.text.lines().enumerate() {
        let code = code_of(line);
        for call in ["t(\"", "tr!(\""] {
            for (at, _) in code.match_indices(call) {
                // Part of a longer name (`set("…")`, `print!("…")`).
                if code[..at].ends_with(|c: char| c.is_alphanumeric() || c == '_') {
                    continue;
                }
                let rest = &code[at + call.len()..];
                let Some(key) = rest.split('"').next() else { continue };
                let is_key = key.contains('.')
                    && key
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._".contains(c));
                if is_key && !keys.contains(key) {
                    found.push(format!("{}:{}: `{key}` isn't in the locales", source.path, n + 1));
                }
            }
        }
    }
    found
}

/// The string literals in a line of code (simple ones: `"…"`, escapes skipped).
fn literals(code: &str) -> impl Iterator<Item = &str> {
    let mut rest = code;
    std::iter::from_fn(move || {
        let start = rest.find('"')? + 1;
        let body = &rest[start..];
        let mut end = None;
        let mut escaped = false;
        for (i, c) in body.char_indices() {
            match c {
                '\\' if !escaped => escaped = true,
                '"' if !escaped => {
                    end = Some(i);
                    break;
                }
                _ => escaped = false,
            }
        }
        let end = end?;
        rest = &body[end + 1..];
        Some(&body[..end])
    })
}

/// The keys a locale file defines (`section.key`).
pub fn locale_keys(text: &str) -> BTreeSet<String> {
    let mut section = String::new();
    let mut keys = BTreeSet::new();
    for line in text.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.to_owned();
        } else if !line.starts_with('#')
            && let Some((key, _)) = line.split_once('=')
        {
            keys.insert(format!("{section}.{}", key.trim()));
        }
    }
    keys
}

#[cfg(test)]
#[path = "../tests/unit/conventions.rs"]
mod tests;
