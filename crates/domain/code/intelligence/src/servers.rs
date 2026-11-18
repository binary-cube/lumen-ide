//! Language servers as an intelligence engine beside the languages' own: the port the app
//! asks ([`LanguageServers`], contributed by the module running the servers), what it
//! answers with, and the servers' descriptions the languages' modules bring as data
//! ([`ServerData`]).
//!
//! Each language has an [`Engine`] the user chose: the language server (the default where
//! one is described), the built-in engine (the language module's own intelligence), or both
//! (their results merged). A server answers only once it's ready: while it's starting,
//! stopped or crashed, the built-in engine answers, so there's never nothing.
//!
//! A server's answer comes later, on another thread: every question returns a [`Reply`] the
//! app looks at when the module wakes it (never waiting for it on the UI thread); one
//! dropped before it's answered cancels its request. Offsets are bytes in the document's
//! version asked about; the module converts them to the server's positions and back.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use kernel::{DocumentId, ProjectId};

use crate::{CompletionItem, HoverInfo, SignatureHelp, Symbol, fuzzy_score};

/// What answers a language's questions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Engine {
    /// The language server (the default where one is described).
    #[default]
    Server,
    /// The language module's own intelligence (the built-in engine).
    BuiltIn,
    /// Both, their results merged (completion joined without duplicates, the diagnostics of
    /// both, the first definition found).
    Both,
}

impl Engine {
    pub const ALL: [Self; 3] = [Self::Server, Self::BuiltIn, Self::Both];

    /// As it's saved (`server`, `built-in`, `both`).
    pub fn id(self) -> &'static str {
        match self {
            Self::Server => "server",
            Self::BuiltIn => "built-in",
            Self::Both => "both",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.id() == id)
    }

    /// A server answers (alone or with the built-in engine).
    pub fn uses_server(self) -> bool {
        self != Self::BuiltIn
    }

    /// The built-in engine answers (alone or with a server).
    pub fn uses_built_in(self) -> bool {
        self != Self::Server
    }
}

/// What the built-in engine keeps answering where a language's server is chosen: what the
/// server doesn't do, or does less well (its description says, `built-in = [...]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Kept {
    /// Its syntax errors and analyses (the server reports no diagnostics of its own: HTML's).
    pub diagnostics: bool,
    /// Its deprecations, shown even where a server's diagnostic marks the same place (the
    /// server doesn't flag deprecated properties: CSS's). Its other analyses are shown
    /// anyway, but where a server's diagnostic is.
    pub deprecated: bool,
    /// Rename and Find Usages (it finds the uses in the other languages' files too: a CSS
    /// class in HTML, PHP, Blade).
    pub rename: bool,
    /// Its suggestions, joined after the server's wherever both have some (PHP's: the
    /// project's and the stubs' names the server's list may lack).
    pub completion: bool,
}

impl Kept {
    /// What can be kept, as the descriptions name it.
    pub const NAMES: [&'static str; 4] = ["diagnostics", "deprecated", "rename", "completion"];

    /// The ones `names` names (unknown names ignored: checked when they're read).
    pub fn from_names<'a>(names: impl IntoIterator<Item = &'a str>) -> Self {
        let mut kept = Self::default();
        for name in names {
            match name {
                "diagnostics" => kept.diagnostics = true,
                "deprecated" => kept.deprecated = true,
                "rename" => kept.rename = true,
                "completion" => kept.completion = true,
                _ => {}
            }
        }
        kept
    }

    /// Both's together.
    pub fn union(self, other: Self) -> Self {
        Self {
            diagnostics: self.diagnostics || other.diagnostics,
            deprecated: self.deprecated || other.deprecated,
            rename: self.rename || other.rename,
            completion: self.completion || other.completion,
        }
    }
}

/// How an asked question goes.
#[derive(Debug)]
pub enum Polled<T> {
    /// Answered (taken: asked again, it's [`Polled::Gone`]).
    Ready(T),
    /// Still being worked on.
    Waiting,
    /// It will never be answered (the server stopped, the request failed or timed out).
    Gone,
}

/// The answer to a question asked of a server, coming later from another thread. Looked
/// at without waiting ([`Self::poll`]) on the UI thread, or waited for ([`Self::wait`]) on
/// a background one. Dropped before it's answered, it cancels the request.
pub struct Reply<T> {
    rx: Receiver<T>,
    /// What cancels the request (run once, if it's dropped unanswered).
    cancel: Option<Box<dyn FnOnce() + Send>>,
}

