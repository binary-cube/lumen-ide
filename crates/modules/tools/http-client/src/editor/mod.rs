//! The visual editor of a request (plan 77l): its method, its URL
//! and Send on top; Params, Body, Auth, Headers, Scripts and Docs in tabs under them; its
//! latest response on the right. What it holds is the request's block of its `.http` file
//! ([`http_requests::draft`]): read on a worker when it opens, written back on a worker a
//! moment after each change (and before a Send) — the file stays a plain `.http` file.

mod form;
mod paint;
mod suggest;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use http_requests::draft::Draft;
use kernel::{Point, Rect};
use ui_components::picker::{Picker, PickerEvent, PickerItem};
use ui_components::tool_panel::KeyReach;
use ui_components::{
    MultilineField, Viewer, ViewerAction, ViewerCursor, ViewerKey, ViewerKind, ViewerPaint,
    ViewerProvider,
};
use ui_kit::TextInput;

use crate::pending::Pending;
use crate::response_view::{Part, ResponseView};
use crate::runs::{RunId, Runs};
pub use form::{AuthKind, BodyKind, Form, Table};

/// The extension its tab is opened by.
pub const EXTENSION: &str = "view-http-request";
/// The worker it reads and writes its file on.
pub const THREAD: &str = "http-editor";
/// How long after the last change the request is written to its file.
const SAVE_AFTER: Duration = Duration::from_millis(600);

/// What its tab is opened with: the file, which request of it (`None`: a new one, written
/// at its end).
#[derive(Debug, Clone)]
pub struct EditorData {
    pub runs: Arc<Runs>,
    pub file: PathBuf,
    pub index: Option<usize>,
}

/// Its tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Params,
    Body,
    Auth,
    Headers,
    Scripts,
    Docs,
    Settings,
}

impl Tab {
    pub const ALL: [Self; 7] = [
        Self::Params,
        Self::Body,
        Self::Auth,
        Self::Headers,
        Self::Scripts,
        Self::Docs,
        Self::Settings,
    ];

    fn label_key(self) -> &'static str {
        match self {
            Self::Params => "http.tab_params",
            Self::Body => "http.tab_body",
            Self::Auth => "http.tab_auth",
            Self::Headers => "http.tab_headers",
            Self::Scripts => "http.tab_scripts",
            Self::Docs => "http.tab_docs",
            Self::Settings => "http.tab_settings",
        }
    }
}

/// Its texts of many lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Area {
    Body,
    Variables,
    Pre,
    Post,
    Docs,
    Bulk,
}

/// What has the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Title,
    Url,
    /// A table's row, its name or (`true`) its value.
    Cell(Table, usize, bool),
    Auth(usize),
    File,
    Timeout,
    ConnectTimeout,
    Area(Area),
}

/// A request's settings, on or off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    Redirects,
    Cookies,
    History,
    Encoding,
}

/// What a press lands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Field(Focus),
    Method,
    Send,
    /// Stop its run going.
    Cancel,
    /// A field emptied (its ×).
    Clear(Focus),
    Toggle(Setting),
    Tab(Tab),
    Check(Table, usize),
    Remove(Table, usize),
    FileToggle(usize),
    Add(Table),
    DeleteAll(Table),
    BodyKind,
    AuthKind,
    KeyPlace,
    Bulk,
    CopyUrl,
    Part(Part),
    DocsMode(bool),
}

/// Which list is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pick {
    Method,
    Body,
    Auth,
    /// What a `{{` typed in a field offers: put in at `start`.
    Suggest(Focus, usize),
}

/// The file read: its text, the request's index, its block as read, its label, its draft.
type Loaded = Result<(String, Option<usize>, Option<String>, Option<String>, Draft), String>;
/// The file written: its text, the request's index, its block as written, its label.
type Saved = Result<(String, Option<usize>, String, Option<String>), Unsaved>;

/// Why it wasn't written.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Unsaved {
    /// Its block changed in the file (typed in the text, another request removed): read
    /// again rather than written over.
    Changed,
    Failed(String),
}

