//! Language intelligence in the editor: completion, go to definition, the symbol index.

use super::*;

impl Workbench {
    /// The language at the cursor (in PHP: the HTML outside `<?php`, the CSS in `<style>`…).
    pub(super) fn language_at_cursor(&self) -> Option<LanguageId> {
        let tab = self.tabs.get(self.active)?;
        let head = tab.doc.selections().primary().head;
        Some(
            tab.syntax
                .snapshot()
                .map_or_else(|| tab.syntax.language().clone(), |s| s.language_at(head).clone()),
        )
    }

    /// After a typed character, the text the language inserts (`</div>`, the cursor left
    /// before it; `php` and a new line after `<?`, the cursor after it): asked of the
    /// language at the cursor, then of the file's own. Its own undo step; one that may be
    /// taken back by the next character is remembered ([`Self::retract_typed`]).
    pub(super) fn auto_close(&mut self, text: &mut TextSystem, typed: char) {
        let Some(tab) = self.tabs.get(self.active) else { return };
        let head = tab.doc.selections().primary().head;
        let own = tab.syntax.language().clone();
        let at_cursor = self.intelligence();
        let of_file = self.support.intelligence.iter().find(|i| i.language() == own).cloned();
        let asked = at_cursor.iter().chain(
            of_file
                .iter()
                .filter(|f| at_cursor.as_ref().is_none_or(|c| c.language() != f.language())),
        );
        let mut snapshot = None;
        let mut insert = None;
        for intel in asked {
            if !intel.on_type_triggers().contains(&typed) {
                continue;
            }
            let text = snapshot.get_or_insert_with(|| tab.text());
            insert = intel.on_type(text, head, typed);
            if insert.is_some() {
                break;
            }
        }
        let Some(insert) = insert else { return };
        let back = insert.text[insert.cursor.min(insert.text.len())..].chars().count();
        self.edit(text, |doc| {
            commands::insert_text(doc, &insert.text);
            for _ in 0..back {
                commands::move_cursors(doc, Motion::Left, false);
            }
        });
        if !insert.retract_on.is_empty()
            && let Some(tab) = self.tabs.get(self.active)
        {
            let head = tab.doc.selections().primary().head;
            self.typed_insert = Some((tab.doc.id(), tab.doc.version(), head, insert.retract_on));
        }
    }

    /// `typed` takes back the text the language just inserted (`=` after `<?php`'s
    /// completion: `<?=`), when nothing was done since: undone, before `typed` goes in.
    pub(super) fn retract_typed(&mut self, text: &mut TextSystem, typed: char) {
        let Some((doc, version, head, retract_on)) = self.typed_insert.take() else { return };
        let Some(tab) = self.tabs.get(self.active) else { return };
        let untouched = tab.doc.id() == doc
            && tab.doc.version() == version
            && tab.doc.selections().primary().head == head;
        if untouched && retract_on.contains(&typed) {
            self.edit(text, |doc| {
                doc.undo();
            });
        }
    }

    /// The intelligence of the language at the cursor (in PHP: PHP, not the surrounding HTML).
    pub(super) fn intelligence(&self) -> Option<Arc<dyn LanguageIntelligence>> {
        let head = self.tabs.get(self.active)?.doc.selections().primary().head;
        self.intelligence_at(head)
    }

    /// The intelligence of the language at `offset` in the active document.
    pub(super) fn intelligence_at(&self, head: usize) -> Option<Arc<dyn LanguageIntelligence>> {
        let tab = self.tabs.get(self.active)?;
        let language = tab
            .syntax
            .snapshot()
            .map_or_else(|| tab.syntax.language().clone(), |s| s.language_at(head).clone());
        self.support.intelligence.iter().find(|i| i.language() == language).cloned()
    }

    /// The path under which the active document appears in the index (unsaved documents have
    /// one too).
    pub(super) fn index_path(tab: &Tab) -> PathBuf {
        tab.doc
            .path()
            .map_or_else(|| PathBuf::from(format!("untitled:{}", tab.doc.id())), Path::to_path_buf)
    }

