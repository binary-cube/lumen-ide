//! What the editor shows in the code beside the text, worked out lazily and in the
//! background, never per frame: the parameters' names at calls (inlay hints) for the
//! visible lines and a margin, and the usages / implementations after declarations (code
//! lenses). The file's language server first where it does them (its capability: vtsls,
//! clangd, intelephense…), its answer empty, an error or none: the built-in engine's.
//!
//! A frame only compares keys (the document, its version, the visible window, the index's
//! builds); a change waits a pause ([`HINTS_DELAY`], [`LENSES_DELAY`]) before anything is
//! asked; meanwhile what's shown follows the edits (its offsets moved through the
//! document's changes). The usages counted in the other files are kept per files'
//! generation: typing in one file recounts only its own text.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use code_intelligence::servers::{Polled, Reply, ServedLens};
use code_intelligence::usages::UsageQuery;
use code_intelligence::{SymbolKind, inlay::InlayHint};
use ui_workbench::{Inlay, LineLens};

use super::*;

/// How long typing pauses before the hints are worked out again (scrolling: at once).
pub(super) const HINTS_DELAY: Duration = Duration::from_millis(300);
/// How long typing pauses before the lenses are counted again.
pub(super) const LENSES_DELAY: Duration = Duration::from_millis(600);
/// The lines worked out around the visible ones (a small scroll asks nothing).
const MARGIN: usize = 60;
/// More project files than this: the usages aren't counted (only the implementations).
const MAX_COUNTED_FILES: usize = 5000;

/// What the hints are for: the document, its version, the lines (a window around the
/// visible ones, by [`MARGIN`]).
type HintKey = (DocumentId, u64, usize, usize);
/// What the lenses are for: the document, its version, the index's builds and the files'
/// generation.
type LensKey = (DocumentId, u64, u64, u64);

/// A lens part: what a click on it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LensAction {
    Usages,
    Implementations,
    Nothing,
}

/// A lens as worked out: the declaration's name offset, its parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Lens {
    pub offset: usize,
    pub parts: Vec<(String, LensAction)>,
}

/// A tab's hints and lenses, with the versions they're about.
#[derive(Debug, Default)]
pub(super) struct TabInsight {
    hints: Arc<[Inlay]>,
    hints_version: u64,
    hints_for: Option<HintKey>,
    lenses: Vec<Lens>,
    lenses_version: u64,
    lenses_for: Option<LensKey>,
    /// The lenses as the view draws them (their lines), for `lines_version`.
    pub lines: Vec<LineLens>,
    lines_version: Option<u64>,
}

/// What came back from a background job.
#[derive(Debug)]
enum Done {
    Hints(HintKey, Vec<InlayHint>),
    Lenses(LensKey, Vec<Lens>),
}

/// The project's hints and lenses being worked out: what's waited for, the jobs' answers,
/// the usages counted in its files.
#[derive(Debug)]
pub(super) struct InsightJobs {
    tx: Sender<Done>,
    rx: Receiver<Done>,
    hints_due: Option<(HintKey, Instant)>,
    hints_running: Option<HintKey>,
    hints_served: Option<(HintKey, Reply<Vec<InlayHint>>)>,
    lenses_due: Option<(LensKey, Instant)>,
    lenses_running: Option<LensKey>,
    lenses_served: Option<(LensKey, Reply<Vec<ServedLens>>)>,
    /// The usages counted in each file but the one asked about, per files' generation.
    counted: Arc<Mutex<Counted>>,
    /// How many jobs were started (the tests check it's not per frame).
    pub started: u32,
}

impl Default for InsightJobs {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            tx,
            rx,
            hints_due: None,
            hints_running: None,
            hints_served: None,
            lenses_due: None,
            lenses_running: None,
            lenses_served: None,
            counted: Arc::default(),
            started: 0,
        }
    }
}