#[derive(Default)]
pub struct RequestEditor {
    /// The request's share of the width (0: the default), the line between it and the
    /// response, where they were last drawn.
    share: f32,
    splitter: Option<ui_components::Splitter>,
    split_area: Rect,
    data: Option<EditorData>,
    form: Option<Form>,
    /// The file's text as last read or written.
    text: String,
    /// What [`Self::text`] offers after `{{`, read from it at the first `{{` typed (let go
    /// when the text changes).
    file_names: Option<std::sync::Arc<suggest::FileNames>>,
    index: Option<usize>,
    /// Its block as last read or written: what a write finds it by (never another's, never
    /// one changed meanwhile).
    block: Option<String>,
    /// Its label as its runs go by (worked out when it's read or written, not each frame).
    label: Option<String>,
    /// A change came while a write was going: written again after it.
    save_again: bool,
    loading: Pending<Loaded>,
    saving: Pending<Saved>,
    /// Changed since it was last written; when.
    changed: Option<Instant>,
    send_after_save: bool,
    error: Option<String>,
    tab: Option<Tab>,
    focus: Option<Focus>,
    picker: Picker,
    picking: Option<Pick>,
    hits: Vec<(Rect, Hit)>,
    /// The field the open list hangs from.
    anchor: Rect,
    hover: Option<Hit>,
    /// The parameters as text (`name=value` lines), while edited so.
    bulk: Option<MultilineField>,
    docs_preview: bool,
    part: Option<Part>,
    /// Its latest run's response, shown on the right.
    response: Option<(RunId, Part, ResponseView)>,
    generation: u64,
    /// The field pressed (what a drag selects in).
    dragging: Option<Focus>,
    action: Option<ViewerAction>,
    /// Its runs follow it (its paint's waker given them).
    pub(crate) waker_known: bool,
    /// The environments its fields' `{{` offers, read on a worker.
    environments: suggest::Read,
    reading_environments: Pending<suggest::Read>,
    /// What the list of suggestions shows.
    suggestions: Vec<suggest::Suggestion>,
}

impl std::fmt::Debug for RequestEditor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequestEditor")
            .field("file", &self.data.as_ref().map(|d| &d.file))
            .field("index", &self.index)
            .finish_non_exhaustive()
    }
}

impl RequestEditor {
    fn tab(&self) -> Tab {
        self.tab.unwrap_or(Tab::Params)
    }

    fn part(&self) -> Part {
        self.part.unwrap_or(Part::Body)
    }

    /// The response's parts shown as tabs: the latest run's (all but Preview while none).
    fn shown_parts(&self) -> Vec<Part> {
        let state = self
            .response
            .as_ref()
            .zip(self.data.as_ref())
            .and_then(|((id, _, _), data)| data.runs.run(*id))
            .map(|run| run.state);
        match state {
            Some(state) => Part::of(&state),
            None => Part::ALL.into_iter().chain([Part::Tests]).collect(),
        }
    }

    /// The file read and the request's draft made (on a worker).
    fn load(&mut self) {
        self.read_environments();
        let Some(data) = self.data.clone() else { return };
        let (known, label) = (self.block.clone(), self.label.clone());
        let index = self.index.or(data.index);
        self.loading.start(THREAD, move || {
            let text = std::fs::read_to_string(&data.file).unwrap_or_default();
            let parsed = http_requests::parse(&text);
            // Read again: its block where it is now, else the request by its label, else
            // the one at its place.
            let index = known
                .as_deref()
                .and_then(|b| http_requests::draft::find_block(&text, b))
                .or_else(|| {
                    let label = label.as_deref()?;
                    parsed.requests.iter().position(|r| r.label() == label)
                })
                .or(index);
            let (draft, block, label) = match index.and_then(|i| parsed.requests.get(i)) {
                Some(request) => {
                    let span = http_requests::draft::block_of(&text, request);
                    let block = text[span].to_owned();
                    (Draft::of(request, &block), Some(block), Some(request.label()))
                }
                None => {
                    let mut draft = Draft::new(&i18n::t("http.new_request_title"));
                    draft.url = "https://".into();
                    (draft, None, None)
                }
            };
            Ok((text, index, block, label, draft))
        });
    }

    /// The environments beside its file read again (on a worker), for its fields' `{{`.
    fn read_environments(&mut self) {
        let Some(data) = self.data.clone() else { return };
        self.reading_environments.start(THREAD, move || {
            Arc::new(http_environments::of(&data.file, data.runs.root().as_deref()))
        });
    }

    /// The run going for it, if one is.
    fn running(&self) -> Option<RunId> {
        let data = self.data.as_ref()?;
        data.runs.running_for(&data.file, self.label.as_deref()?)
    }

