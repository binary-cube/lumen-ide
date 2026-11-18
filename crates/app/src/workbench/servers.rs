//! The language servers in the editor (the language servers module's port): the project's
//! documents announced on its bus as they open, change and close — the module follows them —
//! and the questions only a server answers well asked of it, never waited for: each
//! [`Reply`] is looked at when the module wakes the app ([`Workbench::poll_servers`]), its
//! answer used at the next frame. A language whose engine is the server's alone gets the
//! server's answers; with both engines, both; while no server of it is ready (starting,
//! stopped, crashed), and for code embedded in another language, the built-in engine's.

use std::ops::Range;
use std::time::{Duration, Instant};

use code_intelligence::servers::{
    Engine, Kept, LanguageServers, Polled, Reply, ServedCompletion, ServedItem, ServedLocation,
    ServerAt,
};
use code_intelligence::{CompletionItem, HoverInfo, SignatureHelp};
use document::events::{DocumentChanged, DocumentClosed, DocumentOpened};

use super::*;

/// A file announced to the project's bus: the document, its language and version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Announced {
    pub doc: DocumentId,
    pub language: String,
    pub version: u64,
}

/// How long a suggestion list, a hover card or a signature waits for a server before the
/// built-in engine's is shown (the server's joins it when it comes). The servers measured
/// answer completion in 0.5–10 ms (p95 under 6 ms, plan 43a–43d) and a list that appears
/// within ~200 ms still reads as immediate, so a healthy server is never cut short while a
/// silent one (busy indexing, stuck) costs a fifth of a second instead of the request's
/// 10 s timeout.
pub(super) const SERVER_GRACE: Duration = Duration::from_millis(200);

/// How long "Go to Declaration" waits for a server before jumping where the built-in engine
/// says (when it knows: otherwise the server's answer is still waited for). Longer than
/// [`SERVER_GRACE`]: the jump is asked for, a server's first semantic answer may wait for its
/// project to load (vtsls: ~1 s), and jumping twice would be worse than waiting a little.
pub(super) const DEFINITION_GRACE: Duration = Duration::from_millis(1000);

/// The suggestions a server gave for a word's start, filtered as it's typed further.
#[derive(Debug)]
pub(super) struct CompletionCache {
    pub doc: DocumentId,
    pub start: usize,
    pub items: Vec<ServedItem>,
    pub incomplete: bool,
    /// They came after the built-in engine's list was shown for the word: they join it
    /// after its suggestions (nothing moves under the cursor).
    pub late: bool,
    /// Nothing came (an error, a timeout, an empty answer: a server still loading, a
    /// document changed under it): kept only for the document's version it was asked at, so
    /// it isn't asked again at every frame but is at the next edit — never given up for good.
    pub retry: Option<u64>,
}

/// A server's suggestions at the cursor, or what it's waiting for.
#[derive(Debug)]
pub(super) enum Suggested {
    /// Its suggestions and the range they replace (`late`: see [`CompletionCache::late`]).
    Items { items: Vec<CompletionItem>, replace: Range<usize>, late: bool },
    /// Asked, not come yet; `overdue`: past [`SERVER_GRACE`] (the built-in engine's
    /// suggestions are shown meanwhile).
    Waiting { overdue: bool },
}

/// A document's version and a word's start (what highlights are asked for).
type HighlightKey = (DocumentId, u64, usize);
/// An announcement published once the tabs were looked at.
type Publish = Box<dyn FnOnce(&kernel::EventBus)>;