/// Usages counted in the other files, per declaration (name, kind, the file it's in),
/// valid for one files' generation.
#[derive(Debug, Default)]
struct Counted {
    generation: u64,
    counts: HashMap<(String, SymbolKind, PathBuf), usize>,
}

/// `offset` moved through `changes` (`None`: it was inside a replaced zone).
fn moved(changes: &[document::TextChange], mut offset: usize) -> Option<usize> {
    for change in changes {
        if offset > change.start && offset < change.old_end {
            return None;
        }
        offset = change.map(offset);
    }
    Some(offset)
}

impl Workbench {
    /// Keeps the active tab's hints and lenses up to date (cheap per frame: keys
    /// compared). `true` if what's shown changed.
    pub(super) fn sync_insight(&mut self) -> bool {
        let mut changed = self.follow_edits();
        changed |= self.sync_hints();
        changed |= self.sync_lenses();
        changed
    }

    /// The hints and lenses moved through the edits since they were worked out.
    fn follow_edits(&mut self) -> bool {
        let Some(tab) = self.tabs.get_mut(self.active) else { return false };
        let version = tab.doc.version();
        let mut changed = false;
        let insight = &mut tab.insight;
        if insight.hints_version != version && !insight.hints.is_empty() {
            let hints = match tab.doc.changes_since(insight.hints_version) {
                Some(changes) => insight
                    .hints
                    .iter()
                    .filter_map(|h| {
                        Some(Inlay { offset: moved(&changes, h.offset)?, label: h.label.clone() })
                    })
                    .collect(),
                None => Vec::new(),
            };
            insight.hints = Arc::from(hints);
            changed = true;
        }
        insight.hints_version = version;
        if insight.lenses_version != version && !insight.lenses.is_empty() {
            let changes = tab.doc.changes_since(insight.lenses_version);
            insight.lenses = match changes {
                Some(changes) => std::mem::take(&mut insight.lenses)
                    .into_iter()
                    .filter_map(|l| Some(Lens { offset: moved(&changes, l.offset)?, ..l }))
                    .collect(),
                None => Vec::new(),
            };
            changed = true;
        }
        insight.lenses_version = version;
        if insight.lines_version != Some(version) || changed {
            let buffer = tab.doc.buffer();
            let mut lines: Vec<LineLens> = insight
                .lenses
                .iter()
                .map(|l| LineLens {
                    line: buffer.line_of(l.offset.min(buffer.len())) as u32,
                    parts: l.parts.iter().map(|(p, _)| p.clone()).collect(),
                })
                .collect();
            lines.dedup_by_key(|l| l.line);
            insight.lines = lines;
            insight.lines_version = Some(version);
        }
        tab.view.set_inlays(Arc::clone(&tab.insight.hints));
        changed
    }

    /// The hints' key now, if they're shown for the active tab.
    fn hint_key(&self) -> Option<HintKey> {
        let tab = self.tabs.get(self.active).filter(|t| t.editor_visible())?;
        let buffer = tab.doc.buffer();
        let visible = tab.view.visible_bytes(&tab.doc, &tab.folds);
        let first = buffer.line_of(visible.start);
        let last = buffer.line_of(visible.end);
        // Whole blocks of lines: a small scroll keeps the key.
        let from = first.saturating_sub(MARGIN) / MARGIN * MARGIN;
        let to = (last + MARGIN).div_ceil(MARGIN) * MARGIN;
        Some((tab.doc.id(), tab.doc.version(), from, to))
    }

