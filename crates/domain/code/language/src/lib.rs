//! Language bounded context: which languages exist, how they are recognized and what info
//! they have (comments, folding). No tree-sitter: parsing is infrastructure's job.
//!
//! Languages come from modules (`LanguageDescriptor` is a contribution). Recognizing a
//! file is hierarchical: specific glob → exact name → extension → shebang → content
//! sniffers (`ContentSniffer`, also contributed by modules) → plain text.

pub mod shared_queries;

use std::path::Path;
use std::sync::Arc;

pub use kernel::LanguageId;

/// How a language recognizes a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileMatcher {
    /// The extension, without the dot: `"php"`.
    Extension(&'static str),
    /// The exact file name: `"composer.lock"`, `".php-cs-fixer.php"`.
    FileName(&'static str),
    /// Glob on the file name, with `*`: `"*.blade.php"`. More specific than the extension.
    Glob(&'static str),
    /// The interpreter in the shebang: `"php"` matches `#!/usr/bin/env php`.
    Shebang(&'static str),
}

/// Where the last element still open in a text starts (its `<`), as a language with closing
/// tags reads its markup.
#[derive(Clone, Copy)]
pub struct UnclosedElement(pub fn(&str) -> Option<usize>);

impl std::fmt::Debug for UnclosedElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("UnclosedElement")
    }
}

/// Descriptions compare by what they say, not by where their functions are in memory.
impl PartialEq for UnclosedElement {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for UnclosedElement {}

/// A language's description, contributed by its module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageDescriptor {
    pub id: LanguageId,
    pub name: &'static str,
    pub matchers: Vec<FileMatcher>,
    pub line_comment: Option<&'static str>,
    pub block_comment: Option<(&'static str, &'static str)>,
    /// The file icon name (e.g. `file-code`, `file-json`); the color comes from the theme,
    /// token `icon.<language id>`.
    pub icon: &'static str,
    /// Characters that are part of words (besides letters, digits and `_`), for
    /// double-click and word navigation: `$` in PHP, `-` in CSS.
    pub word_chars: &'static str,
    /// Quotes that are auto-closed when typed (JS adds `` ` ``).
    pub quotes: &'static [char],
    /// A line ending with one of these (before the cursor) puts the next one a level in,
    /// where the syntax tree doesn't say (YAML's `key:`).
    pub indent_after: &'static [char],
    /// It has markup closing tags: `</` typed first on a line takes the indentation of the
    /// element it closes (HTML and what embeds it: PHP, Blade, Twig, JSX). The language's
    /// own reading of its markup: where the last element still open in a text starts.
    pub closing_tags: Option<UnclosedElement>,
    /// What a fragment of it (a snippet in a document, a tooltip's example) needs before it
    /// to be read as this language, and what says it already has it: PHP's `<?php` unless
    /// `<?` is in it (else it reads as HTML).
    pub fragment_opening: Option<(&'static str, &'static str)>,
    /// A file of it shown in the file icons' preview (in the settings): its place among
    /// them and its name (`index.php`). `None`: not shown there.
    pub icon_sample: Option<(u8, &'static str)>,
}

impl LanguageDescriptor {
    pub fn new(id: &'static str, name: &'static str) -> Self {
        Self {
            id: LanguageId::new_static(id),
            name,
            matchers: Vec::new(),
            line_comment: None,
            block_comment: None,
            icon: "file-code",
            word_chars: "",
            quotes: &['"', '\''],
            indent_after: &[],
            closing_tags: None,
            fragment_opening: None,
            icon_sample: None,
        }
    }

    /// A file named `name` shown in the file icons' preview, at place `order`.
    pub fn icon_sample(mut self, order: u8, name: &'static str) -> Self {
        self.icon_sample = Some((order, name));
        self
    }

    /// A fragment of it needs `opening` before it unless it holds `marker`.
    pub fn fragment_opening(mut self, opening: &'static str, marker: &'static str) -> Self {
        self.fragment_opening = Some((opening, marker));
        self
    }

    /// What goes before `fragment` so it reads as this language (nothing when it needs none
    /// or has it).
    pub fn fragment_prefix(&self, fragment: &str) -> &'static str {
        match self.fragment_opening {
            Some((opening, marker)) if !fragment.contains(marker) => opening,
            _ => "",
        }
    }

    pub fn indent_after(mut self, chars: &'static [char]) -> Self {
        self.indent_after = chars;
        self
    }

    pub fn closing_tags(mut self, unclosed: fn(&str) -> Option<usize>) -> Self {
        self.closing_tags = Some(UnclosedElement(unclosed));
        self
    }

    /// Where the last element still open in `before` starts (its `<`), for a language with
    /// closing tags; `None` otherwise (or nothing open).
    pub fn unclosed_element(&self, before: &str) -> Option<usize> {
        self.closing_tags.and_then(|unclosed| (unclosed.0)(before))
    }

    pub fn quotes(mut self, quotes: &'static [char]) -> Self {
        self.quotes = quotes;
        self
    }

    pub fn word_chars(mut self, chars: &'static str) -> Self {
        self.word_chars = chars;
        self
    }

    pub fn icon(mut self, name: &'static str) -> Self {
        self.icon = name;
        self
    }

    pub fn extensions(mut self, extensions: &[&'static str]) -> Self {
        self.matchers.extend(extensions.iter().map(|e| FileMatcher::Extension(e)));
        self
    }

    pub fn file_names(mut self, names: &[&'static str]) -> Self {
        self.matchers.extend(names.iter().map(|n| FileMatcher::FileName(n)));
        self
    }

    pub fn globs(mut self, globs: &[&'static str]) -> Self {
        self.matchers.extend(globs.iter().map(|g| FileMatcher::Glob(g)));
        self
    }

    pub fn shebangs(mut self, interpreters: &[&'static str]) -> Self {
        self.matchers.extend(interpreters.iter().map(|i| FileMatcher::Shebang(i)));
        self
    }

    pub fn line_comment(mut self, token: &'static str) -> Self {
        self.line_comment = Some(token);
        self
    }

    pub fn block_comment(mut self, open: &'static str, close: &'static str) -> Self {
        self.block_comment = Some((open, close));
        self
    }
}

/// A file's icon by name / extension (e.g. `.env` → key, `*.png` → image).
/// Contributed by modules; takes priority over the language icon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileIconRule {
    pub matcher: FileMatcher,
    /// The icon name (from the icon set).
    pub icon: &'static str,
    /// The theme color key: token `icon.<color>`.
    pub color: &'static str,
}

impl FileIconRule {
    pub fn new(matcher: FileMatcher, icon: &'static str, color: &'static str) -> Self {
        Self { matcher, icon, color }
    }
}

/// A folder's color by its name, for what it holds (`src` → source, `tests` → test,
/// `vendor` → library, `config` → config). Contributed by modules; the folder keeps its icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FolderColorRule {
    /// The folder's name (any case).
    pub name: &'static str,
    /// The theme color key: token `icon.<color>`, or a role the palette knows (`source`,
    /// `test`, `library`).
    pub color: &'static str,
}

/// An SVG icon contributed by a module (registered in the app's icon set).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IconSvg {
    pub name: &'static str,
    pub svg: &'static str,
}

/// How confident a content sniffer is (0–100).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Confidence(pub u8);

/// "Does this text look like <language>?" Contributed by the language module. Must be
/// cheap: it receives only the beginning of the text, size-limited.
pub trait ContentSniffer: Send + Sync {
    fn language(&self) -> LanguageId;
    fn sniff(&self, text: &str) -> Option<Confidence>;

    /// Quick filter, without copying the text: can the text start (after whitespace) with
    /// this character? Yes by default; a sniffer that knows better avoids useless work.
    fn may_start_with(&self, _first: char) -> bool {
        true
    }
}

impl std::fmt::Debug for dyn ContentSniffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ContentSniffer({})", self.language())
    }
}