    /// Requests re-extracting the active document's symbols (on the symbol thread), if it
    /// changed since the last extraction. The result goes into the index, and
    /// `poll_symbols` marks the version.
    pub(super) fn refresh_symbols(&mut self) {
        let level = self.tab_level();
        let ready = self.index_ready();
        let Some(tab) = self.tabs.get(self.active) else { return };
        let version = tab.doc.version();
        if tab.symbols_version == Some(version)
            && tab.diagnostics.checked_level == level
            && tab.diagnostics.checked_ready == ready
        {
            return;
        }
        let language = tab.syntax.language();
        let Some(intel) = self.support.intelligence.iter().find(|i| i.language() == *language)
        else {
            return;
        };
        if self.data.symbol_worker.is_none() {
            let wake: Arc<dyn Fn() + Send + Sync> = match &self.waker {
                Some(w) => Arc::clone(&w.0),
                None => Arc::new(|| {}),
            };
            self.data.symbol_worker = crate::symbols::SymbolWorker::start(wake);
        }
        let Some(worker) = &mut self.data.symbol_worker else {
            self.refresh_symbols_now();
            return;
        };
        worker.request(crate::symbols::Job {
            dependencies_left_out: self.data.marks.get().excludes_dependencies(),
            doc: tab.doc.id(),
            version,
            path: Self::index_path(tab),
            text: tab.doc.buffer().snapshot(),
            intel: Arc::clone(intel),
            index: Arc::clone(&self.data.symbols),
            level,
            index_ready: ready,
        });
    }

    /// The project's index is built and not indexing: what's checked against it (an
    /// unknown class) can be said.
    pub(super) fn index_ready(&self) -> bool {
        self.data.indexer.as_ref().is_some_and(|i| !i.is_busy())
    }

    /// The language level the active file is checked against (its own exception, or the
    /// project's), if its language has levels.
    pub(super) fn tab_level(&self) -> Option<String> {
        let tab = self.tabs.get(self.active)?;
        let language = tab.syntax.language();
        let project = crate::levels::LanguageLevels::get(&self.data.levels.chosen, language)?;
        let file = self.active_relative_path();
        let level = file
            .as_deref()
            .and_then(|f| self.data.levels.file_level(f, language))
            .unwrap_or(project);
        Some(level.to_owned())
    }

    /// The type (class, interface, trait, enum) at the cursor: the one named there (`new
    /// Foo`, `Foo::`, a type hint), otherwise the one containing it. Its FQN.
    pub(super) fn type_at_cursor(&mut self) -> Option<String> {
        self.refresh_symbols_now();
        let naming = self.intelligence().map(|i| i.naming()).unwrap_or_default();
        let separator = naming.namespace_separator;
        let tab = self.tabs.get(self.active)?;
        let head = tab.doc.selections().primary().head;
        let text = tab.text();
        let index = self.data.symbols.read().ok()?;
        let file = index.file(&Self::index_path(tab))?;
        // The name under the cursor, qualified (`\App\Foo`, `Models\User`) if it is.
        if let Some(range) = code_intelligence::rename::name_at(&text, head, "", naming.sigil) {
            let start = text[..range.start]
                .char_indices()
                .rev()
                .take_while(|(_, c)| c.is_alphanumeric() || *c == '_' || Some(*c) == separator)
                .last()
                .map_or(range.start, |(i, _)| i);
            let fqn = file.resolve_class(&text[start..range.end], head, &naming);
            if let Some((_, ty)) = index.type_by_fqn(&fqn) {
                return Some(ty.fqn.to_string());
            }
        }
        (code_intelligence::scope_at(file, head)
            .into_iter()
            .rev()
            .map(|i| &file.symbols[i])
            .find(|s| s.kind.is_type())
            .map(|s| s.fqn.clone()))
        .map(String::from)
    }

