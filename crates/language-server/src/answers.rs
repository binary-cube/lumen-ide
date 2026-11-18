//! The requests answered: the engines' results in LSP's shapes.

use std::collections::HashSet;
use std::io::Write;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError};

use code_intelligence::usages::{UsageQuery, matches};
use code_intelligence::{
    CompletionItem, CompletionKind, LanguageIntelligence, Location, SymbolKind, complete,
    definition, hover, signature_help,
};
use lsp_protocol::text::{Encoding, Lines, Position, range_value};
use lsp_protocol::uri;
use serde_json::{Value, json};

use crate::session::Session;

/// A place asked about.
pub(crate) struct At {
    pub(crate) path: PathBuf,
    pub(crate) text: String,
    pub(crate) offset: usize,
    pub(crate) engine: Arc<dyn LanguageIntelligence>,
    /// The file's language.
    pub(crate) language: String,
}

const PLAIN_TEXT: u8 = 1;
const SNIPPET: u8 = 2;

impl<W: Write> Session<W> {
    /// The answer to `method`; `None`: it isn't served.
    pub fn answer(&self, method: &str, params: &Value) -> Option<Value> {
        Some(match method {
            "textDocument/completion" => self.completion(params),
            "textDocument/hover" => self.hover(params),
            "textDocument/signatureHelp" => self.signature(params),
            "textDocument/definition" => self.definition(params),
            "textDocument/references" => self.references(params),
            "textDocument/documentSymbol" => self.symbols(params),
            "textDocument/prepareRename" => self.prepare_rename(params),
            "textDocument/rename" => self.rename(params),
            _ => return None,
        })
    }

    pub(crate) fn at(&self, params: &Value) -> Option<At> {
        let path = uri::to_path(params.pointer("/textDocument/uri")?.as_str()?)?;
        let open = self.open.get(&path)?;
        let position = Position::from_value(params.get("position")?)?;
        Some(At {
            offset: open.lines.offset(position, self.encoding),
            text: open.lines.text().to_owned(),
            engine: Arc::clone(self.engines.of(&path)?),
            language: self.engines.language_of(&path),
            path,
        })
    }

    pub(crate) fn range_in(&self, lines: &Lines, range: &Range<usize>) -> Value {
        let (start, end) = lines.range(range, self.encoding);
        range_value(start, end)
    }

    fn completion(&self, params: &Value) -> Value {
        let Some(at) = self.at(params) else { return Value::Null };
        let Some(open) = self.open.get(&at.path) else { return Value::Null };
        let help = self.help_completion(&at);
        let (replace, items) = match help {
            Some(own) if !own.merge => (own.replace, own.items),
            help => {
                let index = self.index.read().unwrap_or_else(PoisonError::into_inner);
                let (context, mut items) = self.with_project(|project| {
                    complete(at.engine.as_ref(), &index, &at.path, &at.text, at.offset, project)
                });
                code_intelligence::template::add_to_language(&mut items, &context.replace, help);
                (context.replace, items)
            }
        };
        let items: Vec<Value> = items
            .iter()
            .enumerate()
            .map(|(i, item)| self.completion_item(&at, &open.lines, &replace, i, item))
            .collect();
        json!({"isIncomplete": false, "items": items})
    }

    fn completion_item(
        &self,
        at: &At,
        lines: &Lines,
        replace: &Range<usize>,
        order: usize,
        item: &CompletionItem,
    ) -> Value {
        let (new_text, format) = if item.cursor_back > 0 && self.snippets {
            (with_cursor(&item.insert, item.cursor_back), SNIPPET)
        } else {
            (item.insert.clone(), PLAIN_TEXT)
        };
        let target = item.replace.as_ref().unwrap_or(replace);
        let mut value = json!({
            "label": item.label,
            "kind": completion_kind(item.kind),
            "sortText": format!("{order:05}"),
            "insertTextFormat": format,
            "textEdit": {"range": self.range_in(lines, target), "newText": new_text},
        });
        if let Some(detail) = &item.detail {
            value["detail"] = json!(detail);
        }
        let import = item.import.as_ref().and_then(|fqn| at.engine.import_edit(&at.text, fqn));
        if let Some((place, statement)) = import {
            value["additionalTextEdits"] =
                json!([{"range": self.range_in(lines, &(place..place)), "newText": statement}]);
        }
        value
    }