    fn sync_hints(&mut self) -> bool {
        let enabled = crate::insight::hints();
        let Some(key) = self.hint_key().filter(|_| enabled) else {
            let tab = self.tabs.get_mut(self.active);
            if let Some(tab) = tab.filter(|t| !t.insight.hints.is_empty()) {
                tab.insight.hints = Arc::from(Vec::new());
                tab.insight.hints_for = None;
                tab.view.set_inlays(Arc::clone(&tab.insight.hints));
                return true;
            }
            return false;
        };
        let tab = &self.tabs[self.active];
        if tab.insight.hints_for == Some(key) {
            return false;
        }
        let jobs = &mut self.data.insight;
        if jobs.hints_running == Some(key)
            || jobs.hints_served.as_ref().is_some_and(|(k, _)| *k == key)
        {
            return false;
        }
        match jobs.hints_due {
            Some((due_key, at)) if due_key == key => {
                if at <= Instant::now() {
                    jobs.hints_due = None;
                    self.start_hints(key);
                }
            }
            _ => {
                // Typed: after a pause; scrolled (the same version): at once.
                let typed = tab.insight.hints_for.is_some_and(|k| k.1 != key.1);
                let delay = if typed { HINTS_DELAY } else { Duration::ZERO };
                jobs.hints_due = Some((key, Instant::now() + delay));
                if delay.is_zero() {
                    jobs.hints_due = None;
                    self.start_hints(key);
                }
            }
        }
        false
    }

    /// The hints for `key` asked of the server, else worked out by the built-in engine.
    fn start_hints(&mut self, key: HintKey) {
        let Some(tab) = self.tabs.get(self.active) else { return };
        let buffer = tab.doc.buffer();
        let last = key.3.min(buffer.line_count().saturating_sub(1));
        let range = buffer.line_start(key.2.min(last))..buffer.line_end(last);
        let head = tab.doc.selections().primary().head;
        if self.served_at(head).uses_server()
            && let Some(reply) = self.ask_server(head, |s, at| s.inlay_hints(at, range.clone()))
        {
            self.data.insight.hints_served = Some((key, reply));
            return;
        }
        self.start_built_in_hints(key, range);
    }

    fn start_built_in_hints(&mut self, key: HintKey, range: Range<usize>) {
        let Some(tab) = self.tabs.get(self.active) else { return };
        let language = tab.syntax.language().clone();
        let Some(intel) =
            self.support.intelligence.iter().find(|i| i.language() == language).cloned()
        else {
            // No engine of ours: nothing to show (and nothing asked again for this key).
            if let Some(tab) = self.tabs.get_mut(self.active) {
                tab.insight.hints_for = Some(key);
            }
            return;
        };
        let path = Self::index_path(tab);
        let text = tab.text();
        let index = self.data.symbols.read().map(|i| i.clone()).unwrap_or_default();
        let tx = self.data.insight.tx.clone();
        let wake = self.waker.as_ref().map(|w| Arc::clone(&w.0));
        self.data.insight.hints_running = Some(key);
        self.data.insight.started += 1;
        self.data.jobs.modules.spawn(
            kernel::Priority::Background,
            Some(&self.data.jobs.cancel),
            move || {
                // The file's own declarations as its text has them now (not yet in the index).
                let mut index = index;
                let mut own = intel.extract(&text);
                own.language = intel.language().to_string().into();
                index.set_file(&path, own);
                let project = code_intelligence::Project::none();
                let hints = code_intelligence::inlay::parameter_hints(
                    intel.as_ref(),
                    &index,
                    &path,
                    &text,
                    range,
                    &project,
                );
                if tx.send(Done::Hints(key, hints)).is_ok()
                    && let Some(wake) = wake
                {
                    wake();
                }
            },
        );
    }

    fn lens_key(&self) -> Option<LensKey> {
        let tab = self.tabs.get(self.active).filter(|t| t.editor_visible())?;
        let builds = self.data.indexer.as_ref().map_or(0, |i| i.builds_finished());
        Some((tab.doc.id(), tab.doc.version(), builds, self.data.generations.files()))
    }

