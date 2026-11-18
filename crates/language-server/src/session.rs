//! One client's session: its lifecycle, the open documents kept in step, and when their
//! diagnostics are published (after a pause in the typing, and again as the index grows).

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, PoisonError, RwLock};
use std::time::{Duration, Instant};

use code_intelligence::{Project, SymbolIndex};
use indexer::ProjectIndexer;
use lsp_protocol::jsonrpc::{Id, Message, ResponseError, codes, write_message};
use lsp_protocol::text::{Encoding, Lines, range_from_value};
use lsp_protocol::uri;
use serde_json::{Value, json};

use crate::engines::Engines;

/// How long the typing has to pause before a document is checked.
const CHECK_AFTER: Duration = Duration::from_millis(150);
/// LSP: a request came before `initialize`.
const NOT_INITIALIZED: i64 = -32002;

/// What the session waits for.
pub enum Input {
    Message(Message),
    /// The index changed.
    Indexed,
    /// The client's stream ended.
    Closed,
}

/// An open document.
pub struct Open {
    pub uri: String,
    pub version: i64,
    pub lines: Lines,
}

pub struct Session<W> {
    pub engines: Engines,
    output: W,
    wake: Sender<Input>,
    pub encoding: Encoding,
    /// The client takes snippets (a cursor placed inside `()`).
    pub snippets: bool,
    pub root: Option<PathBuf>,
    pub indexer: Option<ProjectIndexer>,
    pub index: Arc<RwLock<SymbolIndex>>,
    pub open: HashMap<PathBuf, Open>,
    pub(crate) frameworks: crate::frameworks::Frameworks,
    /// The language levels the project declares (`php` → `8.2`).
    levels: Vec<(kernel::LanguageId, String)>,
    to_check: HashSet<PathBuf>,
    check_at: Option<Instant>,
    initialized: bool,
    shutting_down: bool,
}

/// Where project `root`'s index is kept: never in the project (other editors' users didn't
/// ask for a folder of ours in it).
fn cache_for(root: &Path) -> PathBuf {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    root.hash(&mut hasher);
    let base = storage::data_dir().unwrap_or_else(std::env::temp_dir);
    base.join("language-server").join(format!("{:016x}", hasher.finish()))
}

impl<W: Write> Session<W> {
    pub fn new(engines: Engines, output: W, wake: Sender<Input>) -> Self {
        Self {
            engines,
            output,
            wake,
            encoding: Encoding::default(),
            snippets: false,
            root: None,
            indexer: None,
            index: Arc::default(),
            open: HashMap::new(),
            frameworks: crate::frameworks::Frameworks::default(),
            levels: Vec::new(),
            to_check: HashSet::new(),
            check_at: None,
            initialized: false,
            shutting_down: false,
        }
    }

    /// Until the client leaves: whether it asked to shut down first.
    pub fn run(mut self, inputs: &Receiver<Input>) -> std::io::Result<bool> {
        loop {
            let input = match self.check_at {
                None => inputs.recv().map_err(|_| RecvTimeoutError::Disconnected),
                Some(at) => inputs.recv_timeout(at.saturating_duration_since(Instant::now())),
            };
            match input {
                Ok(Input::Message(Message::Request { id, method, params })) => {
                    self.request(id, &method, &params)?;
                }
                Ok(Input::Message(Message::Notification { method, params })) => {
                    if method == "exit" {
                        return Ok(self.shutting_down);
                    }
                    self.notification(&method, &params);
                }
                Ok(Input::Message(Message::Response { .. })) => {}
                Ok(Input::Indexed) => {
                    let all: Vec<PathBuf> = self.open.keys().cloned().collect();
                    self.check_later(all);
                }
                Err(RecvTimeoutError::Timeout) => self.publish()?,
                Ok(Input::Closed) | Err(RecvTimeoutError::Disconnected) => return Ok(false),
            }
        }
    }