/// The threshold from which a content sniffer decides a file's language.
const SNIFF_THRESHOLD: Confidence = Confidence(60);

/// How many bytes from the start of the file the content sniffers receive.
pub const SNIFF_BYTES: usize = 4096;

/// A collapsible region (blocks, objects, elements, sections).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldRange {
    /// The node's start offset; the anchor through which a collapsed region keeps
    /// its state after edits.
    pub start_byte: usize,
    /// The node's end offset (for incremental updates).
    pub end_byte: usize,
    pub start_line: u32,
    pub end_line: u32,
    /// What is shown in place of the collapsed content (`12 keys`, `40 items`; `None`: `⋯`).
    pub placeholder: Option<FoldCount>,
}

/// How many entries a collapsed object or list holds (the interface words it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoldCount {
    /// An object's keys.
    Keys(usize),
    /// A list's items.
    Items(usize),
}

/// All known languages and content sniffers.
#[derive(Default)]
pub struct LanguageRegistry {
    languages: Vec<LanguageDescriptor>,
    sniffers: Vec<Arc<dyn ContentSniffer>>,
    icon_rules: Vec<FileIconRule>,
    folder_rules: Vec<FolderColorRule>,
}

impl std::fmt::Debug for LanguageRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LanguageRegistry")
            .field("languages", &self.languages.iter().map(|l| l.id.as_str()).collect::<Vec<_>>())
            .field("sniffers", &self.sniffers.len())
            .finish()
    }
}