    fn hover(&self, params: &Value) -> Value {
        let Some(at) = self.at(params) else { return Value::Null };
        let Some(open) = self.open.get(&at.path) else { return Value::Null };
        let index = self.index.read().unwrap_or_else(PoisonError::into_inner);
        let info = self.help_hover(&at).or_else(|| {
            self.with_project(|project| {
                hover(at.engine.as_ref(), &index, &at.path, &at.text, at.offset, project)
            })
        });
        let Some(info) = info else { return Value::Null };
        let language = at.engine.language();
        let mut text = format!("```{language}\n{}\n```", info.code);
        if let Some(doc) = &info.doc {
            text.push_str("\n\n");
            text.push_str(doc);
        }
        json!({
            "contents": {"kind": "markdown", "value": text},
            "range": self.range_in(&open.lines, &info.range),
        })
    }

    fn signature(&self, params: &Value) -> Value {
        let Some(at) = self.at(params) else { return Value::Null };
        let index = self.index.read().unwrap_or_else(PoisonError::into_inner);
        let help = self.with_project(|project| {
            signature_help(at.engine.as_ref(), &index, &at.path, &at.text, at.offset, project)
        });
        let Some(help) = help else { return Value::Null };
        // LSP counts a label's offsets in UTF-16, whatever was negotiated.
        let utf16 = |to: usize| Encoding::Utf16.len(&help.code[..to]);
        let parameters: Vec<Value> =
            help.params.iter().map(|r| json!({"label": [utf16(r.start), utf16(r.end)]})).collect();
        json!({
            "signatures": [{"label": help.code, "parameters": parameters}],
            "activeSignature": 0,
            "activeParameter": help.active,
        })
    }

    pub(crate) fn definitions(&self, at: &At) -> Vec<Location> {
        let index = self.index.read().unwrap_or_else(PoisonError::into_inner);
        self.with_project(|project| {
            definition(at.engine.as_ref(), &index, &at.path, &at.text, at.offset, project)
        })
    }

    /// The length of the name declared at `location` (0 if it's no declaration known).
    fn name_len(&self, location: &Location) -> usize {
        let index = self.index.read().unwrap_or_else(PoisonError::into_inner);
        let symbols = index.file(&location.path).map(|f| f.symbols.as_slice()).unwrap_or_default();
        symbols.iter().find(|s| s.offset == location.offset).map_or(0, |s| s.name.len())
    }

    fn location(&self, path: &Path, lines: &Lines, range: &Range<usize>) -> Value {
        json!({"uri": uri::from_path(path), "range": self.range_in(lines, range)})
    }

    fn definition(&self, params: &Value) -> Value {
        let Some(at) = self.at(params) else { return Value::Null };
        let found = self.help_definition(&at).map_or_else(|| self.definitions(&at), |l| vec![l]);
        let places = found.iter().filter_map(|l| {
            let lines = Lines::new(self.text_of(&l.path)?);
            Some(self.location(&l.path, &lines, &(l.offset..l.offset + self.name_len(l))))
        });
        Value::Array(places.collect())
    }

