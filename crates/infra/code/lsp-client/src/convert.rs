//! A server's answers as the Intelligence ports': its positions turned into bytes of the
//! texts they're about (the documents as the server has them, the others read from disk),
//! its kinds into ours, its Markdown into the hover card's parts.

use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};

use code_intelligence::servers::{
    ChangeEdit, CodeAction, FileChange, ServedCompletion, ServedItem, ServedLocation,
    WorkspaceChange,
};
use code_intelligence::{
    CompletionItem, CompletionKind, HoverInfo, SignatureHelp, Symbol, SymbolKind,
};
use serde_json::Value;
use workbench_model::{Diagnostic, Severity};

use crate::text::{Encoding, Lines, Position, range_from_value};
use crate::{markdown, snippet, uri};

/// The texts positions are counted in: the open documents' as the server has them, the
/// others' as on disk (read once per answer).
pub trait Texts {
    /// File `path`'s text and lines.
    fn lines(&mut self, path: &Path) -> Option<Cow<'_, Lines>>;
}

/// Texts read from disk, kept while one answer is converted (line endings as the editor
/// keeps them: `\n`).
#[derive(Debug, Default)]
pub struct DiskTexts {
    read: HashMap<PathBuf, Option<Lines>>,
}

impl DiskTexts {
    pub fn read(&mut self, path: &Path) -> Option<&Lines> {
        self.read
            .entry(path.to_path_buf())
            .or_insert_with(|| {
                let text = std::fs::read_to_string(path).ok()?;
                Some(Lines::new(if text.contains('\r') {
                    text.replace("\r\n", "\n")
                } else {
                    text
                }))
            })
            .as_ref()
    }
}

/// A span of `lines` from LSP's JSON range.
fn bytes_of(lines: &Lines, range: &Value, encoding: Encoding) -> Option<Range<usize>> {
    let (start, end) = range_from_value(range)?;
    Some(lines.bytes(start, end, encoding))
}

/// LSP's completion kinds as ours.
pub fn completion_kind(kind: u64) -> CompletionKind {
    match kind {
        2 => CompletionKind::Method,
        3 | 4 => CompletionKind::Function,
        5 | 10 => CompletionKind::Property,
        6 | 18 => CompletionKind::Variable,
        7 | 22 | 25 => CompletionKind::Class,
        8 | 9 => CompletionKind::Interface,
        13 => CompletionKind::Class,
        14 | 24 => CompletionKind::Keyword,
        15 => CompletionKind::Snippet,
        20 | 21 => CompletionKind::Constant,
        12 | 11 | 16 => CompletionKind::Value,
        17 | 19 => CompletionKind::Value,
        _ => CompletionKind::Value,
    }
}

/// LSP's completion kinds by name (`CompletionItemKind`, 1–25), as a server's description
/// names them (`completion-kinds`).
pub const LSP_COMPLETION_KINDS: [&str; 25] = [
    "text",
    "method",
    "function",
    "constructor",
    "field",
    "variable",
    "class",
    "interface",
    "module",
    "property",
    "unit",
    "value",
    "enum",
    "keyword",
    "snippet",
    "color",
    "file",
    "reference",
    "folder",
    "enum-member",
    "constant",
    "struct",
    "event",
    "operator",
    "type-parameter",
];

/// Our completion kinds by name (what a description may map LSP's to).
const OUR_COMPLETION_KINDS: [(&str, CompletionKind); 17] = [
    ("class", CompletionKind::Class),
    ("interface", CompletionKind::Interface),
    ("function", CompletionKind::Function),
    ("method", CompletionKind::Method),
    ("property", CompletionKind::Property),
    ("constant", CompletionKind::Constant),
    ("variable", CompletionKind::Variable),
    ("keyword", CompletionKind::Keyword),
    ("tag", CompletionKind::Tag),
    ("attribute", CompletionKind::Attribute),
    ("value", CompletionKind::Value),
    ("selector", CompletionKind::Selector),
    ("table", CompletionKind::Table),
    ("view", CompletionKind::View),
    ("column", CompletionKind::Column),
    ("schema", CompletionKind::Schema),
    ("snippet", CompletionKind::Snippet),
];

