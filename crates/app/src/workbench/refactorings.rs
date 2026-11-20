//! Refactoring in the workbench: what the symbol at the cursor is (a rename's target, with
//! the ports answering for the project — the refactoring module's `ports`), applying a
//! refactoring's plan to the files (open ones in the editor, unsaved; closed ones on disk)
//! and undoing it, every file at once. The rules are the refactor domain's use cases; here
//! they're given the tabs, the disk and the index, and what they did is announced.

use super::*;
use crate::refactor_ports::{
    Declared, IndexConflicts, IndexResolver, ModuleLanguages, ProjectSources,
};
use code_intelligence::rename;
use code_intelligence::{Location, Naming, SymbolKind};
use refactor::Plan;
use refactor::application::{Languages as _, NotRenamed, RenameContext};
use refactor::applying::{Applied, ProjectFiles};

/// The symbol a rename at the cursor renames.
type Found = rename::Subject;

impl Workbench {
    /// What a rename at the cursor renames, with the project's sources and the resolver.
    pub(super) fn rename_context(&mut self) -> Result<RenameContext, String> {
        let head = self.tabs.get(self.active).map_or(0, |t| t.doc.selections().primary().head);
        if let Some(own) = self.module_symbol_at(head) {
            return Ok(own);
        }
        self.rename_context_at(head)
    }

    /// A module's own name at `head` of the active document (a `.env` key), else a
    /// framework's (a route's), asked before the language servers and the built-in engine.
    fn module_symbol_at(&mut self, head: usize) -> Option<RenameContext> {
        if self.support.cursor_symbols.is_empty() {
            return self.framework_symbol_at(head);
        }
        let tab = self.tabs.get(self.active)?;
        let path = tab.doc.path().filter(|p| !is_virtual(p))?.to_path_buf();
        let language = self.language_at(head)?;
        let text = tab.text();
        let root = self.project.as_ref().map(|p| p.root_path().to_path_buf());
        let at = workbench_model::CursorContext {
            project: self.project.as_ref().map(|_| self.data.id),
            root: root.as_deref(),
            queries: self.queries(),
            open: self.open_texts(),
            path: &path,
            language: language.as_str(),
            text: &text,
            offset: head,
        };
        self.support
            .cursor_symbols
            .iter()
            .find_map(|s| s.rename_context(&at))
            .or_else(|| self.framework_symbol_at(head))
    }

    /// A framework's own name at `head` of the active document (a route's in `path('…')`):
    /// renamed where the framework places it ([`workbench_model::framework_names`]).
    fn framework_symbol_at(&mut self, head: usize) -> Option<RenameContext> {
        let queries = self.queries()?;
        let source = self.tabs.get(self.active)?.text();
        let (support, name) = self.framework_name(&source, head)?;
        let open = self.open_texts();
        Some(workbench_model::framework_names::rename_context(support, name, queries, open))
    }

    /// The open documents' text (unsaved changes included), by path — a snapshot for a
    /// background thread.
    fn open_texts(&self) -> Arc<HashMap<PathBuf, String>> {
        let open = all_tabs(&self.tabs, &self.editors.groups).filter_map(|t| {
            Some((t.doc.path().filter(|p| !is_virtual(p))?.to_path_buf(), t.doc.buffer().text()))
        });
        Arc::new(open.collect())
    }

    /// Renames tab `i`'s file to `name` as a refactoring when a module takes it over (the
    /// type named after it renamed with it, PSR-4: its preview opens). `false`: none did
    /// (a plain rename then).
    pub fn rename_file(&mut self, i: usize, name: &str) -> bool {
        let Some(path) = self.tabs.get(i).and_then(|t| t.doc.path()).map(Path::to_path_buf) else {
            return false;
        };
        self.activate(i);
        let support = Arc::clone(&self.support);
        support.file_refactorings.iter().any(|f| f.rename(self, &path, name))
    }