impl<T> std::fmt::Debug for Reply<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reply").field("answered", &self.cancel.is_none()).finish()
    }
}

/// Where an answer is sent (the other end of a [`Reply`]).
#[derive(Debug)]
pub struct Replier<T>(Sender<T>);

impl<T> Replier<T> {
    /// Sends the answer: `false` if nobody waits for it any more.
    pub fn send(self, value: T) -> bool {
        self.0.send(value).is_ok()
    }
}

impl<T> Reply<T> {
    /// A question's two ends: where its answer is sent, and what waits for it.
    pub fn pair() -> (Replier<T>, Self) {
        let (tx, rx) = mpsc::channel();
        (Replier(tx), Self { rx, cancel: None })
    }

    /// An answer known already.
    pub fn ready(value: T) -> Self {
        let (tx, reply) = Self::pair();
        tx.send(value);
        reply
    }

    /// What dropping it unanswered does (cancels its request).
    #[must_use]
    pub fn on_cancel(mut self, cancel: impl FnOnce() + Send + 'static) -> Self {
        self.cancel = Some(Box::new(cancel));
        self
    }

    /// The answer if it came, without waiting.
    pub fn poll(&mut self) -> Polled<T> {
        match self.rx.try_recv() {
            Ok(value) => {
                self.cancel = None;
                Polled::Ready(value)
            }
            Err(TryRecvError::Empty) => Polled::Waiting,
            Err(TryRecvError::Disconnected) => {
                self.cancel = None;
                Polled::Gone
            }
        }
    }

    /// Waits up to `timeout` for the answer (on a background thread: never on the UI's).
    pub fn wait(mut self, timeout: Duration) -> Option<T> {
        match self.rx.recv_timeout(timeout) {
            Ok(value) => {
                self.cancel = None;
                Some(value)
            }
            Err(RecvTimeoutError::Disconnected) => {
                self.cancel = None;
                None
            }
            Err(RecvTimeoutError::Timeout) => None,
        }
    }
}

impl<T> Drop for Reply<T> {
    fn drop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel();
        }
    }
}

/// Where a question is asked: a document of an open project (a file of the project, its
/// language's server), at a byte offset of the document's version `version`.
#[derive(Debug, Clone, Copy)]
pub struct ServerAt<'a> {
    pub project: ProjectId,
    pub document: DocumentId,
    pub path: &'a Path,
    /// The document's language (its id).
    pub language: &'a str,
    pub version: u64,
    pub offset: usize,
}

/// A suggestion of a server, with what ranks it among the others.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServedItem {
    pub item: CompletionItem,
    /// What the typed prefix is matched against (the label, unless the server says).
    pub filter: String,
    /// The server's own order among its suggestions (compared as text).
    pub sort: String,
}

/// A server's suggestions at a place.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ServedCompletion {
    pub items: Vec<ServedItem>,
    /// The server may suggest others as more is typed: asked again then, never filtered
    /// from these alone.
    pub incomplete: bool,
}

/// `items` matching `prefix` (fuzzily, against what each is filtered by), the best first:
/// how well it matches, then the server's own order. Scores count down from the best, so
/// merged with the built-in engine's they keep their place.
pub fn rank(items: &[ServedItem], prefix: &str) -> Vec<CompletionItem> {
    let mut scored: Vec<(i32, &ServedItem)> =
        items.iter().filter_map(|s| Some((fuzzy_score(prefix, &s.filter)?, s))).collect();
    scored.sort_by(|(a, x), (b, y)| b.cmp(a).then_with(|| x.sort.cmp(&y.sort)));
    let mut seen = std::collections::HashSet::new();
    scored
        .into_iter()
        .filter(|(_, s)| seen.insert((s.item.label.clone(), s.item.detail.clone())))
        .map(|(score, s)| CompletionItem { score: score + 1000, ..s.item.clone() })
        .collect()
}

/// The suggestions of both engines joined: the server's first (they know the types), the
/// built-in engine's that the server doesn't have after, each label once.
pub fn merge_completions(
    served: Vec<CompletionItem>,
    built_in: Vec<CompletionItem>,
) -> Vec<CompletionItem> {
    let mut labels: std::collections::HashSet<String> =
        served.iter().map(|i| i.label.clone()).collect();
    let mut out = served;
    out.extend(built_in.into_iter().filter(|i| labels.insert(i.label.clone())));
    out
}

