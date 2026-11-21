//! "Search in Project" as this module's view, over the window (a modal the app opens with
//! ⌘⇧F): the panel (`search_panel`), searched through the project's queries (its text and
//! its folders), with semantic search's model. A result opens at its line, a
//! folder shows in the tree; the searches after which something was opened are kept per
//! project (by its identity), for the next time.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use kernel::Point;
use text_search::{SearchQuery, Strategy};
use ui_components::control::{Control, Controls, NOWHERE};
use ui_components::tool_panel::KeyReach;
use ui_components::{
    AppRequest, Painter, Viewer, ViewerAction, ViewerCursor, ViewerKey, ViewerKind, ViewerPaint,
    ViewerProvider,
};
use workbench_model::ProjectQueries;
use workspace::{FileMatch, PathQuery};

use crate::exact::{ExactSearch, Found};
use crate::search_panel::{
    ExactState, ProjectSearch, ProjectSearchAction, SUGGESTIONS, focus_line,
};
use crate::semantic::{SearchMode, SemanticSearch};

mod panel_part;

use panel_part::{Ask, PanelPart};

/// The extension of the view (the app opens it over the window).
pub const SEARCH_VIEW: &str = "view-project-search";

/// The folders listed above the files.
const FOLDERS: usize = 6;
/// The files whose name matches, listed first.
const NAMED: usize = 4;
/// The files listed for `name:line`.
const FILES: usize = 20;

/// A file found by its name for `name:line`, as a result at that line (its badge shows it).
fn at_line(path: String, line: u32) -> text_search::Hit {
    text_search::Hit { path, start_line: line, end_line: line, preview: String::new(), score: 0.0 }
}

/// What the view is opened with: the project (its identity), its queries, semantic search's
/// model, the editor's selection (on one line) to search for and the folder to search in
/// (relative to the project; a folder's "Find in Files…").
#[derive(Clone)]
pub struct SearchData {
    pub project: kernel::ProjectId,
    pub queries: Arc<dyn ProjectQueries>,
    pub semantic: Option<Arc<dyn SemanticSearch>>,
    pub selection: Option<String>,
    pub folder: Option<String>,
    /// Opened to replace (its row shown).
    pub replace: bool,
}

impl std::fmt::Debug for SearchData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchData")
            .field("root", &self.queries.root())
            .field("selection", &self.selection)
            .field("folder", &self.folder)
            .finish_non_exhaustive()
    }
}

/// The recent searches of each open project, by its identity (never its name or path).
fn recent() -> &'static Mutex<HashMap<kernel::ProjectId, Vec<String>>> {
    static RECENT: OnceLock<Mutex<HashMap<kernel::ProjectId, Vec<String>>>> = OnceLock::new();
    RECENT.get_or_init(Mutex::default)
}

/// The files' mask last used in each open project (by its identity): the search opens with
/// it again.
fn masks() -> &'static Mutex<HashMap<kernel::ProjectId, String>> {
    static MASKS: OnceLock<Mutex<HashMap<kernel::ProjectId, String>>> = OnceLock::new();
    MASKS.get_or_init(Mutex::default)
}

/// The folders searched in last in each open project (by its identity), newest first: the
/// folder's field suggests them while it's empty.
fn recent_folders() -> &'static Mutex<HashMap<kernel::ProjectId, Vec<String>>> {
    static FOLDERS: OnceLock<Mutex<HashMap<kernel::ProjectId, Vec<String>>>> = OnceLock::new();
    FOLDERS.get_or_init(Mutex::default)
}

/// The recent folders kept at most.
const RECENT_FOLDERS: usize = 6;

/// `folder` (relative, without the trailing `/`) first among `recent`, once, at most
/// [`RECENT_FOLDERS`].
fn keep_folder(recent: &mut Vec<String>, folder: &str) {
    recent.retain(|f| f != folder);
    recent.insert(0, folder.to_owned());
    recent.truncate(RECENT_FOLDERS);
}