    /// Renames entry `from` to `to` (in its folder: the tree's Rename…) as a tab's file is
    /// renamed: a file holding the type named after it as a refactoring when a module takes
    /// it over (the type renamed with it: its preview, the file opened for it), anything
    /// else just renamed (its tabs, or those under a folder, following); why not, told.
    /// `true`: renamed, or its preview opened.
    pub fn rename_entry(&mut self, from: &Path, to: &Path) -> bool {
        let name = to.file_name().map(|n| n.to_string_lossy().into_owned());
        let Some(name) = name.filter(|_| to.parent() == from.parent()) else { return false };
        if !self.is_dir(from) && self.has_type_named_after(from) {
            let open =
                all_tabs(&self.tabs, &self.editors.groups).any(|t| t.doc.path() == Some(from));
            let opened = open || self.open_file(from).is_ok();
            if let Some(i) = self.tabs.iter().position(|t| t.doc.path() == Some(from))
                && opened
                && self.rename_file(i, &name)
            {
                return true;
            }
        }
        match self.rename_path(from, &name) {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!(error = %e, "the rename failed");
                let reason = match e.kind() {
                    std::io::ErrorKind::AlreadyExists => i18n::t("files.name_taken"),
                    _ => e.to_string(),
                };
                let message = i18n::tr!(
                    "files.rename_failed",
                    name = kernel::paths::file_name(from),
                    reason = reason
                );
                workbench_model::WorkbenchHost::notify(
                    self,
                    workbench_model::Notification::error(message),
                );
                false
            }
        }
    }

    /// File `path` holds the type named after it (its language keeps one per file: PSR-4),
    /// as the index knows it.
    fn has_type_named_after(&self, path: &Path) -> bool {
        let languages = ModuleLanguages(&self.support.intelligence);
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or_default();
        if !languages.type_per_file(extension) {
            return false;
        }
        self.data.symbols.read().is_ok_and(|index| {
            index.file(path).is_some_and(|file| {
                refactor::application::type_named_after(path, file, &languages).is_some()
            })
        })
    }

    /// What renaming the type in file `path` named after it renames (its language keeps one
    /// type per file): `None` if there's no such type, or the file isn't the active tab's.
    pub(super) fn rename_type_of_file(&mut self, path: &Path) -> Option<RenameContext> {
        let support = Arc::clone(&self.support);
        let languages = ModuleLanguages(&support.intelligence);
        let active = self.tabs.get(self.active).and_then(|t| t.doc.path());
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or_default();
        if active != Some(path) || !languages.type_per_file(extension) {
            return None;
        }
        path.file_stem().and_then(|s| s.to_str())?;
        self.refresh_symbols_now();
        let offset = self.data.symbols.read().ok().and_then(|index| {
            refactor::application::type_named_after(path, index.file(path)?, &languages)
        })?;
        self.rename_context_at(offset).ok()
    }

    /// What Find Usages at the cursor looks for (plan 46a): the symbol a rename there
    /// renames, its places the language server's references (where it answers them, the
    /// built-in engine's in the other languages' files — Blade, Twig, a script's function in
    /// a page) — joined with the built-in engine's in the server's own files too where it
    /// keeps renaming (phpactor's: each finds some the other misses) or both engines answer;
    /// the built-in engine's alone when the server has nothing (an error, a timeout). Asked
    /// whatever the rename does: a server without rename (intelephense without a licence key)
    /// still finds the usages.
    pub(super) fn usages_context(&mut self) -> Result<RenameContext, String> {
        let head = self.tabs.get(self.active).map_or(0, |t| t.doc.selections().primary().head);
        if let Some(own) = self.module_symbol_at(head) {
            return Ok(own);
        }
        let engine = self.served_at(head);
        let served = if engine.uses_server() { self.served_references(head) } else { None };
        let Some(reply) = served else { return self.rename_context_at(head) };
        let joined =
            engine == code_intelligence::servers::Engine::Both || self.served_kept().rename;
        let places = crate::refactor_ports::ServedPlaces::References(reply);
        self.served_context(head, places, joined)
    }

    /// What a rename at `head` of the active document renames: the places its language
    /// server says (waited for by the preview, in the background; none: the built-in
    /// engine's), else — or where the built-in engine keeps renaming (CSS: it finds the uses
    /// in HTML, PHP, Blade too) — the built-in engine's.
    fn rename_context_at(&mut self, head: usize) -> Result<RenameContext, String> {
        let served = if self.served_at(head).uses_server() && !self.served_kept().rename {
            self.served_rename_places(head)
        } else {
            None
        };
        let Some(reply) = served else { return self.rename_context_built_in(head) };
        self.served_context(head, crate::refactor_ports::ServedPlaces::Rename(reply), false)
    }

    /// The symbol at `head` with its places as the server answers them (`places`): its own
    /// in the files of its languages (`joined`: the built-in engine's there too), the
    /// built-in engine's in the others'.
    fn served_context(
        &mut self,
        head: usize,
        reply: crate::refactor_ports::ServedPlaces,
        joined: bool,
    ) -> Result<RenameContext, String> {
        let built_in = self.rename_context_built_in(head);
        let tab = self.tabs.get(self.active).ok_or_else(|| i18n::t("refactor.nothing_here"))?;
        let source = tab.text();
        let word_chars = self.intelligence_at(head).map_or("$", |i| i.word_chars());
        let name = code_intelligence::word_at(&source, head, word_chars)
            .map(|r| source[r].to_owned())
            .ok_or_else(|| i18n::t("refactor.nothing_here"))?;
        let open = self.open_texts();
        let (mut target, fallback, conflicts) = match built_in {
            Ok(c) => (c.target, Some((c.sources, c.resolver)), c.conflicts),
            Err(_) => (
                refactor::application::RenameTarget {
                    name: name.clone(),
                    style: refactor::NameStyle { extra: "", sigil: None },
                    scope: refactor::application::Scope::Project,
                    file: None,
                },
                None,
                Arc::new(crate::refactor_ports::NoConflicts)
                    as Arc<dyn refactor::application::ConflictCheck + Send + Sync>,
            ),
        };
        // The server's places are wherever they are (a local's are in its file anyway); the
        // built-in engine's local scope kept for when it answers instead.
        let local =
            match std::mem::replace(&mut target.scope, refactor::application::Scope::Project) {
                refactor::application::Scope::Local { path, range } => Some((path, range)),
                refactor::application::Scope::Project => None,
            };
        target.name = name;
        if target.style.extra.is_empty() && word_chars.contains('$') {
            target.style = refactor::NameStyle { extra: "$", ..target.style };
        }
        let covered = self.served_files(head);
        let shared = Arc::new(
            crate::refactor_ports::ServedRename::new(reply, open, fallback, covered, local)
                .joined(joined),
        );
        Ok(RenameContext {
            target,
            sources: Arc::new(crate::refactor_ports::ServedSources(Arc::clone(&shared))),
            resolver: Arc::new(crate::refactor_ports::ServedResolver(shared)),
            conflicts,
        })
    }

    /// The files the server answering the active document at `head` knows: those of the
    /// languages it answers (by their names, as the editor detects them).
    fn served_files(&self, head: usize) -> crate::refactor_ports::Covered {
        let languages = self.served_languages(head);
        let support = Arc::clone(&self.support);
        Arc::new(move |path: &Path| {
            let language = support.languages.detect(path, "");
            languages.iter().any(|l| l == language.as_str())
        })
    }

    /// What a rename at `head` of the active document renames, as the built-in engine finds.
    fn rename_context_built_in(&mut self, head: usize) -> Result<RenameContext, String> {
        let nothing = || i18n::t("refactor.nothing_here");
        self.refresh_symbols_now();
        let tab = self.tabs.get(self.active).ok_or_else(nothing)?;
        let path = tab.doc.path().filter(|p| !is_virtual(p)).map(Path::to_path_buf);
        let path = path.ok_or_else(nothing)?;
        let source = tab.text();
        let found = self.symbol_to_rename(&path, &source, head).ok_or_else(nothing)?;
        let root = self.project.as_ref().map(|p| p.root_path().to_path_buf());
        let support = Arc::clone(&self.support);
        let languages = ModuleLanguages(&support.intelligence);
        let target =
            refactor::application::rename_target(&found, &path, root.as_deref(), &languages)
                .map_err(|NotRenamed::InLibrary| i18n::t("refactor.in_library"))?;

        let kind = found.declared.kind;
        let extensions = kind.map_or(&[][..], |k| {
            crate::refactor_ports::extensions(&support.intelligence, &found.language, k)
        });
        let (sources, resolver) = self.project_ports(&path, extensions, found.declared.clone());
        let naming = languages.naming(&found.declared.at.path);
        let conflicts = IndexConflicts {
            index: Arc::clone(&self.data.symbols),
            target: found.declared,
            local: found.local.clone().map(|range| (source.to_string(), range)),
            naming,
        };
        Ok(RenameContext {
            target,
            sources: Arc::new(sources),
            resolver: Arc::new(resolver),
            conflicts: Arc::new(conflicts),
        })
    }

    /// The project's sources (the files with `extensions`; without a project, `path` alone)
    /// and the resolver for `declared` — snapshots for a background thread.
    fn project_ports(
        &mut self,
        path: &Path,
        extensions: &'static [&'static str],
        declared: Declared,
    ) -> (ProjectSources, IndexResolver) {
        let root = self.project.as_ref().map(|p| p.root_path().to_path_buf());
        let sources = self.project_sources(path, extensions);
        let resolver = IndexResolver {
            root,
            index: Arc::clone(&self.data.symbols),
            intelligence: self.support.intelligence.clone(),
            open: Arc::clone(&sources.open),
            levels: self.data.levels.for_file(self.active_relative_path().as_deref()),
            target: declared,
        };
        (sources, resolver)
    }

    /// The project's files with `extensions` (without a project, `path` alone), open ones as
    /// in the editor — a snapshot for a background thread.
    pub(super) fn project_sources(
        &mut self,
        path: &Path,
        extensions: &'static [&'static str],
    ) -> ProjectSources {
        let root = self.project.as_ref().map(|p| p.root_path().to_path_buf());
        let open = self.open_texts();
        // The project's files through its queries; without a project, the file alone.
        let queries = self.queries();
        let base = root.unwrap_or_else(|| path.parent().map(Path::to_path_buf).unwrap_or_default());
        let only = queries.is_none().then(|| path.to_path_buf());
        let filter = workspace::FileFilter::code(extensions);
        ProjectSources { root: base, queries, only, filter, open }
    }

    /// What a Change Signature at the cursor changes: the function / method there (or used
    /// there), or a class's constructor (on the class or `new X`).
    pub(super) fn signature_context(
        &mut self,
    ) -> Result<refactor::signature::SignatureContext, String> {
        use refactor::signature::{CallKind, SignatureContext, SignatureTarget};
        let nothing = || i18n::t("refactor.no_function");
        self.refresh_symbols_now();
        let tab = self.tabs.get(self.active).ok_or_else(nothing)?;
        let path = tab.doc.path().filter(|p| !is_virtual(p)).map(Path::to_path_buf);
        let path = path.ok_or_else(nothing)?;
        let source = tab.text();
        let head = tab.doc.selections().primary().head;
        let found = self.symbol_to_rename(&path, &source, head).ok_or_else(nothing)?;
        let kind = found.declared.kind.ok_or_else(nothing)?;
        let naming = self.naming(&found.language);
        let is_constructor = |name: &str| naming.is_constructor(name);
        // (what the calls name, how they look, the declaration, what `new X` resolves to)
        let (name, call_kind, decl, resolved) = {
            let index = self.data.symbols.read().map_err(|_| nothing())?;
            let class_of = |fqn: &str| {
                index.type_by_fqn(fqn).map(|(p, s)| {
                    let at = Location { path: p.to_path_buf(), offset: s.offset, line: s.line };
                    (
                        s.name.clone(),
                        Declared {
                            at,
                            kind: Some(s.kind),
                            fqn: s.fqn.to_string(),
                            container: None,
                        },
                    )
                })
            };
            match kind {
                SymbolKind::Function => {
                    (found.name.clone(), CallKind::Function, found.declared.clone(), None)
                }
                SymbolKind::Method if is_constructor(&found.name) => {
                    let class = found.declared.container.as_deref().ok_or_else(nothing)?;
                    let (name, declared) = class_of(class).ok_or_else(nothing)?;
                    (
                        name.to_string(),
                        CallKind::Constructor,
                        found.declared.clone(),
                        Some(declared),
                    )
                }
                SymbolKind::Method => {
                    (found.name.clone(), CallKind::Method, found.declared.clone(), None)
                }
                k if k.is_type() => {
                    let (p, s) = index
                        .members_of(&found.declared.fqn)
                        .into_iter()
                        .find(|(_, s)| is_constructor(&s.name))
                        .ok_or_else(|| i18n::t("refactor.no_constructor"))?;
                    let at = Location { path: p.to_path_buf(), offset: s.offset, line: s.line };
                    let decl = Declared {
                        at,
                        kind: Some(s.kind),
                        fqn: s.fqn.to_string(),
                        container: s.container.clone(),
                    };
                    (found.name.clone(), CallKind::Constructor, decl, Some(found.declared.clone()))
                }
                _ => return Err(nothing()),
            }
        };
        let root = self.project.as_ref().map(|p| p.root_path().to_path_buf());
        if refactor::application::in_library(root.as_deref(), &decl.at.path) {
            return Err(i18n::t("refactor.in_library"));
        }
        // The declaration's parameter list, read with the language's syntax.
        let text = all_tabs(&self.tabs, &self.editors.groups)
            .find(|t| t.doc.path() == Some(decl.at.path.as_path()))
            .map(|t| t.doc.buffer().text())
            .or_else(|| read_text(&*self.ports.disk, &decl.at.path))
            .ok_or_else(nothing)?;
        // The language's syntax, as its module contributed it (by the declaration's file).
        let extension = decl.at.path.extension().and_then(|e| e.to_str()).unwrap_or_default();
        let syntax = self
            .support
            .signatures
            .iter()
            .find(|s| s.extensions().contains(&extension))
            .cloned()
            .ok_or_else(nothing)?;
        let open = decl.at.offset
            + text.get(decl.at.offset..).and_then(|t| t.find('(')).ok_or_else(nothing)?;
        let (params, items) = refactor::signature::split_list(&text, open).ok_or_else(nothing)?;
        let old = items.iter().map(|r| syntax.parse(&text[r.clone()])).collect();
        let lookup = if call_kind == CallKind::Constructor { SymbolKind::Class } else { kind };
        let extensions =
            crate::refactor_ports::extensions(&self.support.intelligence, &found.language, lookup);
        let (sources, resolver) =
            self.project_ports(&path, extensions, resolved.unwrap_or_else(|| decl.clone()));
        Ok(SignatureContext {
            target: SignatureTarget {
                name,
                kind: call_kind,
                path: decl.at.path.clone(),
                params,
                old,
            },
            sources: Arc::new(sources),
            resolver: Arc::new(resolver),
            syntax,
        })
    }

    /// How `language` writes the names a rename changes (its module says).
    fn naming(&self, language: &str) -> Naming {
        let module = self.support.intelligence.iter().find(|i| i.language().as_str() == language);
        module.map(|i| i.naming()).unwrap_or_default()
    }

    /// The symbol at `head` of `path`: a declaration there, the declaration of what's
    /// used there, or a local variable (the domain's [`code_intelligence::rename`]).
    fn symbol_to_rename(&self, path: &Path, source: &str, head: usize) -> Option<Found> {
        // Found before the index is read for the rest (it reads it itself).
        let definition = self.definition_at(head);
        let index = self.data.symbols.read().ok()?;
        // The file's language when it isn't indexed: by its extension.
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or_default();
        let language = self
            .support
            .intelligence
            .iter()
            .find(|i| i.extensions().iter().any(|e| e.eq_ignore_ascii_case(extension)))
            .map(|i| i.language().as_str().into())
            .unwrap_or_default();
        let naming_of = |language: &str| self.naming(language);
        rename::subject_at(&index, path, source, head, language, definition, &naming_of)
    }

    /// Applies `plan`'s chosen changes, as one batch of changes. Returns how many places
    /// changed; nothing is when a file it changes is open locked (read-only: `Err` says which).
    pub(super) fn apply_refactoring(&mut self, plan: &Plan) -> Result<usize, String> {
        let edited: Vec<PathBuf> = plan
            .files
            .iter()
            .filter(|f| f.occurrences.iter().any(|o| o.chosen))
            .map(|f| f.path.clone())
            .collect();
        self.refuse_locked_files(&edited)?;
        self.as_one_batch(|wb| wb.apply_plan(plan))
    }

    /// Undoes the last refactoring, as one batch of changes (see [`Self::undo_plan`]). A
    /// file it changed open locked: nothing is undone (the user told; it can be later).
    pub(super) fn undo_refactoring(&mut self) -> Option<usize> {
        let files = self.data.refactoring_undo.as_ref().map(|u| u.files()).unwrap_or_default();
        if let Err(why) = self.refuse_locked_files(&files) {
            workbench_model::WorkbenchHost::notify(
                self,
                workbench_model::Notification::warning(why),
            );
            return None;
        }
        self.as_one_batch(Self::undo_plan)
    }

    /// Runs `f` announced as one batch of changes: whoever follows the files (the index, the
    /// checkers, Git, the local copy's uploads) waits for its end and takes them as one,
    /// instead of reacting to each file.
    fn as_one_batch<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let id = uuid::Uuid::new_v4().as_u128();
        self.data.events.publish(&workspace::events::BulkChangeStarted { id });
        let result = f(self);
        self.data.events.publish(&workspace::events::BulkChangeEnded { id });
        result
    }

    fn apply_plan(&mut self, plan: &Plan) -> Result<usize, String> {
        let applied = refactor::applying::apply_plan(plan, &mut WorkbenchFiles(self));
        for problem in &applied.problems {
            tracing::warn!(%problem, "a refactoring's change wasn't made");
        }
        if applied.changed_nothing() {
            return Err(i18n::t("refactor.stale"));
        }
        let Applied { places, files, skipped, undo, .. } = applied;
        self.reindex_changed(undo.files(), undo.renames());
        let message = match undo.renames() {
            // A plain move: what moved.
            [(_, to)] if places == 0 => {
                let name = kernel::paths::file_name(to);
                i18n::tr!("refactor.moved", name = name.as_str())
            }
            _ => i18n::tr!("refactor.applied", places = places, files = files),
        };
        self.data.refactoring_undo = Some(undo);
        let notification = if skipped > 0 {
            workbench_model::Notification::warning(message)
                .with_detail(i18n::tr!("refactor.skipped", count = skipped))
        } else {
            workbench_model::Notification::success(message)
        };
        workbench_model::WorkbenchHost::notify(self, notification);
        Ok(places)
    }

    /// Undoes the last refactoring: the renames back, every file's text as it was — except
    /// the files changed since (their work is kept): how many those were.
    fn undo_plan(&mut self) -> Option<usize> {
        let undo = self.data.refactoring_undo.take()?;
        let undone = refactor::applying::undo_plan(undo, &mut WorkbenchFiles(self));
        for problem in &undone.problems {
            tracing::warn!(%problem, "a refactoring's change wasn't undone");
        }
        // What changed back (for the index): the files at their names again.
        self.reindex_changed(undone.files, &undone.renames);
        Some(undone.skipped)
    }

    /// Writes a file that isn't open (a refactoring's change, or its undo) as a save does:
    /// atomically, the local history keeping what was there, announced.
    fn write_closed(&mut self, path: &Path, text: &str) -> io::Result<()> {
        let event = document::events::FileWillSave { path: path.to_path_buf() };
        self.data.events.publish(&event);
        document::DocumentStorage::write(&*self.ports.disk, path, text.as_bytes())?;
        self.data.disk.written(text.len() as u64);
        let event = document::events::FileSaved { path: path.to_path_buf() };
        self.data.events.publish(&event);
        Ok(())
    }

    /// The refactored files: the open ones' declarations from their tabs at once, the others
    /// announced (`RefactoringApplied`: the index takes them without waiting for quiet).
    fn reindex_changed(&mut self, files: Vec<PathBuf>, renames: &[(PathBuf, PathBuf)]) {
        // A moved folder: each indexed file in it, moved.
        let mut renamed = Vec::new();
        if let Ok(index) = self.data.symbols.read() {
            for (from, to) in renames {
                if self.ports.disk.kind(to) != Some(NodeKind::Directory) {
                    renamed.push((from.clone(), to.clone()));
                    continue;
                }
                for (path, _) in index.files() {
                    if let Some(new) = workspace::clipboard::moved_path(path, from, to) {
                        renamed.push((path.to_path_buf(), new));
                    }
                }
            }
        }
        // Edited files under their name after the renames.
        let now = |path: &PathBuf| {
            renamed.iter().find(|(from, _)| from == path).map_or(path, |(_, to)| to).clone()
        };
        let changed: Vec<PathBuf> = files.iter().map(now).collect();
        // The open files were edited in their tabs (unsaved): their declarations from their
        // text, at once (a few files); the index reads the others from disk (and these once
        // saved) — it would read the old text for these.
        let open: Vec<(PathBuf, String)> = changed
            .iter()
            .filter_map(|path| {
                let tab = all_tabs(&self.tabs, &self.editors.groups)
                    .find(|t| t.doc.path() == Some(path.as_path()))?;
                Some((path.clone(), tab.doc.buffer().text()))
            })
            .collect();
        // Extracted first: the index is locked only to put them in.
        let extracted: Vec<(&PathBuf, code_intelligence::FileSymbols)> = open
            .iter()
            .filter_map(|(path, text)| {
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                let intel = self
                    .support
                    .intelligence
                    .iter()
                    .find(|i| i.extensions().iter().any(|e| e.eq_ignore_ascii_case(ext)))?;
                let mut symbols = intel.extract(text);
                symbols.language = intel.language().to_string().into();
                Some((path, symbols))
            })
            .collect();
        if let Ok(mut index) = self.data.symbols.write() {
            for (path, symbols) in extracted {
                index.set_file(path, symbols);
            }
        }
        let unsaved = open.into_iter().map(|(path, _)| path).collect();
        for tab in all_tabs_mut(&mut self.tabs, &mut self.editors.groups) {
            tab.symbols_version = None;
        }
        let event = refactor::events::RefactoringApplied { changed, unsaved, renamed };
        self.data.events.publish(&event);
    }

    /// Renames file `from` to `to` (not over an existing one); its tabs follow.
    pub(super) fn move_file(&mut self, from: &Path, to: &Path) -> std::io::Result<()> {
        refactor::applying::rename_entry(&*self.ports.disk, from, to)?;
        if let Some(tree) = &mut self.project {
            for dir in [from.parent(), to.parent()].into_iter().flatten() {
                let _ = workspace::refresh_dir(
                    tree,
                    dir,
                    &listing::ListedFs::new(&*self.ports.disk, &self.support.project_services),
                );
            }
        }
        // Announced: the indexes (the paths among them) follow, and whoever keeps the file by
        // its path (a console's module).
        self.data.events.publish(&workspace::events::FileRenamed {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
        });
        // The tabs of the file, or of the files in the folder, follow it (their views and
        // bars too).
        self.follow_move(from, to);
        Ok(())
    }
}