    fn sync_lenses(&mut self) -> bool {
        let enabled = crate::insight::lenses();
        let Some(key) = self.lens_key().filter(|_| enabled) else {
            let tab = self.tabs.get_mut(self.active);
            if let Some(tab) = tab.filter(|t| !t.insight.lenses.is_empty()) {
                tab.insight.lenses.clear();
                tab.insight.lines.clear();
                tab.insight.lenses_for = None;
                return true;
            }
            return false;
        };
        let tab = &self.tabs[self.active];
        if tab.insight.lenses_for == Some(key) {
            return false;
        }
        let jobs = &mut self.data.insight;
        if jobs.lenses_running == Some(key)
            || jobs.lenses_served.as_ref().is_some_and(|(k, _)| *k == key)
        {
            return false;
        }
        match jobs.lenses_due {
            Some((due_key, at)) if due_key == key => {
                if at <= Instant::now() {
                    jobs.lenses_due = None;
                    self.start_lenses(key);
                }
            }
            _ => {
                // First shown at once; after an edit, a pause.
                let typed = tab.insight.lenses_for.is_some();
                let at = Instant::now() + if typed { LENSES_DELAY } else { Duration::ZERO };
                jobs.lenses_due = Some((key, at));
                if !typed {
                    jobs.lenses_due = None;
                    self.start_lenses(key);
                }
            }
        }
        false
    }

    fn start_lenses(&mut self, key: LensKey) {
        let Some(tab) = self.tabs.get(self.active) else { return };
        let head = tab.doc.selections().primary().head;
        if self.served_at(head).uses_server()
            && let Some(reply) = self.ask_server(head, |s, at| s.code_lens(at))
        {
            self.data.insight.lenses_served = Some((key, reply));
            return;
        }
        self.start_built_in_lenses(key);
    }

    fn start_built_in_lenses(&mut self, key: LensKey) {
        let Some(tab) = self.tabs.get(self.active) else { return };
        let language = tab.syntax.language().clone();
        let intel = self.support.intelligence.iter().find(|i| i.language() == language).cloned();
        // Only where types or functions are declared (and their usages can be found).
        let Some(intel) = intel
            .filter(|i| i.features().contains(&code_intelligence::LanguageFeature::Navigation))
        else {
            if let Some(tab) = self.tabs.get_mut(self.active) {
                tab.insight.lenses_for = Some(key);
            }
            return;
        };
        let path = Self::index_path(tab);
        let text = tab.text();
        let root = self.project.as_ref().map(|p| p.root_path().to_path_buf());
        let queries = self.queries();
        let index = self.data.symbols.read().map(|i| i.clone()).unwrap_or_default();
        let counted = Arc::clone(&self.data.insight.counted);
        let tx = self.data.insight.tx.clone();
        let wake = self.waker.as_ref().map(|w| Arc::clone(&w.0));
        self.data.insight.lenses_running = Some(key);
        self.data.insight.started += 1;
        self.data.jobs.modules.spawn(
            kernel::Priority::Background,
            Some(&self.data.jobs.cancel),
            move || {
                let input =
                    LensInput { intel, index, path, text, root, queries, generation: key.3 };
                let lenses = built_in_lenses(&input, &counted);
                if tx.send(Done::Lenses(key, lenses)).is_ok()
                    && let Some(wake) = wake
                {
                    wake();
                }
            },
        );
    }