/// What the folder's field suggests: with nothing typed, the recent folders first, then
/// the others (each once); at most `limit`.
fn with_recent(
    typed: &str,
    recent: &[String],
    completed: Vec<String>,
    limit: usize,
) -> Vec<String> {
    if !typed.trim().trim_matches('/').is_empty() {
        return completed;
    }
    let mut out: Vec<String> = recent.iter().take(limit).cloned().collect();
    for folder in completed {
        if out.len() == limit {
            break;
        }
        if !out.contains(&folder) {
            out.push(folder);
        }
    }
    out
}

/// Project `id` closed: its recent searches and its mask go.
pub(crate) fn forget(id: kernel::ProjectId) {
    if let Ok(mut all) = recent().lock() {
        all.remove(&id);
    }
    if let Ok(mut all) = masks().lock() {
        all.remove(&id);
    }
    if let Ok(mut all) = recent_folders().lock() {
        all.remove(&id);
    }
}

#[derive(Debug)]
pub(crate) struct SearchProvider;

impl ViewerProvider for SearchProvider {
    fn id(&self) -> &'static str {
        "indexing.search"
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == SEARCH_VIEW).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(SearchView::default()))
    }
}

#[derive(Default)]
pub(crate) struct SearchView {
    panel: ProjectSearch,
    /// Its controls' state of use (see [`SearchView::with_controls`]).
    controls: Controls,
    /// What the panel asked of the view at its last press, key or text.
    ask: Option<Ask>,
    data: Option<SearchData>,
    /// The mode shown last (a change redraws: the model downloading…).
    mode: SearchMode,
    action: Option<ViewerAction>,
    /// A result or a folder opened, or given up: the app closes it.
    done: bool,
    /// `User.php:12:5`: the place (0-based line, column) the files found open at.
    place: Option<(u32, u32)>,
    scale: f32,
    /// Every occurrence being found (the field's options or the replacement asked for it),
    /// what it matches with, and the lines found so far.
    exact: Option<ExactSearch>,
    matcher: Option<document::Matcher>,
    found: Vec<Found>,
    /// Wakes the window when lines found arrive.
    waker: Option<Arc<dyn Fn() + Send + Sync>>,
    /// The searches asked (counted), and the newest one going.
    asked: u64,
    searching: Option<(u64, std::sync::mpsc::Receiver<Searched>)>,
}

/// The worker searches run on.
const SEARCH_THREAD: &str = "project-search";

/// What a search is asked: the field's text, the folder it's kept in, the mask, whether
/// meanings are searched too.
struct Asked {
    query: String,
    scope: Option<String>,
    mask: String,
    semantic: bool,
}

/// What a search found: the files' text, the files and folders by name.
struct Searched {
    results: Vec<text_search::Hit>,
    named: Vec<FileMatch>,
    folders: Vec<FileMatch>,
}

