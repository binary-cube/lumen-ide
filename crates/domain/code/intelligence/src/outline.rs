//! A file's outline (the declaration tree) and the chain of declarations at the cursor
//! (breadcrumb: `Car › stop()`), inferred from the symbols' ranges.

use crate::symbols::{FileSymbols, Symbol, SymbolKind};

/// An outline row: the symbol's index in [`FileSymbols::symbols`] and the depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutlineItem {
    pub index: usize,
    pub depth: usize,
}

/// The symbols that appear in the outline (CSS variables and selectors have no useful
/// hierarchy, but appear as top-level rows).
fn listed(s: &Symbol) -> bool {
    !s.name.is_empty()
}

/// The declaration tree, in file order; the depth comes from the nesting of
/// ranges (a method is in the class body).
pub fn outline(file: &FileSymbols) -> Vec<OutlineItem> {
    let mut order: Vec<usize> =
        (0..file.symbols.len()).filter(|&i| listed(&file.symbols[i])).collect();
    order.sort_by_key(|&i| {
        (file.symbols[i].range.start, std::cmp::Reverse(file.symbols[i].range.end))
    });
    let mut stack: Vec<usize> = Vec::new();
    let mut out = Vec::with_capacity(order.len());
    for i in order {
        let range = &file.symbols[i].range;
        while let Some(&top) = stack.last() {
            let outer = &file.symbols[top].range;
            if outer.start <= range.start && range.end <= outer.end && outer != range {
                break;
            }
            stack.pop();
        }
        out.push(OutlineItem { index: i, depth: stack.len() });
        stack.push(i);
    }
    out
}

/// The declarations containing `offset`, from the outermost to the innermost.
pub fn scope_at(file: &FileSymbols, offset: usize) -> Vec<usize> {
    let mut chain: Vec<usize> = (0..file.symbols.len())
        .filter(|&i| {
            let s = &file.symbols[i];
            listed(s) && s.range.start <= offset && offset <= s.range.end && !s.range.is_empty()
        })
        .collect();
    chain.sort_by_key(|&i| std::cmp::Reverse(file.symbols[i].range.len()));
    chain
}

/// A declaration's label in the breadcrumb / outline: functions with `()`, a type's
/// properties with their language's `sigil` (`$name` in PHP).
pub fn label(s: &Symbol, sigil: Option<char>) -> String {
    match (s.kind, sigil) {
        (SymbolKind::Function | SymbolKind::Method | SymbolKind::Mixin, _) => {
            format!("{}()", s.name)
        }
        (SymbolKind::Property, Some(sigil))
            if s.container.is_some() && !s.name.starts_with(sigil) =>
        {
            format!("{sigil}{}", s.name)
        }
        _ => s.name.to_string(),
    }
}

#[cfg(test)]
#[path = "../tests/unit/outline.rs"]
mod tests;
