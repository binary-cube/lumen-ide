//! A folder's environments as tables (plan 77l): a tab for `$shared` and each environment,
//! its variables as rows (name, value, private — a private one's value hidden, in the
//! private file, never committed), a new environment, the JSON files opened as text. An
//! environment is renamed in place (a double click on its tab, or its menu) and deleted from
//! its menu, after a question (its variables would be lost: the table has no undo). Read
//! and written on a worker ([`http_requests::env_edit`]), a moment after the last change.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use http_requests::env_edit::{self, Row, Table};
use i18n::t;
use kernel::{Point, Rect, Rgba};
use render_api::Quad;
use ui_components::control::{Control, Controls, Press, Thumb};
use ui_components::ds::{
    self, ButtonKind, ButtonSpec, Check, Ctx, Tone, ToolbarButton, button_size,
};
use ui_components::tab_strip::{self, UnderlineTab};
use ui_components::tool_panel::KeyReach;
use ui_components::{
    EdgeScrollbar, ViewMenuEntry, Viewer, ViewerAction, ViewerCursor, ViewerKey, ViewerKind,
    ViewerPaint, ViewerProvider, WithHost,
};
use ui_kit::{TextInput, draw_text};

use crate::pending::Pending;

/// The extension its tab is opened by.
pub const EXTENSION: &str = "view-http-env";
const THREAD: &str = "http-editor";
const SAVE_AFTER: Duration = Duration::from_millis(600);
const BAR: f32 = 44.0;
const ROW: f32 = 34.0;
/// A variable's remove button: its middle this far from the table's right (logical pixels).
const TRASH_RIGHT: f32 = 10.0;
/// The space between two environments' tabs (logical pixels).
const TAB_GAP: f32 = 4.0;

/// What its tab is opened with: a request file (its environments' folder is found from it)
/// and the project's root.
#[derive(Debug, Clone)]
pub struct EnvData {
    pub file: PathBuf,
    pub root: Option<PathBuf>,
}

/// The files read: their folder, their texts, the tables.
type Loaded = (PathBuf, Option<String>, Option<String>, Vec<Table>);

/// The files written: their new texts (public, private), or why not (`true`: they changed
/// on disk since read — read again, nothing written).
type Saved = Result<(Option<String>, Option<String>), (String, bool)>;

/// A row being typed in.
#[derive(Debug)]
struct Line {
    name: TextInput,
    value: TextInput,
    private: bool,
    json: bool,
}

impl Line {
    fn of(row: &Row) -> Self {
        let mut value = TextInput::new(&row.value);
        value.set_masked(row.private);
        Self { name: TextInput::new(&row.name), value, private: row.private, json: row.json }
    }

    fn empty() -> Self {
        Self { name: TextInput::new(""), value: TextInput::new(""), private: false, json: false }
    }

    fn row(&self) -> Row {
        Row {
            name: self.name.text().to_owned(),
            value: self.value.text().to_owned(),
            private: self.private,
            json: self.json,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Tab(usize),
    AddEnvironment,
    OpenPublic,
    OpenPrivate,
    Cell(usize, bool),
    Private(usize),
    Remove(usize),
}

#[derive(Default)]
pub struct EnvView {
    data: Option<EnvData>,
    folder: Option<PathBuf>,
    texts: (Option<String>, Option<String>),
    names: Vec<String>,
    lines: Vec<Vec<Line>>,
    tab: usize,
    focus: Option<(usize, bool)>,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
    loading: Pending<Loaded>,
    saving: Pending<Saved>,
    changed: Option<Instant>,
    action: Option<ViewerAction>,
    error: Option<String>,
    /// How far its rows are scrolled (pixels), and the most they can be (as last drawn).
    scroll: f32,
    max_scroll: f32,
    /// A row's height and how many rows show (as last drawn): a row the keyboard moves to
    /// is scrolled into view.
    row_px: f32,
    visible_rows: usize,
    /// Each environment's name in the files (`None`: new), beside `names`: a renamed one's
    /// settings follow it. The names being written (they're the files' once written).
    was: Vec<Option<String>>,
    writing: Vec<String>,
    /// The environment whose name is being typed on its tab.
    renaming: Option<(usize, TextInput)>,
    /// The tab a right click was on (its menu's rows act on it).
    menu_tab: Option<usize>,
    /// An environment the user said to delete (the question's answer comes here).
    deleting: Arc<Mutex<Option<String>>>,
    /// Its parts' state of use (see [`EnvView::with_controls`]).
    controls: Controls,
    /// The rows' scrollbar, along the view's right edge.
    bar: EdgeScrollbar,
}

impl std::fmt::Debug for EnvView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnvView").field("folder", &self.folder).finish_non_exhaustive()
    }
}