/// How a server's completion kinds are shown: LSP's general meaning ([`completion_kind`]),
/// except where its description says otherwise (`completion-kinds`: the CSS server's
/// properties are CSS properties, shown as the built-in engine shows them; the HTML
/// server's are tags).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompletionKinds(Vec<(u64, CompletionKind)>);

impl CompletionKinds {
    /// From a description's map (LSP's kind's name → ours); an unknown name is an error.
    pub fn parse<'a>(
        map: impl IntoIterator<Item = (&'a String, &'a String)>,
    ) -> Result<Self, String> {
        let mut out = Vec::new();
        for (theirs, ours) in map {
            let Some(index) = LSP_COMPLETION_KINDS.iter().position(|k| k == theirs) else {
                return Err(format!(
                    "completion kind `{theirs}` isn't one of LSP's ({})",
                    LSP_COMPLETION_KINDS.join(", ")
                ));
            };
            let Some((_, kind)) = OUR_COMPLETION_KINDS.iter().find(|(name, _)| name == ours) else {
                let names: Vec<&str> = OUR_COMPLETION_KINDS.iter().map(|(n, _)| *n).collect();
                return Err(format!("completion kind `{ours}` isn't one of {}", names.join(", ")));
            };
            out.push((index as u64 + 1, *kind));
        }
        Ok(Self(out))
    }

    /// LSP's kind `kind` as shown.
    pub fn of(&self, kind: u64) -> CompletionKind {
        self.0.iter().find(|(k, _)| *k == kind).map_or_else(|| completion_kind(kind), |(_, c)| *c)
    }
}

/// A completion answer (a list, or the items alone) at byte `at` of `lines`. Each item's
/// `resolve` is its place in the answer (the driver keeps the items to resolve them).
pub fn completion(value: &Value, lines: &Lines, encoding: Encoding, at: usize) -> ServedCompletion {
    completion_with(value, lines, encoding, at, &CompletionKinds::default())
}

/// The same, its kinds shown as `kinds` says.
pub fn completion_with(
    value: &Value,
    lines: &Lines,
    encoding: Encoding,
    at: usize,
    kinds: &CompletionKinds,
) -> ServedCompletion {
    let (items, incomplete, defaults) = match value {
        Value::Array(items) => (items.as_slice(), false, None),
        Value::Object(list) => (
            list.get("items").and_then(Value::as_array).map_or(&[][..], Vec::as_slice),
            list.get("isIncomplete").and_then(Value::as_bool).unwrap_or(false),
            list.get("itemDefaults"),
        ),
        _ => (&[][..], false, None),
    };
    let default_range = defaults.and_then(|d| d.get("editRange")).and_then(|r| {
        // A plain range, or `{insert, replace}`.
        bytes_of(lines, r, encoding).or_else(|| bytes_of(lines, r.get("insert")?, encoding))
    });
    let default_format = defaults.and_then(|d| d.get("insertTextFormat")).and_then(Value::as_u64);
    let items = items
        .iter()
        .enumerate()
        .filter_map(|(i, item)| {
            let defaults = (default_range.clone(), default_format);
            completion_item(item, lines, encoding, at, defaults, i, kinds)
        })
        .collect();
    ServedCompletion { items, incomplete }
}