/// The files' text and the folders' names searched as `asked` says (on a worker).
fn search(queries: &dyn ProjectQueries, asked: &Asked) -> Searched {
    let Asked { query, scope, mask, semantic } = asked;
    let mask = workspace::FileMask::parse(mask);
    let inside = |path: &str| scope.as_deref().is_none_or(|s| path.starts_with(s));
    // A file: in the folder and of the mask's.
    let taken = |path: &str| inside(path) && mask.accepts(path);
    // `User.php:12`: the name is searched (the files by it, at that line).
    let (name, place) = workspace::split_location(query);
    let results = if query.is_empty() {
        // Opened: the text index read ahead of the first letter typed.
        queries.text(&SearchQuery::new(String::new(), 1));
        Vec::new()
    } else if let Some((line, _)) = place {
        let files = queries.paths(&PathQuery { text: name, folders: 0, files: FILES * 4 });
        let files = files.files.into_iter().filter(|f| taken(&f.path)).take(FILES);
        files.map(|file| at_line(file.path, line)).collect()
    } else {
        let mut search = SearchQuery::new(query.clone(), 50);
        if !semantic {
            search.strategy = Strategy::Lexical;
        }
        search.paths = scope.iter().cloned().collect();
        let mut hits = queries.text(&search);
        hits.retain(|hit| mask.accepts(&hit.path));
        hits
    };
    // By name (not at a line): the files whose own name has every letter typed, in order
    // (`api.doc.json`, `apidoc`), and the folders; at most FOLDERS rows of both.
    let (named, folders) = if name.is_empty() || place.is_some() {
        (Vec::new(), Vec::new())
    } else {
        // In a folder, more are asked for: those outside it are left out.
        let more = if scope.is_some() { 8 } else { 1 };
        let query = PathQuery { text: name, folders: FOLDERS * more, files: NAMED * 4 * more };
        let found = queries.paths(&query);
        let letters = name.chars().filter(|c| !c.is_whitespace()).count();
        let named: Vec<_> = found
            .files
            .into_iter()
            .filter(|m| m.name_positions.len() == letters && taken(&m.path))
            .take(NAMED)
            .collect();
        let room = FOLDERS.saturating_sub(named.len()).max(2);
        let folders = found.folders.into_iter().filter(|m| inside(&m.path));
        (named, folders.take(room).collect())
    };
    Searched { results, named, folders }
}

impl std::fmt::Debug for SearchView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchView").field("data", &self.data).finish_non_exhaustive()
    }
}

/// A line found, as a result's row: the file, the line (and its text as what it shows).
fn hit_of(found: &Found) -> text_search::Hit {
    let line = found.line.line;
    text_search::Hit {
        path: found.path.clone(),
        start_line: line,
        end_line: line,
        preview: found.line.text.clone(),
        score: 0.0,
    }
}

