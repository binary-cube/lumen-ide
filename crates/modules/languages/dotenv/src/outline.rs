//! A `.env` file as the editor's Structure panel and breadcrumb see it: its sections
//! ([`SymbolKind::Section`]) holding its keys ([`SymbolKind::Constant`]), and its sections'
//! folding regions (`N keys`). Pure: the frameworks' rules are given.

use code_intelligence::env::EnvSections;
use code_intelligence::{FileSymbols, Symbol, SymbolKind, Visibility};
use languages::{FoldCount, FoldRange};

use crate::sections::{Section, sections};
use crate::syntax::EnvFile;

/// The file's sections and keys, a key inside its section's range (the outline's nesting
/// and the breadcrumb's chain come from the ranges: a key has no container, which is a
/// type's).
pub fn symbols(text: &str, rules: &[EnvSections]) -> FileSymbols {
    let file = EnvFile::parse(text);
    let sections = sections(text, &file, rules)
        .into_iter()
        .map(|s| symbol(s.label, SymbolKind::Section, s.range, s.label_at.start, s.first_line));
    let keys = file.entries().map(|(line, entry)| {
        let name = text[entry.key.clone()].to_owned();
        symbol(name, SymbolKind::Constant, line.span.clone(), entry.key.start, line.first)
    });
    FileSymbols {
        language: crate::ID.into(),
        symbols: sections.chain(keys).collect(),
        ..FileSymbols::default()
    }
}

fn symbol(
    name: String,
    kind: SymbolKind,
    range: std::ops::Range<usize>,
    offset: usize,
    line: u32,
) -> Symbol {
    Symbol {
        fqn: name.clone().into_boxed_str(),
        name: name.into_boxed_str(),
        kind,
        container: None,
        parents: Box::default(),
        range,
        offset,
        line,
        // Never the value: it may be a secret, and the panels show the detail.
        detail: None,
        is_static: false,
        visibility: Visibility::Public,
        note: None,
        types: None,
    }
}

/// The sections' folding regions, showing how many keys each holds.
pub fn folds(text: &str, rules: &[EnvSections]) -> Vec<FoldRange> {
    let file = EnvFile::parse(text);
    sections(text, &file, rules).iter().filter(|s| s.last_line > s.first_line).map(fold).collect()
}

fn fold(section: &Section) -> FoldRange {
    FoldRange {
        start_byte: section.range.start,
        end_byte: section.range.end,
        start_line: section.first_line,
        end_line: section.last_line,
        placeholder: Some(FoldCount::Keys(section.keys)),
    }
}

#[cfg(test)]
#[path = "../tests/unit/outline.rs"]
mod tests;