    /// What a `{{` typed before the caret of `focus` offers, listed (nothing: closed).
    fn suggest(&mut self, focus: Focus) {
        // A field of one line or many (a body, a script): only what's typed after `{{` taken
        // (not the field's whole text).
        let typed = |text: &str, cursor: usize| {
            suggest::typed(text, cursor).map(|(start, prefix)| (start, prefix.to_owned()))
        };
        let open = match focus {
            Focus::Area(area) => match self.area(area) {
                Some(field) => typed(field.text(), field.cursor()),
                None => return,
            },
            _ => match self.input(focus) {
                Some(input) => typed(input.text(), input.cursor()),
                None => return,
            },
        };
        let Some((start, prefix)) = open else {
            if matches!(self.picking, Some(Pick::Suggest(..))) {
                self.picker.close();
                self.picking = None;
            }
            return;
        };
        let Some(data) = self.data.clone() else { return };
        let environment = data.runs.environment(&data.file);
        let answer = |request: &str| -> crate::assist::Answer {
            let outcome = data.runs.latest(&data.file, request)?;
            let headers = outcome.response.headers.iter().map(|h| h.name.clone()).collect();
            Some((headers, outcome.json.clone()))
        };
        let file = std::sync::Arc::clone(
            self.file_names
                .get_or_insert_with(|| std::sync::Arc::new(suggest::FileNames::of(&self.text))),
        );
        self.suggestions = suggest::suggestions(
            &prefix,
            &self.environments,
            environment.as_deref(),
            &file,
            &answer,
        );
        if self.suggestions.is_empty() {
            self.picker.close();
            self.picking = None;
            return;
        }
        let items = self
            .suggestions
            .iter()
            .map(|s| PickerItem {
                label: s.label.clone(),
                detail: s.detail.clone(),
                ..PickerItem::default()
            })
            .collect();
        self.picker.open_suggestions(items, None, true);
        self.picking = Some(Pick::Suggest(focus, start));
    }

    /// A change typed: written a moment after the last one.
    fn changed(&mut self) {
        self.changed = Some(Instant::now());
        self.error = None;
    }

    /// The request written into its file now (on a worker).
    fn save(&mut self) {
        // Nothing to write into (no file, no form): nothing due any more either.
        self.changed = None;
        let (Some(data), Some(form)) = (self.data.clone(), &self.form) else { return };
        // One write at a time: the next after it (a new request appended once).
        if self.saving.is_busy() {
            self.save_again = true;
            return;
        }
        let draft = form.draft();
        let block = self.block.clone();
        self.saving.start(THREAD, move || {
            let text = std::fs::read_to_string(&data.file).unwrap_or_default();
            // Its block found again as it was read: if it changed (or went), not written over.
            let index = match &block {
                Some(block) => {
                    Some(http_requests::draft::find_block(&text, block).ok_or(Unsaved::Changed)?)
                }
                None => None,
            };
            let written = http_requests::draft::write_into(&text, index, &draft);
            if let Some(dir) = data.file.parent() {
                std::fs::create_dir_all(dir).map_err(|e| Unsaved::Failed(e.to_string()))?;
            }
            files::write_atomic(&data.file, written.as_bytes())
                .map_err(|e| Unsaved::Failed(e.to_string()))?;
            let parsed = http_requests::parse(&written);
            let new_block = draft.to_text();
            let index = http_requests::draft::find_block(&written, &new_block)
                .or_else(|| index.or_else(|| parsed.requests.len().checked_sub(1)));
            let label = index.and_then(|i| parsed.requests.get(i)).map(|r| r.label());
            Ok((written, index, new_block, label))
        });
    }

    /// Send: written first, then run (with what it uses, as its file's requests run).
    fn send(&mut self) {
        self.send_after_save = true;
        self.save();
    }

    fn run_now(&mut self) {
        let (Some(data), Some(index)) = (&self.data, self.index) else { return };
        let parsed = http_requests::parse(&self.text);
        let Some(request) = parsed.requests.get(index) else { return };
        crate::job::run_at(&data.runs, &data.file, &self.text, request.range.start);
        self.part = Some(Part::Body);
    }

    /// What came back from the workers: the draft read, the file written.
    fn take_work(&mut self) -> bool {
        let mut changed = false;
        if let Some(read) = self.reading_environments.take() {
            self.environments = read;
        }
        if let Some(loaded) = self.loading.take() {
            match loaded {
                Ok((text, index, block, label, draft)) => {
                    self.text = text;
                    self.file_names = None;
                    self.index = index;
                    self.block = block;
                    self.label = label;
                    self.form = Some(Form::of(&draft));
                    // Each table with its empty row to type a new entry in.
                    for table in [Table::Params, Table::Headers, Table::Form, Table::Multipart] {
                        self.ensure_new_row(table);
                    }
                }
                Err(why) => self.error = Some(why),
            }
            changed = true;
        }
        if let Some(saved) = self.saving.take() {
            match saved {
                Ok((text, index, block, label)) => {
                    self.text = text;
                    self.file_names = None;
                    self.index = index;
                    self.block = Some(block);
                    self.label = label;
                    // Written: a variable added to the environment meanwhile shows.
                    self.read_environments();
                    if std::mem::take(&mut self.send_after_save) {
                        self.run_now();
                    }
                }
                // Changed in the file meanwhile: read again, said.
                Err(Unsaved::Changed) => {
                    self.send_after_save = false;
                    self.save_again = false;
                    self.load();
                    self.error = Some(i18n::t("http.changed_outside"));
                }
                Err(Unsaved::Failed(why)) => {
                    self.send_after_save = false;
                    self.error = Some(why);
                }
            }
            if std::mem::take(&mut self.save_again) {
                self.save();
            }
            changed = true;
        }
        changed
    }