/// The name being typed on an environment's tab, as a control: a press puts its caret (a
/// drag selects nothing), ↵ and ⇥ take it if it can be one, Esc gives it up, ⌘A selects
/// it; what it has selected isn't copied.
struct Naming<'a> {
    renaming: &'a mut Option<(usize, TextInput)>,
    names: &'a mut Vec<String>,
    error: &'a mut Option<String>,
    changed: &'a mut Option<Instant>,
    /// Where its field was drawn.
    area: Option<Rect>,
}

impl Naming<'_> {
    /// The typed name taken, if it can be one; else why not, and it stays typed.
    fn finish(&mut self) {
        let Some((i, input)) = &*self.renaming else { return };
        let (i, name) = (*i, input.text().trim().to_owned());
        let others =
            self.names.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, n)| n.as_str());
        if let Some(why) = env_edit::valid_name(&name, others) {
            *self.error = Some(t(match why {
                env_edit::NameError::Empty => "http.env_name_empty",
                env_edit::NameError::Reserved => "http.env_name_reserved",
                env_edit::NameError::Taken => "http.env_name_taken",
            }));
            return;
        }
        *self.renaming = None;
        *self.error = None;
        if self.names.get(i).is_some_and(|n| *n != name) {
            self.names[i] = name;
            *self.changed = Some(Instant::now());
        }
    }
}

impl Control for Naming<'_> {
    fn area(&self) -> Option<Rect> {
        self.area.filter(|_| self.renaming.is_some())
    }

    fn press(&mut self, p: Point, clicks: u32) -> Press {
        let Some((_, input)) = self.renaming else { return Press::Ignored };
        input.press(p.x, clicks);
        Press::Took
    }

    /// It has the keys while it's typed.
    fn focusable(&self) -> bool {
        self.renaming.is_some()
    }

    fn keeps_tab(&self) -> bool {
        true
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        match key {
            ViewerKey::Enter | ViewerKey::Tab => self.finish(),
            ViewerKey::Escape => {
                *self.renaming = None;
                *self.error = None;
            }
            _ => {
                let Some((_, input)) = self.renaming else { return false };
                return reach.edit(input, key);
            }
        }
        true
    }

    fn text(&mut self, typed: &str) -> bool {
        let Some((_, input)) = self.renaming else { return false };
        let one_line: String = typed.chars().filter(|c| !c.is_control()).collect();
        input.insert(&one_line);
        true
    }

    fn command(&mut self, id: &str) -> bool {
        let Some((_, input)) = self.renaming.as_mut().filter(|_| id == "select_all") else {
            return false;
        };
        input.select_all();
        true
    }
}

/// The shown environment's variables as a control: a press on a cell puts its caret there;
/// with the keys, ⇥ and ↵ go on to the next cell (the value after the name, the next row's
/// name after the value: scrolled into view), Esc leaves the table; what's typed goes in,
/// and the last row typed in gets an empty one after it.
struct Cells<'a> {
    lines: Option<&'a mut Vec<Line>>,
    focus: &'a mut Option<(usize, bool)>,
    changed: &'a mut Option<Instant>,
    hits: &'a [(Rect, Hit)],
    rows: Rows<'a>,
}

/// How the rows are scrolled (as last drawn), for the row the keys move to.
struct Rows<'a> {
    scroll: &'a mut f32,
    row_px: f32,
    visible_rows: usize,
}

impl Rows<'_> {
    /// Row `row` scrolled into view (whole rows, as they're drawn).
    fn keep_visible(&mut self, row: usize) {
        if self.row_px <= 0.0 || self.visible_rows == 0 {
            return;
        }
        let first = (*self.scroll / self.row_px).round() as usize;
        if row < first {
            *self.scroll = row as f32 * self.row_px;
        } else if row >= first + self.visible_rows {
            *self.scroll = (row + 1 - self.visible_rows) as f32 * self.row_px;
        }
    }
}

