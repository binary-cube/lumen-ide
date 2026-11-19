//! The "Commit" view (a tab), as a staging screen: the changed files in two lists (staged,
//! not staged), a checkbox moving each from one to the other; beside them the selected file's
//! changes in hunks, each staged, unstaged or thrown away on its own; under them the message
//! (several lines: its first one the subject; ↵ a new line, ⌘ / Ctrl+↵ commits; the draft kept
//! per repository until it's committed) and the Commit button, committing what's staged.
//! Everything that touches the repository runs in the background; what can't be undone is
//! asked first. `git` must be installed to write — without it the view says so.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

use i18n::{t, tr};
use kernel::{Point, Rect, Rgba};
use render_api::Quad;
use text_layout::Font;
use ui_components::ds::{self, Ctx, Tone, radius, space};
use ui_components::{
    AppRequest, LayerPainter, MultilineField, MultilineSpec, Orientation, SplitterStyle,
    ViewMenuEntry, ViewerCursor, WithHost,
};
use ui_components::{Viewer, ViewerAction, ViewerKey, ViewerPaint};
use ui_kit::draw_text;
use version_control::application::{CommitError, amend_selected, compare_with};
use version_control::staging::{DiffHunk, LineKind, hunks};
use version_control::{Commit, FileState, PatchTarget, Repository, Revision, StagedFile, VcsError};
use workbench_model::{ChoiceItem, ChoiceList, LayoutSizes, Question, WorkbenchHost};

use crate::divider::Divider;

use self::diff::*;
use self::paint::*;

mod diff;
mod menu;
mod paint;
mod places;
mod selection;
mod syntax;
mod text_selection;
mod view;

/// The extension of the view's virtual tab.
pub const EXTENSION: &str = "view-commit";

/// What the view is opened with.
#[derive(Clone)]
pub struct CommitData {
    pub repo: Arc<dyn Repository>,
    /// The project whose changes it follows (`None`: it reads them only when asked).
    pub project: Option<kernel::ProjectId>,
    /// In the Git window (its Commit tab: the repository's places beside it), not a tab.
    pub docked: bool,
}

/// The key of the Git window's Commit tab.
pub const KEY: &str = "git.commit-tab";

/// How many lines around a change a hunk shows.
const CONTEXT: usize = 3;
/// A file larger than this shows no hunks (it's still staged whole).
const MAX_DIFF_BYTES: usize = 4 * 1024 * 1024;
/// The panes that scroll.
const STAGED: usize = 0;
const UNSTAGED: usize = 1;
const DIFF: usize = 2;

/// The screen's dividers: the lists and the changes, the two lists, the rest and the message;
/// docked, the places and the rest.
#[derive(Debug)]
struct Dividers([Divider; 4]);

impl Default for Dividers {
    fn default() -> Self {
        Self([
            Divider::new(Orientation::Vertical, "git.commit-lists", 0.36, (240.0, 280.0)),
            Divider::new(Orientation::Horizontal, "git.commit-sections", 0.5, (96.0, 96.0)),
            Divider::new(Orientation::Horizontal, "git.commit-footer", 0.8, (220.0, 112.0))
                .after_first(),
            Divider::new(Orientation::Vertical, "git.commit-places", 0.18, (180.0, 640.0))
                .after_first(),
        ])
    }
}

/// The height of a card's head (its title's row).
const CARD_HEAD: f32 = 34.0;
/// A list's keys at its card's bottom (logical pixels).
const LIST_KEYS_H: f32 = 34.0;
/// The places' filter field's height (logical pixels).
const PLACES_FILTER_H: f32 = 30.0;
/// The height under which the message goes beside the lists and the changes, in a column of
/// [`MESSAGE_COLUMN`] (a short bottom window; logical pixels).
const COMPACT_H: f32 = 400.0;
const MESSAGE_COLUMN: f32 = 340.0;

/// What a click hit (`staged`: of the staged list; a file by its place in `files`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Check(usize, bool),
    Row(usize, bool),
    All(bool),
    StageHunk(usize),
    DiscardHunk(usize),
    Refresh,
    /// Stash… in the header.
    Stash,
    Commit,
    Message,
    Amend,
    /// A changed line of the changes (hunk, line).
    Line(usize, usize),
    /// A row of the places beside the screen (docked).
    Place(usize),
    /// The places' filter field, and its ×.
    PlacesFilter,
    ClearPlacesFilter,
}