    /// Its latest run's response shown (made again when another run came, or another part
    /// was chosen).
    fn follow_response(&mut self) {
        let Some(data) = &self.data else { return };
        let Some(label) = self.label.clone() else { return };
        let latest =
            data.runs.runs().into_iter().rev().find(|r| r.file == data.file && r.label == label);
        let Some(run) = latest else {
            self.response = None;
            return;
        };
        let part = self.part();
        if self.response.as_ref().is_some_and(|(id, p, _)| *id == run.id && *p == part) {
            return;
        }
        self.response =
            Some((run.id, part, ResponseView::new(Arc::clone(&data.runs), run.id, part)));
    }

    fn open_picker(&mut self, pick: Pick) {
        let Some(form) = &self.form else { return };
        let (items, chosen): (Vec<PickerItem>, Option<usize>) = match pick {
            Pick::Method => (
                form::METHODS
                    .iter()
                    .map(|m| PickerItem { label: (*m).into(), ..PickerItem::default() })
                    .collect(),
                form::METHODS.iter().position(|m| *m == form.method),
            ),
            Pick::Body => (
                BodyKind::ALL
                    .iter()
                    .map(|k| PickerItem { label: i18n::t(k.label_key()), ..PickerItem::default() })
                    .collect(),
                BodyKind::ALL.iter().position(|k| *k == form.body_kind),
            ),
            Pick::Auth => (
                AuthKind::ALL
                    .iter()
                    .map(|k| PickerItem { label: i18n::t(k.label_key()), ..PickerItem::default() })
                    .collect(),
                AuthKind::ALL.iter().position(|k| *k == form.auth_kind),
            ),
            Pick::Suggest(..) => return,
        };
        self.picker.open(items, chosen);
        self.picking = Some(pick);
    }

    fn picked(&mut self, i: usize) {
        let (Some(pick), Some(form)) = (self.picking.take(), self.form.as_mut()) else { return };
        match pick {
            Pick::Method => {
                if let Some(method) = form::METHODS.get(i) {
                    form.method = (*method).to_owned();
                }
            }
            Pick::Body => {
                if let Some(kind) = BodyKind::ALL.get(i) {
                    form.set_body_kind(*kind);
                }
            }
            Pick::Auth => {
                if let Some(kind) = AuthKind::ALL.get(i) {
                    form.set_auth_kind(*kind);
                }
            }
            Pick::Suggest(focus, start) => {
                let Some(insert) = self.suggestions.get(i).map(|s| s.insert.clone()) else {
                    return;
                };
                if let Focus::Area(area) = focus {
                    // Many lines: what's typed after `{{` taken back, the name put in.
                    let Some(field) = self.area(area) else { return };
                    let (text, cursor) = (field.text().to_owned(), field.cursor());
                    let typed = text.get(start..cursor).map_or(0, |t| t.chars().count());
                    for _ in 0..typed {
                        field.backspace(false);
                    }
                    let open_path = insert.ends_with(".$") || insert.ends_with('.');
                    let closed = text[cursor..].starts_with("}}") || open_path;
                    field.insert(&format!("{insert}{}", if closed { "" } else { "}}" }));
                    self.typed_in(focus);
                    self.suggest(focus);
                    return;
                }
                let Some(input) = self.input(focus) else { return };
                let (text, cursor) = (input.text().to_owned(), input.cursor());
                let (new, caret) = suggest::apply(&text, start, cursor, &insert);
                input.set_text(&new);
                input.select(caret..caret);
                self.typed_in(focus);
                // A path still being written: its next steps offered.
                self.suggest(focus);
                return;
            }
        }
        self.changed();
    }

    /// The one-line field `focus` is.
    fn input(&mut self, focus: Focus) -> Option<&mut TextInput> {
        let form = self.form.as_mut()?;
        match focus {
            Focus::Title => Some(&mut form.title),
            Focus::Url => Some(&mut form.url),
            Focus::Cell(table, row, value) => {
                let row = form.rows(table).get_mut(row)?;
                Some(if value { &mut row.value } else { &mut row.name })
            }
            Focus::Auth(i) => form.auth.get_mut(i),
            Focus::File => Some(&mut form.file),
            Focus::Timeout => Some(&mut form.timeout),
            Focus::ConnectTimeout => Some(&mut form.connect_timeout),
            Focus::Area(_) => None,
        }
    }

    /// The field of many lines `area` is.
    fn area(&mut self, area: Area) -> Option<&mut MultilineField> {
        if area == Area::Bulk {
            return self.bulk.as_mut();
        }
        let form = self.form.as_mut()?;
        Some(match area {
            Area::Body => &mut form.body,
            Area::Variables => &mut form.variables,
            Area::Pre => &mut form.pre,
            Area::Post => &mut form.post,
            Area::Docs => &mut form.docs,
            Area::Bulk => unreachable!("handled above"),
        })
    }

