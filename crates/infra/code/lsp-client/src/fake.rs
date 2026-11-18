//! A small language server for tests and the app's snapshots: it speaks LSP on two streams
//! (in memory, or a process's stdin and stdout), keeps the documents it's sent — applying
//! their changes with its own position counting, not the client's — and answers from
//! their words: completion (a member list after `.`, the document's names), hover,
//! signature help, definition, references, highlights, rename, symbols, formatting (trailing
//! spaces), a code action, implementations (a word's later occurrences), the type (its last
//! one), a type hierarchy (`Base` above, `Child` below), inlay hints (`n:` after `(` before a
//! number), code lenses (on each `function`, the later ones to resolve) and document links
//! (`https://…`); it reports a warning on each `TODO` and an error on each
//! `FIXME`. What it does is told by a [`Behaviour`] (slow, hung, crashing on a method, its
//! encoding) and what it got is kept in a [`Seen`] for the tests to look at.

use std::collections::{BTreeMap, HashMap};
use std::io::{self, BufReader, Read, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use crate::jsonrpc::{self, Id, Message, ResponseError, codes};
use crate::launch::{Connection, FnLauncher};

/// How the fake server behaves.
#[derive(Debug, Clone, Default)]
pub struct Behaviour {
    /// Each answer waits this long first.
    pub delay: Duration,
    /// After `initialize`, it stops reading anything (a server stuck in its work).
    pub hang: bool,
    /// Its diagnostics are asked for (`textDocument/diagnostic`), never pushed.
    pub pull: bool,
    /// It ends (a crash) when it gets this method.
    pub crash_on: Option<String>,
    /// It counts columns in UTF-8 bytes (else UTF-16 units, LSP's default).
    pub utf8: bool,
    /// It syncs whole documents (else incremental changes).
    pub full_sync: bool,
    /// It reports progress (`Indexing`) after `initialize`.
    pub progress: bool,
    /// It suggests nothing (an empty list for every completion).
    pub silent_completion: bool,
    /// The methods it answers with an error (`textDocument/hover`…).
    pub failing: Vec<String>,
    /// The methods it answers with nothing (`null`).
    pub empty: Vec<String>,
    /// The methods it never answers (the client's timeout ends them).
    pub unanswered: Vec<String>,
    /// The methods it answers only after this long (a server busy elsewhere).
    pub slow: Vec<(String, Duration)>,
    /// The URLs it asks the client for once initialized (`vscode/content`, as the JSON
    /// server asks for its schemas).
    pub contents: Vec<String>,
    /// A message it asks to be shown once initialized (`window/showMessage`: its type, its
    /// text), echoing `{options}` with its initialization options (a licence's activation).
    pub message: Option<(u8, String)>,
    /// The capabilities it doesn't have (`renameProvider`: intelephense without a licence).
    pub without: Vec<String>,
}

/// What the fake server got.
#[derive(Debug, Default)]
pub struct Seen {
    /// Every method, in order (requests and notifications).
    pub methods: Vec<String>,
    /// The requests cancelled (`$/cancelRequest`'s ids).
    pub cancelled: Vec<i64>,
    /// The documents' texts as it has them, by URI.
    pub documents: BTreeMap<String, String>,
    /// The changes it got, one per `didChange`.
    pub changes: usize,
    /// `workspace/didChangeWatchedFiles`' URIs.
    pub watched: Vec<String>,
    /// The settings it was given last.
    pub settings: Value,
    /// Its `initialize` parameters.
    pub initialize: Value,
    /// What the client answered to its `workspace/configuration` (for section `fake`).
    pub configuration: Value,
    /// Whether the client applied the edit it asked for (`workspace/applyEdit`).
    pub applied: Option<bool>,
    /// The client's answers to its `vscode/content` requests, by URL (`Err`: refused).
    pub contents: BTreeMap<String, Result<String, String>>,
}

impl Seen {
    pub fn count(&self, method: &str) -> usize {
        self.methods.iter().filter(|m| *m == method).count()
    }
}

/// A launcher starting a fake server in memory each time (on its own thread), counting its
/// launches; `behaviour(n)` says how the `n`th (from 0) behaves.
pub fn launcher(
    seen: Arc<Mutex<Seen>>,
    launches: Arc<Mutex<usize>>,
    behaviour: impl Fn(usize) -> Behaviour + Send + 'static,
) -> FnLauncher {
    FnLauncher(Box::new(move || {
        let n = {
            let mut l = launches.lock().map_err(|_| io::Error::other("poisoned"))?;
            *l += 1;
            *l - 1
        };
        // What the client writes, the server reads; and the other way round.
        let (from_client, server_input) = io::pipe()?;
        let (to_client, server_output) = io::pipe()?;
        let seen = Arc::clone(&seen);
        let behaviour = behaviour(n);
        std::thread::Builder::new().name("fake-language-server".into()).spawn(move || {
            serve(from_client, server_output, &behaviour, &seen);
        })?;
        Ok(Connection {
            input: Box::new(server_input),
            output: Box::new(to_client),
            errors: None,
            pid: None,
            keep: None,
        })
    }))
}

/// Serves LSP on `input` / `output` until `exit`, the end of `input` or a crash.
pub fn serve(input: impl Read, mut output: impl Write, behaviour: &Behaviour, seen: &Mutex<Seen>) {
    let mut reader = BufReader::new(input);
    let mut server = Server { docs: HashMap::new(), utf8: behaviour.utf8 };
    let mut hung = false;
    loop {
        if hung {
            // Stuck: reads nothing more, answers nothing (until the process is killed).
            std::thread::sleep(Duration::from_secs(3600));
            continue;
        }
        let Ok(Some(message)) = jsonrpc::read_message(&mut reader) else { return };
        let method = match &message {
            Message::Request { method, .. } | Message::Notification { method, .. } => {
                method.clone()
            }
            Message::Response { id, result } => {
                if let Ok(mut s) = seen.lock() {
                    match (id, result) {
                        (Id::Text(t), Ok(v)) if t == "configuration" => s.configuration = v.clone(),
                        (Id::Text(t), Ok(v)) if t == "apply" => s.applied = v["applied"].as_bool(),
                        (Id::Text(t), answer) if t.starts_with("content:") => {
                            let answer = match answer {
                                Ok(v) => Ok(v.as_str().unwrap_or_default().to_owned()),
                                Err(e) => Err(e.message.clone()),
                            };
                            s.contents.insert(t["content:".len()..].to_owned(), answer);
                        }
                        _ => {}
                    }
                }
                continue;
            }
        };
        if let Ok(mut s) = seen.lock() {
            s.methods.push(method.clone());
        }
        if behaviour.crash_on.as_deref() == Some(method.as_str()) {
            return;
        }
        match message {
            Message::Request { id, method, params } => {
                if !behaviour.delay.is_zero() {
                    std::thread::sleep(behaviour.delay);
                }
                if method == "initialize" {
                    if let Ok(mut s) = seen.lock() {
                        s.initialize = params.clone();
                    }
                    let result = initialize_result(behaviour);
                    let _ = send(&mut output, Message::Response { id, result: Ok(result) });
                    if behaviour.progress {
                        let _ = send(
                            &mut output,
                            notification(
                                "$/progress",
                                json!({"token": "index", "value": {"kind": "begin", "title": "Indexing", "percentage": 40}}),
                            ),
                        );
                    }
                    if let Some((kind, text)) = &behaviour.message {
                        let options =
                            params.get("initializationOptions").cloned().unwrap_or_default();
                        let text = text.replace("{options}", &options.to_string());
                        let _ = send(
                            &mut output,
                            notification(
                                "window/showMessage",
                                json!({"type": kind, "message": text}),
                            ),
                        );
                    }
                    hung = behaviour.hang;
                    continue;
                }
                if behaviour.unanswered.contains(&method) {
                    continue;
                }
                if let Some((_, wait)) = behaviour.slow.iter().find(|(m, _)| *m == method) {
                    std::thread::sleep(*wait);
                }
                let result = if behaviour.silent_completion && method == "textDocument/completion" {
                    Ok(json!({"isIncomplete": false, "items": []}))
                } else if behaviour.failing.contains(&method) {
                    Err(ResponseError { code: codes::INTERNAL_ERROR, message: "it failed".into() })
                } else if behaviour.empty.contains(&method) {
                    Ok(Value::Null)
                } else {
                    server.request(&method, &params)
                };
                let _ = send(&mut output, Message::Response { id, result });
                if method == "workspace/executeCommand"
                    && let Some(uri) = server.docs.keys().min().cloned()
                {
                    let zero = json!({"line": 0, "character": 0});
                    let edit = json!({"changes": {uri: [{"range": {"start": zero, "end": zero}, "newText": "// done\n"}]}});
                    let request = Message::Request {
                        id: Id::Text("apply".into()),
                        method: "workspace/applyEdit".into(),
                        params: json!({"label": "Run a command", "edit": edit}),
                    };
                    let _ = send(&mut output, request);
                }
            }
            Message::Notification { method, params } => {
                match method.as_str() {
                    "exit" => return,
                    "initialized" => {
                        let request = Message::Request {
                            id: Id::Text("configuration".into()),
                            method: "workspace/configuration".into(),
                            params: json!({"items": [{"section": "fake"}, {"section": "missing.key"}]}),
                        };
                        let _ = send(&mut output, request);
                        for url in &behaviour.contents {
                            let request = Message::Request {
                                id: Id::Text(format!("content:{url}")),
                                method: "vscode/content".into(),
                                params: json!([url]),
                            };
                            let _ = send(&mut output, request);
                        }
                    }
                    "$/cancelRequest" => {
                        if let (Ok(mut s), Some(id)) =
                            (seen.lock(), params.get("id").and_then(Value::as_i64))
                        {
                            s.cancelled.push(id);
                        }
                    }
                    "workspace/didChangeConfiguration" => {
                        if let Ok(mut s) = seen.lock() {
                            s.settings = params.get("settings").cloned().unwrap_or(Value::Null);
                        }
                    }
                    "workspace/didChangeWatchedFiles" => {
                        if let Ok(mut s) = seen.lock() {
                            let uris = params["changes"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(|c| c["uri"].as_str().map(str::to_owned));
                            s.watched.extend(uris);
                        }
                    }
                    _ => {}
                }
                if let Some(uri) = server.notification(&method, &params) {
                    if let Ok(mut s) = seen.lock() {
                        if method == "textDocument/didChange" {
                            s.changes += 1;
                        }
                        match server.docs.get(&uri) {
                            Some(text) => s.documents.insert(uri.clone(), text.clone()),
                            None => s.documents.remove(&uri),
                        };
                    }
                    if !behaviour.pull
                        && let Some(diagnostics) = server.diagnostics(&uri)
                    {
                        let params = json!({"uri": uri, "diagnostics": diagnostics});
                        let _ = send(
                            &mut output,
                            notification("textDocument/publishDiagnostics", params),
                        );
                    }
                }
            }
            Message::Response { .. } => {}
        }
    }
}

fn notification(method: &str, params: Value) -> Message {
    Message::Notification { method: method.into(), params }
}

fn send(output: &mut impl Write, message: Message) -> io::Result<usize> {
    jsonrpc::write_message(output, &message.to_value())
}

fn initialize_result(behaviour: &Behaviour) -> Value {
    let mut result = full_capabilities(behaviour);
    if let Some(capabilities) = result.get_mut("capabilities").and_then(Value::as_object_mut) {
        for key in &behaviour.without {
            capabilities.remove(key);
        }
        if behaviour.pull {
            capabilities
                .insert("diagnosticProvider".into(), json!({"interFileDependencies": false}));
        }
    }
    result
}

fn full_capabilities(behaviour: &Behaviour) -> Value {
    json!({
        "capabilities": {
            "positionEncoding": if behaviour.utf8 { "utf-8" } else { "utf-16" },
            "textDocumentSync": {"openClose": true, "change": if behaviour.full_sync { 1 } else { 2 }, "save": {"includeText": false}},
            "completionProvider": {"triggerCharacters": ["."], "resolveProvider": true},
            "hoverProvider": true,
            "signatureHelpProvider": {"triggerCharacters": ["(", ","]},
            "definitionProvider": true,
            "referencesProvider": true,
            "documentHighlightProvider": true,
            "documentSymbolProvider": true,
            "documentFormattingProvider": true,
            "renameProvider": {"prepareProvider": true},
            "codeActionProvider": {"resolveProvider": false},
            "implementationProvider": true,
            "typeDefinitionProvider": true,
            "typeHierarchyProvider": true,
            "inlayHintProvider": true,
            "codeLensProvider": {"resolveProvider": true},
            "documentLinkProvider": {"resolveProvider": false},
        },
        "serverInfo": {"name": "fake", "version": "1"},
    })
}

/// The fake server's documents.
struct Server {
    docs: HashMap<String, String>,
    utf8: bool,
}

/// A name's characters.
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

impl Server {
    /// Byte `offset` of `text` as a position, counted here (not with the client's code).
    fn position(&self, text: &str, offset: usize) -> Value {
        let before = &text[..offset];
        let line = before.matches('\n').count();
        let start = before.rfind('\n').map_or(0, |i| i + 1);
        let column: usize =
            if self.utf8 { offset - start } else { before[start..].encode_utf16().count() };
        json!({"line": line, "character": column})
    }

    /// A position as a byte offset of `text` (clamped).
    fn offset(&self, text: &str, position: &Value) -> usize {
        let line = position["line"].as_u64().unwrap_or(0) as usize;
        let column = position["character"].as_u64().unwrap_or(0) as usize;
        let mut start = 0;
        for _ in 0..line {
            match text[start..].find('\n') {
                Some(i) => start += i + 1,
                None => return text.len(),
            }
        }
        let end = text[start..].find('\n').map_or(text.len(), |i| start + i);
        let mut units = 0;
        for (i, c) in text[start..end].char_indices() {
            if units >= column {
                return start + i;
            }
            units += if self.utf8 { c.len_utf8() } else { c.len_utf16() };
        }
        end
    }

    fn range(&self, text: &str, start: usize, end: usize) -> Value {
        json!({"start": self.position(text, start), "end": self.position(text, end)})
    }

    /// A notification about a document: the URI whose text changed.
    fn notification(&mut self, method: &str, params: &Value) -> Option<String> {
        let uri = params["textDocument"]["uri"].as_str()?.to_owned();
        match method {
            "textDocument/didOpen" => {
                self.docs.insert(uri.clone(), params["textDocument"]["text"].as_str()?.to_owned());
            }
            "textDocument/didChange" => {
                let mut text = self.docs.get(&uri)?.clone();
                for change in params["contentChanges"].as_array()? {
                    let new = change["text"].as_str()?;
                    match change.get("range") {
                        Some(range) => {
                            let start = self.offset(&text, &range["start"]);
                            let end = self.offset(&text, &range["end"]).max(start);
                            text.replace_range(start..end, new);
                        }
                        None => text = new.to_owned(),
                    }
                }
                self.docs.insert(uri.clone(), text);
            }
            "textDocument/didClose" => {
                self.docs.remove(&uri);
            }
            _ => return None,
        }
        Some(uri)
    }

    /// A warning on each `TODO`, an error on each `FIXME` (none for a closed document).
    fn diagnostics(&self, uri: &str) -> Option<Value> {
        let Some(text) = self.docs.get(uri) else { return Some(json!([])) };
        let mut out = Vec::new();
        for (word, severity, message) in
            [("TODO", 2, "A task is left here."), ("FIXME", 1, "Something is broken here.")]
        {
            for (at, _) in text.match_indices(word) {
                out.push(json!({"range": self.range(text, at, at + word.len()), "severity": severity, "message": message, "source": "fake", "code": word.to_lowercase()}));
            }
        }
        Some(Value::Array(out))
    }

    /// The word around byte `offset` of `text`.
    fn word(text: &str, offset: usize) -> Option<(usize, usize)> {
        let start = text[..offset]
            .char_indices()
            .rev()
            .take_while(|(_, c)| is_word(*c))
            .last()
            .map_or(offset, |(i, _)| i);
        let end =
            offset + text[offset..].find(|c: char| !is_word(c)).unwrap_or(text.len() - offset);
        (start < end).then_some((start, end))
    }

    /// Every whole-word occurrence of `word` in `text`.
    fn occurrences(text: &str, word: &str) -> Vec<(usize, usize)> {
        text.match_indices(word)
            .filter(|(at, _)| {
                let before = text[..*at].chars().next_back();
                let after = text[at + word.len()..].chars().next();
                !before.is_some_and(is_word) && !after.is_some_and(is_word)
            })
            .map(|(at, _)| (at, at + word.len()))
            .collect()
    }

    fn request(&mut self, method: &str, params: &Value) -> Result<Value, ResponseError> {
        let missing =
            || ResponseError { code: codes::INVALID_PARAMS, message: "unknown document".into() };
        if method == "shutdown" {
            return Ok(Value::Null);
        }
        if method == "completionItem/resolve" {
            let mut item = params.clone();
            if item["label"] == "readFile" {
                item["additionalTextEdits"] = json!([{"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}}, "newText": "import { readFile } from 'fs';\n"}]);
                item["detail"] = json!("function readFile(path: string): string");
            }
            return Ok(item);
        }
        if method == "workspace/executeCommand" {
            return Ok(Value::Null);
        }
        // A type's supertype is `Base`, its subtype `Child` (each once: theirs are none).
        if let Some(up) = match method {
            "typeHierarchy/supertypes" => Some(true),
            "typeHierarchy/subtypes" => Some(false),
            _ => None,
        } {
            let item = &params["item"];
            let name = item["name"].as_str().unwrap_or_default();
            if matches!(name, "Base" | "Child") {
                return Ok(json!([]));
            }
            let mut other = item.clone();
            other["name"] = json!(if up { "Base" } else { "Child" });
            return Ok(json!([other]));
        }
        if method == "codeLens/resolve" {
            let mut lens = params.clone();
            lens["command"] = json!({"title": "resolved", "command": "fake.lens"});
            return Ok(lens);
        }
        let uri = params["textDocument"]["uri"].as_str().unwrap_or_default().to_owned();
        let text = self.docs.get(&uri).cloned().ok_or_else(missing)?;
        let at = params.get("position").or_else(|| params.get("range").map(|r| &r["start"]));
        let offset = at.map_or(0, |p| self.offset(&text, p));
        let word = Self::word(&text, offset);
        let name = word.map(|(s, e)| text[s..e].to_owned());
        Ok(match method {
            "textDocument/completion" => {
                let start = word.map_or(offset, |(s, _)| s).min(offset);
                let after_dot = text[..start].ends_with('.');
                let range = self.range(&text, start, offset);
                let mut items: Vec<Value> = Vec::new();
                if after_dot {
                    for (i, member) in ["log", "error", "warn", "table"].iter().enumerate() {
                        items.push(json!({"label": member, "kind": 2, "detail": format!("(method) console.{member}(...data: any[]): void"),
                            "sortText": format!("{i}"), "insertTextFormat": 2, "textEdit": {"range": range, "newText": format!("{member}($1)$0")}}));
                    }
                } else {
                    let mut names: Vec<&str> = text
                        .split(|c: char| !is_word(c))
                        .filter(|w| w.len() >= 3 && !w.chars().all(|c| c.is_ascii_digit()))
                        .collect();
                    names.sort_unstable();
                    names.dedup();
                    // A stylesheet's words are its properties (as the CSS server says them),
                    // any other file's its variables.
                    let stylesheet = uri.ends_with(".css") || uri.ends_with(".scss");
                    let kind = if stylesheet { 10 } else { 6 };
                    for (i, n) in
                        names.iter().enumerate().filter(|(_, n)| Some(**n) != name.as_deref())
                    {
                        items.push(json!({"label": n, "kind": kind, "sortText": format!("1{i:04}"), "textEdit": {"range": range, "newText": n}}));
                    }
                    items.push(json!({"label": "readFile", "kind": 3, "sortText": "2", "data": {"from": "fs"}}));
                }
                json!({"isIncomplete": false, "items": items})
            }
            "textDocument/hover" => match word {
                Some((s, e)) => {
                    json!({"contents": {"kind": "markdown", "value": format!("```typescript\nconst {}: number\n```\nThe value of **{}**.", &text[s..e], &text[s..e])},
                    "range": self.range(&text, s, e)})
                }
                None => Value::Null,
            },
            "textDocument/signatureHelp" => {
                let open = text[..offset].rfind('(');
                match open.and_then(|o| Self::word(&text, o).map(|(s, _)| (s, o))) {
                    Some((s, o)) => {
                        let args = text[o + 1..offset].matches(',').count();
                        json!({"signatures": [{"label": format!("{}(a: number, b: string): void", &text[s..o]),
                            "parameters": [{"label": "a: number"}, {"label": "b: string"}]}], "activeSignature": 0, "activeParameter": args})
                    }
                    None => Value::Null,
                }
            }
            "textDocument/definition" => match &name {
                Some(n) => match Self::occurrences(&text, n).first() {
                    Some(&(s, e)) => json!([{"uri": uri, "range": self.range(&text, s, e)}]),
                    None => json!([]),
                },
                None => json!([]),
            },
            "textDocument/references" => {
                let Some(n) = &name else { return Ok(json!([])) };
                let mut out = Vec::new();
                let mut uris: Vec<&String> = self.docs.keys().collect();
                uris.sort();
                for u in uris {
                    let t = &self.docs[u];
                    out.extend(
                        Self::occurrences(t, n)
                            .into_iter()
                            .map(|(s, e)| json!({"uri": u, "range": self.range(t, s, e)})),
                    );
                }
                Value::Array(out)
            }
            "textDocument/documentHighlight" => match &name {
                Some(n) => Value::Array(
                    Self::occurrences(&text, n)
                        .into_iter()
                        .map(|(s, e)| json!({"range": self.range(&text, s, e), "kind": 1}))
                        .collect(),
                ),
                None => json!([]),
            },
            "textDocument/prepareRename" => match word {
                Some((s, e)) => {
                    json!({"range": self.range(&text, s, e), "placeholder": &text[s..e]})
                }
                None => Value::Null,
            },
            "textDocument/rename" => {
                let Some(n) = &name else { return Ok(Value::Null) };
                let new_name = params["newName"].as_str().unwrap_or_default();
                let mut changes = serde_json::Map::new();
                for (u, t) in &self.docs {
                    let edits: Vec<Value> = Self::occurrences(t, n)
                        .into_iter()
                        .map(|(s, e)| json!({"range": self.range(t, s, e), "newText": new_name}))
                        .collect();
                    if !edits.is_empty() {
                        changes.insert(u.clone(), Value::Array(edits));
                    }
                }
                json!({"changes": changes})
            }
            "textDocument/diagnostic" => {
                // The text's length as the version of what it reports.
                let id = text.len().to_string();
                if params["previousResultId"].as_str() == Some(id.as_str()) {
                    json!({"kind": "unchanged", "resultId": id})
                } else {
                    json!({"kind": "full", "resultId": id, "items": self.diagnostics(&uri)})
                }
            }
            "textDocument/documentSymbol" => {
                let mut out = Vec::new();
                for keyword in ["function ", "class "] {
                    for (at, _) in text.match_indices(keyword) {
                        let start = at + keyword.len();
                        if let Some((s, e)) = Self::word(&text, start) {
                            let end = text[s..].find('\n').map_or(text.len(), |i| s + i);
                            out.push(json!({"name": &text[s..e], "kind": if keyword == "class " { 5 } else { 12 },
                                "range": self.range(&text, at, end), "selectionRange": self.range(&text, s, e)}));
                        }
                    }
                }
                Value::Array(out)
            }
            "textDocument/formatting" => {
                let mut edits = Vec::new();
                let mut start = 0;
                for line in text.split('\n') {
                    let trimmed = line.trim_end_matches([' ', '\t']);
                    if trimmed.len() < line.len() {
                        edits.push(json!({"range": self.range(&text, start + trimmed.len(), start + line.len()), "newText": ""}));
                    }
                    start += line.len() + 1;
                }
                Value::Array(edits)
            }
            // Every later occurrence of the word implements it; its last one is its type.
            "textDocument/implementation" => match &name {
                Some(n) => Value::Array(
                    Self::occurrences(&text, n)
                        .into_iter()
                        .skip(1)
                        .map(|(s, e)| json!({"uri": uri, "range": self.range(&text, s, e)}))
                        .collect(),
                ),
                None => json!([]),
            },
            "textDocument/typeDefinition" => match &name {
                Some(n) => match Self::occurrences(&text, n).last() {
                    Some(&(s, e)) => json!([{"uri": uri, "range": self.range(&text, s, e)}]),
                    None => json!([]),
                },
                None => json!([]),
            },
            "textDocument/prepareTypeHierarchy" => match word {
                Some((s, e)) => {
                    json!([{"name": &text[s..e], "kind": 5, "detail": "Fake", "uri": uri,
                    "range": self.range(&text, s, e), "selectionRange": self.range(&text, s, e)}])
                }
                None => json!([]),
            },
            // `n:` before each number right after `(`.
            "textDocument/inlayHint" => {
                let start = params["range"]["start"].clone();
                let end = params["range"]["end"].clone();
                let (from, to) = (self.offset(&text, &start), self.offset(&text, &end));
                let mut out = Vec::new();
                for (at, _) in text.match_indices('(') {
                    let next = text[at + 1..].chars().next();
                    if at >= from && at < to && next.is_some_and(|c| c.is_ascii_digit()) {
                        out.push(json!({"position": self.position(&text, at + 1), "label": "n:", "kind": 2}));
                    }
                }
                Value::Array(out)
            }
            // The first `function` gets `2 references`, the others a lens to resolve.
            "textDocument/codeLens" => {
                let mut out = Vec::new();
                for (i, (at, _)) in text.match_indices("function ").enumerate() {
                    let range = self.range(&text, at, at + 8);
                    out.push(if i == 0 {
                        json!({"range": range, "command": {"title": "2 references", "command": "fake.lens"}})
                    } else {
                        json!({"range": range, "data": i})
                    });
                }
                Value::Array(out)
            }
            "textDocument/documentLink" => {
                let mut out = Vec::new();
                for (at, _) in text.match_indices("https://") {
                    let end = text[at..]
                        .find(|c: char| c.is_whitespace() || c == '\'' || c == '"')
                        .map_or(text.len(), |n| at + n);
                    out.push(
                        json!({"range": self.range(&text, at, end), "target": &text[at..end]}),
                    );
                }
                Value::Array(out)
            }
            "textDocument/codeAction" => match word {
                Some((s, e)) => {
                    json!([{"title": format!("Uppercase '{}'", &text[s..e]), "kind": "refactor.rewrite",
                    "edit": {"changes": {uri: [{"range": self.range(&text, s, e), "newText": text[s..e].to_uppercase()}]}}},
                    {"title": "Run a command", "command": "fake.command"}])
                }
                None => json!([]),
            },
            _ => {
                return Err(ResponseError {
                    code: codes::METHOD_NOT_FOUND,
                    message: method.into(),
                });
            }
        })
    }
}

/// Serves on this process's stdin and stdout (the app's `--fake-language-server`).
pub fn serve_stdio() {
    let seen = Mutex::new(Seen::default());
    let behaviour = Behaviour {
        progress: std::env::var_os("EDITOR_FAKE_LSP_PROGRESS").is_some(),
        ..Behaviour::default()
    };
    serve(io::stdin().lock(), io::stdout().lock(), &behaviour, &seen);
}
