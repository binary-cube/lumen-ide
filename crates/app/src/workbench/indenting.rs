//! Indentation from the syntax tree: where a new line goes on Enter (the base of the
//! innermost block open there, `syntax_ts::IndentAnchor`) and a markup closing tag typed at
//! the start of a line going back to its element's indentation. Without an up-to-date tree,
//! the rules the language declares (`LanguageDescriptor::indent_after`, `closing_tags`),
//! then the document's text rules (`commands::newline`) decide.

use document::commands;

use super::*;

/// A line's leading spaces / tabs.
fn leading(line: &str) -> String {
    line.chars().take_while(|c| *c == ' ' || *c == '\t').collect()
}

impl Workbench {
    /// For each cursor of the active tab: the base indentation of a new line typed there
    /// (`None`: the text's rules). Only from a tree parsed at this very version.
    pub(super) fn enter_bases(&self) -> Vec<(usize, Option<String>)> {
        let Some(tab) = self.tabs.get(self.active) else { return Vec::new() };
        let doc = &tab.doc;
        let buffer = doc.buffer();
        let snapshot = tab.syntax.snapshot().filter(|s| s.version() == doc.version());
        let text = buffer.snapshot();
        doc.selections()
            .iter()
            .map(|sel| {
                let offset = sel.range().start;
                let line = buffer.line_of(offset);
                let anchor = snapshot
                    .and_then(|s| s.indent_anchor(&text, offset))
                    .map(|a| leading(&buffer.line_text(a.line)));
                // The language's own rule (YAML: after `key:` the value goes one level in).
                let declared = || {
                    let before = buffer.slice(buffer.line_start(line)..offset);
                    let language = snapshot?.language_at(offset).clone();
                    let after = self.support.languages.get(&language)?.indent_after;
                    before.trim_end().ends_with(after).then(|| leading(&before))
                };
                (offset, anchor.or_else(declared))
            })
            .collect()
    }

    /// `</` typed first on a line, in a language with closing tags: the line goes back to
    /// the indentation of the element it closes (the last one still open above: matched in
    /// the text, as a bracket is — the tree, mid-tag, can't tell).
    pub(super) fn outdent_closing_tag(&mut self, text: &mut TextSystem, typed: char) {
        if typed != '/' {
            return;
        }
        let Some(tab) = self.tabs.get(self.active) else { return };
        let head = tab.doc.selections().primary().head;
        let language = tab
            .syntax
            .snapshot()
            .map_or_else(|| tab.syntax.language().clone(), |s| s.language_at(head).clone());
        let Some(descriptor) =
            self.support.languages.get(&language).filter(|l| l.closing_tags.is_some())
        else {
            return;
        };
        let (doc, buffer) = (&tab.doc, tab.doc.buffer());
        let head = doc.selections().primary().head;
        let start = buffer.line_start(buffer.line_of(head));
        let before = buffer.slice(start..head);
        if before.trim_start() != "</" || doc.selections().len() != 1 {
            return;
        }
        let Some(open) = descriptor.unclosed_element(&buffer.slice(0..start)) else {
            return;
        };
        let base = leading(&buffer.line_text(buffer.line_of(open)));
        let current = leading(&before);
        if base == current {
            return;
        }
        self.edit(text, |doc| {
            commands::replace_range(doc, start..start + current.len(), &base);
        });
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/indenting.rs"]
mod tests;