    /// What typing in `focus` changes beyond its text: the URL and the parameters follow each
    /// other; a table's last row typed in gets a new empty one after it.
    fn typed_in(&mut self, focus: Focus) {
        let Some(form) = self.form.as_mut() else { return };
        match focus {
            Focus::Url => form.url_typed(),
            Focus::Cell(table, row, _) => {
                let rows = form.rows(table);
                if row + 1 == rows.len() {
                    rows.push(form::Row::new("", "", true));
                }
                if table == Table::Params {
                    form.params_changed();
                }
            }
            Focus::Area(Area::Bulk) => {
                let text = self.bulk.as_ref().map(|b| b.text().to_owned()).unwrap_or_default();
                form.params = text
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .map(|l| {
                        let (name, value) = l.split_once('=').unwrap_or((l, ""));
                        form::Row::new(name.trim(), value.trim(), true)
                    })
                    .collect();
                form.params_changed();
            }
            _ => {}
        }
        self.changed();
    }

    /// The table's rows with an empty one to type a new entry into, last.
    fn ensure_new_row(&mut self, table: Table) {
        if let Some(form) = self.form.as_mut() {
            let rows = form.rows(table);
            if rows.last().is_none_or(|r| !r.name.text().is_empty() || !r.value.text().is_empty()) {
                rows.push(form::Row::new("", "", true));
            }
        }
    }

    fn press_hit(&mut self, hit: Hit, p: Point, clicks: u32) {
        match hit {
            Hit::Field(focus) => {
                self.focus = Some(focus);
                self.dragging = Some(focus);
                match focus {
                    Focus::Area(area) => {
                        if let Some(field) = self.area(area) {
                            field.press(p, clicks, false);
                        }
                    }
                    _ => {
                        if let Some(input) = self.input(focus) {
                            input.press(p.x, clicks);
                        }
                    }
                }
            }
            Hit::Method => self.open_picker(Pick::Method),
            Hit::Cancel => {
                if let (Some(data), Some(id)) = (&self.data, self.running()) {
                    data.runs.cancel(id);
                }
            }
            Hit::Clear(focus) => {
                if let Some(input) = self.input(focus) {
                    input.set_text("");
                }
                self.focus = Some(focus);
                self.typed_in(focus);
            }
            Hit::Toggle(setting) => {
                if let Some(form) = self.form.as_mut() {
                    let on = match setting {
                        Setting::Redirects => &mut form.follow_redirects,
                        Setting::Cookies => &mut form.cookie_jar,
                        Setting::History => &mut form.history,
                        Setting::Encoding => &mut form.encoding,
                    };
                    *on = !*on;
                }
                self.changed();
            }
            Hit::BodyKind => self.open_picker(Pick::Body),
            Hit::AuthKind => self.open_picker(Pick::Auth),
            Hit::Send => self.send(),
            Hit::Tab(tab) => {
                self.tab = Some(tab);
                self.focus = None;
                match tab {
                    Tab::Params => self.ensure_new_row(Table::Params),
                    Tab::Headers => self.ensure_new_row(Table::Headers),
                    Tab::Body => {
                        self.ensure_new_row(Table::Form);
                        self.ensure_new_row(Table::Multipart);
                    }
                    _ => {}
                }
            }
            Hit::Check(table, row) => {
                if let Some(r) = self.form.as_mut().and_then(|f| f.rows(table).get_mut(row)) {
                    r.enabled = !r.enabled;
                }
                if table == Table::Params
                    && let Some(form) = self.form.as_mut()
                {
                    form.params_changed();
                }
                self.changed();
            }
            Hit::Remove(table, row) => {
                if let Some(form) = self.form.as_mut() {
                    let rows = form.rows(table);
                    if row < rows.len() {
                        rows.remove(row);
                    }
                    if table == Table::Params {
                        form.params_changed();
                    }
                }
                self.focus = None;
                self.ensure_new_row(table);
                self.changed();
            }
            Hit::FileToggle(row) => {
                if let Some(r) = self.form.as_mut().and_then(|f| f.parts.get_mut(row)) {
                    r.file = !r.file;
                }
                self.changed();
            }
            Hit::Add(table) => {
                self.ensure_new_row(table);
                let last = self.form.as_mut().map_or(0, |f| f.rows(table).len().saturating_sub(1));
                self.focus = Some(Focus::Cell(table, last, false));
            }
            Hit::DeleteAll(table) => {
                if let Some(form) = self.form.as_mut() {
                    form.rows(table).clear();
                    if table == Table::Params {
                        form.params_changed();
                    }
                }
                self.ensure_new_row(table);
                self.changed();
            }
            Hit::KeyPlace => {
                if let Some(form) = self.form.as_mut() {
                    form.key_in_query = !form.key_in_query;
                }
                self.changed();
            }
            Hit::Bulk => {
                self.bulk = match self.bulk.take() {
                    Some(_) => None,
                    None => {
                        let lines: Vec<String> = self
                            .form
                            .as_ref()
                            .map(|f| {
                                f.params
                                    .iter()
                                    .filter(|r| r.enabled && !r.name.text().is_empty())
                                    .map(|r| format!("{}={}", r.name.text(), r.value.text()))
                                    .collect()
                            })
                            .unwrap_or_default();
                        Some(MultilineField::new(&lines.join("\n")))
                    }
                };
                if self.bulk.is_some() {
                    self.focus = Some(Focus::Area(Area::Bulk));
                } else {
                    self.ensure_new_row(Table::Params);
                }
            }
            Hit::CopyUrl => {
                if let Some(form) = &self.form {
                    let url = form.url.text().to_owned();
                    self.action =
                        Some(ViewerAction::Request(ui_components::AppRequest::CopyText(url)));
                }
            }
            Hit::Part(part) => {
                self.part = Some(part);
                self.follow_response();
            }
            Hit::DocsMode(preview) => self.docs_preview = preview,
        }
    }