/// One suggestion.
fn completion_item(
    item: &Value,
    lines: &Lines,
    encoding: Encoding,
    at: usize,
    (default_range, default_format): (Option<Range<usize>>, Option<u64>),
    index: usize,
    kinds: &CompletionKinds,
) -> Option<ServedItem> {
    let label = item.get("label")?.as_str()?.to_owned();
    let text = |key: &str| item.get(key).and_then(Value::as_str);
    let format = item.get("insertTextFormat").and_then(Value::as_u64).or(default_format);
    let edit = item.get("textEdit");
    let (raw, range) = match edit {
        Some(edit) => {
            let range = edit.get("range").or_else(|| edit.get("insert"));
            (
                edit.get("newText").and_then(Value::as_str),
                range.and_then(|r| bytes_of(lines, r, encoding)),
            )
        }
        None => (text("textEditText").or_else(|| text("insertText")), default_range),
    };
    let raw = raw.unwrap_or(&label);
    let (insert, cursor) =
        if format == Some(2) { snippet::expand(raw) } else { (raw.to_owned(), raw.len()) };
    let cursor_back = insert[cursor..].chars().count();
    let details = item.get("labelDetails");
    let detail = text("detail")
        .map(str::to_owned)
        .or_else(|| {
            details.and_then(|d| d.get("description")).and_then(Value::as_str).map(str::to_owned)
        })
        .filter(|d| !d.is_empty());
    let edits = item
        .get("additionalTextEdits")
        .and_then(Value::as_array)
        .map(|edits| text_edits_in(edits, lines, encoding))
        .unwrap_or_default();
    // A range ending before the cursor ends at it (what was typed since is replaced too).
    let replace = range.map(|r| r.start.min(at)..r.end.max(at));
    Some(ServedItem {
        filter: text("filterText").unwrap_or(&label).to_owned(),
        sort: text("sortText").map_or_else(|| format!("{index:08}"), str::to_owned),
        item: CompletionItem {
            kind: kinds.of(item.get("kind").and_then(Value::as_u64).unwrap_or(1)),
            detail,
            insert,
            cursor_back,
            replace,
            edits,
            resolve: Some(index as u64),
            label,
            ..CompletionItem::default()
        },
    })
}

/// Text edits of one text as byte replacements, in order.
fn text_edits_in(
    edits: &[Value],
    lines: &Lines,
    encoding: Encoding,
) -> Vec<(Range<usize>, String)> {
    let mut out: Vec<(Range<usize>, String)> = edits
        .iter()
        .filter_map(|e| {
            let range = bytes_of(lines, e.get("range")?, encoding)?;
            Some((range, e.get("newText")?.as_str()?.to_owned()))
        })
        .collect();
    out.sort_by_key(|(r, _)| r.start);
    out
}

/// A resolved completion item's additions to `item` (its imports, its detail).
pub fn resolved(
    value: &Value,
    item: &CompletionItem,
    lines: &Lines,
    encoding: Encoding,
) -> CompletionItem {
    let mut out = item.clone();
    if let Some(edits) = value.get("additionalTextEdits").and_then(Value::as_array) {
        out.edits = text_edits_in(edits, lines, encoding);
    }
    if out.detail.is_none() {
        out.detail = value.get("detail").and_then(Value::as_str).map(str::to_owned);
    }
    out.resolve = None;
    out
}

/// A hover's text: Markdown, a plain string, a marked string (`{language, value}`) or a
/// list of them.
fn hover_text(contents: &Value) -> String {
    match contents {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts.iter().map(hover_text).collect::<Vec<_>>().join("\n\n"),
        Value::Object(o) => {
            let value = o.get("value").and_then(Value::as_str).unwrap_or_default();
            match o.get("language").and_then(Value::as_str) {
                Some(language) => format!("```{language}\n{value}\n```"),
                None => value.to_owned(),
            }
        }
        _ => String::new(),
    }
}

/// A hover answer as the card's: the signature, the documentation, the word it's about
/// (`word`: the editor's, when the server says no range).
pub fn hover(
    value: &Value,
    lines: &Lines,
    encoding: Encoding,
    word: Range<usize>,
) -> Option<HoverInfo> {
    let text = hover_text(value.get("contents")?);
    let (code, doc) = markdown::split(&text);
    if code.trim().is_empty() && doc.is_none() {
        return None;
    }
    let range = value.get("range").and_then(|r| bytes_of(lines, r, encoding)).unwrap_or(word);
    Some(HoverInfo { range, code, doc, ..HoverInfo::default() })
}

