//! The open API for embedded languages (see docs/plan/04-module-limbaj.md, 3b).
//!
//! Besides the declarative rules in `injections.scm`, any module can contribute an
//! [`InjectionProvider`] (rules in code, e.g. the argument of `json_decode`), and
//! content sniffers (`ContentSniffer`, from the domain) recognize strings that
//! look like JSON, YAML etc.

use std::ops::Range;

use document::TextSnapshot;
use languages::LanguageId;
use tree_sitter::{Language, Node, Query, QueryCursor, QueryMatch, StreamingIterator};

use crate::text::SnapshotText;

/// Where an injection comes from; on overlap, the higher-priority source wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum InjectionSource {
    Sniffed,
    Rule,
    Declarative,
    CommentMarker,
}

/// A region of a host layer that is written in another language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Injection {
    /// Multiple ranges = fragments parsed together (e.g. the HTML in a PHP file).
    pub ranges: Vec<Range<usize>>,
    pub language: LanguageId,
    pub source: InjectionSource,
}

/// What an [`InjectionProvider`] receives.
#[derive(Debug, Clone, Copy)]
pub struct InjectionContext<'a> {
    pub host: &'a LanguageId,
    /// The host grammar (for compiling its own queries).
    pub language: &'a Language,
    pub root: Node<'a>,
    pub text: &'a TextSnapshot,
    /// Zones changed since the previous parse (`None` = the whole layer). Injections
    /// outside them are kept automatically; it is enough to look for them here.
    pub changed: Option<&'a [Range<usize>]>,
}

impl InjectionContext<'_> {
    pub fn node_text(&self, node: Node<'_>) -> String {
        self.text.slice(node.byte_range())
    }

    /// Runs `query` over the layer (only in the changed zones, if any) and calls `f`
    /// for each match. Predicates (`#eq?`, `#match?`) are evaluated.
    pub fn for_each_match(&self, query: &Query, mut f: impl FnMut(&QueryMatch<'_, '_>)) {
        let mut cursor = QueryCursor::new();
        let whole = [self.root.byte_range()];
        for range in self.changed.unwrap_or(&whole) {
            cursor.set_byte_range(range.clone());
            let mut matches = cursor.matches(query, self.root, SnapshotText(self.text));
            while let Some(m) = matches.next() {
                f(m);
            }
        }
    }
}

/// Injection rules written in code, contributed by modules.
pub trait InjectionProvider: Send + Sync {
    /// Identifier for logging / disabling (e.g. `php.json_decode`).
    fn id(&self) -> &str;
    /// The host languages it runs on.
    fn hosts(&self) -> &[LanguageId];
    fn detect(&self, ctx: &InjectionContext<'_>, out: &mut Vec<Injection>);
}

impl std::fmt::Debug for dyn InjectionProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.id())
    }
}

/// The language name from a `language=json` style text (a marker in a comment).
pub(crate) fn comment_marker(comment: &str) -> Option<String> {
    let at = comment.find("language=")?;
    let name: String = comment[at + "language=".len()..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    (!name.is_empty()).then(|| name.to_ascii_lowercase())
}

/// The normalized language name (e.g. a heredoc label `JSON` → `json`).
pub(crate) fn normalize_language(name: &str) -> LanguageId {
    let lower = name.trim().to_ascii_lowercase();
    LanguageId::new(match lower.as_str() {
        "js" => "javascript",
        "yml" => "yaml",
        "htm" => "html",
        // A code block's short names (```sh, ```ts, ```md).
        "sh" | "shell" | "zsh" | "console" => "bash",
        "ts" => "typescript",
        "md" => "markdown",
        "jsonc" | "json5" => "json",
        // A heredoc named after its database (`<<<PGSQL`, `<<<MYSQL`): one SQL language.
        "pgsql" | "psql" | "postgres" | "postgresql" | "plpgsql" | "mysql" | "sqlite" => "sql",
        other => other,
    })
}

#[cfg(test)]
#[path = "../tests/unit/injection.rs"]
mod tests;