impl SearchView {
    /// Its controls (the panel: its fields, its rows, its scrollbar), the topmost first,
    /// with their state of use; what the panel asked is answered after.
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let mut panel = PanelPart { panel: &mut self.panel, scale: self.scale, ask: &mut self.ask };
        let mut parts: [&mut dyn Control; 1] = [&mut panel];
        let answer = f(&mut self.controls, &mut parts);
        if let Some(ask) = self.ask.take() {
            self.answer(ask);
        }
        answer
    }

    /// What the panel asked: an action, a field's caret, a search again, the folders.
    fn answer(&mut self, ask: Ask) {
        match ask {
            Ask::Act(action) => self.act(action),
            Ask::Field(action, p) => {
                self.act(action);
                if self.panel.place_caret(p) {
                    self.typed();
                }
            }
            Ask::Typed => self.typed(),
            Ask::Suggest => self.suggest(),
        }
    }

    fn root(&self) -> Option<PathBuf> {
        self.data.as_ref().map(|d| d.queries.root())
    }

    fn mode(&self) -> SearchMode {
        self.data
            .as_ref()
            .and_then(|d| d.semantic.as_ref())
            .map_or(SearchMode::Lexical, |s| s.mode())
    }

    /// Every occurrence of the field's text, as its options say: in the background (the
    /// lines come by `poll`); what isn't a pattern is said instead.
    fn refresh_exact(&mut self) {
        let Some(data) = &self.data else { return };
        self.exact = None;
        self.matcher = None;
        self.found.clear();
        self.panel.set_can_replace(false);
        self.place = None;
        self.panel.set_named(Vec::new(), Vec::new());
        self.panel.set_results(Vec::new());
        let options = self.panel.options;
        let query = document::SearchQuery {
            pattern: self.panel.input.text().to_owned(),
            regex: options.regex,
            case_sensitive: options.case,
            whole_word: options.word,
        };
        match document::Matcher::new(&query) {
            // Not a pattern, but a file name's (`*.png`, `src/*.php`): the files named so.
            Err(_) if options.regex && looks_like_glob(&query.pattern) => {
                self.panel.set_error(None);
                self.panel.set_exact(None);
                let files = files_named(&data.queries.file_list(), query.pattern.trim());
                self.panel.set_listed(files);
            }
            Err(error) => {
                self.panel.set_error(Some(readable_error(&error.to_string())));
                self.panel.set_exact(None);
            }
            Ok(None) => {
                self.panel.set_error(None);
                self.panel.set_exact(None);
            }
            Ok(Some(matcher)) => {
                self.panel.set_error(None);
                let under = self.panel.scope_prefix().map(|s| s.trim_end_matches('/').to_owned());
                let files = crate::exact::Files {
                    under,
                    mask: workspace::FileMask::parse(self.panel.mask.text()),
                };
                let queries = Arc::clone(&data.queries);
                self.matcher = Some(matcher.clone());
                self.exact = Some(ExactSearch::start(queries, matcher, files, self.waker.clone()));
                self.panel.set_exact(Some(ExactState { done: false, capped: false }));
            }
        }
    }

    /// Searches again for the field's text: the files' text, the folders' names (with an
    /// option on, every occurrence instead).
    fn refresh(&mut self) {
        if self.panel.options.exact() || self.panel.replace_open() {
            self.refresh_exact();
            return;
        }
        self.exact = None;
        self.matcher = None;
        self.found.clear();
        self.panel.set_error(None);
        self.panel.set_exact(None);
        let Some(data) = &self.data else { return };
        // A file name's pattern (`*.png`, one word with a `*` or `?`): the files named so.
        let typed = self.panel.input.text().trim();
        if looks_like_glob(typed) && !typed.contains(char::is_whitespace) {
            let files = files_named(&data.queries.file_list(), typed);
            self.panel.set_listed(files);
            return;
        }
        // Searches keep the semantic model (loaded once, shared by every window).
        let semantic = data.semantic.as_ref();
        if let Some(semantic) = semantic {
            semantic.touch();
        }
        let query = self.panel.input.text().trim().to_owned();
        let (_, place) = workspace::split_location(&query);
        self.place = place;
        let asked = Asked {
            query,
            // In a folder: only what's in it (the folder itself isn't among what it holds).
            scope: self.panel.scope_prefix(),
            mask: self.panel.mask.text().to_owned(),
            semantic: semantic.is_some_and(|s| s.enabled()),
        };
        let queries = Arc::clone(&data.queries);
        // On a worker (the whole text index scored, the paths matched: never at each key on
        // the UI thread), the newest one wins; here and now without a waker (tests).
        let Some(waker) = self.waker.clone() else {
            let found = search(queries.as_ref(), &asked);
            self.show(found);
            return;
        };
        self.asked += 1;
        let (number, (tx, rx)) = (self.asked, std::sync::mpsc::channel());
        kernel::work::run(SEARCH_THREAD, move || {
            let _ = tx.send(search(queries.as_ref(), &asked));
            waker();
        });
        self.searching = Some((number, rx));
    }

    /// What a search found, shown.
    fn show(&mut self, found: Searched) {
        self.panel.set_results(found.results);
        self.panel.set_named(found.named, found.folders);
    }

    /// The folders the one typed goes on to, while it's typed in.
    fn suggest(&mut self) {
        let Some(data) = &self.data else { return };
        if !self.panel.in_scope() {
            self.panel.clear_suggestions();
            return;
        }
        let typed = self.panel.scope.text();
        let completed = data.queries.complete_folder(typed, SUGGESTIONS);
        let recent = recent_folders()
            .lock()
            .ok()
            .and_then(|all| all.get(&data.project).cloned())
            .unwrap_or_default();
        self.panel.set_suggestions(with_recent(typed, &recent, completed, SUGGESTIONS));
    }

    /// What was typed changed: in the folder's field, its suggestions too; the search again
    /// (not for the replacement: what's found stays). The mask is kept for the project.
    fn typed(&mut self) {
        if self.panel.in_replace() {
            return;
        }
        if let (Some(data), Ok(mut all)) = (&self.data, masks().lock()) {
            all.insert(data.project, self.panel.mask.text().to_owned());
        }
        if self.panel.in_scope() {
            self.suggest();
        }
        self.refresh();
    }

    /// Every occurrence found replaced, as one refactoring (undone as one); the search closes.
    fn replace_all(&mut self) {
        let (Some(data), Some(matcher)) = (&self.data, &self.matcher) else { return };
        let regex = self.panel.options.regex;
        let root = data.queries.root();
        let with = self.panel.replace.text();
        let plan = crate::exact::replace_plan(&self.found, matcher, with, regex, &root);
        if plan.files.is_empty() {
            return;
        }
        self.remember();
        self.done = true;
        self.action = Some(ViewerAction::ApplyRefactoring(plan));
    }

    /// The panel's recent searches kept as the project's (every window's search).
    fn keep_recent(&self) {
        let Some(data) = &self.data else { return };
        if let Ok(mut all) = recent().lock() {
            all.insert(data.project, self.panel.recent_searches().to_vec());
        }
    }

    /// Keeps the search among the project's recent ones, and its folder among the recent
    /// folders.
    fn remember(&mut self) {
        self.panel.remember();
        self.keep_recent();
        let Some(data) = &self.data else { return };
        if let Some(folder) = self.panel.scope_prefix()
            && let Ok(mut all) = recent_folders().lock()
        {
            keep_folder(all.entry(data.project).or_default(), folder.trim_end_matches('/'));
        }
    }

    fn act(&mut self, action: ProjectSearchAction) {
        let root = self.root().unwrap_or_default();
        match action {
            ProjectSearchAction::Open(i) => {
                let Some(hit) = self.panel.result(i).cloned() else { return };
                self.remember();
                self.done = true;
                let path = root.join(&hit.path);
                self.action = Some(match self.place {
                    // As written (1-based), the column too.
                    Some((line, column)) => ViewerAction::OpenFile {
                        path,
                        line: Some(line + 1),
                        column: Some(column + 1),
                    },
                    None => {
                        let line = focus_line(&hit, self.panel.input.text()).0;
                        ViewerAction::OpenAt { path, line }
                    }
                });
            }
            ProjectSearchAction::OpenNamed(i) => {
                let Some(file) = self.panel.named(i).map(|f| f.path.clone()) else { return };
                self.remember();
                self.done = true;
                self.action = Some(ViewerAction::OpenFile {
                    path: root.join(file),
                    line: None,
                    column: None,
                });
            }
            ProjectSearchAction::OpenFolder(i) => {
                let Some(folder) = self.panel.folder(i).map(|f| f.path.clone()) else { return };
                self.remember();
                self.done = true;
                self.action =
                    Some(ViewerAction::Request(AppRequest::ShowFolder(root.join(folder))));
            }
            ProjectSearchAction::Recent(i) => {
                let Some(query) = self.panel.recent(i).map(str::to_owned) else { return };
                self.panel.input.set_text(&query);
                self.refresh();
            }
            ProjectSearchAction::ForgetRecent(i) => {
                self.panel.forget_recent(i);
                self.keep_recent();
            }
            ProjectSearchAction::ClearRecent => {
                self.panel.clear_recent();
                self.keep_recent();
            }
            ProjectSearchAction::EnableSemantic => {
                if let Some(semantic) = self.data.as_ref().and_then(|d| d.semantic.as_ref()) {
                    semantic.enable();
                }
            }
            ProjectSearchAction::ToggleScope => {
                let had = self.panel.scope_prefix();
                self.panel.toggle_scope();
                self.suggest();
                if had != self.panel.scope_prefix() {
                    self.refresh();
                }
            }
            ProjectSearchAction::FocusScope => {
                self.panel.focus_scope(true);
                self.suggest();
            }
            ProjectSearchAction::FocusQuery => self.panel.focus_scope(false),
            ProjectSearchAction::FocusMask => self.panel.focus_mask(true),
            ProjectSearchAction::ToggleReplace => {
                let open = !self.panel.replace_open();
                self.panel.set_replace_open(open);
                self.refresh();
            }
            ProjectSearchAction::FocusReplace => self.panel.focus_replace(true),
            ProjectSearchAction::ReplaceAll => self.replace_all(),
            ProjectSearchAction::Toggle(option) => {
                self.panel.toggle_option(option);
                self.refresh();
            }
            ProjectSearchAction::Suggestion(i) => {
                if self.panel.take_suggestion(Some(i)) {
                    self.typed();
                }
            }
            ProjectSearchAction::Close => {
                self.done = true;
                self.action = Some(ViewerAction::Close);
            }
        }
    }
}