    /// The servers' answers and the jobs' results, kept. `true` if one came (a redraw).
    pub(super) fn poll_insight(&mut self) -> bool {
        let mut changed = false;
        // The server's hints: empty or none, the built-in engine's.
        if let Some((key, reply)) = self.data.insight.hints_served.as_mut().map(|(k, r)| (*k, r)) {
            let answer = match reply.poll() {
                Polled::Waiting => None,
                Polled::Ready(hints) => Some(hints),
                Polled::Gone => Some(Vec::new()),
            };
            if let Some(hints) = answer {
                self.data.insight.hints_served = None;
                if hints.is_empty() {
                    if let Some(tab) = self.tabs.get(self.active).filter(|t| t.doc.id() == key.0) {
                        let buffer = tab.doc.buffer();
                        let last = key.3.min(buffer.line_count().saturating_sub(1));
                        let range = buffer.line_start(key.2.min(last))..buffer.line_end(last);
                        self.start_built_in_hints(key, range);
                    }
                } else {
                    changed |= self.take_hints(key, hints);
                }
            }
        }
        if let Some((key, reply)) = self.data.insight.lenses_served.as_mut().map(|(k, r)| (*k, r)) {
            let answer = match reply.poll() {
                Polled::Waiting => None,
                Polled::Ready(lenses) => Some(lenses),
                Polled::Gone => Some(Vec::new()),
            };
            if let Some(served) = answer {
                self.data.insight.lenses_served = None;
                if served.is_empty() {
                    if self.tabs.get(self.active).is_some_and(|t| t.doc.id() == key.0) {
                        self.start_built_in_lenses(key);
                    }
                } else {
                    let lenses = served
                        .into_iter()
                        .map(|l| Lens {
                            offset: l.offset,
                            parts: vec![(l.title, usages_action(true))],
                        })
                        .collect();
                    changed |= self.take_lenses(key, lenses);
                }
            }
        }
        let done: Vec<Done> = self.data.insight.rx.try_iter().collect();
        for done in done {
            changed |= match done {
                Done::Hints(key, hints) => {
                    self.data.insight.hints_running = None;
                    self.take_hints(key, hints)
                }
                Done::Lenses(key, lenses) => {
                    self.data.insight.lenses_running = None;
                    self.take_lenses(key, lenses)
                }
            };
        }
        changed
    }

    /// Hints worked out for `key`: shown in its tab (moved through the edits since).
    fn take_hints(&mut self, key: HintKey, mut hints: Vec<InlayHint>) -> bool {
        let Some(tab) =
            all_tabs_mut(&mut self.tabs, &mut self.editors.groups).find(|t| t.doc.id() == key.0)
        else {
            return false;
        };
        hints.sort_by_key(|h| h.offset);
        let inlays: Vec<Inlay> = match tab.doc.changes_since(key.1) {
            Some(changes) => hints
                .into_iter()
                .filter_map(|h| Some(Inlay { offset: moved(&changes, h.offset)?, label: h.label }))
                .collect(),
            None => Vec::new(),
        };
        tab.insight.hints = Arc::from(inlays);
        tab.insight.hints_version = tab.doc.version();
        tab.insight.hints_for = Some(key);
        tab.view.set_inlays(Arc::clone(&tab.insight.hints));
        true
    }

    /// Lenses worked out for `key`: shown in its tab (moved through the edits since).
    fn take_lenses(&mut self, key: LensKey, lenses: Vec<Lens>) -> bool {
        let Some(tab) =
            all_tabs_mut(&mut self.tabs, &mut self.editors.groups).find(|t| t.doc.id() == key.0)
        else {
            return false;
        };
        tab.insight.lenses = match tab.doc.changes_since(key.1) {
            Some(changes) => lenses
                .into_iter()
                .filter_map(|l| Some(Lens { offset: moved(&changes, l.offset)?, ..l }))
                .collect(),
            None => Vec::new(),
        };
        tab.insight.lenses.sort_by_key(|l| l.offset);
        tab.insight.lenses_version = tab.doc.version();
        tab.insight.lenses_for = Some(key);
        tab.insight.lines_version = None;
        true
    }