/// A place a server points at (a declaration, a usage): a file and the bytes there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServedLocation {
    pub path: PathBuf,
    pub range: Range<usize>,
    /// 0-based.
    pub line: u32,
}

/// A replacement in a file a server asks for (a rename's, a formatting's, a fix's).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeEdit {
    pub range: Range<usize>,
    /// What's there now (checked before applying: the text may have changed since).
    pub original: String,
    pub text: String,
    /// The line (0-based) it starts on.
    pub line: u32,
}

/// The changes to one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub path: PathBuf,
    /// Sorted, not overlapping.
    pub edits: Vec<ChangeEdit>,
}

/// What a server asks to change in the project (an LSP workspace edit).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkspaceChange {
    pub files: Vec<FileChange>,
    /// Files renamed (from, to).
    pub renames: Vec<(PathBuf, PathBuf)>,
}

impl WorkspaceChange {
    pub fn is_empty(&self) -> bool {
        self.renames.is_empty() && self.files.iter().all(|f| f.edits.is_empty())
    }
}

/// A fix or a refactoring a server offers at a place (the bulb's rows).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeAction {
    pub title: String,
    /// Its kind as the server says (`quickfix`, `refactor.extract`, `source.organizeImports`).
    pub kind: Option<String>,
    /// The one the server recommends.
    pub preferred: bool,
    /// What [`LanguageServers::run_action`] runs it by (the module's handle).
    pub id: u64,
}

/// A lens a server writes beside a declaration (`3 references`): where, and what it says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServedLens {
    /// The declaration's start (bytes): the lens is on its line.
    pub offset: usize,
    pub title: String,
}

/// The port to the language servers (contributed by the module running them), asked by the
/// app for a project's documents. Every question returns `None` when no server answers
/// for it now (the built-in engine does); otherwise its [`Reply`].
pub trait LanguageServers: Send + Sync + std::fmt::Debug {
    /// What answers language `language`'s whole files in project `project` now: the user's
    /// choice, the built-in engine while no server of it is ready.
    fn served(&self, project: ProjectId, language: &str) -> Engine;

    /// What the built-in engine keeps answering for language `language` where its server
    /// answers (see [`Kept`]).
    fn kept(&self, project: ProjectId, language: &str) -> Kept {
        let _ = (project, language);
        Kept::default()
    }

    /// The languages the server answering `language` answers too, `language` itself
    /// included: its files are the ones a rename's places from that server cover (the
    /// others' — a function's uses in HTML or PHP — are the built-in engine's to find).
    fn languages_with(&self, project: ProjectId, language: &str) -> Vec<String> {
        let _ = project;
        vec![language.to_owned()]
    }

    /// What answers `language`'s files now, named for the status bar (a server, the
    /// built-in engine, both); `None` while none of its servers can answer (nothing to
    /// switch between).
    fn engine_label(&self, project: ProjectId, language: &str) -> Option<String> {
        let _ = (project, language);
        None
    }

    /// The characters, besides a name's, after which the server suggests (`.`, `"`, `/`).
    fn completion_triggers(&self, project: ProjectId, language: &str) -> String {
        let _ = (project, language);
        String::new()
    }

    fn complete(&self, at: &ServerAt<'_>) -> Option<Reply<ServedCompletion>> {
        let _ = at;
        None
    }

    /// The suggestion `item` (one of a [`Self::complete`]'s) worked out in full: what it
    /// adds elsewhere in the file (an import), its documentation.
    fn resolve(&self, at: &ServerAt<'_>, item: &CompletionItem) -> Option<Reply<CompletionItem>> {
        let _ = (at, item);
        None
    }

    fn hover(&self, at: &ServerAt<'_>) -> Option<Reply<Option<HoverInfo>>> {
        let _ = at;
        None
    }

    fn signature_help(&self, at: &ServerAt<'_>) -> Option<Reply<Option<SignatureHelp>>> {
        let _ = at;
        None
    }

    fn definition(&self, at: &ServerAt<'_>) -> Option<Reply<Vec<ServedLocation>>> {
        let _ = at;
        None
    }

    /// The symbol's usages, its declaration included.
    fn references(&self, at: &ServerAt<'_>) -> Option<Reply<Vec<ServedLocation>>> {
        let _ = at;
        None
    }