/// The signature being typed: its label, its parameters' ranges in it and the active one.
pub fn signature_help(value: &Value) -> Option<SignatureHelp> {
    let signatures = value.get("signatures")?.as_array()?;
    let active = value.get("activeSignature").and_then(Value::as_u64).unwrap_or(0) as usize;
    let signature = signatures.get(active).or_else(|| signatures.first())?;
    let code = signature.get("label")?.as_str()?.to_owned();
    let mut params = Vec::new();
    let mut from = 0;
    for p in signature.get("parameters").and_then(Value::as_array).into_iter().flatten() {
        let range = match p.get("label") {
            Some(Value::String(label)) => {
                let at = code[from..].find(label.as_str()).map(|i| i + from);
                at.map(|start| start..start + label.len())
            }
            // `[start, end]` in UTF-16 units of the label.
            Some(Value::Array(ends)) => {
                let unit = |i: usize| ends.get(i).and_then(Value::as_u64).map(|n| n as usize);
                match (unit(0), unit(1)) {
                    (Some(s), Some(e)) => {
                        let start = crate::text::column_to_byte(&code, s, Encoding::Utf16);
                        Some(
                            start
                                ..crate::text::column_to_byte(&code, e, Encoding::Utf16).max(start),
                        )
                    }
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some(range) = range {
            from = range.end;
            params.push(range);
        }
    }
    let active_param = signature
        .get("activeParameter")
        .or_else(|| value.get("activeParameter"))
        .and_then(Value::as_u64)
        .map(|n| n as usize)
        .filter(|&n| n < params.len().max(1));
    Some(SignatureHelp { code, params, active: active_param })
}

/// Where a URI and its range point, in the text `texts` has for it.
fn location_in(
    uri_text: &str,
    range: &Value,
    texts: &mut dyn Texts,
    encoding: Encoding,
) -> Option<ServedLocation> {
    let path = uri::to_path(uri_text)?;
    let lines = texts.lines(&path)?;
    let (start, end) = range_from_value(range)?;
    let bytes = lines.bytes(start, end, encoding);
    Some(ServedLocation { path, range: bytes, line: start.line })
}

/// A definition's or the references' answer: a location, a list of them, or links.
pub fn locations(value: &Value, texts: &mut dyn Texts, encoding: Encoding) -> Vec<ServedLocation> {
    let items: Vec<&Value> = match value {
        Value::Array(items) => items.iter().collect(),
        Value::Object(_) => vec![value],
        _ => Vec::new(),
    };
    let mut out: Vec<ServedLocation> = items
        .into_iter()
        .filter_map(|item| {
            if let Some(target) = item.get("targetUri").and_then(Value::as_str) {
                let range = item.get("targetSelectionRange").or_else(|| item.get("targetRange"))?;
                return location_in(target, range, texts, encoding);
            }
            location_in(item.get("uri")?.as_str()?, item.get("range")?, texts, encoding)
        })
        .collect();
    out.dedup();
    out
}

/// A type of a server's hierarchy (`TypeHierarchyItem`) as a place: its name, kind, its
/// `detail` as the qualified part (`App\\Models` → `App\\Models\\User`), where it is.
pub fn hierarchy_place(
    item: &Value,
    texts: &mut dyn Texts,
    encoding: Encoding,
) -> Option<code_intelligence::hierarchy::Place> {
    let name = item.get("name")?.as_str()?.to_owned();
    let kind =
        item.get("kind").and_then(Value::as_u64).and_then(symbol_kind).unwrap_or(SymbolKind::Class);
    let range = item.get("selectionRange").or_else(|| item.get("range"))?;
    let at = location_in(item.get("uri")?.as_str()?, range, texts, encoding);
    let detail = item.get("detail").and_then(Value::as_str).unwrap_or_default();
    let fqn = match detail.trim() {
        "" => name.clone(),
        d if d.ends_with(&name) => d.to_owned(),
        d => format!("{d}\\{name}"),
    };
    Some(code_intelligence::hierarchy::Place {
        fqn,
        name,
        kind,
        container: None,
        offset: at.as_ref().map_or(0, |l| l.range.start),
        line: at.as_ref().map_or(0, |l| l.line),
        path: at.map(|l| l.path),
    })
}

/// The inlay hints' answer: each one's place in the document and its label (its parts
/// joined), the parameters' ending with their `:`.
pub fn inlay_hints(
    value: &Value,
    lines: &Lines,
    encoding: Encoding,
) -> Vec<code_intelligence::inlay::InlayHint> {
    use code_intelligence::inlay::{InlayHint, InlayKind};
    let mut out: Vec<InlayHint> = value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|hint| {
            let position = crate::text::Position::from_value(hint.get("position")?)?;
            let label = match hint.get("label")? {
                Value::String(s) => s.clone(),
                Value::Array(parts) => parts
                    .iter()
                    .filter_map(|p| p.get("value").and_then(Value::as_str))
                    .collect::<String>(),
                _ => return None,
            };
            let label = label.trim().to_owned();
            if label.is_empty() {
                return None;
            }
            let kind = match hint.get("kind").and_then(Value::as_u64) {
                Some(1) => InlayKind::Type,
                _ => InlayKind::Parameter,
            };
            Some(InlayHint { offset: lines.offset(position, encoding), label, kind })
        })
        .collect();
    out.sort_by_key(|h| h.offset);
    out.dedup();
    out
}

/// A lens of the code lens answer: where (its range's start) and its title — `None` for one
/// still to be worked out (`codeLens/resolve`) or without a title.
pub fn lens(
    value: &Value,
    lines: &Lines,
    encoding: Encoding,
) -> Option<code_intelligence::servers::ServedLens> {
    let (start, _) = range_from_value(value.get("range")?)?;
    let title = value.get("command")?.get("title")?.as_str()?.trim().to_owned();
    (!title.is_empty()).then(|| code_intelligence::servers::ServedLens {
        offset: lines.offset(start, encoding),
        title,
    })
}

/// The document links' answer: the ones leading to a file (`file://`) or a web address.
pub fn document_links(
    value: &Value,
    lines: &Lines,
    encoding: Encoding,
) -> Vec<code_intelligence::links::DocumentLink> {
    use code_intelligence::links::{DocumentLink, LinkTarget};
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|link| {
            let range = bytes_of(lines, link.get("range")?, encoding)?;
            let target = link.get("target")?.as_str()?;
            let target = if target.starts_with("http://") || target.starts_with("https://") {
                LinkTarget::Url(target.to_owned())
            } else {
                // `file:///x/y.css#L12`: a line (1-based) after the path.
                let (file, fragment) = target.split_once('#').unwrap_or((target, ""));
                let line = fragment
                    .strip_prefix('L')
                    .and_then(|l| l.split([',', '-']).next()?.parse::<u32>().ok());
                LinkTarget::File {
                    path: uri::to_path(file)?,
                    line: line.map(|l| l.saturating_sub(1)),
                }
            };
            Some(DocumentLink { range, target })
        })
        .collect()
}

/// The document's highlights (the symbol's occurrences in it).
pub fn highlights(value: &Value, lines: &Lines, encoding: Encoding) -> Vec<Range<usize>> {
    let mut out: Vec<Range<usize>> = value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|h| bytes_of(lines, h.get("range")?, encoding))
        .filter(|r| !r.is_empty())
        .collect();
    out.sort_by_key(|r| r.start);
    out.dedup();
    out
}