/// The workbench's files as a refactoring changes them: the open ones in their tabs (the
/// first tab showing the file), the closed ones on disk, written as a save does.
struct WorkbenchFiles<'a>(&'a mut Workbench);

impl ProjectFiles for WorkbenchFiles<'_> {
    fn open_text(&self, path: &Path) -> Option<String> {
        let tab =
            all_tabs(&self.0.tabs, &self.0.editors.groups).find(|t| t.doc.path() == Some(path))?;
        Some(tab.doc.buffer().text())
    }

    fn edit_open(&mut self, path: &Path, edits: &[refactor::TextEdit]) {
        let wb = &mut *self.0;
        let tab =
            all_tabs_mut(&mut wb.tabs, &mut wb.editors.groups).find(|t| t.doc.path() == Some(path));
        if let Some(tab) = tab {
            let edits = edits
                .iter()
                .map(|e| document::Edit::new(e.range.clone(), e.text.clone()))
                .collect();
            tab.doc.apply(document::Transaction::new(edits), None, document::EditKind::Command);
        }
    }

    fn replace_open(&mut self, path: &Path, text: String) {
        let wb = &mut *self.0;
        let tab =
            all_tabs_mut(&mut wb.tabs, &mut wb.editors.groups).find(|t| t.doc.path() == Some(path));
        if let Some(tab) = tab {
            let len = tab.doc.buffer().len();
            tab.doc.apply(
                document::Transaction::single(0..len, text),
                None,
                document::EditKind::Command,
            );
        }
    }

    fn read(&self, path: &Path) -> Option<String> {
        read_text(&*self.0.ports.disk, path)
    }

    fn write(&mut self, path: &Path, text: &str) -> io::Result<()> {
        self.0.write_closed(path, text)
    }

    fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()> {
        self.0.move_file(from, to)
    }
}