impl Cells<'_> {
    /// The cell drawn topmost at `p`.
    fn cell_at(&self, p: Point) -> Option<(usize, bool)> {
        match top_hit(self.hits, p)? {
            Hit::Cell(row, value) => Some((row, value)),
            _ => None,
        }
    }

    fn input(&mut self, (row, value): (usize, bool)) -> Option<&mut TextInput> {
        let line = self.lines.as_deref_mut()?.get_mut(row)?;
        Some(if value { &mut line.value } else { &mut line.name })
    }

    /// A row typed in: the last one gets an empty one after it; written a moment after.
    fn typed(&mut self, (row, _): (usize, bool)) {
        if let Some(lines) = self.lines.as_deref_mut()
            && row + 1 == lines.len()
        {
            lines.push(Line::empty());
        }
        *self.changed = Some(Instant::now());
    }
}

impl Control for Cells<'_> {
    fn hit(&self, p: Point) -> bool {
        self.cell_at(p).is_some()
    }

    fn press(&mut self, p: Point, clicks: u32) -> Press {
        let Some(cell) = self.cell_at(p) else { return Press::Ignored };
        *self.focus = Some(cell);
        if let Some(input) = self.input(cell) {
            input.press(p.x, clicks);
        }
        Press::Took
    }

    fn cursor(&self, _p: Point) -> Option<ViewerCursor> {
        Some(ViewerCursor::Text)
    }

    fn focusable(&self) -> bool {
        true
    }

    fn keeps_tab(&self) -> bool {
        true
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        let Some(focus) = *self.focus else { return false };
        match key {
            ViewerKey::Escape => {
                *self.focus = None;
                true
            }
            ViewerKey::Tab | ViewerKey::Enter => {
                let (row, value) = focus;
                let len = self.lines.as_deref().map_or(0, Vec::len);
                let next = if !value {
                    (row, true)
                } else if row + 1 < len {
                    (row + 1, false)
                } else {
                    focus
                };
                *self.focus = Some(next);
                self.rows.keep_visible(next.0);
                true
            }
            _ => {
                let edits = matches!(key, ViewerKey::Backspace | ViewerKey::Delete);
                let Some(input) = self.input(focus) else { return false };
                let used = reach.edit(input, key);
                if used && edits {
                    self.typed(focus);
                }
                used
            }
        }
    }

    fn text(&mut self, typed: &str) -> bool {
        let Some(focus) = *self.focus else { return false };
        let one_line: String = typed.chars().filter(|c| !c.is_control()).collect();
        let Some(input) = self.input(focus) else { return false };
        input.insert(&one_line);
        self.typed(focus);
        true
    }

    fn command(&mut self, id: &str) -> bool {
        let Some(focus) = (*self.focus).filter(|_| id == "select_all") else { return false };
        self.input(focus).map(TextInput::select_all).is_some()
    }
}

/// What's drawn topmost at `p`.
fn top_hit(hits: &[(Rect, Hit)], p: Point) -> Option<Hit> {
    hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)
}

/// The shown environment's cells' place among the view's parts (after the name being typed).
const CELLS: usize = 1;

impl EnvView {
    /// The cell typed in, read-only (what it has selected): a private value is never lent
    /// (it isn't copied out).
    fn typed_cell(&self) -> Option<&dyn Control> {
        let (row, value) = self.focus?;
        let line = self.lines.get(self.tab)?.get(row)?;
        if value && line.private {
            return None;
        }
        Some(if value { &line.value } else { &line.name })
    }

