//! A `.http` file as the Structure panel and the breadcrumb see it: its requests (by their
//! name, else their method and URL; their method and path as the detail) and its variables
//! (`@host = …`, never their values: they may be secrets).

use code_intelligence::{FileSymbols, Symbol, SymbolKind, Visibility};

/// The file's requests and variables, each over its block (the nesting comes from the
/// ranges).
pub fn symbols(text: &str) -> FileSymbols {
    let file = http_requests::parse(text);
    let line_start =
        |line: u32| -> usize { text.split_inclusive('\n').take(line as usize).map(str::len).sum() };
    let variables = file.variables.iter().map(|v| {
        let at = line_start(v.line);
        let end = text[at..].find('\n').map_or(text.len(), |n| at + n);
        symbol(v.name.clone(), SymbolKind::Variable, at..end, at, v.line, None)
    });
    let requests = file.requests.iter().map(|r| {
        let at = line_start(r.line);
        let name = r.name.clone().unwrap_or_else(|| format!("{} {}", r.method, r.url));
        let detail = r.name.as_ref().map(|_| format!("{} {}", r.method, r.url));
        symbol(name, SymbolKind::Function, r.range.clone(), at, r.line, detail)
    });
    FileSymbols {
        language: crate::ID.into(),
        symbols: variables.chain(requests).collect(),
        ..FileSymbols::default()
    }
}

fn symbol(
    name: String,
    kind: SymbolKind,
    range: std::ops::Range<usize>,
    offset: usize,
    line: u32,
    detail: Option<String>,
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
        detail: detail.map(String::into_boxed_str),
        is_static: false,
        visibility: Visibility::Public,
        note: None,
        types: None,
    }
}

#[cfg(test)]
#[path = "../tests/unit/outline.rs"]
mod tests;