/// Where the keyboard goes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Keys {
    /// It types the message.
    #[default]
    Message,
    /// It moves through the lists (a file was clicked, or Tab).
    Lists,
    /// It types the places' filter (it was clicked).
    Places,
}

/// The Git window's log shown (`reference`: only its history).
fn show_log(reference: Option<String>) -> ViewerAction {
    ViewerAction::WithHost(WithHost::once(move |host| {
        crate::log_view::show_log_of(host, reference);
    }))
}

/// What a repository job ends with.
type Done = Result<(), VcsError>;

/// The current branch and the files, read together in the background.
type StatusRead = (Option<String>, Result<Vec<StagedFile>, VcsError>);

#[derive(Default)]
pub struct CommitView {
    repo: Option<Arc<dyn Repository>>,
    project: Option<kernel::ProjectId>,
    /// The generation of the project's changes the lists were read after.
    generation: u64,
    branch: Option<String>,
    /// The changed files, as the staging area sees them.
    files: Vec<StagedFile>,
    /// The file shown beside the lists, and from which list.
    selected: Option<(String, bool)>,
    /// What's chosen on that list (the shown file among it), where a ⇧ run starts, and the
    /// modifiers of the press being handled.
    marked: std::collections::BTreeSet<String>,
    anchor: Option<String>,
    mods: ui_components::PressMods,
    diff: Option<FileDiff>,
    diff_rx: Option<Receiver<FileDiff>>,
    message: MultilineField,
    /// Where the mouse is (the wheel scrolls what's under it).
    pointer: Point,
    /// Amending the last commit; whether it was pushed already; the message before (back
    /// when it's left); its message and that, read in the background.
    amend: bool,
    amend_pushed: bool,
    before_amend: Option<String>,
    amend_rx: Option<Receiver<(Option<String>, bool)>>,
    /// Reading the status / moving files / committing, in the background.
    status_rx: Option<Receiver<StatusRead>>,
    act_rx: Option<Receiver<Done>>,
    /// What `act_rx` waits for is still the user's answer: no frames meanwhile.
    asking: bool,
    commit_rx: Option<Receiver<Result<String, CommitError>>>,
    /// The last outcome: (text, it's an error).
    outcome: Option<(String, bool)>,
    waker: Option<Arc<dyn Fn() + Send + Sync>>,
    hits: Vec<(Rect, Hit)>,
    /// The files' state badges, as last drawn (their tooltips: the state in words).
    badges: Vec<(Rect, version_control::FileState)>,
    hover: Option<Hit>,
    /// The message was pressed: the mouse dragged selects in it.
    selecting: bool,
    keys: Keys,
    /// The shown file's row is brought into view at the next paint (it was moved to by key).
    reveal: bool,
    dividers: Dividers,
    /// The project's sizes (the dividers' shares).
    sizes: Option<LayoutSizes>,
    /// What a menu row asked; a comparison being read.
    pending: Option<ViewerAction>,
    opening: ui_components::deferred::Deferred,
    /// The panes' scrollbars (hidden at rest), scroll and place.
    bars: [ui_components::EdgeScrollbar; 3],
    scrolls: [f32; 3],
    /// The changes' code scrolled sideways, and how far it can go (the widest line painted).
    diff_x: f32,
    /// The code's text selected with the mouse, and where the code was last painted.
    code: text_selection::CodeSelection,
    /// The changed lines chosen, to act on instead of their whole hunk, and where a ⇧ run starts.
    lines: std::collections::BTreeSet<(usize, usize)>,
    line_anchor: Option<(usize, usize)>,
    diff_overflow: f32,
    panes: [Rect; 3],
    /// Docked in the Git window: the repository's places beside the screen, read in the
    /// background with the lists; their rows (the sections and folders folded), where they
    /// were drawn, how far they're scrolled.
    docked: bool,
    /// Made again at a start where there's no repository any more.
    gone: bool,
    places: places::Places,
    /// The repository's references every view shares ([`crate::refs_store`]): the places'
    /// branches, tags, stashes, submodules taken from them, the read taken last, the
    /// remotes (a branch's menu pushes to them).
    shared: Option<Arc<crate::refs_store::Refs>>,
    shared_taken: u64,
    remotes: Vec<String>,
    folded: std::collections::HashSet<String>,
    place_rows: Vec<places::PlaceRow>,
    /// What filters the places (their rows only those containing it).
    places_filter: ui_kit::TextInput,
    places_area: Rect,
    places_scroll: f32,
}