    /// Its parts (the name being typed on a tab, the shown environment's cells), with their
    /// state of use. The keys are the first's that takes them: the name's while it's typed
    /// (a cell pressed meanwhile doesn't take them from it), else the table's.
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let named = self.renaming.as_ref().map(|(i, _)| Hit::Tab(*i));
        let area = self.hits.iter().rev().find(|(_, h)| Some(*h) == named).map(|(r, _)| *r);
        let mut naming = Naming {
            renaming: &mut self.renaming,
            names: &mut self.names,
            error: &mut self.error,
            changed: &mut self.changed,
            area,
        };
        let mut typed = None;
        // The thumb moves its own copy (the cells hold the offset for the keys' moves).
        let before = self.scroll;
        let mut dragged = before;
        let mut cells = Cells {
            lines: self.lines.get_mut(self.tab),
            focus: &mut self.focus,
            changed: &mut typed,
            hits: &self.hits,
            rows: Rows {
                scroll: &mut self.scroll,
                row_px: self.row_px,
                visible_rows: self.visible_rows,
            },
        };
        let mut thumb = Thumb { bar: &mut self.bar, offset: &mut dragged };
        let mut parts: [&mut dyn Control; 3] = [&mut naming, &mut cells, &mut thumb];
        let controls = &mut self.controls;
        crate::parts::keys_to_first(controls, &mut parts);
        let answer = f(controls, &mut parts);
        crate::parts::keys_to_first(controls, &mut parts);
        if dragged != before {
            self.scroll = dragged.clamp(0.0, self.max_scroll);
        }
        if typed.is_some() {
            self.changed = typed;
        }
        answer
    }

    fn load(&mut self) {
        let Some(data) = self.data.clone() else { return };
        self.loading.start(THREAD, move || {
            let folder = http_environments::folder_for(&data.file, data.root.as_deref());
            let read = |name: &str| std::fs::read_to_string(folder.join(name)).ok();
            let (public, private) =
                (read(http_requests::env::PUBLIC), read(http_requests::env::PRIVATE));
            let tables = env_edit::tables(public.as_deref(), private.as_deref());
            (folder, public, private, tables)
        });
    }

    fn tables(&self) -> Vec<Table> {
        self.names
            .iter()
            .zip(&self.lines)
            .enumerate()
            .map(|(i, (name, lines))| Table {
                name: name.clone(),
                rows: lines.iter().map(Line::row).collect(),
                was: self.was.get(i).cloned().flatten(),
            })
            .collect()
    }

    fn save(&mut self) {
        self.changed = None;
        let Some(folder) = self.folder.clone() else { return };
        let tables = self.tables();
        self.writing = self.names.clone();
        let (public, private) = self.texts.clone();
        let root = self.data.as_ref().and_then(|d| d.root.clone());
        self.saving.start(THREAD, move || {
            let failed = |e: String| (e, false);
            // Written over what's on disk only if it's what was read: another's change (the
            // text editor's, git's) is never lost.
            let read = |name: &str| std::fs::read_to_string(folder.join(name)).ok();
            let disk = (read(http_requests::env::PUBLIC), read(http_requests::env::PRIVATE));
            if disk != (public.clone(), private.clone()) {
                return Err((t("http.changed_outside"), true));
            }
            let (new_public, new_private) =
                env_edit::write(&tables, public.as_deref(), private.as_deref()).map_err(failed)?;
            std::fs::create_dir_all(&folder).map_err(|e| failed(e.to_string()))?;
            files::write_atomic(&folder.join(http_requests::env::PUBLIC), new_public.as_bytes())
                .map_err(|e| failed(e.to_string()))?;
            if let Some(text) = &new_private {
                files::write_atomic(&folder.join(http_requests::env::PRIVATE), text.as_bytes())
                    .map_err(|e| failed(e.to_string()))?;
                // The user's own file: never committed.
                if let Some(root) = &root {
                    crate::gitignore::ensure(root);
                }
            }
            Ok((Some(new_public), new_private))
        });
    }

    fn take_work(&mut self) -> bool {
        let mut changed = false;
        if let Some((folder, public, private, tables)) = self.loading.take() {
            self.folder = Some(folder);
            self.texts = (public, private);
            self.renaming = None;
            if tables.is_empty() {
                self.names = vec![http_requests::env::SHARED.to_owned(), "dev".to_owned()];
                self.lines = vec![vec![Line::empty()], vec![Line::empty()]];
                self.was = vec![None, None];
            } else {
                self.names = tables.iter().map(|t| t.name.clone()).collect();
                self.was = tables.iter().map(|t| t.was.clone()).collect();
                self.lines = tables
                    .iter()
                    .map(|t| {
                        t.rows.iter().map(Line::of).chain(std::iter::once(Line::empty())).collect()
                    })
                    .collect();
            }
            self.tab = self.tab.min(self.names.len().saturating_sub(1));
            changed = true;
        }
        if let Some(saved) = self.saving.take() {
            match saved {
                Ok(texts) => {
                    self.texts = texts;
                    self.error = None;
                    // The names written are the files' now.
                    for (was, name) in self.was.iter_mut().zip(std::mem::take(&mut self.writing)) {
                        *was = Some(name);
                    }
                }
                Err((why, reload)) => {
                    self.error = Some(why);
                    if reload {
                        self.load();
                    }
                }
            }
            changed = true;
        }
        changed
    }

    /// Row `row` scrolled into view (whole rows, as they're drawn).
    #[cfg(test)]
    fn keep_visible(&mut self, row: usize) {
        let (row_px, visible_rows) = (self.row_px, self.visible_rows);
        Rows { scroll: &mut self.scroll, row_px, visible_rows }.keep_visible(row);
    }

    /// Environment `i`'s name typed on its tab (`$shared` keeps its own).
    fn start_rename(&mut self, i: usize) {
        let Some(name) = self.names.get(i).filter(|n| *n != http_requests::env::SHARED) else {
            return;
        };
        let mut input = TextInput::new(name);
        input.select_all();
        self.tab = i;
        self.focus = None;
        self.renaming = Some((i, input));
    }

    /// The typed name taken (Enter), if it can be one; else why not, and it stays typed.
    fn finish_rename(&mut self) {
        Naming {
            renaming: &mut self.renaming,
            names: &mut self.names,
            error: &mut self.error,
            changed: &mut self.changed,
            area: None,
        }
        .finish();
    }

    /// Environment `i` deleted once the user says so (its variables go from both files).
    fn ask_delete(&mut self, i: usize) {
        let Some(name) = self.names.get(i).filter(|n| *n != http_requests::env::SHARED).cloned()
        else {
            return;
        };
        let answer = Arc::clone(&self.deleting);
        let question = workbench_model::Question {
            title: i18n::tr!("http.env_delete_title", name = name.as_str()),
            message: t("http.env_delete_message"),
            icon: "trash",
            yes: t("http.env_delete_yes"),
            danger: true,
            list: Vec::new(),
        };
        self.action = Some(ViewerAction::WithHost(WithHost::once(move |host| {
            host.confirm(
                question,
                Box::new(move |_| {
                    *answer.lock().unwrap_or_else(PoisonError::into_inner) = Some(name);
                }),
            );
        })));
    }

    /// The environment the user said to delete, deleted.
    fn take_deleted(&mut self) -> bool {
        let Some(name) = self.deleting.lock().unwrap_or_else(PoisonError::into_inner).take() else {
            return false;
        };
        let Some(i) = self.names.iter().position(|n| *n == name) else { return false };
        self.names.remove(i);
        self.lines.remove(i);
        if i < self.was.len() {
            self.was.remove(i);
        }
        self.renaming = None;
        self.focus = None;
        self.tab = self.tab.min(self.names.len().saturating_sub(1));
        self.changed = Some(Instant::now());
        true
    }

    fn tab_at(&self, p: Point) -> Option<usize> {
        match top_hit(&self.hits, p)? {
            Hit::Tab(i) => Some(i),
            _ => None,
        }
    }

    fn open(&mut self, name: &str) {
        let Some(folder) = &self.folder else { return };
        let path = folder.join(name);
        self.action = Some(ViewerAction::OpenFile { path, line: None, column: None });
    }
}