/// LSP's symbol kinds as ours (`None`: not a declaration we list: a namespace, a key).
pub fn symbol_kind(kind: u64) -> Option<SymbolKind> {
    Some(match kind {
        5 | 23 => SymbolKind::Class,
        11 => SymbolKind::Interface,
        10 => SymbolKind::Enum,
        12 => SymbolKind::Function,
        6 | 9 => SymbolKind::Method,
        7 | 8 => SymbolKind::Property,
        14 => SymbolKind::Constant,
        13 => SymbolKind::Variable,
        22 => SymbolKind::EnumCase,
        _ => return None,
    })
}

/// The document's symbols (a tree of them, or a flat list) as declarations.
pub fn symbols(value: &Value, lines: &Lines, encoding: Encoding) -> Vec<Symbol> {
    let mut out = Vec::new();
    for item in value.as_array().into_iter().flatten() {
        symbol(item, lines, encoding, None, &mut out);
    }
    out.sort_by_key(|s| s.range.start);
    out
}

fn symbol(
    item: &Value,
    lines: &Lines,
    encoding: Encoding,
    container: Option<&str>,
    out: &mut Vec<Symbol>,
) {
    let Some(name) = item.get("name").and_then(Value::as_str) else { return };
    // A `DocumentSymbol` has its range; a `SymbolInformation`, a location.
    let range = item.get("range").or_else(|| item.get("location").and_then(|l| l.get("range")));
    let Some(range) = range.and_then(|r| bytes_of(lines, r, encoding)) else { return };
    let selection = item.get("selectionRange").and_then(|r| bytes_of(lines, r, encoding));
    let offset = selection.as_ref().map_or(range.start, |s| s.start);
    let kind = item.get("kind").and_then(Value::as_u64).and_then(symbol_kind);
    let container = container
        .map(str::to_owned)
        .or_else(|| item.get("containerName").and_then(Value::as_str).map(str::to_owned))
        .filter(|c| !c.is_empty());
    if let Some(kind) = kind {
        out.push(Symbol {
            name: name.to_owned().into_boxed_str(),
            kind,
            fqn: (container.as_ref().map_or_else(|| name.to_owned(), |c| format!("{c}.{name}")))
                .into_boxed_str(),
            container: container.as_deref().map(Into::into),
            parents: Box::default(),
            range,
            offset,
            line: lines.position(offset, encoding).line,
            detail: (item
                .get("detail")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .filter(|d| !d.is_empty()))
            .map(String::into_boxed_str),
            is_static: false,
            visibility: code_intelligence::Visibility::Public,
            note: None,
            types: None,
        });
    }
    // Members are inside their type; a namespace's are its own top level.
    let inner =
        if kind.is_some_and(SymbolKind::is_type) { Some(name) } else { container.as_deref() };
    for child in item.get("children").and_then(Value::as_array).into_iter().flatten() {
        symbol(child, lines, encoding, inner, out);
    }
}