    /// When a pause ends and the hints or lenses are worked out.
    /// Only the editor in front's: a pause waited for by a tab no longer shown (another kind
    /// of tab in front) would be due for ever — a frame asked at every frame.
    pub(super) fn insight_deadline(&self) -> Option<Instant> {
        let jobs = &self.data.insight;
        let shown = self.tabs.get(self.active).filter(|t| t.editor_visible()).map(|t| t.doc.id());
        let ours = |doc| shown == Some(doc);
        [
            jobs.hints_due.filter(|(k, _)| ours(k.0)).map(|(_, at)| at),
            jobs.lenses_due.filter(|(k, _)| ours(k.0)).map(|(_, at)| at),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    /// Nothing is waited for or worked out (snapshots wait for it).
    #[cfg(feature = "snapshot")]
    pub fn insight_settled(&self) -> bool {
        let jobs = &self.data.insight;
        let tab = self.tabs.get(self.active);
        let done = tab.is_none_or(|t| {
            (t.insight.hints_for.is_some() || !crate::insight::hints() || !t.editor_visible())
                && (t.insight.lenses_for.is_some()
                    || !crate::insight::lenses()
                    || !t.editor_visible())
        });
        done && jobs.hints_due.is_none()
            && jobs.hints_running.is_none()
            && jobs.hints_served.is_none()
            && jobs.lenses_due.is_none()
            && jobs.lenses_running.is_none()
            && jobs.lenses_served.is_none()
    }

    /// A lens part clicked (`lens`: its index among the drawn ones): the cursor put on its
    /// declaration, its usages found or its implementations listed.
    pub(super) fn lens_clicked(&mut self, text: &mut TextSystem, lens: usize, part: usize) -> bool {
        let Some(tab) = self.tabs.get(self.active) else { return false };
        let Some(line) = tab.insight.lines.get(lens).map(|l| l.line) else { return false };
        let buffer = tab.doc.buffer();
        let found = tab
            .insight
            .lenses
            .iter()
            .find(|l| buffer.line_of(l.offset.min(buffer.len())) as u32 == line)
            .and_then(|l| Some((l.offset, l.parts.get(part)?.1)));
        let Some((offset, action)) = found else { return false };
        self.focus = Focus::Editor;
        self.edit(text, |doc| {
            commands::click(doc, offset.min(doc.buffer().len()) + 1, false, false)
        });
        match action {
            LensAction::Usages => self.run_command(workbench_model::FIND_USAGES).unwrap_or(false),
            LensAction::Implementations => self.go_to_related(text, Related::Implementations),
            LensAction::Nothing => false,
        }
    }
}

/// What a lens job reads: the file's language, the index, the file and its text, the
/// project (its files read for the usages), the files' generation.
struct LensInput {
    intel: Arc<dyn LanguageIntelligence>,
    index: code_intelligence::SymbolIndex,
    path: PathBuf,
    text: Arc<str>,
    root: Option<PathBuf>,
    queries: Option<Arc<dyn workbench_model::ProjectQueries>>,
    generation: u64,
}

/// The lenses of a file's declarations (its types, functions and methods, constructors
/// aside): how many lines use each in the project's own files (not its libraries), how
/// many types implement it / methods override it.
fn built_in_lenses(input: &LensInput, counted: &Mutex<Counted>) -> Vec<Lens> {
    let intel = input.intel.as_ref();
    let naming = intel.naming();
    let file = intel.extract(&input.text);
    let declared: Vec<&code_intelligence::Symbol> = file
        .symbols
        .iter()
        .filter(|s| matches!(s.kind, SymbolKind::Function | SymbolKind::Method) || s.kind.is_type())
        .filter(|s| !(s.kind == SymbolKind::Method && naming.is_constructor(&s.name)))
        .collect();
    if declared.is_empty() {
        return Vec::new();
    }
    let subtypes = code_intelligence::hierarchy::Subtypes::of(&input.index);
    let relative =
        input.root.as_ref().and_then(|r| input.path.strip_prefix(r).ok()).map(Path::to_path_buf);
    let usages = count_lens_usages(input, &declared, relative.as_deref(), counted);
    declared
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let mut parts = Vec::new();
            if let Some(n) = usages.as_ref().and_then(|u| u.get(i)).copied() {
                let label = match n {
                    0 => i18n::t("insight.no_usages"),
                    1 => i18n::t("insight.one_usage"),
                    n if n >= code_intelligence::usages::LIMIT => {
                        i18n::tr!("insight.usages_many", count = n)
                    }
                    n => i18n::tr!("insight.usages", count = n),
                };
                parts.push((label, usages_action(n != 0)));
            }
            let (count, key_one, key_many) = if s.kind.is_type() {
                (
                    subtypes.implementations(&s.fqn).len(),
                    "insight.one_implementation",
                    "insight.implementations",
                )
            } else if let (SymbolKind::Method, Some(container)) = (s.kind, &s.container) {
                (
                    subtypes.overrides(&input.index, container, &s.name, &naming).len(),
                    "insight.one_override",
                    "insight.overrides",
                )
            } else {
                (0, "", "")
            };
            if count > 0 {
                let label =
                    if count == 1 { i18n::t(key_one) } else { i18n::tr!(key_many, count = count) };
                parts.push((label, LensAction::Implementations));
            }
            Lens { offset: s.offset, parts }
        })
        .filter(|l| !l.parts.is_empty())
        .collect()
}

