//! `textDocument/rename`: only the places the navigation says are the renamed symbol change (a
//! client has nowhere to ask about the unsure ones), never in the project's dependencies.

use std::collections::HashMap;
use std::io::Write;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError};

use code_intelligence::rename::{Match, Subject, subject_at};
use code_intelligence::usages::{UsageQuery, matches};
use code_intelligence::{Location, SourceRules, SymbolKind};
use lsp_protocol::text::Lines;
use lsp_protocol::uri;
use refactor::{NameStyle, find_word};
use serde_json::{Value, json};

use crate::answers::At;
use crate::session::Session;

impl<W: Write> Session<W> {
    /// What the cursor would rename: its range, or nothing to rename there.
    pub(crate) fn prepare_rename(&self, params: &Value) -> Value {
        let Some(at) = self.at(params) else { return Value::Null };
        let Some(rules) = at.engine.source_rules() else { return Value::Null };
        let Some(subject) = self.subject(&at, &rules) else { return Value::Null };
        if self.in_dependencies(&subject.declared.at.path) {
            return Value::Null;
        }
        let word = word_at(&at.text, at.offset, &subject.name, rules.naming.sigil);
        let lines = Lines::new(at.text);
        word.map_or(Value::Null, |range| self.range_in(&lines, &range))
    }

    /// The changes renaming what's at the cursor to `newName`.
    pub(crate) fn rename(&self, params: &Value) -> Value {
        let Some(wanted) = params.get("newName").and_then(Value::as_str) else {
            return Value::Null;
        };
        let Some(at) = self.at(params) else { return Value::Null };
        let Some(rules) = at.engine.source_rules() else { return Value::Null };
        let Some(subject) = self.subject(&at, &rules) else { return Value::Null };
        if self.in_dependencies(&subject.declared.at.path) {
            return Value::Null;
        }
        let sigil = rules.naming.sigil;
        let Some(new_name) = spelled_like(&subject.name, wanted, sigil) else {
            return Value::Null;
        };
        let places = match subject.declared.kind {
            Some(kind) if subject.local.is_none() => {
                self.project_places(&at, &rules, &subject, kind)
            }
            _ => {
                let scope = subject.local.clone().unwrap_or(0..at.text.len());
                let found = local_places(&at.text, &subject.name, sigil, scope);
                found.into_iter().map(|range| (at.path.clone(), range)).collect()
            }
        };
        let mut changes: HashMap<String, Vec<Value>> = HashMap::new();
        let mut by_file: HashMap<PathBuf, Vec<Range<usize>>> = HashMap::new();
        for (path, range) in places {
            by_file.entry(path).or_default().push(range);
        }
        for (path, ranges) in by_file {
            let Some(source) = self.text_of(&path) else { continue };
            let lines = Lines::new(source);
            let edits = ranges
                .iter()
                .map(|r| json!({"range": self.range_in(&lines, r), "newText": new_name}));
            changes.insert(uri::from_path(&path), edits.collect());
        }
        json!({ "changes": changes })
    }

    fn subject(&self, at: &At, rules: &SourceRules) -> Option<Subject> {
        let definition = self.definitions(at).into_iter().next();
        let index = self.index.read().unwrap_or_else(PoisonError::into_inner);
        let naming = rules.naming;
        let language: Arc<str> = at.engine.language().as_str().into();
        subject_at(&index, &at.path, &at.text, at.offset, language, definition, &|_| naming)
    }

    /// Inside a folder of dependencies of the project (`vendor`, `node_modules`).
    fn in_dependencies(&self, path: &Path) -> bool {
        let relative = self.root.as_deref().and_then(|root| path.strip_prefix(root).ok());
        relative.is_none_or(|relative| {
            relative
                .components()
                .any(|c| c.as_os_str().to_str().is_some_and(kernel::paths::is_dependency_dir))
        })
    }

    /// The places of the project that are the symbol, its declaration among them.
    fn project_places(
        &self,
        at: &At,
        rules: &SourceRules,
        subject: &Subject,
        kind: SymbolKind,
    ) -> Vec<(PathBuf, Range<usize>)> {
        let declared = &subject.declared;
        let query = UsageQuery {
            name: subject.name.clone(),
            kind,
            language: at.engine.language().to_string(),
            declared_in: Some(declared.at.path.clone()),
            declared_line: declared.at.line,
        };
        let extensions = at.engine.usage_extensions(kind);
        let files: Vec<PathBuf> = {
            let index = self.index.read().unwrap_or_else(PoisonError::into_inner);
            index.files().map(|(p, _)| p.to_path_buf()).collect()
        };
        let mut places = Vec::new();
        for path in files.into_iter().filter(|p| {
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or_default();
            (extensions.is_empty() || extensions.contains(&ext)) && !self.in_dependencies(p)
        }) {
            let Some(text) = self.text_of(&path) else { continue };
            for start in matches(&text, &query, rules) {
                let place = At {
                    path: path.clone(),
                    text: text.clone(),
                    offset: start,
                    engine: Arc::clone(&at.engine),
                    language: at.language.clone(),
                };
                if self.is_subject(subject, &place) {
                    places.push((path.clone(), start..start + query.name.len()));
                }
            }
        }
        let name = declared.at.offset..declared.at.offset + query.name.len();
        let declaration = (declared.at.path.clone(), name);
        if !places.contains(&declaration) {
            places.push(declaration);
        }
        places
    }

    fn is_subject(&self, subject: &Subject, place: &At) -> bool {
        let here = Location { path: place.path.clone(), offset: place.offset, line: 0 };
        if subject.declared.is(&here) {
            return true;
        }
        let found = self.definitions(place);
        let index = self.index.read().unwrap_or_else(PoisonError::into_inner);
        subject.declared.by_definitions_in(&found, &index) == Match::Target
    }
}

/// The name `wanted`, with or without its sigil as `current` has it. `None`: not a name.
fn spelled_like(current: &str, wanted: &str, sigil: Option<char>) -> Option<String> {
    let bare = sigil.map_or(wanted, |s| wanted.strip_prefix(s).unwrap_or(wanted));
    let valid = !bare.is_empty() && bare.chars().all(|c| c.is_alphanumeric() || c == '_');
    let starts_digit = bare.chars().next().is_some_and(char::is_numeric);
    if !valid || starts_digit {
        return None;
    }
    Some(match sigil.filter(|s| current.starts_with(*s)) {
        Some(sigil) => format!("{sigil}{bare}"),
        None => bare.to_owned(),
    })
}

/// Where `name` is written inside `scope` of `text`.
fn local_places(
    text: &str,
    name: &str,
    sigil: Option<char>,
    scope: Range<usize>,
) -> Vec<Range<usize>> {
    let style = NameStyle::CODE.with_sigil(sigil);
    let inside = |r: &Range<usize>| scope.start <= r.start && r.end <= scope.end;
    find_word(text, name, style).into_iter().filter(inside).collect()
}

/// The written `name` that holds `offset`.
fn word_at(text: &str, offset: usize, name: &str, sigil: Option<char>) -> Option<Range<usize>> {
    let style = NameStyle::CODE.with_sigil(sigil);
    find_word(text, name, style).into_iter().find(|r| r.start <= offset && offset <= r.end)
}

#[cfg(test)]
#[path = "../tests/unit/rename.rs"]
mod tests;