    /// The symbol's uses in the indexed files of the languages that can use it, by the same
    /// rules as Find Usages.
    fn references(&self, params: &Value) -> Value {
        let Some(at) = self.at(params) else { return Value::Null };
        let Some(rules) = at.engine.source_rules() else { return json!([]) };
        let declarations = self.definitions(&at);
        let index = self.index.read().unwrap_or_else(PoisonError::into_inner);
        let declared = declarations.iter().find_map(|l| {
            let symbols = &index.file(&l.path)?.symbols;
            Some((l, symbols.iter().find(|s| s.offset == l.offset)?.clone()))
        });
        let Some((location, symbol)) = declared else { return json!([]) };
        let query = UsageQuery {
            name: symbol.name.to_string(),
            kind: symbol.kind,
            language: at.engine.language().to_string(),
            declared_in: Some(location.path.clone()),
            declared_line: symbol.line,
        };
        let with_declaration =
            params.pointer("/context/includeDeclaration").and_then(Value::as_bool).unwrap_or(false);
        let extensions = at.engine.usage_extensions(symbol.kind);
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        let files: Vec<PathBuf> = index
            .files()
            .map(|(p, _)| p.to_path_buf())
            .filter(|p| {
                let ext = p.extension().and_then(|e| e.to_str()).unwrap_or_default();
                extensions.is_empty() || extensions.contains(&ext)
            })
            .collect();
        drop(index);
        for path in files {
            let Some(source) = self.text_of(&path) else { continue };
            let lines = Lines::new(source);
            for start in matches(lines.text(), &query, &rules) {
                if start == location.offset && path == location.path && !with_declaration {
                    continue;
                }
                seen.insert((path.clone(), start));
                out.push(self.location(&path, &lines, &(start..start + query.name.len())));
            }
        }
        // A declaration its kind's rules don't match (`function name(`).
        if with_declaration
            && !seen.contains(&(location.path.clone(), location.offset))
            && let Some(source) = self.text_of(&location.path)
        {
            let range = location.offset..location.offset + query.name.len();
            out.push(self.location(&location.path, &Lines::new(source), &range));
        }
        Value::Array(out)
    }

    fn symbols(&self, params: &Value) -> Value {
        let path =
            params.pointer("/textDocument/uri").and_then(Value::as_str).and_then(uri::to_path);
        let Some((path, open)) = path.and_then(|p| self.open.get(&p).map(|o| (p, o))) else {
            return Value::Null;
        };
        let index = self.index.read().unwrap_or_else(PoisonError::into_inner);
        let symbols = index.file(&path).map(|f| f.symbols.as_slice()).unwrap_or_default();
        let found = symbols.iter().map(|s| {
            let mut value = json!({
                "name": s.name,
                "kind": symbol_kind(s.kind),
                "location": self.location(&path, &open.lines, &s.range),
            });
            if let Some(container) = &s.container {
                value["containerName"] = json!(container.as_ref());
            }
            value
        });
        Value::Array(found.collect())
    }
}

/// `insert` as a snippet with the cursor `back` characters from its end.
fn with_cursor(insert: &str, back: usize) -> String {
    let at = insert.char_indices().rev().nth(back - 1).map_or(0, |(i, _)| i);
    let escape = |s: &str| s.replace('\\', "\\\\").replace('$', "\\$").replace('}', "\\}");
    format!("{}$0{}", escape(&insert[..at]), escape(&insert[at..]))
}

fn completion_kind(kind: CompletionKind) -> u8 {
    match kind {
        CompletionKind::Method => 2,
        CompletionKind::Function => 3,
        CompletionKind::Variable => 6,
        CompletionKind::Class => 7,
        CompletionKind::Interface => 8,
        CompletionKind::Property | CompletionKind::Attribute => 10,
        CompletionKind::Value => 12,
        CompletionKind::Constant => 21,
        _ => 14,
    }
}

fn symbol_kind(kind: SymbolKind) -> u8 {
    match kind {
        SymbolKind::Class | SymbolKind::Trait => 5,
        SymbolKind::Method => 6,
        SymbolKind::Property => 7,
        SymbolKind::Enum => 10,
        SymbolKind::Interface => 11,
        SymbolKind::Function => 12,
        SymbolKind::Constant | SymbolKind::ClassConstant => 14,
        SymbolKind::EnumCase => 22,
        _ => 13,
    }
}

#[cfg(test)]
#[path = "../tests/unit/answers.rs"]
mod tests;