    fn request(&mut self, id: Id, method: &str, params: &Value) -> std::io::Result<()> {
        let result = if method == "initialize" {
            Ok(self.initialize(params))
        } else if !self.initialized {
            Err(ResponseError { code: NOT_INITIALIZED, message: "not initialized".into() })
        } else if method == "shutdown" {
            self.shutting_down = true;
            Ok(Value::Null)
        } else {
            self.answer(method, params).ok_or_else(|| ResponseError {
                code: codes::METHOD_NOT_FOUND,
                message: format!("{method}: not served"),
            })
        };
        self.send(&Message::Response { id, result }.to_value())
    }

    fn initialize(&mut self, params: &Value) -> Value {
        self.initialized = true;
        let offered = params.pointer("/capabilities/general/positionEncodings");
        let takes_utf8 = offered
            .and_then(Value::as_array)
            .is_some_and(|all| all.iter().any(|e| e.as_str() == Some("utf-8")));
        self.encoding = if takes_utf8 { Encoding::Utf8 } else { Encoding::Utf16 };
        self.snippets = params
            .pointer("/capabilities/textDocument/completion/completionItem/snippetSupport")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        self.root = params
            .pointer("/workspaceFolders/0/uri")
            .or_else(|| params.get("rootUri"))
            .and_then(Value::as_str)
            .and_then(uri::to_path);
        self.start_index();
        self.start_frameworks();
        json!({
            "capabilities": {
                "positionEncoding": self.encoding.name(),
                "textDocumentSync": {"openClose": true, "change": 2, "save": true},
                "completionProvider": {"triggerCharacters": self.engines.triggers()},
                "hoverProvider": true,
                "signatureHelpProvider": {"triggerCharacters": ["(", ","]},
                "definitionProvider": true,
                "referencesProvider": true,
                "documentSymbolProvider": true,
                "renameProvider": {"prepareProvider": true},
            },
            "serverInfo": {"name": "language-server", "version": env!("CARGO_PKG_VERSION")},
        })
    }

    fn start_index(&mut self) {
        let Some(root) = self.root.clone() else { return };
        self.levels = self.with_project(|project| {
            let languages = project.languages.iter();
            languages.filter_map(|i| Some((i.language(), i.detect_level(project)?.level))).collect()
        });
        let wake = self.wake.clone();
        let waker = Arc::new(move || {
            let _ = wake.send(Input::Indexed);
        });
        // Without an index it still answers for the open documents.
        let cache = cache_for(&root);
        let engines = self.engines.intelligence.clone();
        if let Ok(indexer) = ProjectIndexer::start_caching_in(root, cache, engines, waker) {
            self.index = indexer.index();
            self.indexer = Some(indexer);
        }
    }

    fn notification(&mut self, method: &str, params: &Value) {
        let path =
            params.pointer("/textDocument/uri").and_then(Value::as_str).and_then(uri::to_path);
        match (method, path) {
            ("textDocument/didOpen", Some(path)) => self.opened(path, params),
            ("textDocument/didChange", Some(path)) => self.changed(&path, params),
            ("textDocument/didClose", Some(path)) => self.closed(path, params),
            ("textDocument/didSave", _) => self.frameworks.disk_changed(),
            ("workspace/didChangeWatchedFiles", _) => self.files_changed(params),
            _ => {}
        }
    }

    fn opened(&mut self, path: PathBuf, params: &Value) {
        let document = &params["textDocument"];
        let text = document["text"].as_str().unwrap_or_default().to_owned();
        let open = Open {
            uri: document["uri"].as_str().unwrap_or_default().to_owned(),
            version: document["version"].as_i64().unwrap_or(0),
            lines: Lines::new(text),
        };
        self.open.insert(path.clone(), open);
        self.reindex(&path);
        self.check_later([path]);
    }

    fn changed(&mut self, path: &Path, params: &Value) {
        let Some(open) = self.open.get_mut(path) else { return };
        open.version = params.pointer("/textDocument/version").and_then(Value::as_i64).unwrap_or(0);
        for change in params["contentChanges"].as_array().into_iter().flatten() {
            let text = change["text"].as_str().unwrap_or_default();
            match range_from_value(&change["range"]) {
                Some((start, end)) => {
                    let range = open.lines.bytes(start, end, self.encoding);
                    open.lines.replace(range, text);
                }
                None => open.lines = Lines::new(text.to_owned()),
            }
        }
        self.reindex(path);
        self.check_later([path.to_path_buf()]);
    }