/// What the window asked the servers and waits for, and their answers (the project's data:
/// gone with it).
#[derive(Debug, Default)]
pub(super) struct ServedRequests {
    /// The files announced, by path (a file shown in two groups is announced once).
    pub announced: HashMap<PathBuf, Announced>,
    completion: Option<(DocumentId, usize, Reply<ServedCompletion>)>,
    pub completion_cache: Option<CompletionCache>,
    /// A completion came: the list is made again at the next frame.
    pub completion_due: bool,
    /// When the pending completion's grace ends (`None`: over, or none pending).
    completion_grace: Option<Instant>,
    /// The built-in engine's list was shown while the pending completion was waited for.
    completion_shown: bool,
    /// The server let a completion's grace pass and hasn't answered one since: the next
    /// word's list doesn't wait for it.
    completion_slow: bool,
    /// The document's version the pending completion was asked at.
    completion_version: u64,
    /// When the pending hover card's, signature's, declaration's grace ends.
    hover_grace: Option<Instant>,
    signature_grace: Option<Instant>,
    definition_grace: Option<Instant>,
    hover: Option<(DocumentId, usize, Reply<Option<HoverInfo>>)>,
    /// The hover card's answer (the document, the word's start): `None` inside, the server
    /// had nothing to say.
    pub hover_answer: Option<(DocumentId, usize, Option<HoverInfo>)>,
    signature: Option<(DocumentId, usize, Reply<Option<SignatureHelp>>)>,
    pub signature_answer: Option<(DocumentId, usize, Option<SignatureHelp>)>,
    pub signature_due: bool,
    highlights: Option<(HighlightKey, Reply<Vec<Range<usize>>>)>,
    pub highlights_answer: Option<(HighlightKey, Vec<Range<usize>>)>,
    /// A suggestion accepted being worked out (its import): the document, its version
    /// right after it was inserted.
    resolve: Option<(DocumentId, u64, Reply<CompletionItem>)>,
    pub resolved: Option<(DocumentId, u64, CompletionItem)>,
    /// "Go to Declaration" asked: the document and where.
    definition: Option<(DocumentId, usize, Reply<Vec<ServedLocation>>)>,
    pub definition_answer: Option<(DocumentId, usize, Vec<ServedLocation>)>,
    /// "Go to Implementation", "Go to Type Declaration", "Type Hierarchy" asked, and the
    /// answer.
    pub navigation: Option<super::navigation::AskedNavigation>,
    /// The document's links asked (once per version, while ⌘ is held), and the answer.
    pub links: Option<(DocumentId, u64, Reply<Vec<code_intelligence::links::DocumentLink>>)>,
    pub links_answer: Option<(DocumentId, u64, Vec<code_intelligence::links::DocumentLink>)>,
    pub navigation_answer:
        Option<(DocumentId, usize, super::navigation::Related, super::navigation::Answer)>,
    /// The document's declarations asked of its server (a language the built-in engine
    /// has no structure for), at a version, its language.
    outline: Option<(DocumentId, u64, String, Reply<Vec<code_intelligence::Symbol>>)>,
    /// The server's declarations of a document at a version (empty: it had none).
    pub outline_answer: Option<(DocumentId, u64, Arc<code_intelligence::FileSymbols>)>,
    /// Bumped by every outline answer (the side panels are given it again).
    pub outline_generation: u64,
}

/// Polls `slot`'s reply: its answer when it came (the slot emptied when it's answered or
/// gone).
fn take<K, T>(slot: &mut Option<(K, usize, Reply<T>)>) -> Option<(K, usize, Option<T>)> {
    let (_, _, reply) = slot.as_mut()?;
    match reply.poll() {
        Polled::Waiting => None,
        Polled::Ready(value) => slot.take().map(|(k, at, _)| (k, at, Some(value))),
        Polled::Gone => slot.take().map(|(k, at, _)| (k, at, None)),
    }
}

impl Workbench {
    /// The language servers' port (the module's), if it's there.
    pub(super) fn servers(&self) -> Option<Arc<dyn LanguageServers>> {
        self.support.servers.first().cloned()
    }

    /// Where a question about the active document at `offset` is asked: its project, the
    /// document announced for its file, its path and language — when the language there is
    /// the file's own (embedded code is the built-in engine's).
    fn server_place(&self, offset: usize) -> Option<(kernel::ProjectId, PathBuf, Announced)> {
        self.project.as_ref()?;
        let tab = self.tabs.get(self.active)?;
        let path = tab.doc.path()?;
        let announced = self.data.served.announced.get(path)?;
        let language = tab.syntax.language();
        if language.as_str() != announced.language {
            return None;
        }
        let at = tab
            .syntax
            .snapshot()
            .map_or_else(|| language.clone(), |s| s.language_at(offset).clone());
        (at == *language).then(|| (self.data.id, path.to_path_buf(), announced.clone()))
    }

    /// What answers the active document at `offset` now.
    pub(super) fn served_at(&self, offset: usize) -> Engine {
        let Some(servers) = self.servers() else { return Engine::BuiltIn };
        match self.server_place(offset) {
            Some((project, _, announced)) => servers.served(project, &announced.language),
            None => Engine::BuiltIn,
        }
    }