impl Viewer for SearchView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        let Some(data) = data.downcast_ref::<SearchData>() else { return };
        let kept = recent().lock().ok().and_then(|all| all.get(&data.project).cloned());
        self.panel.set_recent_searches(kept.unwrap_or_default());
        if let Some(selection) = &data.selection {
            self.panel.input.set_text(selection);
        }
        if let Some(folder) = &data.folder {
            self.panel.set_scope(folder);
        }
        // The mask used last in the project.
        if let Some(mask) = masks().lock().ok().and_then(|all| all.get(&data.project).cloned()) {
            self.panel.set_mask(&mask);
        }
        if data.replace {
            // Its row shown; the keyboard stays in the search (what to replace comes first).
            self.panel.set_replace_open(true);
            self.panel.focus_replace(false);
        }
        self.data = Some(data.clone());
        self.panel.open();
        // The keyboard in the panel from the start (its search's field).
        self.with_controls(|c, parts| c.focus(parts, Some(PANEL)));
        self.mode = self.mode();
        self.refresh();
    }

    /// Over the whole window (it centers itself).
    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        if ctx.waker.is_some() {
            self.waker.clone_from(&ctx.waker);
        }
        self.scale = ctx.scale;
        self.panel.set_viewport(ctx.bounds, ctx.scale);
        let (palette, icons) = (ctx.palette, ctx.icons);
        let queries = self.data.as_ref().map(|d| Arc::clone(&d.queries));
        let file_icon = |name: &str| match &queries {
            // The icons' registry decides (the chosen set's, else ours by language).
            Some(q) => {
                let (icon, language) = q.file_icon(name);
                let (id, tint) = icons.file(name, icon, &language);
                (id, palette.tint(&tint))
            }
            None => (icons.id_or_file("file"), palette.file_icon("")),
        };
        let mode = self.mode.clone();
        let painter = Painter { text: ctx.text, scene: ctx.scene, palette, icons };
        self.panel.paint(&mode, &file_icon, painter);
        // What the panel wants of the frames, as drawn.
        self.with_controls(|c, parts| c.settle(parts));
    }

    /// Outside the panel: nothing (it closes only with esc). A click anywhere but on the
    /// folders suggested or the folder's field closes their list (what it hit still acts).
    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        self.with_controls(|c, parts| c.press(parts, p, 1));
        self.action.take()
    }

    fn hover(&mut self, p: Point) -> bool {
        self.with_controls(|c, parts| c.hover(parts, Some(p)))
    }

    fn drag(&mut self, p: Point) -> bool {
        self.with_controls(|c, parts| c.drag(parts, p)).is_some_and(|(_, moved)| moved)
    }

    fn release(&mut self) -> bool {
        self.with_controls(|c, parts| c.release(parts)).is_some_and(|(_, changed)| changed)
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        self.controls.cursor_at(p)
    }

    fn tooltip_at(&self, p: Point) -> Option<ui_components::Tip> {
        self.controls.tooltip_at(p)
    }

    /// The panel's, over the whole window: with the mouse not moved over it yet (it opened
    /// under a still mouse), the wheel is still its.
    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        self.with_controls(|c, parts| {
            c.scroll_hovered(parts, dx, dy).or_else(|| c.scroll(parts, NOWHERE, dx, dy))
        })
        .is_some()
    }

    fn wants_text(&self) -> bool {
        true
    }

    fn text_input(&mut self, text: &str) -> bool {
        self.with_controls(|c, parts| c.text(parts, text)).is_some()
    }

    fn paste(&mut self, text: &str) -> bool {
        self.with_controls(|c, parts| c.paste(parts, text)).is_some()
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        self.edit_key_with(key, KeyReach::default())
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        self.with_controls(|c, parts| c.key(parts, key, reach)).is_some()
    }

    /// ⌘A in its field; `refresh`: searched again (the index or the model got ready).
    fn command(&mut self, id: &str) -> bool {
        if id == "refresh" {
            self.refresh();
            return true;
        }
        self.with_controls(|c, parts| c.command(parts, id)).is_some()
    }

    /// Read from `&self`: the panel lends its search's field by its place (what's copied
    /// is what's searched).
    fn selected_text(&self) -> Option<String> {
        self.controls
            .selected_text_with(|i| (i == PANEL).then_some(&self.panel.input as &dyn Control))
    }

    /// The model's state changed (downloading, loaded), lines found arrived: a redraw (and
    /// frames while they're being found).
    fn poll(&mut self) -> bool {
        let mode = self.mode();
        let mut changed = mode != self.mode;
        self.mode = mode;
        // The newest search's results (an older one's never come: it was replaced).
        let found = self.searching.as_ref().and_then(|(_, rx)| rx.try_recv().ok());
        if let Some(found) = found {
            self.searching = None;
            self.show(found);
            changed = true;
        }
        if let Some(exact) = &mut self.exact {
            let (found, arrived) = exact.poll();
            if !found.is_empty() {
                self.panel.append_results(found.iter().map(hit_of).collect());
                self.found.extend(found);
            }
            if arrived {
                let state = ExactState { done: exact.done(), capped: exact.capped() };
                self.panel.set_exact(Some(state));
                // Replaced only once every occurrence is known (all of them, not the first).
                let can = state.done && !state.capped && !self.found.is_empty();
                self.panel.set_can_replace(can && self.panel.replace_open());
            }
            changed |= arrived || !exact.done();
        }
        changed
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }

    fn closed(&self) -> bool {
        self.done
    }

    fn is_animating(&self) -> bool {
        self.controls.is_animating()
    }
}