    /// Re-extracts the active document's symbols now, on the UI thread (for explicitly
    /// requested actions, e.g. "go to definition" right after an edit).
    pub(super) fn refresh_symbols_now(&mut self) {
        let Some(tab) = self.tabs.get(self.active) else { return };
        if tab.symbols_version == Some(tab.doc.version()) {
            return;
        }
        let language = tab.syntax.language();
        let Some(intel) = self.support.intelligence.iter().find(|i| i.language() == *language)
        else {
            return;
        };
        // Extracted first: the index is locked only to put the result in, so its readers
        // (other threads) never wait for a parse.
        let mut symbols = intel.extract(&tab.text());
        symbols.language = intel.language().to_string().into();
        // The index may be read right now by a thread (the canvas graph, usages): the UI doesn't
        // wait for it — extraction is retried on the next refresh.
        let Ok(mut index) = self.data.symbols.try_write() else { return };
        index.set_file(&Self::index_path(tab), symbols);
        drop(index);
        let version = tab.doc.version();
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.symbols_version = Some(version);
        }
    }

    /// The versions with symbols extracted on the symbol thread. `true` if anything changed.
    pub fn poll_symbols(&mut self) -> bool {
        let Some(worker) = &self.data.symbol_worker else { return false };
        let mut changed = false;
        for analysis in worker.finished() {
            if let Some(tab) = self.tabs.iter_mut().find(|t| t.doc.id() == analysis.doc) {
                tab.symbols_version = Some(analysis.version);
                if tab.diagnostics.level_issues != analysis.issues
                    || tab.diagnostics.checked_level != analysis.level
                    || tab.diagnostics.deprecated != analysis.deprecated
                    || tab.diagnostics.unused != analysis.unused
                    || tab.diagnostics.requirements != analysis.requirements
                    || tab.diagnostics.problems != analysis.problems
                {
                    tab.diagnostics.requirements = analysis.requirements;
                    tab.diagnostics.problems = analysis.problems;
                    tab.diagnostics.level_issues = analysis.issues;
                    tab.diagnostics.deprecated = analysis.deprecated;
                    tab.diagnostics.unused = analysis.unused;
                    tab.diagnostics.issues_generation += 1;
                }
                tab.diagnostics.checked_level = analysis.level;
                tab.diagnostics.checked_ready = analysis.index_ready;
                changed = true;
            }
        }
        changed
    }

    /// Opens the project's index (the indexing module's): it loads `<project dir>/index`, reindexes
    /// what changed and keeps up to date from the project's events by itself.
    pub(super) fn start_symbol_index(&mut self, root: PathBuf) {
        self.data.indexer = None;
        let Some(provider) = self.support.index_providers.first() else { return };
        let wake: Arc<dyn Fn() + Send + Sync> = match &self.waker {
            Some(w) => Arc::clone(&w.0),
            None => Arc::new(|| {}),
        };
        // Into the handle others may hold already.
        let read = workspace::marks::ProjectMarks::read(&root, &*self.ports.disk);
        self.data.marks.replace((*read.get()).clone());
        let context = workbench_model::IndexContext {
            id: self.data.id,
            root: &root,
            events: &self.data.events,
            wake,
            intelligence: self.support.intelligence.clone(),
            marks: self.data.marks.clone(),
        };
        match provider.open(context) {
            Ok(index) => {
                self.data.symbols = index.symbols();
                self.data.indexer = Some(index);
                // A new index counts its builds from none (what was announced stays).
                self.data.indexing_seen.1 = 0;
                for tab in &mut self.tabs {
                    tab.symbols_version = None;
                }
            }
            Err(e) => tracing::warn!(error = %e, "the index didn't open"),
        }
    }

    /// The open project's symbol index (a measurement empties it to see what it held).
    #[cfg(test)]
    pub(crate) fn symbol_index(
        &self,
    ) -> Option<std::sync::Arc<std::sync::RwLock<code_intelligence::SymbolIndex>>> {
        self.data.indexer.as_ref().map(|i| i.symbols())
    }

    /// Indexing is in progress (for the status bar).
    pub fn indexing(&self) -> bool {
        self.data.indexer.as_ref().is_some_and(|i| i.is_busy())
    }

    /// `true` if the indexing state changed since the last call (the status bar).
    pub fn poll_indexer(&mut self) -> bool {
        // The model the index finds by meaning with, taken or given back (turned on / off,
        // let go while unused): an open search finds with it now.
        if self.data.indexer.as_ref().is_some_and(|i| i.sync_model()) {
            self.modal_view_command("refresh");
        }
        let busy = self.indexing();
        let builds = self.data.indexer.as_ref().map_or(0, |i| i.builds_finished());
        let (was_busy, seen) = self.data.indexing_seen;
        self.data.indexing_seen = (busy, builds);
        // A build ended since the last look (even one that started after it), or it stopped
        // being busy without one (the semantic vectors).
        let ended = builds != seen || (was_busy && !busy);
        let index = workspace::events::IndexKind::Symbols;
        let events = self.data.events.clone();
        if ended {
            if !was_busy {
                // Started and ended between two looks: announced both, in order.
                events.publish(&workspace::events::IndexingStarted { index });
            }
            // Built from the disk: the open tabs' declarations come from their text again
            // (unsaved changes count).
            for tab in all_tabs_mut(&mut self.tabs, &mut self.editors.groups) {
                tab.symbols_version = None;
            }
            events.publish(&workspace::events::IndexingFinished { index });
            self.measure_index_memory();
        }
        if busy && (ended || !was_busy) {
            events.publish(&workspace::events::IndexingStarted { index });
        }
        // Files reindexed one by one (a save, another program's change, a rename): said on
        // the project's bus, where the open documents' checks follow it.
        let updated = self.data.indexer.as_ref().map(|i| i.take_updated()).unwrap_or_default();
        if !updated.is_empty() {
            events.publish(&workspace::events::IndexUpdated { index, paths: updated });
        }
        self.recheck_after_index_updates();
        ended || busy != was_busy
    }

    /// What the symbol index takes per folder, measured in the background and said on the
    /// project's bus.
    fn measure_index_memory(&self) {
        const DEPTH: usize = 3;
        let Some(index) = self.data.indexer.as_ref().map(|i| i.symbols()) else { return };
        let Some(root) = self.project.as_ref().map(|p| p.root_path().to_path_buf()) else {
            return;
        };
        let events = self.data.events.clone();
        kernel::work::run("index-memory", move || {
            let symbols = index.read().map(|index| index.clone());
            let Ok(symbols) = symbols else { return };
            let folders = std::sync::Arc::new(symbols.memory_by_folder(&root, DEPTH));
            events.publish(&workspace::events::IndexMemoryMeasured { folders });
        });
    }

    /// The open documents whose language checks them against the project (PHP's unknown
    /// methods, argument counts) checked again on the symbols' thread when other files'
    /// declarations changed (`IndexUpdated`, heard on the project's bus): each document
    /// once, however many files changed; a document's own reindex (its save) doesn't count.
    fn recheck_after_index_updates(&mut self) {
        let changed = self.data.index_updates.take();
        if changed.is_empty() || !self.index_ready() {
            return;
        }
        let mut jobs = Vec::new();
        for tab in all_tabs(&self.tabs, &self.editors.groups) {
            let path = Self::index_path(tab);
            if changed.iter().all(|c| *c == path)
                || jobs.iter().any(|j: &crate::symbols::Job| j.doc == tab.doc.id())
            {
                continue;
            }
            let language = tab.syntax.language();
            let Some(intel) = self
                .support
                .intelligence
                .iter()
                .find(|i| i.language() == *language && i.checks_project())
            else {
                continue;
            };
            jobs.push(crate::symbols::Job {
                dependencies_left_out: self.data.marks.get().excludes_dependencies(),
                doc: tab.doc.id(),
                version: tab.doc.version(),
                path,
                text: tab.doc.buffer().snapshot(),
                intel: Arc::clone(intel),
                index: Arc::clone(&self.data.symbols),
                level: tab.diagnostics.checked_level.clone(),
                index_ready: true,
            });
        }
        if jobs.is_empty() {
            return;
        }
        let Some(worker) = self.symbol_worker() else { return };
        for job in jobs {
            worker.recheck(job);
        }
    }

    /// The symbols' thread (started on the first request).
    fn symbol_worker(&mut self) -> Option<&mut crate::symbols::SymbolWorker> {
        if self.data.symbol_worker.is_none() {
            let wake: Arc<dyn Fn() + Send + Sync> = match &self.waker {
                Some(w) => Arc::clone(&w.0),
                None => Arc::new(|| {}),
            };
            self.data.symbol_worker = crate::symbols::SymbolWorker::start(wake);
        }
        self.data.symbol_worker.as_mut()
    }

    /// Project access for intelligence: the root, the file contents (open ones from the
    /// editor, with unsaved changes) and directory listing.
    pub(super) fn with_project<R>(
        &self,
        f: impl FnOnce(&code_intelligence::Project<'_>) -> R,
    ) -> R {
        let open = |path: &Path| {
            let tab = self.tabs.iter().find(|t| t.doc.path() == Some(path))?;
            Some(tab.text_copy())
        };
        let root = self.project.as_ref().map(FileTree::root_path);
        // The project's levels, with the active file's exceptions (e.g. a script on PHP 7.4).
        let levels = self.data.levels.for_file(self.active_relative_path().as_deref());
        let kept = Some(&*self.data.disk_reads);
        crate::project_disk::with_project_kept(
            root,
            &levels,
            &self.support.intelligence,
            &open,
            kept,
            f,
        )
    }

    /// Recomputes the suggestions for the cursor position (hides them if there are none).
    pub fn update_completion(&mut self, text: &mut TextSystem) -> bool {
        let Some(tab) = self.tabs.get(self.active) else { return false };
        let source = tab.text();
        let head = tab.doc.selections().primary().head;
        self.completion_assist = None;
        // A module's help with the document first (a console's, from its source's schema),
        // then the project's frameworks (e.g. a template name in `render('…')`): their own
        // suggestions, or ones that join the language's. The assists' generation read
        // before asking: what they start in the background may end before the list is
        // made, and its end must still make the list again.
        let generation = self.assists_generation();
        let assisted = self.assist_completion(&source, head);
        if assisted.is_some() {
            self.completion_assist = Some(generation);
        }
        let framework = code_intelligence::template::join_suggestions(
            assisted,
            self.framework_completion(&source, head),
        );
        if let Some(own) = framework.as_ref().filter(|f| !f.merge) {
            let (items, replace) = (own.items.clone(), own.replace.clone());
            return self.show_completion(text, items, &source, replace);
        }
        // A language server's suggestions (the file's own language): alone, or joined with
        // the built-in engine's below.
        let served = self.served_at(head);
        let mut served_items = None;
        // The server's list came after the built-in engine's was shown for the word: it
        // joins it after (nothing moves under the cursor, the selection stays).
        let mut late = false;
        // The server's list is waited for: the built-in engine's is shown meanwhile.
        let mut waiting = false;
        if served.uses_server() {
            use super::servers::Suggested;
            match self.served_completion(&source, head) {
                // Nothing from it (a class from the project's CSS in HTML's `class="…"`): the
                // built-in engine's, as for a hover or a declaration.
                Some(Suggested::Items { items, replace, late: false })
                    if served == code_intelligence::servers::Engine::Server
                        && !items.is_empty() =>
                {
                    // What no server knows joins its list: the project's names from other
                    // languages (a stylesheet's classes in a script's `classList.add('…')`)
                    // through the built-in engine, the frameworks' (React's hooks, htmx's
                    // attributes) — or all the built-in engine's suggestions, where its
                    // language keeps them (PHP's: the union).
                    if self.served_kept().completion || self.built_in_joins_server(&source, head) {
                        served_items = Some(items);
                    } else {
                        let items = Self::with_framework_items(items, framework.as_ref());
                        return self.show_completion(text, items, &source, replace);
                    }
                }
                Some(Suggested::Items { items, late: came_late, .. }) => {
                    served_items = Some(items);
                    late = came_late;
                }
                // Asked: the list comes with the answer (the one shown stays meanwhile) —
                // within its grace; past it, the built-in engine's.
                Some(Suggested::Waiting { overdue: false })
                    if served == code_intelligence::servers::Engine::Server =>
                {
                    return self.completion.is_open();
                }
                Some(Suggested::Waiting { .. }) => waiting = true,
                None => {}
            }
        }
        // The server's suggestions where the built-in engine has none of its own (no
        // intelligence for the language, or nothing to suggest there): its alone.
        let served_alone = |wb: &mut Self, text: &mut TextSystem, served: Vec<_>| {
            let replace = served
                .iter()
                .find_map(|i: &code_intelligence::CompletionItem| i.replace.clone())
                .map_or(head..head, |r| r.start..head);
            let items = Self::with_framework_items(served, framework.as_ref());
            wb.show_completion(text, items, &source, replace)
        };
        let Some(intel) = self.intelligence() else {
            if let Some(served) = served_items.filter(|s| !s.is_empty()) {
                return served_alone(self, text, served);
            }
            // A language without an engine, its server silent here (no schema for this
            // file): what a framework knows of it is all there is.
            if let Some(f) = framework {
                return self.show_completion(text, f.items, &source, f.replace);
            }
            self.completion.close();
            return false;
        };
        // The context is cheap; the document's symbols are re-extracted only if needed.
        match intel.context(&source, head).kind {
            code_intelligence::ContextKind::None => {
                if let Some(served) = served_items.filter(|s| !s.is_empty()) {
                    return served_alone(self, text, served);
                }
                if let Some(f) = framework {
                    return self.show_completion(text, f.items, &source, f.replace);
                }
                self.completion.close();
                return false;
            }
            code_intelligence::ContextKind::Vocabulary(_) => {}
            _ => self.refresh_symbols(),
        }
        let Some(tab) = self.tabs.get(self.active) else { return false };
        let head = tab.doc.selections().primary().head;
        let path = Self::index_path(tab);
        let Ok(index) = self.data.symbols.read() else { return false };
        let (context, mut items) = self.with_project(|project| {
            code_intelligence::complete(intel.as_ref(), &index, &path, &source, head, project)
        });
        drop(index);
        code_intelligence::template::add_to_language(&mut items, &context.replace, framework);
        if let Some(served) = served_items {
            items = match late {
                true => code_intelligence::servers::merge_completions(items, served),
                false => code_intelligence::servers::merge_completions(served, items),
            };
        }
        let shown = self.show_completion(text, items, &source, context.replace);
        if waiting && self.completion.is_open() {
            self.built_in_list_shown();
        }
        shown
    }

    /// Whether the built-in engine's suggestions at `head` join a server's: there, they are
    /// the project's names of other languages (a CSS class, id or variable), which no
    /// server of this language knows.
    fn built_in_joins_server(&self, source: &str, head: usize) -> bool {
        self.intelligence().is_some_and(|intel| {
            matches!(intel.context(source, head).kind, code_intelligence::ContextKind::Symbols(_))
        })
    }

    /// A server's suggestions with the frameworks' that join a list (`merge`), after them,
    /// each replacing its own range.
    fn with_framework_items(
        served: Vec<code_intelligence::CompletionItem>,
        framework: Option<&code_intelligence::FrameworkCompletion>,
    ) -> Vec<code_intelligence::CompletionItem> {
        let Some(f) = framework.filter(|f| f.merge) else { return served };
        let own = f
            .items
            .iter()
            .cloned()
            .map(|item| code_intelligence::CompletionItem {
                replace: item.replace.clone().or_else(|| Some(f.replace.clone())),
                ..item
            })
            .collect();
        code_intelligence::servers::merge_completions(served, own)
    }

    fn show_completion(
        &mut self,
        text: &mut TextSystem,
        items: Vec<code_intelligence::CompletionItem>,
        source: &str,
        replace: std::ops::Range<usize>,
    ) -> bool {
        let prefix = source.get(replace.clone()).unwrap_or_default().to_owned();
        let caret = self.caret_rect(text).unwrap_or_default();
        let viewport = Rect::new(0.0, 0.0, self.size.width as f32, self.size.height as f32);
        self.completion.show(items, prefix, caret, viewport, self.scale);
        self.completion_replace = self.completion.is_open().then_some(replace);
        true
    }

    /// Parameter help: on `trigger` (`(` / `,` typed, a suggestion with parentheses
    /// accepted) the enclosing call's signature is looked up; while it's shown, the argument
    /// being typed follows the cursor, and leaving the call hides it. Cheap when hidden.
    pub(super) fn update_param_hint(&mut self, trigger: bool) -> bool {
        if !trigger && !self.param_hint.is_open() {
            return false;
        }
        let Some(tab) = self.tabs.get(self.active).filter(|t| t.editor_visible()) else {
            return self.param_hint.hide();
        };
        let selection = tab.doc.selections().primary();
        if tab.doc.selections().len() != 1 || !selection.is_empty() {
            return self.param_hint.hide();
        }
        let head = selection.head;
        let buffer = tab.doc.buffer();
        // Only the text just before the cursor is scanned for the call.
        let start = buffer.line_start(buffer.line_of(head.saturating_sub(4096)));
        let before = buffer.slice(start..head);
        let name_chars = self.intelligence().map_or("", |i| i.word_chars());
        let dashes = self.dash_comments();
        let Some(call) =
            code_intelligence::enclosing_call_in(&before, before.len(), name_chars, dashes)
        else {
            return self.param_hint.hide();
        };
        let open = start + call.open;
        if self.param_hint.open_at() == Some(open)
            && let Some(card) = self.param_hint.card()
        {
            let active =
                code_intelligence::active_parameter(&card.code, &card.params, call.argument);
            return self.param_hint.set_active(active);
        }
        if !trigger {
            return self.param_hint.hide();
        }
        // A language server's signature (asked: shown with its answer; nothing to say: the
        // built-in engine's).
        let served = match self.served_at(head).uses_server() {
            true => self.served_signature(head),
            false => None,
        };
        let card = match served {
            Some(None) => return false,
            Some(Some(Some(help))) => Some(ui_workbench::ParamHintCard {
                code: help.code,
                params: help.params,
                active: help.active,
            }),
            Some(Some(None)) | None => self.param_hint_card(head),
        };
        match card {
            Some(card) => {
                self.param_hint.show(open, card);
                true
            }
            None => self.param_hint.hide(),
        }
    }

    /// The active file's language has `--` line comments (SQL): a call's search skips them.
    fn dash_comments(&self) -> bool {
        self.intelligence()
            .and_then(|i| i.code_style())
            .is_some_and(|style| style.comments.contains(&"--"))
    }

    fn param_hint_card(&self, head: usize) -> Option<ui_workbench::ParamHintCard> {
        let tab = self.tabs.get(self.active)?;
        let source = tab.text();
        let name_chars = self.intelligence().map_or("", |i| i.word_chars());
        let call =
            code_intelligence::enclosing_call_in(&source, head, name_chars, self.dash_comments());
        // A module's, for the documents it knows (a console's function, from its source).
        if let Some(call) = &call
            && let Some(code) = self.assist_signature(&source, call.name.end)
            && let Some(params) = code_intelligence::parameters(&code)
        {
            let active = code_intelligence::active_parameter(&code, &params, call.argument);
            return Some(ui_workbench::ParamHintCard { code, params, active });
        }
        // A framework's function (e.g. React's `useState`).
        if let Some(call) = &call
            && let Some(info) = self.framework_hover(&source, call.name.end - 1)
            && let Some(params) = code_intelligence::parameters(&info.code)
        {
            let active = code_intelligence::active_parameter(&info.code, &params, call.argument);
            return Some(ui_workbench::ParamHintCard { code: info.code, params, active });
        }
        let intel = self.intelligence_at(head)?;
        let path = Self::index_path(tab);
        // A function the language itself provides (SQL's, by the file's dialect).
        if let Some(call) = &call
            && let Some(code) = intel.call_signature(&path, &source[call.name.clone()])
            && let Some(params) = code_intelligence::parameters(&code)
        {
            let active = code_intelligence::active_parameter(&code, &params, call.argument);
            return Some(ui_workbench::ParamHintCard { code, params, active });
        }
        // The index may be busy (being written): no help rather than waiting.
        let index = self.data.symbols.try_read().ok()?;
        let help = self.with_project(|project| {
            code_intelligence::signature_help(intel.as_ref(), &index, &path, &source, head, project)
        })?;
        Some(ui_workbench::ParamHintCard {
            code: help.code,
            params: help.params,
            active: help.active,
        })
    }

    /// Inserts the selected suggestion in place of the prefix.
    pub(super) fn accept_completion(&mut self, text: &mut TextSystem) -> bool {
        let (Some(item), Some(range)) =
            (self.completion.selected().cloned(), self.completion_replace.take())
        else {
            self.completion.close();
            return false;
        };
        self.completion.close();
        let intel = self.intelligence();
        // A server's suggestion replaces its own range (from where it says, to the cursor).
        let range = match &item.replace {
            Some(own) if own.start <= range.end => own.start..range.end,
            _ => range,
        };
        let start = range.start;
        let changed = self.edit(text, |doc| {
            let head = doc.selections().primary().head;
            commands::click(doc, range.start, false, false);
            commands::click(doc, head.max(range.start), true, false);
            commands::insert_text(doc, &item.insert);
            for _ in 0..item.cursor_back {
                commands::move_cursors(doc, Motion::Left, false);
            }
            // The import of the chosen type (e.g. `use App\Models\User;`), above; the cursor
            // stays in place (selections move along with the text).
            if let (Some(fqn), Some(intel)) = (&item.import, &intel)
                && let Some((at, statement)) = intel.import_edit(&doc.buffer().text(), fqn)
            {
                let edit = document::Edit::new(at..at, statement);
                doc.apply(
                    document::Transaction::new(vec![edit]),
                    None,
                    document::EditKind::Command,
                );
            }
            // What a server's suggestion adds before it (an import).
            let before: Vec<document::Edit> = item
                .edits
                .iter()
                .filter(|(r, _)| r.end <= start)
                .map(|(r, t)| document::Edit::new(r.clone(), t.clone()))
                .collect();
            if !before.is_empty() {
                doc.apply(document::Transaction::new(before), None, document::EditKind::Command);
            }
        });
        if changed && (item.resolve.is_some() || item.replace.is_some()) {
            self.after_served_accept(&item, start);
        }
        // A function with parentheses: its parameters right away.
        if changed && item.cursor_back > 0 {
            self.update_param_hint(true);
        }
        changed
    }

    #[cfg(feature = "snapshot")]
    /// The cursor at the end of line `line` (1-based; the last one by default), for
    /// snapshots.
    pub fn place_cursor_at_line_end(&mut self, text: &mut TextSystem, line: Option<usize>) {
        self.edit(text, |doc| {
            let buffer = doc.buffer();
            let last = buffer.line_count().saturating_sub(1);
            let line = line.map_or(last, |l| l.saturating_sub(1).min(last));
            let end = buffer.line_end(line);
            commands::click(doc, end, false, false);
        });
    }

    #[cfg(feature = "snapshot")]
    /// Selects the first occurrence of `needle` (diagnostics / snapshots).
    pub fn select_first(&mut self, text: &mut TextSystem, needle: &str) -> bool {
        let Some(tab) = self.tabs.get(self.active) else { return false };
        let Some(start) = tab.doc.buffer().text().find(needle) else { return false };
        let end = start + needle.len();
        self.edit(text, |doc| {
            commands::click(doc, start, false, false);
            commands::click(doc, end, true, false);
        })
    }

    /// Jumps to the declaration of what's under the cursor. `false` if not found.
    pub fn goto_definition(&mut self, text: &mut TextSystem) -> bool {
        let Some(head) = self.tabs.get(self.active).map(|t| t.doc.selections().primary().head)
        else {
            return false;
        };
        // A module's command for what's there (a console's table: its structure), or its
        // places to choose from (a key's `.env` files).
        let assisted =
            self.tabs.get(self.active).and_then(|t| self.assist_definition(&t.text(), head));
        match assisted {
            Some(code_intelligence::AssistTarget::Command(command)) => {
                return self.run_command_named(&command).unwrap_or(false);
            }
            Some(code_intelligence::AssistTarget::Choose { title, places }) if places.len() > 1 => {
                return self.choose_place(text, &title, places);
            }
            _ => {}
        }
        // A language server's declaration (the jump comes with its answer; none, an error, a
        // timeout: the built-in engine's) — unless a module or a framework knows where the
        // name leads (a template, a component, a route): no server does.
        let explained = self.tabs.get(self.active).is_some_and(|tab| {
            let source = tab.text();
            self.assist_definition(&source, head).is_some()
                || self.framework_definition(&source, head).is_some()
        });
        if !explained && self.served_at(head).uses_server() && self.ask_served_definition(head) {
            return true;
        }
        self.goto_built_in_definition(text, head)
    }

    /// Jumps to the declaration the built-in engine finds for what's at `head`.
    pub(super) fn goto_built_in_definition(&mut self, text: &mut TextSystem, head: usize) -> bool {
        self.refresh_symbols_now();
        let Some(target) = self.definition_at(head) else {
            tracing::debug!("no declaration found");
            return false;
        };
        let path = Self::index_path(&self.tabs[self.active]);
        let target = &target;
        if target.path != path
            && let Err(e) = self.open_file(&target.path)
        {
            tracing::warn!(path = %target.path.display(), error = %e, "can't open");
            return false;
        }
        let offset = target.offset;
        self.move_focus(Focus::Editor);
        self.jump(text, |doc| commands::click(doc, offset.min(doc.buffer().len()), false, false))
    }

    /// The declaration of the symbol at `offset` in the active document.
    pub(super) fn definition_at(&self, offset: usize) -> Option<code_intelligence::Location> {
        self.find_definition(offset, true)
    }

    /// [`definition_at`](Self::definition_at) for a hint (⌘ over a word, on every word the
    /// mouse meets): never waiting for the indexer (`None` while it writes).
    pub(super) fn definition_hint(&self, offset: usize) -> Option<code_intelligence::Location> {
        self.find_definition(offset, false)
    }

    fn find_definition(&self, offset: usize, wait: bool) -> Option<code_intelligence::Location> {
        let tab = self.tabs.get(self.active)?;
        let source = tab.text();
        if let Some(found) =
            self.assist_definition(&source, offset).as_ref().and_then(|t| t.first_place())
        {
            return Some(found.clone());
        }
        if let Some(found) = self.framework_definition(&source, offset) {
            return Some(found);
        }
        let path = Self::index_path(tab);
        // Another language's name an assist found here (a PHP class in a template's `@var`).
        if let Some((intel, target)) = self.assist_code(&source, offset) {
            let index = if wait {
                self.data.symbols.read().ok()?
            } else {
                self.data.symbols.try_read().ok()?
            };
            let locations = self.with_project(|project| {
                code_intelligence::definition_of(
                    intel.as_ref(),
                    &index,
                    &path,
                    &source,
                    offset,
                    &target,
                    project,
                )
            });
            return locations.into_iter().next();
        }
        let intel = self.intelligence_at(offset)?;
        let index = if wait {
            self.data.symbols.read().ok()?
        } else {
            self.data.symbols.try_read().ok()?
        };
        let locations = self.with_project(|project| {
            code_intelligence::definition(intel.as_ref(), &index, &path, &source, offset, project)
        });
        locations.into_iter().next()
    }

    // ─── Breadcrumb and side panel ───

    /// Re-extracts the active document's symbols after each finished parse (not on every
    /// keystroke): the breadcrumb and the structure panel use them.
    pub(super) fn refresh_symbols_after_parse(&mut self) {
        let level = self.tab_level();
        let ready = self.index_ready();
        let Some(tab) = self.tabs.get(self.active) else { return };
        let version = tab.doc.version();
        if tab.symbols_version == Some(version)
            && tab.diagnostics.checked_level == level
            && tab.diagnostics.checked_ready == ready
        {
            return;
        }
        let parsed = tab.syntax.snapshot().is_some_and(|s| s.version() == version)
            || !self.support.syntax.grammars().contains(tab.syntax.language());
        if parsed {
            self.refresh_symbols();
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/intelligence.rs"]
mod tests;