    /// Asks the active document's server at `offset` (`None`: no server answers there).
    pub(super) fn ask_server<T>(
        &self,
        offset: usize,
        ask: impl FnOnce(&dyn LanguageServers, &ServerAt<'_>) -> Option<T>,
    ) -> Option<T> {
        let servers = self.servers()?;
        let (project, path, announced) = self.server_place(offset)?;
        if servers.served(project, &announced.language) == Engine::BuiltIn {
            return None;
        }
        let at = ServerAt {
            project,
            document: announced.doc,
            path: &path,
            language: &announced.language,
            version: announced.version,
            offset,
        };
        ask(servers.as_ref(), &at)
    }

    /// The project's files open in tabs announced on its bus: opened, changed (a new
    /// version), closed. Cheap when nothing changed (a version compared per tab).
    pub(super) fn announce_documents(&mut self) {
        if self.project.is_none() || self.support.servers.is_empty() {
            return;
        }
        let root = self.project.as_ref().map(|p| p.root_path().to_path_buf()).unwrap_or_default();
        let mut seen: Vec<PathBuf> = Vec::new();
        let mut events: Vec<Publish> = Vec::new();
        for tab in all_tabs(&self.tabs, &self.editors.groups) {
            let Some(path) = tab.doc.path() else { continue };
            if is_virtual(path)
                || !path.starts_with(&root)
                || !tab.holds_text()
                || seen.iter().any(|p| p == path)
            {
                continue;
            }
            seen.push(path.to_path_buf());
            let now = Announced {
                doc: tab.doc.id(),
                language: tab.syntax.language().to_string(),
                version: tab.doc.version(),
            };
            let before = self.data.served.announced.get(path);
            if before == Some(&now) {
                continue;
            }
            let text = tab.doc.buffer().snapshot();
            let path = path.to_path_buf();
            match before {
                Some(old) if old.doc == now.doc && old.language == now.language => {
                    let e = DocumentChanged {
                        document: now.doc,
                        path: path.clone(),
                        version: now.version,
                        text,
                    };
                    events.push(Box::new(move |bus| bus.publish(&e)));
                }
                old => {
                    if let Some(old) = old {
                        let e = DocumentClosed { document: old.doc, path: path.clone() };
                        events.push(Box::new(move |bus| bus.publish(&e)));
                    }
                    let e = DocumentOpened {
                        document: now.doc,
                        path: path.clone(),
                        language: now.language.clone(),
                        version: now.version,
                        text,
                    };
                    events.push(Box::new(move |bus| bus.publish(&e)));
                }
            }
            self.data.served.announced.insert(path, now);
        }
        let gone: Vec<(PathBuf, DocumentId)> = self
            .data
            .served
            .announced
            .iter()
            .filter(|(p, _)| !seen.contains(p))
            .map(|(p, a)| (p.clone(), a.doc))
            .collect();
        for (path, doc) in gone {
            self.data.served.announced.remove(&path);
            let e = DocumentClosed { document: doc, path };
            events.push(Box::new(move |bus| bus.publish(&e)));
        }
        let bus = self.data.events.clone();
        for publish in events {
            publish(&bus);
        }
    }

    /// The servers' answers come: kept for the next frame. `true` if one came (a redraw).
    pub fn poll_servers(&mut self) -> bool {
        let served = &mut self.data.served;
        let mut changed = false;
        if let Some((doc, start, answer)) = take(&mut served.completion) {
            changed = true;
            served.completion_due = true;
            served.completion_grace = None;
            if answer.is_some() {
                served.completion_slow = false;
            }
            // Joining the built-in engine's list shown for the word (also when an incomplete
            // list is asked again as it's typed): its order stays.
            let late = std::mem::take(&mut served.completion_shown)
                || served
                    .completion_cache
                    .as_ref()
                    .is_some_and(|c| c.late && c.doc == doc && c.start == start);
            // No answer (an error, a timeout, a crash): as if it had nothing to suggest —
            // the built-in engine's suggestions are shown, the server not asked again for
            // the word (it would fail the same way, on every frame).
            let answer = answer.unwrap_or_default();
            let retry = answer.items.is_empty().then_some(served.completion_version);
            served.completion_cache = Some(CompletionCache {
                doc,
                start,
                items: answer.items,
                incomplete: answer.incomplete,
                late,
                retry,
            });
        }
        if let Some((doc, start, answer)) = take(&mut served.hover) {
            changed = true;
            served.hover_grace = None;
            served.hover_answer = Some((doc, start, answer.flatten()));
        }
        if let Some((doc, at, answer)) = take(&mut served.signature) {
            changed = true;
            served.signature_grace = None;
            served.signature_due = true;
            served.signature_answer = Some((doc, at, answer.flatten()));
        }
        if let Some((key, reply)) = served.highlights.as_mut().map(|(k, r)| (*k, r)) {
            match reply.poll() {
                Polled::Waiting => {}
                Polled::Ready(ranges) => {
                    served.highlights = None;
                    served.highlights_answer = Some((key, ranges));
                    changed = true;
                }
                // No answer: none, so it isn't asked again for the same word and version.
                Polled::Gone => {
                    served.highlights = None;
                    served.highlights_answer = Some((key, Vec::new()));
                }
            }
        }
        if let Some((doc, version, reply)) = served.resolve.as_mut().map(|(d, v, r)| (*d, *v, r)) {
            match reply.poll() {
                Polled::Waiting => {}
                Polled::Ready(item) => {
                    served.resolve = None;
                    served.resolved = Some((doc, version, item));
                    changed = true;
                }
                Polled::Gone => served.resolve = None,
            }
        }
        if let Some((doc, at, answer)) = take(&mut served.definition) {
            changed = true;
            served.definition_grace = None;
            served.definition_answer = Some((doc, at, answer.unwrap_or_default()));
        }
        if let Some((doc, version, reply)) = served.links.as_mut().map(|(d, v, r)| (*d, *v, r)) {
            match reply.poll() {
                Polled::Waiting => {}
                Polled::Ready(links) => {
                    served.links = None;
                    served.links_answer = Some((doc, version, links));
                    // The link under the mouse is looked for again with them.
                    self.link = None;
                    changed = true;
                }
                Polled::Gone => {
                    served.links = None;
                    served.links_answer = Some((doc, version, Vec::new()));
                }
            }
        }
        if let Some((doc, version, language, reply)) = served.outline.as_mut() {
            let answer = match reply.poll() {
                Polled::Waiting => None,
                Polled::Ready(symbols) => Some(symbols),
                // No answer: none for this version (asked again at the next one).
                Polled::Gone => Some(Vec::new()),
            };
            if let Some(symbols) = answer {
                let file = code_intelligence::FileSymbols {
                    language: Arc::from(language.as_str()),
                    namespace: None,
                    uses: Vec::new(),
                    symbols,
                };
                served.outline_answer = Some((*doc, *version, Arc::new(file)));
                served.outline = None;
                served.outline_generation += 1;
                changed = true;
            }
        }
        changed |= self.poll_navigation();
        changed |= self.poll_actions();
        changed
    }

    /// The active document's declarations as its server sees them (plan 46d), for the
    /// Structure panel — only where the built-in engine has none: its language has no
    /// structure of ours, or ours found nothing in it. The last answer is kept per document
    /// and used until the next version's comes; one question at a time, its answer read by
    /// `poll_servers` (nothing waited for here).
    pub(super) fn served_outline(&mut self) -> Option<Arc<code_intelligence::FileSymbols>> {
        let tab = self.tabs.get(self.active)?;
        let (doc, version) = (tab.doc.id(), tab.doc.version());
        let language = tab.syntax.language().as_str().to_owned();
        let ours_known =
            self.support.intelligence.iter().any(|i| i.language().as_str() == language);
        if ours_known {
            // Ours extracted for this version and empty: nothing of ours to show.
            if tab.symbols_version != Some(version) {
                return self.kept_outline(doc);
            }
            let path = Self::index_path(tab);
            let ours = self
                .data
                .symbols
                .try_read()
                .ok()?
                .file(&path)
                .is_some_and(|f| !f.symbols.is_empty());
            if ours {
                return None;
            }
        }
        let kept = self.kept_outline(doc);
        let answered = matches!(&self.data.served.outline_answer, Some((d, v, _)) if *d == doc && *v == version);
        let asking =
            matches!(&self.data.served.outline, Some((d, v, ..)) if *d == doc && *v == version);
        if !answered
            && !asking
            && self.served_at(0).uses_server()
            && let Some(reply) = self.ask_server(0, |s, at| s.symbols(at))
        {
            self.data.served.outline = Some((doc, version, language, reply));
        }
        kept
    }

    /// A document's declarations are being asked of its server (the snapshots wait).
    #[cfg(feature = "snapshot")]
    pub fn outline_asked(&self) -> bool {
        self.data.served.outline.is_some()
    }

    /// The server's last declarations of document `doc` (any version).
    fn kept_outline(&self, doc: DocumentId) -> Option<Arc<code_intelligence::FileSymbols>> {
        match &self.data.served.outline_answer {
            Some((d, _, file)) if *d == doc && !file.symbols.is_empty() => Some(Arc::clone(file)),
            _ => None,
        }
    }

    /// When a question asked of a server is next looked at without its answer (its grace
    /// ends: the built-in engine's answer is shown then). One redraw is scheduled for it.
    pub fn served_deadline(&self) -> Option<Instant> {
        let s = &self.data.served;
        let pending = [
            s.completion.as_ref().and(s.completion_grace),
            s.hover.as_ref().and(s.hover_grace),
            s.signature.as_ref().and(s.signature_grace),
            s.definition.as_ref().and(s.definition_grace),
            self.navigation_deadline(),
            self.actions_deadline(),
            self.insight_deadline(),
        ];
        pending.into_iter().flatten().min()
    }

    /// A hover card is waited for from a server, within its grace.
    pub(super) fn served_hover_waiting(&self) -> bool {
        let s = &self.data.served;
        s.hover.is_some() && s.hover_grace.is_some_and(|at| Instant::now() < at)
    }

    /// The questions whose grace ended without an answer: the built-in engine's answer
    /// meanwhile — the list, the signature at the same place, the declaration it knows.
    fn after_grace(&mut self, text: &mut TextSystem) {
        let now = Instant::now();
        // Nothing due (most frames): nothing looked at.
        if self.served_deadline().is_none_or(|at| at > now) {
            return;
        }
        self.navigation_after_grace(text);
        let over = |grace: &mut Option<Instant>| grace.take_if(|at| *at <= now).is_some();
        // A card's grace over: the built-in engine's card is made by this frame's hover
        // (no grace left to schedule frames for).
        over(&mut self.data.served.hover_grace);
        let here = self.tabs.get(self.active).map(|t| {
            let head = t.doc.selections().primary().head;
            (t.doc.id(), head, self.word_start(&t.text(), head))
        });
        let served = &mut self.data.served;
        if served.completion.is_some() && over(&mut served.completion_grace) {
            served.completion_slow = true;
            let waited = served.completion.as_ref().map(|(d, s, _)| (*d, *s));
            if here.map(|(d, _, start)| (d, start)) == waited && self.focus == Focus::Editor {
                self.update_completion(text);
            }
        }
        let served = &mut self.data.served;
        if served.signature.is_some() && over(&mut served.signature_grace) {
            let waited = served.signature.as_ref().map(|(d, at, _)| (*d, *at));
            if here.map(|(d, head, _)| (d, head)) == waited {
                self.update_param_hint(true);
            }
        }
        let served = &mut self.data.served;
        if served.definition.is_some() && over(&mut served.definition_grace) {
            let Some((doc, at)) = served.definition.as_ref().map(|(d, at, _)| (*d, *at)) else {
                return;
            };
            if here.map(|(d, ..)| d) != Some(doc) {
                return;
            }
            // Where the built-in engine knows, it's jumped to (the server's answer dropped:
            // never a second jump); otherwise the server's is still waited for.
            self.refresh_symbols_now();
            if self.definition_at(at).is_some() {
                self.data.served.definition = None;
                self.goto_built_in_definition(text, at);
            }
        }
    }

    /// What came from the servers, used now (it needs the text system: a list shown, a
    /// jump, an import inserted).
    pub(super) fn use_server_answers(&mut self, text: &mut TextSystem) {
        self.after_grace(text);
        if std::mem::take(&mut self.data.served.completion_due) {
            let open = self.completion.is_open();
            let waited = self.data.served.completion_cache.is_some();
            if (open || waited) && self.focus == Focus::Editor {
                self.update_completion(text);
            }
        }
        if std::mem::take(&mut self.data.served.signature_due) {
            // Asked when `(` or `,` was typed: shown now.
            self.update_param_hint(true);
        }
        if let Some((doc, version, item)) = self.data.served.resolved.take() {
            self.add_resolved_edits(text, doc, version, &item);
        }
        if let Some((doc, at, locations)) = self.data.served.definition_answer.take() {
            self.jump_to_served(text, doc, at, locations);
        }
        self.use_navigation_answer(text);
    }

    // ─── Completion ───

    /// The start of the word being typed before `head` (letters, digits, `_` and the
    /// language's own word characters).
    fn word_start(&self, source: &str, head: usize) -> usize {
        let extra = self.intelligence_at(head).map_or_else(
            || {
                let tab = self.tabs.get(self.active);
                tab.and_then(|t| self.support.languages.get(t.syntax.language()))
                    .map_or("", |l| l.word_chars)
            },
            |i| i.word_chars(),
        );
        let head = head.min(source.len());
        source[..head]
            .char_indices()
            .rev()
            .take_while(|(_, c)| c.is_alphanumeric() || *c == '_' || extra.contains(*c))
            .last()
            .map_or(head, |(i, _)| i)
    }

    /// A server's suggestions at the cursor: ranked from what it gave for this word, or asked
    /// and waited for. `None`: no server answers here.
    pub(super) fn served_completion(&mut self, source: &str, head: usize) -> Option<Suggested> {
        let doc = self.tabs.get(self.active)?.doc.id();
        let version = self.tabs.get(self.active)?.doc.version();
        let start = self.word_start(source, head);
        let announced = self.server_place(head).map(|(_, _, a)| a.doc);
        let cached = self.data.served.completion_cache.as_ref().filter(|c| {
            Some(c.doc) == announced
                && c.start == start
                && (!c.incomplete || self.data.served.completion.is_none())
                && c.retry.is_none_or(|asked| asked == version)
        });
        if let Some(cache) = cached {
            let prefix = source.get(start..head).unwrap_or_default();
            let items = code_intelligence::servers::rank(&cache.items, prefix);
            // An incomplete list is asked again as more is typed (shown meanwhile).
            let late = cache.late;
            if cache.incomplete
                && self.data.served.completion.is_none()
                && let Some(reply) = self.ask_server(head, |s, at| s.complete(at))
            {
                // Its list is shown meanwhile: no grace to wait for.
                let served = &mut self.data.served;
                served.completion = Some((announced.unwrap_or(doc), start, reply));
                served.completion_version = version;
                served.completion_grace = None;
            }
            return Some(Suggested::Items { items, replace: start..head, late });
        }
        // Asked already for this word: waiting.
        let served = &self.data.served;
        if served.completion.as_ref().is_some_and(|(d, s, _)| Some(*d) == announced && *s == start)
        {
            let overdue = served.completion_grace.is_none_or(|at| at <= Instant::now());
            return Some(Suggested::Waiting { overdue });
        }
        let reply = self.ask_server(head, |s, at| s.complete(at))?;
        let served = &mut self.data.served;
        served.completion = Some((announced.unwrap_or(doc), start, reply));
        served.completion_version = version;
        served.completion_shown = false;
        // A server that let the last grace pass isn't waited for again until it answers.
        served.completion_grace = (!served.completion_slow).then(|| Instant::now() + SERVER_GRACE);
        Some(Suggested::Waiting { overdue: served.completion_slow })
    }

    /// The built-in engine's list was shown while a server's was waited for: the server's
    /// joins it after when it comes.
    pub(super) fn built_in_list_shown(&mut self) {
        if self.data.served.completion.is_some() {
            self.data.served.completion_shown = true;
        }
    }

    /// The list was dismissed (Esc, a character that ends the word, the word erased): the
    /// server's pending question is dropped (cancelled) and its list forgotten, so an answer
    /// coming later doesn't open it again.
    pub(super) fn dismiss_served_completion(&mut self) {
        let served = &mut self.data.served;
        served.completion = None;
        served.completion_cache = None;
        served.completion_due = false;
        served.completion_grace = None;
        served.completion_shown = false;
    }

    /// Whether suggestions belong at `head` after an erase: a word is being typed there, or
    /// it follows a character after which the language or its server suggests (`.`, `->`).
    pub(super) fn suggests_after_erase(&self, source: &str, head: usize) -> bool {
        let head = head.min(source.len());
        if self.word_start(source, head) < head {
            return true;
        }
        let Some(before) = source[..head].chars().next_back() else { return false };
        let triggers = self.intelligence().map_or("", |i| i.completion_triggers());
        triggers.contains(before)
            || self.served_triggers(head).contains(before)
            || self.assist_triggers(head).contains(before)
    }

    /// The characters after which the active document's server suggests.
    pub(super) fn served_triggers(&self, head: usize) -> String {
        let Some(servers) = self.servers() else { return String::new() };
        match self.server_place(head) {
            Some((project, _, a)) if servers.served(project, &a.language) != Engine::BuiltIn => {
                servers.completion_triggers(project, &a.language)
            }
            _ => String::new(),
        }
    }

    /// A server's suggestion accepted: its own range replaced (to the cursor), what it adds
    /// elsewhere (an import) inserted, or worked out by the server and inserted when it comes.
    pub(super) fn after_served_accept(&mut self, item: &CompletionItem, inserted_at: usize) {
        if item.resolve.is_some() && item.edits.is_empty() {
            let reply = self.ask_server(inserted_at, |s, at| s.resolve(at, item));
            if let (Some(reply), Some(tab)) = (reply, self.tabs.get(self.active)) {
                self.data.served.resolve = Some((tab.doc.id(), tab.doc.version(), reply));
            }
        }
        self.data.served.completion_cache = None;
    }

    /// A resolved suggestion's imports, inserted if the document didn't change since it
    /// was accepted (they're about that text).
    fn add_resolved_edits(
        &mut self,
        text: &mut TextSystem,
        doc: DocumentId,
        version: u64,
        item: &CompletionItem,
    ) {
        if item.edits.is_empty() {
            return;
        }
        let Some(tab) = self.tabs.get(self.active) else { return };
        if tab.doc.id() != doc || tab.doc.version() != version {
            return;
        }
        let edits: Vec<document::Edit> =
            item.edits.iter().map(|(r, t)| document::Edit::new(r.clone(), t.clone())).collect();
        self.edit(text, |doc| {
            doc.apply(document::Transaction::new(edits), None, document::EditKind::Command);
        });
    }

    // ─── Hover, signature, highlights, definition ───

    /// The server's hover card for the word at `word` (its start): its answer when it came
    /// (`Some(Some(None))`: nothing to say, or nothing yet past its grace), else asked or
    /// waited for (`Some(None)`). `None`: no server answers here.
    pub(super) fn served_hover(&mut self, word: usize) -> Option<Option<Option<HoverInfo>>> {
        let doc = self.server_place(word).map(|(_, _, a)| a.doc)?;
        if let Some((d, at, answer)) = &self.data.served.hover_answer
            && *d == doc
            && *at == word
        {
            return Some(Some(answer.clone()));
        }
        if self.data.served.hover.as_ref().is_some_and(|(d, at, _)| *d == doc && *at == word) {
            // Past its grace: nothing from it yet, the built-in engine's card (the server's
            // answer is kept for the word when it comes).
            return Some((!self.served_hover_waiting()).then_some(None));
        }
        let reply = self.ask_server(word, |s, at| s.hover(at))?;
        self.data.served.hover = Some((doc, word, reply));
        self.data.served.hover_grace = Some(Instant::now() + SERVER_GRACE);
        Some(None)
    }

    /// The server's signature at `head`: its answer when it came for there (`Some(Some(_))`,
    /// `None` inside: nothing to say, or nothing yet past its grace), else asked or waited
    /// for (`Some(None)`). `None`: no server answers here.
    pub(super) fn served_signature(
        &mut self,
        head: usize,
    ) -> Option<Option<Option<SignatureHelp>>> {
        let doc = self.server_place(head).map(|(_, _, a)| a.doc)?;
        if let Some((d, at, answer)) = self.data.served.signature_answer.take()
            && d == doc
            && at == head
        {
            return Some(Some(answer));
        }
        let served = &self.data.served;
        if served.signature.as_ref().is_some_and(|(d, at, _)| *d == doc && *at == head) {
            // Past its grace: nothing from it yet, the built-in engine's parameters.
            let waiting = served.signature_grace.is_some_and(|at| Instant::now() < at);
            return Some((!waiting).then_some(None));
        }
        let reply = self.ask_server(head, |s, at| s.signature_help(at))?;
        self.data.served.signature = Some((doc, head, reply));
        self.data.served.signature_grace = Some(Instant::now() + SERVER_GRACE);
        Some(None)
    }

    /// The occurrences of the symbol at the cursor, as the server sees them (asked once per
    /// version and word; shown while the cursor rests in one of them, nothing selected).
    pub(super) fn sync_served_highlights(&mut self) {
        let Some(tab) = self.tabs.get(self.active) else { return };
        let selection = tab.doc.selections().primary();
        if !selection.is_empty() || tab.doc.selections().len() != 1 {
            return;
        }
        let head = selection.head;
        // Only where a server answers (nothing copied otherwise).
        if self.served_at(head) == Engine::BuiltIn {
            return;
        }
        let Some((_, _, announced)) = self.server_place(head) else { return };
        let Some(tab) = self.tabs.get(self.active) else { return };
        let source = tab.text();
        let start = self.word_start(&source, head);
        let end = head
            + source[head..]
                .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
                .unwrap_or(source.len() - head);
        if start == end {
            return;
        }
        let key = (announced.doc, announced.version, start);
        let answered = self
            .data
            .served
            .highlights_answer
            .as_ref()
            .filter(|(k, _)| *k == key)
            .map(|(_, r)| r.clone());
        match answered {
            Some(ranges) => {
                let tab = &mut self.tabs[self.active];
                let shown = if ranges.len() >= 2
                    && ranges.iter().any(|r| r.start <= head && head <= r.end)
                {
                    ranges
                } else {
                    Vec::new()
                };
                if tab.occurrences != shown {
                    tab.occurrences = shown;
                    tab.marks_generation += 1;
                }
            }
            None => {
                if self.data.served.highlights.as_ref().is_none_or(|(k, _)| *k != key)
                    && let Some(reply) = self.ask_server(head, |s, at| s.highlights(at))
                {
                    self.data.served.highlights = Some((key, reply));
                }
            }
        }
    }

    /// "Go to Declaration" asked of the server: `true` if it's asked (the jump comes with
    /// its answer).
    pub(super) fn ask_served_definition(&mut self, head: usize) -> bool {
        let Some(doc) = self.server_place(head).map(|(_, _, a)| a.doc) else { return false };
        match self.ask_server(head, |s, at| s.definition(at)) {
            Some(reply) => {
                self.data.served.definition = Some((doc, head, reply));
                self.data.served.definition_grace = Some(Instant::now() + DEFINITION_GRACE);
                true
            }
            None => false,
        }
    }

    /// The server's declaration: jumped to (none: the built-in engine's, as before).
    fn jump_to_served(
        &mut self,
        text: &mut TextSystem,
        doc: DocumentId,
        at: usize,
        locations: Vec<ServedLocation>,
    ) {
        let Some(tab) = self.tabs.get(self.active) else { return };
        let here = tab.doc.path().map(Path::to_path_buf);
        if self.server_place(at).map(|(_, _, a)| a.doc) != Some(doc) {
            return;
        }
        let Some(target) = locations.into_iter().next() else {
            self.goto_built_in_definition(text, at);
            return;
        };
        if here.as_deref() != Some(target.path.as_path())
            && let Err(e) = self.open_file(&target.path)
        {
            tracing::warn!("can't open {}: {e}", target.path.display());
            return;
        }
        let offset = target.range.start;
        self.focus = Focus::Editor;
        self.jump(text, |doc| commands::click(doc, offset.min(doc.buffer().len()), false, false));
    }

    /// The places a rename at `head` changes, as the server says (on a background thread:
    /// the rename's preview waits for them there). `None`: no server answers here.
    pub(super) fn served_rename_places(
        &self,
        head: usize,
    ) -> Option<Reply<Result<Vec<ServedLocation>, String>>> {
        self.ask_server(head, |s, at| s.rename_places(at))
    }

    /// The usages of the symbol at `head` of the active document its server knows (its
    /// declaration included), where it answers references.
    pub(super) fn served_references(&self, head: usize) -> Option<Reply<Vec<ServedLocation>>> {
        self.ask_server(head, |s, at| s.references(at))
    }

    /// The languages the server answering the active document at `head` answers (its own
    /// included; just its own when no server answers there).
    pub(super) fn served_languages(&self, head: usize) -> Vec<String> {
        match (self.servers(), self.server_place(head)) {
            (Some(servers), Some((project, _, a))) => servers.languages_with(project, &a.language),
            (_, place) => place.map(|(_, _, a)| a.language).into_iter().collect(),
        }
    }

    /// The active document's language's engine now, and what the built-in engine keeps
    /// answering there (nothing where no server answers: it answers everything).
    pub(super) fn active_engine(&self) -> (Engine, Kept) {
        let none = (Engine::BuiltIn, Kept::default());
        let Some(tab) = self.tabs.get(self.active) else { return none };
        let Some(servers) = self.servers() else { return none };
        let Some(path) = tab.doc.path() else { return none };
        let Some(announced) = self.data.served.announced.get(path) else { return none };
        if self.project.is_none() {
            return none;
        }
        match servers.served(self.data.id, &announced.language) {
            Engine::BuiltIn => none,
            engine => (engine, servers.kept(self.data.id, &announced.language)),
        }
    }

    /// What the built-in engine keeps answering for the active document where its server
    /// answers (its description says: HTML's has no diagnostics, CSS's no deprecations…).
    pub(super) fn served_kept(&self) -> Kept {
        self.active_engine().1
    }

    /// The built-in engine's own syntax errors aren't shown where the server alone answers
    /// (it reports its own: they'd be twice) — unless it reports none (HTML's). Its analyses
    /// stay, but where a server's diagnostic marks the same place.
    pub(super) fn server_alone(&self) -> bool {
        let (engine, kept) = self.active_engine();
        engine == Engine::Server && !kept.diagnostics
    }
}

#[cfg(feature = "snapshot")]
impl Workbench {
    /// A language server answers the active document at its cursor (the snapshots wait
    /// for it).
    pub fn served_at_cursor(&self) -> bool {
        let head = self.tabs.get(self.active).map_or(0, |t| t.doc.selections().primary().head);
        self.served_at(head) != Engine::BuiltIn
    }

    /// How many marks the active document has (errors, warnings).
    pub fn mark_count(&self) -> usize {
        self.tabs.get(self.active).map_or(0, |t| t.diagnostics.marks.len())
    }

    /// The completion list is shown.
    pub fn completion_open(&self) -> bool {
        self.completion.is_open()
    }

    /// The open project's root.
    pub fn project_root_path(&self) -> Option<PathBuf> {
        self.project.as_ref().map(|p| p.root_path().to_path_buf())
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/servers.rs"]
mod tests;