impl CommitView {
    fn refresh(&mut self) {
        let Some(repo) = self.repo.clone() else { return };
        let (tx, rx) = std::sync::mpsc::channel();
        let waker = self.waker.clone();
        kernel::work::run("git-status", move || {
            let _ = tx.send((repo.branch(), repo.staged_files()));
            if let Some(wake) = waker {
                wake();
            }
        });
        self.status_rx = Some(rx);
        if self.docked {
            self.read_places();
        }
    }

    /// The repository's places read again, in the background (the shared references: every
    /// view of them follows).
    fn read_places(&mut self) {
        let Some(repo) = &self.repo else { return };
        match &self.shared {
            Some(shared) => shared.refresh(),
            None => self.shared = Some(crate::refs_store::of(repo, self.waker.clone())),
        }
    }

    /// The shared references' newest read taken: the places made again from it. `true` if
    /// there was one, or one is being read.
    pub(crate) fn take_places(&mut self) -> bool {
        let Some(shared) = &self.shared else { return false };
        if let Some(wake) = &self.waker {
            shared.follow(Arc::clone(wake));
        }
        let busy = shared.reading();
        let (number, read) = shared.current();
        let Some(read) = read.filter(|_| number != self.shared_taken) else { return busy };
        self.shared_taken = number;
        self.places = places::Places {
            refs: read.refs.clone(),
            stashes: read.stashes.clone(),
            submodules: read.submodules.clone(),
            states: read.states.clone(),
        };
        self.remotes.clone_from(&read.remotes);
        self.lay_places();
        true
    }

    /// The reference place row `i` is (with what a local branch tracks), for its menu.
    fn menu_ref(&self, i: usize) -> Option<crate::ref_menu::MenuRef> {
        let places::Place::Ref { name, kind, current, .. } = &self.place_rows.get(i)?.place else {
            return None;
        };
        let local = *kind == version_control::RefKind::Branch;
        let upstream = local
            .then(|| self.places.states.iter().find(|s| s.branch == *name))
            .flatten()
            .and_then(|s| s.upstream.clone());
        let (kind, current) = (Some(*kind), *current);
        Some(crate::ref_menu::MenuRef { name: name.clone(), local, upstream, kind, current })
    }

    /// The places' rows made again (what they're made of changed, or a fold).
    fn lay_places(&mut self) {
        let query = self.places_filter.text();
        self.place_rows = places::rows(&self.places, self.branch.as_deref(), &self.folded, query);
    }

    /// The places' filter edited: their rows again, from the top when what it holds changed.
    fn edit_places_filter(&mut self, edit: impl FnOnce(&mut ui_kit::TextInput)) {
        let before = self.places_filter.text().to_owned();
        edit(&mut self.places_filter);
        if self.places_filter.text() != before {
            self.places_scroll = 0.0;
            self.lay_places();
        }
    }

    /// A key while the places' filter has the keyboard: it edits it; Esc empties it (or,
    /// empty, gives the keyboard back to the message, as Tab does).
    fn places_key(&mut self, key: ViewerKey, reach: ui_components::KeyReach) -> bool {
        match key {
            ViewerKey::Escape if !self.places_filter.text().is_empty() => {
                self.edit_places_filter(|input| input.set_text(""));
            }
            ViewerKey::Escape | ViewerKey::Tab => self.keys = Keys::Message,
            _ => {
                let mut edited = false;
                self.edit_places_filter(|input| edited = reach.edit(input, key));
                return edited;
            }
        }
        true
    }

    /// Place `i` clicked: a section or a folder folds or opens; the history, a search, a
    /// reference shows the log (that reference's history only); a submodule shows its folder.
    fn pick_place(&mut self, i: usize) -> Option<ViewerAction> {
        use places::{Place, Work};
        let place = self.place_rows.get(i)?.place.clone();
        let fold = |view: &mut Self, key: String| {
            if !view.folded.remove(&key) {
                view.folded.insert(key);
            }
            view.lay_places();
            None
        };
        match place {
            Place::Section { key, .. } => fold(self, key.to_owned()),
            Place::Folder { key, .. } => fold(self, key),
            Place::Work(Work::FileStatus) | Place::Stash(_) => None,
            Place::Work(_) => Some(show_log(None)),
            Place::Ref { name, .. } => Some(show_log(Some(name))),
            Place::Submodule(path) => {
                let root = self.repo.as_ref()?.root().to_path_buf();
                Some(ViewerAction::Request(AppRequest::ShowFolder(root.join(path))))
            }
        }
    }

