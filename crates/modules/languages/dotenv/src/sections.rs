//! A `.env` file's sections. The generic rule: a comment above a group of keys names it
//! (blank lines part the groups; a commented-out entry names nothing). The frameworks' own
//! rules come as [`EnvSections`]: fenced blocks (Symfony Flex's `###> vendor/package ###` …
//! `###< vendor/package ###`, their keys never split further) and, in a file without them,
//! the unnamed groups named by their keys' common prefix (Laravel's `APP_`, `DB_`…). Pure:
//! the rules are given.

use std::ops::Range;

use code_intelligence::env::EnvSections;

use crate::syntax::{EnvFile, Line, LineKind};

/// A section of the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub label: String,
    /// Where its label is written (a marker's or a comment's text, the first key's prefix).
    pub label_at: Range<usize>,
    /// From its first line's start to its last line's end.
    pub range: Range<usize>,
    pub first_line: u32,
    pub last_line: u32,
    /// How many keys it holds.
    pub keys: usize,
}

/// The sections of `text` (read as `file`) by `rules`, in order.
pub fn sections(text: &str, file: &EnvFile, rules: &[EnvSections]) -> Vec<Section> {
    let mut out: Vec<Section> = rules
        .iter()
        .filter_map(|rule| match *rule {
            EnvSections::Fenced { open, close, trail } => Some((open, close, trail)),
            EnvSections::KeyPrefix => None,
        })
        .flat_map(|(open, close, trail)| fenced(text, file, open, close, trail))
        .collect();
    let fenced_lines: Vec<(u32, u32)> = out.iter().map(|s| (s.first_line, s.last_line)).collect();
    let outside =
        |line: &Line| !fenced_lines.iter().any(|&(a, b)| a <= line.first && line.last <= b);
    let by_prefix = fenced_lines.is_empty() && rules.contains(&EnvSections::KeyPrefix);
    let mut unnamed = Vec::new();
    for group in groups(file, &outside) {
        match heading(text, group) {
            Some(label_at) => out.push(section(text, group, label_at)),
            None if by_prefix => unnamed.extend(prefix_runs(text, group)),
            None => {}
        }
    }
    out.extend(join_runs(file, unnamed));
    out.sort_by_key(|s| s.range.start);
    out
}

/// The blocks between `open label trail` and `close label trail` comments.
fn fenced(text: &str, file: &EnvFile, open: &str, close: &str, trail: &str) -> Vec<Section> {
    let marker = |line: &Line, start: &str| -> Option<Range<usize>> {
        let LineKind::Comment { .. } = line.kind else { return None };
        let written = &text[line.span.clone()];
        let label = written.strip_prefix(start)?.strip_suffix(trail)?;
        let at = line.span.start + start.len();
        let trimmed = label.trim();
        let skipped = label.len() - label.trim_start().len();
        (!trimmed.is_empty()).then_some(at + skipped..at + skipped + trimmed.len())
    };
    let mut opened: Vec<(usize, Range<usize>)> = Vec::new();
    let mut out = Vec::new();
    for (i, line) in file.lines.iter().enumerate() {
        if let Some(label) = marker(line, open) {
            opened.push((i, label));
        } else if let Some(label) = marker(line, close)
            && let Some(at) =
                opened.iter().rposition(|(_, l)| text[l.clone()] == text[label.clone()])
        {
            let (first, label_at) = opened.remove(at);
            let lines = &file.lines[first..=i];
            out.push(section(text, lines, label_at));
        }
    }
    out
}

/// The runs of non-blank lines `keep` takes, in order.
fn groups<'a>(file: &'a EnvFile, keep: &dyn Fn(&Line) -> bool) -> Vec<&'a [Line]> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, line) in file.lines.iter().enumerate() {
        let part = keep(line) && line.kind != LineKind::Blank;
        match (part, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push(&file.lines[s..i]);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push(&file.lines[s..]);
    }
    out
}

/// The comment naming a group of keys: the first of the comments above its first key that
/// says something (not `#####`, not a commented-out `KEY=value`). Its text, trimmed of
/// decorations (`--- Mail ---` → `Mail`).
fn heading(text: &str, group: &[Line]) -> Option<Range<usize>> {
    let first_entry = group.iter().position(|l| matches!(l.kind, LineKind::Entry(_)))?;
    group[..first_entry].iter().find_map(|line| {
        let LineKind::Comment { text: comment } = &line.kind else { return None };
        let written = &text[comment.clone()];
        let commented_entry = EnvFile::parse(written)
            .lines
            .first()
            .is_some_and(|l| matches!(l.kind, LineKind::Entry(_)));
        let decoration = |c: char| c.is_whitespace() || "-=*#~_:<>".contains(c);
        let label = written.trim_matches(decoration);
        if label.is_empty() || commented_entry {
            return None;
        }
        let start = comment.start + written.len() - written.trim_start_matches(decoration).len();
        Some(start..start + label.len())
    })
}

/// A section over `lines`, named by what's written at `label_at`.
fn section(text: &str, lines: &[Line], label_at: Range<usize>) -> Section {
    let (first, last) = (&lines[0], &lines[lines.len() - 1]);
    Section {
        label: text[label_at.clone()].to_owned(),
        label_at,
        range: first.span.start..last.span.end,
        first_line: first.first,
        last_line: last.last,
        keys: lines.iter().filter(|l| matches!(l.kind, LineKind::Entry(_))).count(),
    }
}

/// A key's prefix: up to its first `_` (included), if something follows it.
fn prefix(text: &str, key: &Range<usize>) -> Option<Range<usize>> {
    let underscore = text[key.clone()].find('_')?;
    (underscore > 0 && underscore + 1 < key.len()).then(|| key.start..key.start + underscore + 1)
}

/// The runs of consecutive keys in `group` (the comments among them part of the run they're
/// in), each named by its keys' common prefix — or by none (`label_at` empty).
fn prefix_runs(text: &str, group: &[Line]) -> Vec<Section> {
    let mut runs: Vec<Section> = Vec::new();
    for line in group {
        let LineKind::Entry(entry) = &line.kind else { continue };
        let label_at = prefix(text, &entry.key).unwrap_or(0..0);
        match runs.last_mut() {
            Some(run) if !label_at.is_empty() && text[label_at.clone()] == run.label => {
                run.range.end = line.span.end;
                run.last_line = line.last;
                run.keys += 1;
            }
            _ => runs.push(Section {
                label: text[label_at.clone()].to_owned(),
                label_at,
                range: line.span.clone(),
                first_line: line.first,
                last_line: line.last,
                keys: 1,
            }),
        }
    }
    runs
}

/// The runs with a prefix joined across blank lines when they share it (`APP_NAME`… then
/// `APP_LOCALE`…): those of two keys or more.
fn join_runs(file: &EnvFile, runs: Vec<Section>) -> Vec<Section> {
    let only_blank_between = |a: u32, b: u32| {
        file.lines.iter().filter(|l| a < l.first && l.last < b).all(|l| l.kind == LineKind::Blank)
    };
    let mut joined: Vec<Section> = Vec::new();
    for run in runs.into_iter().filter(|r| !r.label.is_empty()) {
        match joined.last_mut() {
            Some(prev)
                if prev.label == run.label
                    && only_blank_between(prev.last_line, run.first_line) =>
            {
                prev.range.end = run.range.end;
                prev.last_line = run.last_line;
                prev.keys += run.keys;
            }
            _ => joined.push(run),
        }
    }
    joined.retain(|r| r.keys >= 2);
    joined
}

#[cfg(test)]
#[path = "../tests/unit/sections.rs"]
mod tests;