/// The panel's place among the view's controls.
const PANEL: usize = 0;

/// The most files a file name's pattern lists.
const NAMED_MOST: usize = 200;

/// `pattern` reads as a file name's (a `*` or `?`, nothing else of a regular expression's):
/// `*.png`, `src/*.php`, `test_??.rs`.
fn looks_like_glob(pattern: &str) -> bool {
    let pattern = pattern.trim();
    !pattern.is_empty()
        && pattern.contains(['*', '?'])
        && !pattern.contains(['\\', '(', ')', '|', '^', '$', '+', '{', '}'])
}

/// The project's files matching `pattern`: by their name, or by their path when it has a
/// `/` — their name lit, at most [`NAMED_MOST`].
fn files_named(files: &[String], pattern: &str) -> Vec<FileMatch> {
    let by_path = pattern.contains('/');
    files
        .iter()
        .filter(|path| {
            let name = path.rsplit('/').next().unwrap_or(path);
            kernel::paths::glob_match(pattern, if by_path { path } else { name })
        })
        .take(NAMED_MOST)
        .map(|path| FileMatch { path: path.clone(), score: 0, name_positions: Vec::new() })
        .collect()
}

/// What's wrong with a pattern, on one line: a regular expression's error says it on its
/// last (`error: repetition operator missing expression`), over the pattern and a caret.
fn readable_error(error: &str) -> String {
    let line = error.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or(error).trim();
    let line = line.strip_prefix("error:").map_or(line, str::trim);
    let mut chars = line.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
#[path = "../tests/unit/search_view.rs"]
mod tests;