    /// The stash place row `i` is.
    fn stash_at(&self, i: usize) -> Option<version_control::Stash> {
        match &self.place_rows.get(i)?.place {
            places::Place::Stash(stash) => Some(stash.clone()),
            _ => None,
        }
    }

    /// Place row `i` double-clicked: a branch or a tag switched to, a stash applied.
    fn place_twice(&mut self, i: usize) -> Option<ViewerAction> {
        match self.place_rows.get(i)?.place.clone() {
            places::Place::Ref { name, kind, current: false, .. } => {
                crate::switch::Wanted::of(&name, kind).map(crate::ref_menu::switch_action)
            }
            places::Place::Stash(stash) => Some(crate::stash_dialog::apply_action(stash)),
            _ => None,
        }
    }

    /// What it did itself (a commit, a stage, a discard) read again by the project's status
    /// too: the tool's count and the tree's marks follow at once, not at the next event
    /// (`.git` alone changed: nothing in the tree to tell).
    pub(crate) fn tell_status(&self) {
        if let Some(changes) = self.project.and_then(crate::status::changes) {
            changes.stale();
        }
    }

    /// The generation of the project's changes, once the project has them.
    fn changes_generation(&self) -> Option<u64> {
        Some(crate::status::changes(self.project?)?.files().0)
    }

