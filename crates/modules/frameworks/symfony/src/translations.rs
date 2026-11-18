//! The project's translations: `translations/<domain>.<locale>.yaml|yml|xlf|xliff` — each
//! key (YAML: nested ones joined with `.`; XLIFF: a unit's `resname`, else its `source`) with
//! its text in each locale. Named by `trans('key')`, `->trans('key')`, `t('key')` and Twig's
//! `'key'|trans`.

use std::ops::Range;
use std::path::PathBuf;

use code_intelligence::{Project, before_string, line_of, quoted_at};

/// A key's text in one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Translation {
    pub key: String,
    /// `messages`, `validators`…
    pub domain: String,
    pub locale: String,
    pub text: Option<String>,
    pub file: PathBuf,
    pub offset: usize,
    pub line: u32,
}

/// Every translation of the project, by key, then locale.
pub fn gather(project: &Project<'_>) -> Vec<Translation> {
    let Some(root) = project.root else { return Vec::new() };
    let mut all = Vec::new();
    for relative in project.files_under("translations", &[".yaml", ".yml", ".xlf", ".xliff"], 2000)
    {
        let name = relative.rsplit('/').next().unwrap_or(&relative);
        // `messages+intl-icu.fr.yaml`: domain `messages`, locale `fr`.
        let mut parts = name.rsplitn(3, '.');
        let (Some(extension), Some(locale), Some(domain)) =
            (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let domain = domain.split('+').next().unwrap_or(domain).to_owned();
        let file = root.join("translations").join(&relative);
        let Some(text) = (project.read)(&file) else { continue };
        let found: Vec<(String, Option<String>, usize)> = if extension.starts_with("xl") {
            from_xliff(&text)
        } else {
            crate::yaml::keys(&text, None)
                .into_iter()
                .map(|k| (k.name, k.value, k.offset))
                .collect()
        };
        for (key, value, offset) in found {
            all.push(Translation {
                key,
                domain: domain.clone(),
                locale: locale.to_owned(),
                text: value,
                file: file.clone(),
                offset,
                line: line_of(&text, offset),
            });
        }
    }
    all.sort_by(|a, b| a.key.cmp(&b.key).then(a.locale.cmp(&b.locale)));
    all
}

/// `<trans-unit id="…" resname="key"><source>key</source><target>text</target>` (XLIFF 1.2)
/// or `<unit id="key"><segment><source>…</source><target>text</target>` (2.0).
fn from_xliff(text: &str) -> Vec<(String, Option<String>, usize)> {
    let between = |from: usize, open: &str, close: &str| -> Option<(usize, String)> {
        let start = text[from..].find(open)? + from + open.len();
        let end = text[start..].find(close)? + start;
        Some((start, text[start..end].to_owned()))
    };
    let attribute = |tag: &str, name: &str| -> Option<(usize, String)> {
        let key = format!(" {name}=\"");
        let start = tag.find(&key)? + key.len();
        let end = tag[start..].find('"')? + start;
        Some((start, tag[start..end].to_owned()))
    };
    let mut out = Vec::new();
    for unit in ["<trans-unit ", "<unit "] {
        for (at, _) in text.match_indices(unit) {
            let tag_end = text[at..].find('>').map_or(text.len(), |e| at + e);
            let tag = &text[at..tag_end];
            let close = if unit == "<unit " { "</unit>" } else { "</trans-unit>" };
            let body_end = text[at..].find(close).map_or(text.len(), |e| at + e);
            let target = between(tag_end, "<target>", "</target>")
                .filter(|(start, _)| *start < body_end)
                .map(|(_, t)| t);
            let named = attribute(tag, "resname")
                .or_else(|| (unit == "<unit ").then(|| attribute(tag, "id")).flatten())
                .map(|(start, key)| (at + start, key))
                .or_else(|| {
                    between(tag_end, "<source>", "</source>").filter(|(s, _)| *s < body_end)
                });
            if let Some((offset, key)) = named {
                out.push((key, target, offset));
            }
        }
    }
    out
}

/// The translation key named at `offset`: its range.
pub fn named_at(text: &str, offset: usize) -> Option<Range<usize>> {
    let content = quoted_at(text, offset)?;
    named(text, &content).then_some(content)
}

/// The translation keys `text` names: each one's range.
pub fn uses(text: &str) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    for quote in ['\'', '"'] {
        let mut open: Option<usize> = None;
        for (at, c) in text.char_indices() {
            if c == '\n' {
                open = None;
            } else if c == quote && !text[..at].ends_with('\\') {
                match open.take() {
                    Some(start) if start + 1 < at && named(text, &(start + 1..at)) => {
                        found.push(start + 1..at);
                    }
                    Some(_) => {}
                    None => open = Some(at),
                }
            }
        }
    }
    found.sort_by_key(|r: &Range<usize>| r.start);
    found
}

/// Whether the string at `content` is a translation key: an argument of `trans(` / `t(`
/// (not of a longer name ending so: `set(`), or followed by Twig's `|trans` (empty: still
/// being typed — completed, never a use).
fn named(text: &str, content: &Range<usize>) -> bool {
    let after = text[content.end..].get(1..).unwrap_or("").trim_start();
    if after.strip_prefix('|').is_some_and(|filter| filter.trim_start().starts_with("trans")) {
        return true;
    }
    let before = before_string(text, content);
    ["trans(", "t("].iter().any(|call| {
        before.strip_suffix(call).is_some_and(|lead| {
            !lead.ends_with(|c: char| c.is_alphanumeric() || c == '_' || c == '$')
                || lead.ends_with("->")
        })
    })
}

#[cfg(test)]
#[path = "../tests/unit/translations.rs"]
mod tests;