    /// Tab from a field: the next one of its row or table.
    fn next_field(&mut self, back: bool) {
        let Some(focus) = self.focus else { return };
        let next = match focus {
            Focus::Title => Focus::Url,
            Focus::Url if !back => Focus::Title,
            Focus::Cell(table, row, value) => {
                let len = self.form.as_mut().map_or(0, |f| f.rows(table).len());
                match (value, back) {
                    (false, false) => Focus::Cell(table, row, true),
                    (true, false) if row + 1 < len => Focus::Cell(table, row + 1, false),
                    (true, true) => Focus::Cell(table, row, false),
                    (false, true) if row > 0 => Focus::Cell(table, row - 1, true),
                    _ => focus,
                }
            }
            Focus::Auth(i) => {
                let len = self.form.as_ref().map_or(0, |f| f.auth.len());
                Focus::Auth(if back {
                    i.saturating_sub(1)
                } else {
                    (i + 1).min(len.saturating_sub(1))
                })
            }
            other => other,
        };
        self.focus = Some(next);
        if let Some(input) = self.input(next) {
            input.select_all();
        }
    }
}

impl Viewer for RequestEditor {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        let Some(data) = data.downcast_ref::<EditorData>() else { return };
        self.data = Some(data.clone());
        self.load();
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        self.loading.set_waker(&ctx.waker);
        self.saving.set_waker(&ctx.waker);
        if let (Some(data), Some(waker)) = (&self.data, &ctx.waker)
            && !self.waker_known
        {
            data.runs.follow(waker);
            self.waker_known = true;
        }
        self.take_work();
        self.paint_editor(ctx);
    }

    fn poll(&mut self) -> bool {
        let mut changed = self.take_work();
        if self.changed.is_some_and(|at| at.elapsed() >= SAVE_AFTER) {
            self.save();
        }
        let generation = self.data.as_ref().map_or(0, |d| d.runs.generation());
        if generation != self.generation {
            self.generation = generation;
            self.follow_response();
            changed = true;
        }
        if let Some((_, _, view)) = self.response.as_mut() {
            changed |= view.poll();
        }
        changed
    }

    /// The write due a moment after the last change (asked of every tab: a hidden one
    /// still saves).
    fn frame_deadline(&self) -> Option<Instant> {
        self.changed.map(|at| at + SAVE_AFTER)
    }

    /// A request running: its loader turns at a loader's pace — only while its tab shows
    /// (the app asks the shown views alone; a hidden one is woken when its run ends).
    fn is_loading(&self) -> bool {
        self.running().is_some()
    }

    /// The splitter moving.
    fn is_animating(&self) -> bool {
        self.splitter.as_ref().is_some_and(|s| s.is_animating())
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        if self.picker.is_open() {
            return self.picker.scroll(dy);
        }
        if let Some(Focus::Area(area)) = self.focus
            && let Some(field) = self.area(area)
        {
            return field.scroll_by(dy);
        }
        if let Some((_, _, view)) = self.response.as_mut() {
            return view.scroll(dx, dy);
        }
        false
    }

    fn pointer_down(
        &mut self,
        p: Point,
        clicks: u32,
        _mods: ui_components::PressMods,
    ) -> Option<ViewerAction> {
        if self.picker.is_open() {
            match self.picker.press(p) {
                Some(PickerEvent::Chosen(i)) => self.picked(i),
                Some(_) => self.picking = None,
                None => {}
            }
            return None;
        }
        if self.splitter.as_mut().is_some_and(|s| s.begin_drag(p)) {
            return None;
        }
        match self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h) {
            Some(hit) => self.press_hit(hit, p, clicks),
            None => self.focus = None,
        }
        self.action.take()
    }

    fn drag(&mut self, p: Point) -> bool {
        if let Some(x) = self.splitter.as_ref().and_then(|s| s.drag_to(p)) {
            let b = self.split_area;
            if b.size.width > 0.0 {
                self.share = ((x - b.min_x()) / b.size.width).clamp(0.2, 0.8);
            }
            return true;
        }
        let Some(focus) = self.dragging else { return false };
        match focus {
            Focus::Area(area) => self.area(area).is_some_and(|f| f.drag(p)),
            _ => {
                if let Some(input) = self.input(focus) {
                    input.extend_to(p.x);
                }
                true
            }
        }
    }

    fn release(&mut self) -> bool {
        self.dragging = None;
        self.splitter.as_mut().is_some_and(|s| s.end_drag())
    }

    fn hover(&mut self, p: Point) -> bool {
        if self.picker.is_open() {
            return self.picker.hover(p);
        }
        let line = self.splitter.as_mut().is_some_and(|s| s.hover(p));
        let hover = self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h);
        (std::mem::replace(&mut self.hover, hover) != hover) | line
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        if self.picker.is_open() {
            return self.picker.cursor_at(p);
        }
        if self.splitter.as_ref().is_some_and(|s| s.contains(p)) {
            return Some(ViewerCursor::ResizeHorizontal);
        }
        match self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)? {
            Hit::Field(_) => Some(ViewerCursor::Text),
            _ => Some(ViewerCursor::Pointer),
        }
    }

    fn takes_keyboard(&self) -> bool {
        true
    }

    fn wants_text(&self) -> bool {
        self.focus.is_some() || self.picker.is_open()
    }

    fn text_input(&mut self, typed: &str) -> bool {
        // The suggestions keep the field's text: it goes on being typed.
        if self.picker.is_open() && !matches!(self.picking, Some(Pick::Suggest(..))) {
            return self.picker.text_input(typed);
        }
        let Some(focus) = self.focus else { return false };
        match focus {
            Focus::Area(area) => {
                let Some(field) = self.area(area) else { return false };
                field.insert(typed);
                self.typed_in(focus);
                self.suggest(focus);
                true
            }
            _ => {
                let one_line: String = typed.chars().filter(|c| !c.is_control()).collect();
                let Some(input) = self.input(focus) else { return false };
                input.insert(&one_line);
                self.typed_in(focus);
                self.suggest(focus);
                true
            }
        }
    }

    fn paste(&mut self, text: &str) -> bool {
        self.text_input(text)
    }

    fn selected_text(&self) -> Option<String> {
        if self.picker.is_open() && !matches!(self.picking, Some(Pick::Suggest(..))) {
            return self.picker.selected_text();
        }
        let focus = self.focus?;
        let form = self.form.as_ref()?;
        let text = match focus {
            Focus::Title => form.title.selected_text(),
            Focus::Url => form.url.selected_text(),
            Focus::Cell(table, row, value) => {
                let rows = match table {
                    Table::Params => &form.params,
                    Table::Headers => &form.headers,
                    Table::Form => &form.form,
                    Table::Multipart => &form.parts,
                };
                let row = rows.get(row)?;
                if value { row.value.selected_text() } else { row.name.selected_text() }
            }
            Focus::Auth(i) => form.auth.get(i)?.selected_text(),
            Focus::File => form.file.selected_text(),
            Focus::Timeout => form.timeout.selected_text(),
            Focus::ConnectTimeout => form.connect_timeout.selected_text(),
            Focus::Area(Area::Bulk) => self.bulk.as_ref()?.selected_text(),
            Focus::Area(Area::Body) => form.body.selected_text(),
            Focus::Area(Area::Variables) => form.variables.selected_text(),
            Focus::Area(Area::Pre) => form.pre.selected_text(),
            Focus::Area(Area::Post) => form.post.selected_text(),
            Focus::Area(Area::Docs) => form.docs.selected_text(),
        };
        (!text.is_empty()).then(|| text.to_owned())
    }

    fn select_all(&mut self) -> bool {
        if self.picker.is_open() && !matches!(self.picking, Some(Pick::Suggest(..))) {
            return self.picker.command("select_all");
        }
        let Some(focus) = self.focus else { return false };
        match focus {
            Focus::Area(area) => self.area(area).map(MultilineField::select_all).is_some(),
            _ => self.input(focus).map(TextInput::select_all).is_some(),
        }
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        // The suggestions take ↑ ↓ ↵ ⇥ Esc; the rest goes on in the field.
        let suggesting = matches!(self.picking, Some(Pick::Suggest(..))) && self.picker.is_open();
        let list_key = matches!(
            key,
            ViewerKey::Up | ViewerKey::Down | ViewerKey::Enter | ViewerKey::Tab | ViewerKey::Escape
        );
        if suggesting && !list_key {
            self.picker.close();
            self.picking = None;
        } else if self.picker.is_open() {
            match self.picker.key(key, reach) {
                Some(PickerEvent::Chosen(i)) => self.picked(i),
                Some(_) => self.picking = None,
                None => {}
            }
            return true;
        }
        let Some(focus) = self.focus else { return false };
        if key == ViewerKey::Escape {
            self.focus = None;
            return true;
        }
        match focus {
            Focus::Area(area) => {
                let Some(field) = self.area(area) else { return false };
                let KeyReach { word, extend } = reach;
                match key {
                    ViewerKey::Backspace => field.backspace(word),
                    ViewerKey::Delete => field.delete(),
                    ViewerKey::Left => field.left(word, extend),
                    ViewerKey::Right => field.right(word, extend),
                    ViewerKey::Up => field.vertical(false, extend),
                    ViewerKey::Down => field.vertical(true, extend),
                    ViewerKey::Home => field.home(word, extend),
                    ViewerKey::End => field.end(word, extend),
                    ViewerKey::Enter => field.newline(),
                    ViewerKey::Tab => field.insert("  "),
                    _ => return false,
                }
                if matches!(
                    key,
                    ViewerKey::Backspace | ViewerKey::Delete | ViewerKey::Enter | ViewerKey::Tab
                ) {
                    self.typed_in(focus);
                }
                true
            }
            _ => match key {
                ViewerKey::Enter if focus == Focus::Url => {
                    self.send();
                    true
                }
                ViewerKey::Enter | ViewerKey::Tab => {
                    self.next_field(reach.extend);
                    true
                }
                _ => {
                    let edits = matches!(key, ViewerKey::Backspace | ViewerKey::Delete);
                    let Some(input) = self.input(focus) else { return false };
                    let used = reach.edit(input, key);
                    if used && edits {
                        self.typed_in(focus);
                    }
                    if used {
                        self.suggest(focus);
                    }
                    used
                }
            },
        }
    }

    fn command(&mut self, id: &str) -> bool {
        // A list's search open: ⌘A, cut, undo are its own (suggestions keep the field's).
        if self.picker.is_open()
            && !matches!(self.picking, Some(Pick::Suggest(..)))
            && self.picker.command(id)
        {
            return true;
        }
        match id {
            // ⌘A in a field: its text.
            "select_all" | "select-all" => self.select_all(),
            "undo" | "redo" => {
                let Some(focus) = self.focus else { return false };
                let redo = id == "redo";
                let done = match focus {
                    Focus::Area(area) => {
                        self.area(area).is_some_and(|f| if redo { f.redo() } else { f.undo() })
                    }
                    _ => self.input(focus).is_some_and(|f| if redo { f.redo() } else { f.undo() }),
                };
                if done {
                    self.typed_in(focus);
                }
                done
            }
            _ => false,
        }
    }

    fn web_page(&self) -> Option<ui_components::WebPage> {
        // The response's page (its Preview tab), over where the response is shown — begun past
        // the splitter's handle: the system's page is drawn over the app's frame, so the part
        // of the handle it covered was hidden (only what's left of it showed).
        let mut page = self.response.as_ref()?.2.web_page()?;
        if let Some(splitter) = &self.splitter {
            page.area = past_handle(page.area, splitter.handle(), splitter.orientation());
        }
        Some(page)
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        // An OAuth sign-in's page, asked for by its run.
        if let Some(asked) = self.data.as_ref().and_then(|d| d.runs.take_asked()) {
            return Some(asked);
        }
        self.action.take()
    }

    fn closed(&self) -> bool {
        false
    }

    fn may_close(&mut self) -> bool {
        // What's changed and not written yet: written now (it's quick, on a worker).
        if self.changed.is_some() {
            self.save();
        }
        true
    }
}

/// The editor's tab.
#[derive(Debug, Default)]
pub struct EditorProvider;

impl ViewerProvider for EditorProvider {
    fn id(&self) -> &'static str {
        "http-request"
    }

    fn kind(&self, path: &std::path::Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &std::path::Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(RequestEditor::default()))
    }
}

/// `area` (a page over a pane) without the part a splitter's `handle` overlaps on its near
/// side: its left edge (or top, the panes stacked) moved past the handle.
pub(crate) fn past_handle(
    area: Rect,
    handle: Rect,
    orientation: ui_components::splitter::Orientation,
) -> Rect {
    use ui_components::splitter::Orientation;
    if handle.size.width <= 0.0 || area.intersect(&handle).is_none() {
        return area;
    }
    match orientation {
        Orientation::Vertical => {
            let left = handle.max_x().min(area.max_x());
            Rect::new(left, area.min_y(), area.max_x() - left, area.size.height)
        }
        Orientation::Horizontal => {
            let top = handle.max_y().min(area.max_y());
            Rect::new(area.min_x(), top, area.size.width, area.max_y() - top)
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/editor/mod.rs"]
mod tests;