/// A file's edits from the server (a formatting's) as a change of `path`, `lines` its text.
pub fn file_change(path: &Path, edits: &Value, lines: &Lines, encoding: Encoding) -> FileChange {
    let edits = edits
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let range = bytes_of(lines, e.get("range")?, encoding)?;
            let text = e.get("newText")?.as_str()?.to_owned();
            Some(ChangeEdit {
                original: lines.text()[range.clone()].to_owned(),
                line: lines.position(range.start, encoding).line,
                range,
                text,
            })
        })
        .collect::<Vec<_>>();
    let mut edits = edits;
    edits.sort_by_key(|e| e.range.start);
    // Overlapping ones (a bad answer) can't be applied: the later is dropped.
    let mut kept: Vec<ChangeEdit> = Vec::with_capacity(edits.len());
    for edit in edits {
        if kept.last().is_none_or(|last| last.range.end <= edit.range.start) {
            kept.push(edit);
        }
    }
    FileChange { path: path.to_path_buf(), edits: kept }
}

/// A workspace edit (`changes` by URI, or `documentChanges` with renames) as our change.
pub fn workspace_edit(value: &Value, texts: &mut dyn Texts, encoding: Encoding) -> WorkspaceChange {
    let mut change = WorkspaceChange::default();
    let mut add = |uri_text: &str, edits: &Value, texts: &mut dyn Texts| {
        let Some(path) = uri::to_path(uri_text) else { return };
        let Some(lines) = texts.lines(&path) else { return };
        let file = file_change(&path, edits, &lines, encoding);
        if !file.edits.is_empty() {
            change.files.push(file);
        }
    };
    if let Some(documents) = value.get("documentChanges").and_then(Value::as_array) {
        let mut renames = Vec::new();
        for document in documents {
            match document.get("kind").and_then(Value::as_str) {
                Some("rename") => {
                    let path = |key: &str| {
                        document.get(key).and_then(Value::as_str).and_then(uri::to_path)
                    };
                    if let (Some(from), Some(to)) = (path("oldUri"), path("newUri")) {
                        renames.push((from, to));
                    }
                }
                Some(_) => {}
                None => {
                    if let (Some(uri_text), Some(edits)) = (
                        document
                            .get("textDocument")
                            .and_then(|d| d.get("uri"))
                            .and_then(Value::as_str),
                        document.get("edits"),
                    ) {
                        add(uri_text, edits, texts);
                    }
                }
            }
        }
        change.renames = renames;
    } else if let Some(changes) = value.get("changes").and_then(Value::as_object) {
        for (uri_text, edits) in changes {
            add(uri_text, edits, texts);
        }
    }
    change
}