impl LanguageRegistry {
    /// Registers a language; one with the same id replaces it (later modules
    /// can refine an existing language).
    pub fn register(&mut self, language: LanguageDescriptor) {
        match self.languages.iter_mut().find(|l| l.id == language.id) {
            Some(existing) => *existing = language,
            None => self.languages.push(language),
        }
    }

    pub fn register_sniffer(&mut self, sniffer: Arc<dyn ContentSniffer>) {
        self.sniffers.push(sniffer);
    }

    pub fn register_icon_rule(&mut self, rule: FileIconRule) {
        self.icon_rules.push(rule);
    }

    pub fn register_folder_rule(&mut self, rule: FolderColorRule) {
        self.folder_rules.push(rule);
    }

    /// The color key of a folder named `name` (the last rule for it wins); `None`: a plain
    /// folder.
    pub fn folder_color(&self, name: &str) -> Option<&'static str> {
        self.folder_rules.iter().rev().find(|r| r.name.eq_ignore_ascii_case(name)).map(|r| r.color)
    }

    /// (icon, color key) for a file: exact name → glob (the most specific)
    /// → extension → the language icon (color = the language id) → generic file.
    pub fn icon_for(&self, file_name: &str) -> (&'static str, String) {
        let lower = file_name.to_lowercase();
        let rule = |pred: &dyn Fn(&FileMatcher) -> bool| {
            self.icon_rules.iter().rev().find(|r| pred(&r.matcher))
        };
        let exact =
            rule(&|m| matches!(m, FileMatcher::FileName(n) if n.eq_ignore_ascii_case(file_name)));
        let glob = || {
            self.icon_rules
                .iter()
                .filter_map(|r| match r.matcher {
                    FileMatcher::Glob(g) if glob_matches(g, &lower) => Some((g.len(), r)),
                    _ => None,
                })
                .max_by_key(|(len, _)| *len)
                .map(|(_, r)| r)
        };
        let extension = || {
            let ext = Path::new(&lower).extension().and_then(|e| e.to_str())?;
            rule(&|m| matches!(m, FileMatcher::Extension(e) if *e == ext))
        };
        if let Some(r) = exact.or_else(glob).or_else(extension) {
            return (r.icon, r.color.to_owned());
        }
        let language = self.detect(Path::new(file_name), "");
        match self.get(&language) {
            Some(l) => (l.icon, language.to_string()),
            None => ("file", "file".to_owned()),
        }
    }

    pub fn get(&self, id: &LanguageId) -> Option<&LanguageDescriptor> {
        self.languages.iter().find(|l| &l.id == id)
    }

    pub fn languages(&self) -> &[LanguageDescriptor] {
        &self.languages
    }

    /// A file's language, by path and (optionally) the beginning of its content.
    pub fn detect(&self, path: &Path, head: &str) -> LanguageId {
        let name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
        let lower = name.to_lowercase();
        let find = |pred: &dyn Fn(&FileMatcher) -> bool| {
            self.languages.iter().find(|l| l.matchers.iter().any(pred)).map(|l| l.id.clone())
        };

        let extension = Path::new(&lower).extension().and_then(|e| e.to_str());
        let by_extension = extension
            .and_then(|ext| find(&|m| matches!(m, FileMatcher::Extension(e) if *e == ext)));
        // Globs are the most specific; among them the longest wins. One open at its end
        // (`.env.*`, `dockerfile.*`) names a family whose suffix is free — except another
        // language's extension: `.env.local.php` is PHP.
        let glob = self
            .languages
            .iter()
            .flat_map(|l| l.matchers.iter().map(move |m| (l, m)))
            .filter_map(|(l, m)| match m {
                FileMatcher::Glob(g) if glob_matches(g, &lower) => Some((g, l.id.clone())),
                _ => None,
            })
            .filter(|(g, id)| {
                !g.ends_with('*') || by_extension.as_ref().is_none_or(|other| other == id)
            })
            .max_by_key(|(g, _)| g.len())
            .map(|(_, id)| id);
        if let Some(id) = glob {
            return id;
        }
        if let Some(id) = find(&|m| matches!(m, FileMatcher::FileName(n) if *n == name)) {
            return id;
        }
        if let Some(id) = by_extension {
            return id;
        }
        if let Some(interpreter) = shebang(head)
            && let Some(id) = find(&|m| matches!(m, FileMatcher::Shebang(s) if *s == interpreter))
        {
            return id;
        }
        let head = &head[..head.floor_char_boundary(SNIFF_BYTES)];
        self.sniffers
            .iter()
            .filter_map(|s| s.sniff(head).map(|c| (c, s.language())))
            .filter(|(c, _)| *c >= SNIFF_THRESHOLD)
            .max_by_key(|(c, _)| *c)
            .map_or(LanguageId::PLAIN_TEXT, |(_, id)| id)
    }
}

/// The interpreter from a shebang: `#!/usr/bin/env php` → `php`, `#!/bin/bash` → `bash`.
fn shebang(head: &str) -> Option<&str> {
    let line = head.lines().next()?.strip_prefix("#!")?;
    let mut parts = line.split_whitespace();
    let program = parts.next()?;
    let program = program.rsplit('/').next()?;
    let interpreter = if program == "env" { parts.find(|p| !p.starts_with('-'))? } else { program };
    // `php8.2` → `php`, `python3` → `python`.
    Some(interpreter.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.'))
}

/// Simple glob on the name (only `*`), case-insensitive.
fn glob_matches(glob: &str, name: &str) -> bool {
    let glob = glob.to_lowercase();
    let parts: Vec<&str> = glob.split('*').collect();
    if parts.len() == 1 {
        return glob == name;
    }
    let mut rest = name;
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            match rest.strip_prefix(part) {
                Some(r) => rest = r,
                None => return false,
            }
        } else if i == parts.len() - 1 {
            return rest.ends_with(part);
        } else {
            match rest.find(part) {
                Some(at) => rest = &rest[at + part.len()..],
                None => return false,
            }
        }
    }
    true
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