impl Viewer for EnvView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        let Some(data) = data.downcast_ref::<EnvData>() else { return };
        self.data = Some(data.clone());
        self.load();
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        let ViewerPaint {
            bounds: b, scale, text, scene, palette: p, icons, waker, focused, ..
        } = ctx;
        self.loading.set_waker(&waker);
        self.saving.set_waker(&waker);
        self.take_work();
        let ds = Ctx::new(p, scale);
        let px = |v: f32| ds.px(v);
        let layer = scene.push_layer(Some(b));
        layer.quad(Quad::filled(b, p.editor_background));
        let mut hits = Vec::new();
        let font = ds.ui_font(0.0);
        // The bar: the folder, the JSON files.
        let bar = Rect::new(b.min_x(), b.min_y(), b.size.width, px(BAR));
        ds::divider(layer, &ds, bar.min_x(), bar.max_x(), bar.max_y());
        let folder = self.folder.as_ref().map(|f| f.display().to_string()).unwrap_or_default();
        let mut x = bar.max_x() - px(12.0);
        for (hit, key) in
            [(Hit::OpenPrivate, "http.env_open_private"), (Hit::OpenPublic, "http.env_open_public")]
        {
            let label = t(key);
            let w = ds::toolbar_button_width(text, &ds, &label);
            let r = Rect::new(
                x - w,
                bar.min_y() + (bar.size.height - px(ds::TOOLBAR_BUTTON_H)) / 2.0,
                w,
                px(ds::TOOLBAR_BUTTON_H),
            );
            ds::toolbar_button(
                layer,
                text,
                icons,
                &ds,
                r,
                ToolbarButton {
                    label: &label,
                    icon: "file-json",
                    accent: false,
                    hovered: self.hover == Some(hit),
                },
            );
            hits.push((r, hit));
            x = r.min_x() - px(8.0);
        }
        let shown = ds::ellipsize(text, &folder, &font, (x - bar.min_x() - px(24.0)).max(0.0));
        let shaped = text.shape_line(&shown, &font);
        draw_text(
            layer,
            &shaped,
            bar.min_x() + px(14.0),
            ds::optical_baseline(bar.min_y(), bar.size.height, &font),
            p.text_muted,
        );
        // The environments' tabs.
        let tabs = Rect::new(b.min_x(), bar.max_y(), b.size.width, px(38.0));
        ds::divider(layer, &ds, tabs.min_x(), tabs.max_x(), tabs.max_y());
        let mut x = tabs.min_x() + px(14.0);
        let mut style = ds::field_style(&ds, font.clone(), true);
        style.padding = px(6.0);
        let tabs_mid = tabs.min_y() + tabs.size.height / 2.0;
        for (i, name) in self.names.iter().enumerate() {
            // Its name being typed: a field in its place.
            if let Some((_, input)) = self.renaming.as_ref().filter(|(r, _)| *r == i) {
                let typed = text.shape_line(input.text(), &font).width;
                let w = (typed + px(28.0)).max(px(120.0));
                let field = Rect::new(x, tabs.min_y() + px(5.0), w, tabs.size.height - px(10.0));
                input.paint(layer, text, field, focused, "", &style);
                hits.push((field, Hit::Tab(i)));
                x = field.max_x() + px(4.0);
                continue;
            }
            let tab = UnderlineTab::new(name).on(i == self.tab);
            let at = (x, tabs.min_y(), tabs.size.height);
            let r = tab_strip::underline_tab(layer, text, &ds, &font, at, &tab);
            hits.push((r, Hit::Tab(i)));
            x = r.max_x() + px(TAB_GAP);
        }
        let add = ds::square_button(&ds, (x, tabs_mid), button_size::COMPACT);
        let spec = ButtonSpec::new(ButtonKind::Ghost)
            .icon("plus")
            .rest_tone(Tone::Neutral)
            .hovered(self.hover == Some(Hit::AddEnvironment));
        ds::button(layer, text, icons, &ds, add, spec);
        hits.push((add, Hit::AddEnvironment));
        // Its variables.
        let area = Rect::new(
            b.min_x() + px(14.0),
            tabs.max_y() + px(10.0),
            b.size.width - px(28.0),
            (b.max_y() - tabs.max_y() - px(14.0)).max(0.0),
        );
        let mut style = ds::field_style(&ds, ds.code_font(-0.5), false);
        style.background = Rgba::TRANSPARENT;
        style.border = Rgba::TRANSPARENT;
        // Between the table's lines: no frame of its own, typed in or not.
        style.border_focused = Rgba::TRANSPARENT;
        let row_h = px(ROW);
        let tail = px(130.0);
        let name_w = ((area.size.width - tail) * 0.4).round();
        let focus = if focused { self.focus } else { None };
        let rows = self.lines.get(self.tab).map_or(0, Vec::len);
        // Scrolled by whole rows: none drawn half under the tabs (nor clicked there).
        let visible = (area.size.height / row_h).floor().max(1.0);
        self.row_px = row_h;
        self.visible_rows = visible as usize;
        self.max_scroll = (rows as f32 - visible).max(0.0) * row_h;
        self.scroll = self.scroll.clamp(0.0, self.max_scroll);
        let first = (self.scroll / row_h).round() * row_h;
        if let Some(lines) = self.lines.get(self.tab) {
            for (i, line) in lines.iter().enumerate() {
                let y = area.min_y() + i as f32 * row_h - first;
                if y < area.min_y() {
                    continue;
                }
                if y + row_h > area.max_y() {
                    break;
                }
                let name_r = Rect::new(area.min_x(), y + px(1.0), name_w, row_h - px(2.0));
                let value_r = Rect::new(
                    name_r.max_x() + px(6.0),
                    y + px(1.0),
                    (area.size.width - tail - name_w - px(6.0)).max(0.0),
                    row_h - px(2.0),
                );
                line.name.paint(
                    layer,
                    text,
                    name_r,
                    focus == Some((i, false)),
                    &t("http.cell_name"),
                    &style,
                );
                line.value.paint(
                    layer,
                    text,
                    value_r,
                    focus == Some((i, true)),
                    &t("http.cell_value"),
                    &style,
                );
                hits.push((name_r, Hit::Cell(i, false)));
                hits.push((value_r, Hit::Cell(i, true)));
                let mid = y + row_h / 2.0;
                let check = ds::checkbox(
                    layer,
                    text,
                    &ds,
                    value_r.max_x() + px(12.0),
                    mid,
                    if line.private { Check::On } else { Check::Off },
                    true,
                );
                let label = text.shape_line(&t("http.env_private"), &ds.ui_font(-1.0));
                draw_text(
                    layer,
                    &label,
                    check.max_x() + px(6.0),
                    ds::optical_baseline(y, row_h, &ds.ui_font(-1.0)),
                    p.text_muted,
                );
                hits.push((
                    Rect::new(check.min_x(), y, check.size.width + px(6.0) + label.width, row_h),
                    Hit::Private(i),
                ));
                // Removing it: in the danger color under the mouse.
                let at = area.max_x() - px(TRASH_RIGHT) - px(button_size::SMALL) / 2.0;
                let trash = ds::square_button(&ds, (at, mid), button_size::SMALL);
                let spec = ButtonSpec::new(ButtonKind::Ghost)
                    .icon("trash")
                    .rest_tone(Tone::Neutral)
                    .hover_content_tone(Tone::Danger)
                    .hovered(self.hover == Some(Hit::Remove(i)));
                ds::button(layer, text, icons, &ds, trash, spec);
                hits.push((trash, Hit::Remove(i)));
                ds::divider(layer, &ds, area.min_x(), area.max_x(), y + row_h);
            }
        }
        let rows_r = Rect::new(b.min_x(), area.min_y(), b.size.width, visible * row_h);
        self.bar.paint(scene, p, scale, rows_r, rows as f32 * row_h, first);
        if let Some(why) = &self.error {
            let layer = scene.push_layer(Some(b));
            let shaped = text.shape_line(why, &ds.ui_font(-1.0));
            draw_text(layer, &shaped, area.min_x(), area.max_y() - px(8.0), p.danger);
        }
        self.hits = hits;
    }

    fn poll(&mut self) -> bool {
        let changed = self.take_work() | self.take_deleted();
        if self.changed.is_some_and(|at| at.elapsed() >= SAVE_AFTER) {
            self.save();
        }
        changed
    }

    fn frame_deadline(&self) -> Option<Instant> {
        self.changed.map(|at| at + SAVE_AFTER)
    }

    fn pointer_down(
        &mut self,
        p: Point,
        clicks: u32,
        _mods: ui_components::PressMods,
    ) -> Option<ViewerAction> {
        // Its parts: the name being typed (its caret moves), a cell (it takes the keys).
        if self.with_controls(|c, parts| c.press(parts, p, clicks)).is_some() {
            return self.action.take();
        }
        match top_hit(&self.hits, p) {
            Some(Hit::Tab(i)) => {
                self.finish_rename();
                if clicks >= 2 {
                    self.start_rename(i);
                } else {
                    self.tab = i;
                    self.focus = None;
                    self.scroll = 0.0;
                }
            }
            Some(Hit::AddEnvironment) => {
                self.finish_rename();
                let name = (1..)
                    .map(|n| if n == 1 { "new".to_owned() } else { format!("new-{n}") })
                    .find(|n| !self.names.contains(n))
                    .unwrap_or_default();
                self.names.push(name);
                self.lines.push(vec![Line::empty()]);
                self.tab = self.names.len() - 1;
                self.changed = Some(Instant::now());
            }
            Some(Hit::OpenPublic) => self.open(http_requests::env::PUBLIC),
            Some(Hit::OpenPrivate) => self.open(http_requests::env::PRIVATE),
            // Taken by the cells (above).
            Some(Hit::Cell(..)) => {}
            Some(Hit::Private(row)) => {
                if let Some(line) = self.lines.get_mut(self.tab).and_then(|l| l.get_mut(row)) {
                    line.private = !line.private;
                    line.value.set_masked(line.private);
                }
                self.changed = Some(Instant::now());
            }
            Some(Hit::Remove(row)) => {
                if let Some(lines) = self.lines.get_mut(self.tab) {
                    if row < lines.len() {
                        lines.remove(row);
                    }
                    if lines.is_empty() {
                        lines.push(Line::empty());
                    }
                }
                self.focus = None;
                self.changed = Some(Instant::now());
            }
            None => {
                self.finish_rename();
                self.focus = None;
            }
        }
        self.action.take()
    }

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        let before = self.scroll;
        self.scroll = (self.scroll + dy).clamp(0.0, self.max_scroll);
        self.scroll != before
    }

    /// The part pressed (the rows' thumb) follows the mouse.
    fn drag(&mut self, p: Point) -> bool {
        self.with_controls(|c, parts| c.drag(parts, p)).is_some_and(|(_, changed)| changed)
    }

    fn release(&mut self) -> bool {
        self.with_controls(|c, parts| c.release(parts)).is_some_and(|(_, changed)| changed)
    }

    fn hover(&mut self, p: Point) -> bool {
        let parts = self.with_controls(|c, parts| c.hover(parts, Some(p)));
        let hover = top_hit(&self.hits, p);
        (std::mem::replace(&mut self.hover, hover) != hover) | parts
    }

    /// A part's (a cell's text cursor), else a hand on what's clickable.
    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        let own = top_hit(&self.hits, p).filter(|h| !matches!(h, Hit::Cell(..)));
        self.controls.cursor_at(p).or_else(|| own.map(|_| ViewerCursor::Pointer))
    }

    fn takes_keyboard(&self) -> bool {
        true
    }

    fn wants_text(&self) -> bool {
        self.focus.is_some() || self.renaming.is_some()
    }

    fn context_menu(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        let i = self.tab_at(p).filter(|i| self.names[*i] != http_requests::env::SHARED)?;
        self.menu_tab = Some(i);
        Some(vec![
            ViewMenuEntry::item("env-rename", t("http.env_rename")).with_icon("pencil"),
            ViewMenuEntry::item("env-delete", t("http.env_delete")).with_icon("trash"),
        ])
    }

    /// To the part with the keys (the name being typed, else the cell).
    fn text_input(&mut self, typed: &str) -> bool {
        self.with_controls(|c, parts| c.text(parts, typed)).is_some()
    }

    fn paste(&mut self, text: &str) -> bool {
        self.text_input(text)
    }

    /// What the part with the keys has selected: the cell typed in (the name being typed on
    /// a tab is never copied out).
    fn selected_text(&self) -> Option<String> {
        self.controls.selected_text_with(|part| match part {
            CELLS => self.typed_cell(),
            _ => None,
        })
    }

    fn select_all(&mut self) -> bool {
        self.with_controls(|c, parts| c.command(parts, "select_all")).is_some()
    }

    fn command(&mut self, id: &str) -> bool {
        match (id, self.menu_tab.take()) {
            ("env-rename", Some(i)) => self.start_rename(i),
            ("env-delete", Some(i)) => self.ask_delete(i),
            _ => return matches!(id, "select_all" | "select-all") && self.select_all(),
        }
        true
    }

    /// To the part with the keys: the name being typed takes ↵ ⇥ Esc, a cell moves on with
    /// ⇥ and ↵.
    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        self.with_controls(|c, parts| c.key(parts, key, reach)).is_some()
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }

    fn may_close(&mut self) -> bool {
        self.finish_rename();
        if self.changed.is_some() {
            self.save();
        }
        true
    }
}

/// The environments' tab.
#[derive(Debug, Default)]
pub struct EnvProvider;

impl ViewerProvider for EnvProvider {
    fn id(&self) -> &'static str {
        "http-environments"
    }

    fn kind(&self, path: &std::path::Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &std::path::Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(EnvView::default()))
    }
}

#[cfg(test)]
#[path = "../tests/unit/env_view.rs"]
mod tests;