    /// Opening the file whose row is at `p` (one that was deleted has nothing to open).
    fn open_at(&self, p: Point) -> Option<ViewerAction> {
        let hit = self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)?;
        let Hit::Row(i, staged) = hit else { return None };
        let file = self.files.get(i)?;
        let state = if staged { file.staged } else { file.unstaged };
        if state == Some(FileState::Deleted) {
            return None;
        }
        let path = self.repo.as_ref()?.root().join(&file.path);
        Some(ViewerAction::OpenFile { path, line: None, column: None })
    }

    /// The places in `files` of the staged (or not staged) list.
    fn list(&self, staged: bool) -> Vec<usize> {
        let on = |f: &StagedFile| if staged { f.staged.is_some() } else { f.unstaged.is_some() };
        (0..self.files.len()).filter(|i| on(&self.files[*i])).collect()
    }

    fn staged_paths(&self) -> Vec<String> {
        self.list(true).into_iter().map(|i| self.files[i].path.clone()).collect()
    }

    /// A job on the repository, in the background; when it ends, the lists are read again.
    fn act(&mut self, job: impl FnOnce(&dyn Repository) -> Done + Send + 'static) {
        let Some(repo) = self.repo.clone() else { return };
        let (tx, rx) = std::sync::mpsc::channel();
        let waker = self.waker.clone();
        kernel::work::run("git-commit", move || finish(&*repo, job, &tx, waker.as_ref()));
        self.act_rx = Some(rx);
        self.outcome = None;
    }

    /// File `i` moved to the other list.
    fn toggle(&mut self, i: usize, staged: bool) {
        let path = self.reached(i, staged);
        if path.is_empty() {
            return;
        }
        self.act(move |repo| if staged { repo.unstage(&path) } else { repo.stage(&path) });
    }

    /// A whole list moved to the other one.
    fn toggle_all(&mut self, staged: bool) {
        let paths: Vec<String> =
            self.list(staged).into_iter().map(|i| self.files[i].path.clone()).collect();
        if !paths.is_empty() {
            self.act(move |repo| if staged { repo.unstage(&paths) } else { repo.stage(&paths) });
        }
    }

    /// File `i` shown, from that list.
    fn select(&mut self, i: usize, staged: bool) {
        let Some(path) = self.files.get(i).map(|f| f.path.clone()) else { return };
        self.marked = std::iter::once(path.clone()).collect();
        self.anchor = Some(path.clone());
        self.selected = Some((path, staged));
        self.scrolls[DIFF] = 0.0;
        self.diff_x = 0.0;
        self.load_diff();
    }

    /// The selection kept if it's still there, else the first file: then its changes read.
    fn reselect(&mut self) {
        let there = self.selected.as_ref().is_some_and(|(path, staged)| {
            self.list(*staged).into_iter().any(|i| self.files[i].path == *path)
        });
        if !there {
            let first = |list: Vec<usize>, staged| list.first().map(|i| (*i, staged));
            let next = first(self.list(false), false).or_else(|| first(self.list(true), true));
            self.selected = next.map(|(i, staged)| (self.files[i].path.clone(), staged));
            self.anchor = self.selected.as_ref().map(|(path, _)| path.clone());
            self.marked.clear();
        }
        self.keep_marked();
        self.load_diff();
    }

    /// What can't be undone, asked first through the host; `job` runs on "yes".
    fn asked(
        &mut self,
        question: Question,
        job: impl FnOnce(&dyn Repository) -> Done + Send + 'static,
    ) -> Option<ViewerAction> {
        let repo = self.repo.clone()?;
        let (tx, rx) = std::sync::mpsc::channel();
        let waker = self.waker.clone();
        self.act_rx = Some(rx);
        self.asking = true;
        self.outcome = None;
        Some(ViewerAction::WithHost(WithHost::once(move |host: &mut dyn WorkbenchHost| {
            // "No" drops the job with its channel: nothing was done.
            host.confirm(
                question,
                Box::new(move |_: &mut dyn WorkbenchHost| {
                    kernel::work::run("git-commit", move || {
                        finish(&*repo, job, &tx, waker.as_ref());
                    });
                }),
            );
        })))
    }

    fn can_commit(&self) -> bool {
        // Amending may change only the message.
        self.commit_rx.is_none()
            && !self.message.text().trim().is_empty()
            && (self.amend || self.files.iter().any(|f| f.staged.is_some()))
    }

    /// "Amend last commit" turned on (its message brought in, whether it was pushed read in
    /// the background) or off (the message as it was before).
    pub fn set_amend(&mut self, on: bool) {
        if on == self.amend {
            return;
        }
        self.amend = on;
        if !on {
            self.amend_pushed = false;
            self.amend_rx = None;
            if let Some(before) = self.before_amend.take() {
                self.message.set_text(&before);
            }
            return;
        }
        self.before_amend = Some(self.message.text().to_owned());
        let Some(repo) = self.repo.clone() else { return };
        let waker = self.waker.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        kernel::work::run("git-commit", move || {
            let _ = tx.send((repo.last_commit_message(), repo.head_pushed()));
            if let Some(wake) = waker {
                wake();
            }
        });
        self.amend_rx = Some(rx);
    }

    /// The draft kept for the repository (until it's committed).
    fn keep_draft(&self) {
        if let Some(repo) = &self.repo {
            drafts().insert(repo.root().to_path_buf(), self.message.text().to_owned());
        }
    }

    fn commit(&mut self) -> bool {
        if !self.can_commit() {
            return false;
        }
        let Some(repo) = self.repo.clone() else { return false };
        let (paths, message) = (self.staged_paths(), self.message.text().trim().to_owned());
        let amend = self.amend;
        let (tx, rx) = std::sync::mpsc::channel();
        let waker = self.waker.clone();
        kernel::work::run("git-commit", move || {
            let result = if amend {
                amend_selected(repo.as_ref(), &paths, &message)
            } else {
                repo.commit_staged(&message).map_err(CommitError::Vcs)
            };
            let _ = tx.send(result);
            if let Some(wake) = waker {
                wake();
            }
        });
        self.commit_rx = Some(rx);
        self.outcome = None;
        true
    }

    fn vcs_error(error: &VcsError) -> String {
        crate::error_message(error)
    }
}

/// `job` done on `repo`, told through `tx`.
fn finish(
    repo: &dyn Repository,
    job: impl FnOnce(&dyn Repository) -> Done,
    tx: &Sender<Done>,
    waker: Option<&Arc<dyn Fn() + Send + Sync>>,
) {
    let _ = tx.send(job(repo));
    if let Some(wake) = waker {
        wake();
    }
}

/// The messages being written, by repository (in memory: the session's).
fn drafts() -> std::sync::MutexGuard<'static, std::collections::HashMap<std::path::PathBuf, String>>
{
    static DRAFTS: std::sync::LazyLock<
        std::sync::Mutex<std::collections::HashMap<std::path::PathBuf, String>>,
    > = std::sync::LazyLock::new(Default::default);
    DRAFTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
#[path = "../tests/unit/commit_view.rs"]
mod tests;