/// A diagnostic's severity as ours (a hint is a note).
fn severity(value: Option<u64>) -> Severity {
    match value {
        Some(1) | None => Severity::Error,
        Some(2) => Severity::Warning,
        _ => Severity::Info,
    }
}

/// The characters (not bytes, not units) before `position` on its line.
fn chars_at(lines: &Lines, position: Position, encoding: Encoding) -> u32 {
    let line = lines.line(position.line as usize);
    let byte = crate::text::column_to_byte(line, position.character as usize, encoding);
    u32::try_from(line[..byte].chars().count()).unwrap_or(u32::MAX)
}

/// A file's published diagnostics as the editor's (`relative`: its path in the project;
/// `server`: who found them, when they don't say).
pub fn diagnostics(
    items: &Value,
    relative: &Path,
    lines: &Lines,
    encoding: Encoding,
    server: &str,
) -> Vec<Diagnostic> {
    items
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|d| {
            let (start, end) = range_from_value(d.get("range")?)?;
            let rule = match d.get("code") {
                Some(Value::String(s)) => Some(s.clone()),
                Some(Value::Number(n)) => Some(n.to_string()),
                _ => None,
            };
            Some(Diagnostic {
                path: relative.to_path_buf(),
                line: start.line,
                column: Some(chars_at(lines, start, encoding)),
                end: Some((end.line, chars_at(lines, end, encoding))),
                severity: severity(d.get("severity").and_then(Value::as_u64)),
                message: d.get("message")?.as_str()?.to_owned(),
                source: d.get("source").and_then(Value::as_str).unwrap_or(server).to_owned(),
                rule,
            })
        })
        .collect()
}

/// The code actions offered (commands and actions), with what runs each (the item itself).
pub fn code_actions(value: &Value) -> Vec<(CodeAction, Value)> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter(|a| a.get("disabled").is_none())
        .enumerate()
        .filter_map(|(i, a)| {
            Some((
                CodeAction {
                    title: a.get("title")?.as_str()?.to_owned(),
                    kind: a.get("kind").and_then(Value::as_str).map(str::to_owned),
                    preferred: a.get("isPreferred").and_then(Value::as_bool).unwrap_or(false),
                    id: i as u64,
                },
                a.clone(),
            ))
        })
        .collect()
}

#[cfg(test)]
#[path = "../tests/unit/convert.rs"]
mod tests;