    /// Its checks cleared; the index back to what's on the disk.
    fn closed(&mut self, path: PathBuf, params: &Value) {
        self.open.remove(&path);
        self.to_check.remove(&path);
        if let Some(uri) = params.pointer("/textDocument/uri") {
            let _ = self.notify_diagnostics(uri, None, &[]);
        }
        if let Some(indexer) = &self.indexer {
            indexer.changed(vec![path]);
        }
    }

    fn files_changed(&self, params: &Value) {
        self.frameworks.disk_changed();
        let Some(indexer) = &self.indexer else { return };
        let changes = params["changes"].as_array().into_iter().flatten();
        let paths = changes.filter_map(|c| c["uri"].as_str().and_then(uri::to_path));
        indexer.changed(paths.filter(|p| !self.open.contains_key(p)).collect());
    }

    /// The open text's declarations in the index: what it says now, saved or not.
    fn reindex(&self, path: &Path) {
        let (Some(open), Some(engine)) = (self.open.get(path), self.engines.of(path)) else {
            return;
        };
        let symbols = engine.extract(open.lines.text());
        self.index.write().unwrap_or_else(PoisonError::into_inner).set_file(path, symbols);
    }

    fn check_later(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        self.to_check.extend(paths);
        if !self.to_check.is_empty() {
            // Not pushed back by each change: a long burst still gets checked.
            self.check_at.get_or_insert_with(|| Instant::now() + CHECK_AFTER);
        }
    }

    fn publish(&mut self) -> std::io::Result<()> {
        self.check_at = None;
        let busy = self.indexer.as_ref().is_some_and(ProjectIndexer::is_busy);
        for path in std::mem::take(&mut self.to_check) {
            let Some(open) = self.open.get(&path) else { continue };
            let (uri, version) = (json!(open.uri), open.version);
            let found = self.diagnostics(&path, !busy);
            self.notify_diagnostics(&uri, Some(version), &found)?;
            if busy {
                // Looked at without the project's checks: again once it's indexed.
                self.to_check.insert(path);
            }
        }
        if busy && !self.to_check.is_empty() {
            self.check_at = Some(Instant::now() + 4 * CHECK_AFTER);
        }
        Ok(())
    }

    fn notify_diagnostics(
        &mut self,
        uri: &Value,
        version: Option<i64>,
        found: &[Value],
    ) -> std::io::Result<()> {
        let params = json!({"uri": uri, "version": version, "diagnostics": found});
        let method = "textDocument/publishDiagnostics".to_owned();
        self.send(&Message::Notification { method, params }.to_value())
    }

    fn send(&mut self, value: &Value) -> std::io::Result<()> {
        write_message(&mut self.output, value).map(|_| ())
    }

    /// `f` with the project as the engines read it: open documents as typed, the rest from
    /// the disk.
    pub fn with_project<R>(&self, f: impl FnOnce(&Project<'_>) -> R) -> R {
        let read = |p: &Path| self.text_of(p);
        let list = |dir: &Path| -> Vec<(String, bool)> {
            std::fs::read_dir(dir)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| {
                    let is_dir = e.file_type().is_ok_and(|t| t.is_dir());
                    (e.file_name().to_string_lossy().into_owned(), is_dir)
                })
                .collect()
        };
        let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
        f(&Project {
            root: self.root.as_deref(),
            read: &read,
            list: &list,
            modified: &modified,
            levels: &self.levels,
            languages: &self.engines.intelligence,
        })
    }

    pub fn text_of(&self, path: &Path) -> Option<String> {
        match self.open.get(path) {
            Some(open) => Some(open.lines.text().to_owned()),
            None => std::fs::read_to_string(path).ok(),
        }
    }
}