    /// The places a rename of the symbol at the cursor changes (its name in each), or why
    /// it can't be renamed.
    fn rename_places(
        &self,
        at: &ServerAt<'_>,
    ) -> Option<Reply<Result<Vec<ServedLocation>, String>>> {
        let _ = at;
        None
    }

    /// The symbol's occurrences in the document (highlighted as the cursor rests on it).
    fn highlights(&self, at: &ServerAt<'_>) -> Option<Reply<Vec<Range<usize>>>> {
        let _ = at;
        None
    }

    /// The document's declarations, as the server sees them.
    fn symbols(&self, at: &ServerAt<'_>) -> Option<Reply<Vec<Symbol>>> {
        let _ = at;
        None
    }

    /// The document laid out by the server (`range`: only that part).
    fn format(
        &self,
        at: &ServerAt<'_>,
        range: Option<Range<usize>>,
    ) -> Option<Reply<Result<WorkspaceChange, String>>> {
        let _ = (at, range);
        None
    }

    /// What implements the type, or overrides the method, at the cursor.
    fn implementations(&self, at: &ServerAt<'_>) -> Option<Reply<Vec<ServedLocation>>> {
        let _ = at;
        None
    }

    /// Where the type of what's at the cursor is declared (a variable's class).
    fn type_definition(&self, at: &ServerAt<'_>) -> Option<Reply<Vec<ServedLocation>>> {
        let _ = at;
        None
    }

    /// The type at the cursor with its supertypes and subtypes.
    fn type_hierarchy(
        &self,
        at: &ServerAt<'_>,
    ) -> Option<Reply<Option<crate::hierarchy::TypeHierarchy>>> {
        let _ = at;
        None
    }

    /// The hints the server shows in `range` (parameters' names, types).
    fn inlay_hints(
        &self,
        at: &ServerAt<'_>,
        range: Range<usize>,
    ) -> Option<Reply<Vec<crate::inlay::InlayHint>>> {
        let _ = (at, range);
        None
    }

    /// The lenses the server writes beside the document's declarations (worked out).
    fn code_lens(&self, at: &ServerAt<'_>) -> Option<Reply<Vec<ServedLens>>> {
        let _ = at;
        None
    }

    /// The links the server finds in the document.
    fn document_links(&self, at: &ServerAt<'_>) -> Option<Reply<Vec<crate::links::DocumentLink>>> {
        let _ = at;
        None
    }

    /// The fixes and refactorings offered for `range` (the cursor's, or the selection).
    fn code_actions(
        &self,
        at: &ServerAt<'_>,
        range: Range<usize>,
    ) -> Option<Reply<Vec<CodeAction>>> {
        let _ = (at, range);
        None
    }

    /// Code action `id` (one of [`Self::code_actions`]'s) worked out: what it changes (a
    /// command the server runs itself changes nothing here: its edits come on their own).
    fn run_action(
        &self,
        at: &ServerAt<'_>,
        id: u64,
    ) -> Option<Reply<Result<WorkspaceChange, String>>> {
        let _ = (at, id);
        None
    }
}

/// The source a server's diagnostics are announced under on its project's bus
/// (`DiagnosticsFound::source`): told apart from the other checkers' by
/// [`is_server_source`].
pub fn diagnostics_source(server: &str) -> String {
    format!("{SOURCE_PREFIX}{server}")
}

/// Whether diagnostics announced under `source` are a language server's.
pub fn is_server_source(source: &str) -> bool {
    source.starts_with(SOURCE_PREFIX)
}

const SOURCE_PREFIX: &str = "lsp-";

/// A language module's servers, described as data: a TOML text with `[[language-servers]]`
/// tables (see the module running them for the format). Contributed at `activate`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerData {
    /// Whose it is (the module's id), for the messages about it.
    pub module: &'static str,
    pub toml: &'static str,
}

/// The servers' descriptions contributed by the languages' modules: put here by the app
/// once they're activated, read by whoever runs the servers (no module names another).
pub fn register_data(data: ServerData) {
    if let Ok(mut all) = DATA.get_or_init(Mutex::default).lock()
        && !all.contains(&data)
    {
        all.push(data);
    }
}

/// Every server description registered.
pub fn data() -> Vec<ServerData> {
    DATA.get_or_init(Mutex::default).lock().map(|all| all.clone()).unwrap_or_default()
}

static DATA: OnceLock<Mutex<Vec<ServerData>>> = OnceLock::new();

#[cfg(test)]
#[path = "../tests/unit/servers.rs"]
mod tests;