/// Each declaration's usages (lines) in the project's own files: the other files' counts
/// kept for the files' generation, this file's counted in its text now. `None`: not
/// counted (too many files, no project, its language not searchable).
fn count_lens_usages(
    input: &LensInput,
    declared: &[&code_intelligence::Symbol],
    relative: Option<&Path>,
    counted: &Mutex<Counted>,
) -> Option<Vec<usize>> {
    let queries = input.queries.as_ref()?;
    let relative = relative?;
    let language = input.intel.language().to_string();
    let (extensions, rules) = queries.usage_search(&language, SymbolKind::Class)?;
    let filter = workspace::FileFilter::code(extensions);
    let files: Vec<String> = queries
        .file_list()
        .iter()
        .filter(|f| {
            filter.accepts(f)
                && !kernel::in_dependencies(Path::new(f.as_str()))
                && Path::new(f.as_str()) != relative
        })
        .cloned()
        .collect();
    if files.len() > MAX_COUNTED_FILES {
        return None;
    }
    let query = |s: &code_intelligence::Symbol| UsageQuery {
        name: s.name.to_string(),
        kind: s.kind,
        language: language.clone(),
        declared_in: Some(relative.to_path_buf()),
        declared_line: s.line,
    };
    let all: Vec<UsageQuery> = declared.iter().map(|s| query(s)).collect();
    // This file's own text, as it is now.
    let own_path = relative.to_string_lossy().into_owned();
    let text = Arc::clone(&input.text);
    let read_own = |_: &Path| Some(text.to_string());
    let own =
        code_intelligence::usages::count_usages(&[own_path], &all, &rules, &read_own, &|| false);
    // The other files': kept for the generation, the missing ones counted (each file read
    // once for all of them).
    let mut kept = counted.lock().ok()?;
    if kept.generation != input.generation {
        kept.generation = input.generation;
        kept.counts.clear();
    }
    let key = |q: &UsageQuery| (q.name.clone(), q.kind, relative.to_path_buf());
    let missing: Vec<UsageQuery> =
        all.iter().filter(|q| !kept.counts.contains_key(&key(q))).cloned().collect();
    if !missing.is_empty() {
        let read = |p: &Path| queries.read_source(p);
        let found =
            code_intelligence::usages::count_usages(&files, &missing, &rules, &read, &|| false);
        for (q, n) in missing.iter().zip(found) {
            kept.counts.insert(key(q), n);
        }
    }
    Some(
        all.iter()
            .zip(own)
            .map(|(q, own)| {
                (own + kept.counts.get(&key(q)).copied().unwrap_or(0))
                    .min(code_intelligence::usages::LIMIT)
            })
            .collect(),
    )
}

/// What a lens counting usages does: lists them, while someone can and there are some.
fn usages_action(any: bool) -> LensAction {
    if any && workbench_model::presence::command(workbench_model::FIND_USAGES) {
        LensAction::Usages
    } else {
        LensAction::Nothing
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/insight.rs"]
mod tests;
