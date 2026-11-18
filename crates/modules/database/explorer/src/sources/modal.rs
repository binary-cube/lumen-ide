//! The data sources' modal as a view: the editor ([`super::form`]) made once the catalog's
//! lists are read, the keys and the mouse on its fields, lists and buttons, the test of a
//! connection in the background, Apply / OK / Cancel, and the question asked before
//! unsaved changes are let go. The list's folders: made from the toolbar or the rows'
//! menu, a source or a folder dragged onto a folder (or a list's title) to move it there.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use database::{DataSourceId, FieldKind, FolderId, Scope, SecretPolicy, SourceColor};
use i18n::t;
use kernel::{Point, Rect, Secret};
use ui_components::{
    AppRequest, KeyReach, ViewMenuEntry, Viewer, ViewerAction, ViewerCursor, ViewerKey, ViewerPaint,
};

use super::form::{Editor, Field, Item, POLICIES, Place, Problem, Tab};
use super::{Probe, SourcesRequest, Tester};
use crate::catalog::Listed;
use workbench_model::Notification;

mod footer;
mod menus;
mod viewer;

/// A list to choose from (a dropdown).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Choice {
    /// A driver's choice field (by id).
    Spec(String),
    /// Where the password is kept.
    Policy,
    /// The driver of a new source (the list's `+`).
    AddDriver,
}

/// The list's buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tool {
    Add,
    Folder,
    Remove,
    Duplicate,
    Move,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Hit {
    Row(usize),
    /// A folder's row (its chevron: `true`).
    Folder(FolderId, bool),
    /// A list's title (a drop there: its top).
    Section(Scope),
    /// A color for the selected source (`None`: its folder's).
    SourceColor(Option<SourceColor>),
    /// A color for the selected folder (`None`: its parent's).
    FolderColor(Option<SourceColor>),
    Field(Field),
    Tab(Tab),
    Tool(Tool),
    Choice(Choice),
    Flag(String),
    ReadOnly,
    /// The selected source's production mark.
    Production,
    /// The selected folder's production mark.
    FolderProduction,
    Schema(String),
    AllSchemas,
    Database(String),
    AllDatabases,
    Browse(String),
    Reveal,
    AddOption,
    RemoveOption(usize),
}

/// A footer's button (the window draws them; a click comes as its command).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Footer {
    Ok,
    Apply,
    Cancel,
    Test,
    Discard,
    KeepEditing,
}

/// The footer's buttons (the window draws them), by the command a click on one sends.
const FOOTER: &[(&str, Footer)] = &[
    ("datasources.footer.ok", Footer::Ok),
    ("datasources.footer.apply", Footer::Apply),
    ("datasources.footer.cancel", Footer::Cancel),
    ("datasources.footer.test", Footer::Test),
    ("datasources.footer.discard", Footer::Discard),
    ("datasources.footer.keep", Footer::KeepEditing),
];

/// The command of footer button `button`.
fn footer_id(button: Footer) -> &'static str {
    FOOTER.iter().find(|(_, b)| *b == button).map_or("", |(id, _)| id)
}

/// The modal's view (see the module's doc).
#[derive(Default)]
pub struct SourcesView {
    pub(super) request: Option<SourcesRequest>,
    pub(super) editor: Option<Editor>,
    pub(super) focus: Option<Field>,
    /// The list open (which), and the list itself (the shared picker).
    pub(super) popup: Option<Choice>,
    pub(super) picker: ui_components::picker::Picker,
    pub(super) tester: Tester,
    /// The source the test's outcome is of.
    pub(super) tested: Option<DataSourceId>,
    /// Closing was asked with changes not applied: the footer asks first.
    pub(super) confirm_close: bool,
    /// Why Apply didn't go through (a field to fix, the catalog's refusal).
    pub(super) error: Option<String>,
    /// The passwords' failure was told (once, as an error notification).
    failure_told: bool,
    /// The passwords' outcome, from the background.
    saved: Arc<Mutex<Option<Result<(), String>>>>,
    /// Close once the passwords are kept (OK).
    close_when_saved: bool,
    /// The last Apply sent passwords to be kept (their outcome comes in `saved`).
    secrets_pending: bool,
    /// A file is being picked for this field.
    browsing: Option<String>,
    /// A row pressed (what it holds, where): a drag starts from it.
    pressed: Option<(Item, Point)>,
    /// Being dragged, and where it would go now.
    pub(super) dragging: Option<Item>,
    pub(super) drop: Option<Place>,
    /// Where the mouse is while dragging (what's dragged follows it).
    pub(super) drag_at: Point,
    pub(super) hits: Vec<(Rect, Hit)>,
    pub(super) hover: Option<Hit>,
    pub(super) anchors: HashMap<Choice, Rect>,
    pub(super) tips: Vec<(Rect, String)>,
    pub(super) form_scroll: f32,
    pub(super) form_bar: ui_components::EdgeScrollbar,
    pub(super) form_max: f32,
    pub(super) list_scroll: f32,
    pub(super) list_max: f32,
    pub(super) form_area: Rect,
    pub(super) list_area: Rect,
    last_hover: Option<Point>,
    action: Option<ViewerAction>,
    waker: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl std::fmt::Debug for SourcesView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourcesView").field("request", &self.request).finish_non_exhaustive()
    }
}

impl SourcesView {
    /// The modal for `request` (its lists read when they're there: now, or at a poll).
    pub fn open(request: SourcesRequest) -> Self {
        let mut view = Self::default();
        view.take(request);
        view
    }

    fn take(&mut self, request: SourcesRequest) {
        request.sessions.catalog().ensure_loaded();
        self.request = Some(request);
        self.make_editor();
    }

    /// The editor, once both lists are read.
    fn make_editor(&mut self) -> bool {
        if self.editor.is_some() {
            return false;
        }
        let Some(request) = self.request.clone() else { return false };
        let catalog = request.sessions.catalog();
        let (project, global) = (catalog.list(Scope::Project), catalog.list(Scope::Global));
        if project == Listed::Loading || global == Listed::Loading {
            return false;
        }
        let folders = project.folders().iter().chain(global.folders()).cloned().collect();
        let mut editor =
            Editor::new(super::drivers(&request.sessions), project.sources(), global.sources())
                .with_folders(folders);
        for (scope, list) in [(Scope::Project, &project), (Scope::Global, &global)] {
            if let Listed::Unreadable(why) = list {
                self.error = Some(i18n::tr!(
                    "datasources.unreadable_list",
                    list = t(scope_key(scope)),
                    why = why.as_str()
                ));
            }
        }
        if let Some(driver) = &request.add {
            editor.add(driver);
            self.focus = Some(Field::Name);
        } else if let Some(id) = request.select {
            let at = editor.entries.iter().position(|e| e.source.id == id);
            editor.select(at);
        } else if let Some(id) = request.select_folder {
            editor.select_folder(id);
        }
        if let Some(focus) = &request.focus
            && editor.input(focus).is_some()
        {
            self.focus = Some(focus.clone());
        }
        self.editor = Some(editor);
        true
    }

    pub fn editor(&self) -> Option<&Editor> {
        self.editor.as_ref()
    }

    pub fn editor_mut(&mut self) -> Option<&mut Editor> {
        self.editor.as_mut()
    }

    pub fn focus(&self) -> Option<&Field> {
        self.focus.as_ref()
    }

    pub fn probe(&self) -> Probe {
        self.tester.probe()
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Closing is waiting for the user to say whether to discard the changes.
    pub fn asks_to_discard(&self) -> bool {
        self.confirm_close
    }

    fn wake(&self) -> Arc<dyn Fn() + Send + Sync> {
        self.waker
            .clone()
            .or_else(|| self.request.as_ref().map(|r| Arc::clone(&r.wake)))
            .unwrap_or_else(|| Arc::new(|| {}))
    }

    // ─── Selecting, editing ───

    pub fn select(&mut self, at: usize) {
        let Some(editor) = &mut self.editor else { return };
        editor.select(Some(at));
        self.focus = None;
        self.close_popup();
        self.form_scroll = 0.0;
        self.error = None;
    }

    pub fn set_tab(&mut self, tab: Tab) {
        if let Some(editor) = &mut self.editor {
            editor.tab = tab;
        }
        self.focus = None;
        self.close_popup();
        self.form_scroll = 0.0;
    }

    /// Text typed into the field having the keys.
    pub fn type_text(&mut self, typed: &str) {
        let Some(field) = self.focus.clone() else { return };
        let typed: String = typed.chars().filter(|c| !c.is_control()).collect();
        let Some(editor) = &mut self.editor else { return };
        if let Some(input) = editor.input_mut(&field) {
            input.insert(&typed);
        }
        self.edited(&field);
    }

    fn edited(&mut self, field: &Field) {
        if let Some(editor) = &mut self.editor {
            editor.edited(field);
        }
        self.error = None;
        self.confirm_close = false;
    }

    pub fn tool(&mut self, tool: Tool) {
        self.close_popup();
        if tool == Tool::Add {
            self.open_popup(Choice::AddDriver);
            return;
        }
        let Some(editor) = &mut self.editor else { return };
        match tool {
            Tool::Add => {}
            Tool::Folder => {
                let place = editor.new_folder_place();
                editor.add_folder(place, &t("datasources.new_folder"));
                self.focus = Some(Field::FolderName);
                self.confirm_close = false;
                self.error = None;
                return;
            }
            Tool::Remove => match editor.selected_folder {
                Some(id) => editor.delete_folder(id),
                None => editor.remove(),
            },
            Tool::Duplicate => editor.duplicate(),
            Tool::Move => editor.move_scope(),
        }
        self.focus = None;
        self.confirm_close = false;
    }

    // ─── Folders ───

    /// The options of a list: their labels and whether each is chosen.
    pub fn options(&self, choice: &Choice) -> Vec<(String, bool)> {
        let Some(editor) = &self.editor else { return Vec::new() };
        match choice {
            Choice::Spec(id) => {
                let Some(field) = editor.field(id) else { return Vec::new() };
                let now = editor.choice_of(&field);
                match &field.kind {
                    FieldKind::Choice(values) => {
                        values.iter().map(|(v, label)| (t(label), *v == now)).collect()
                    }
                    _ => Vec::new(),
                }
            }
            Choice::Policy => {
                let entry = editor.current();
                let now = entry.map(|e| crate::credentials::effective(&e.source));
                let global = entry.is_some_and(|e| e.source.scope == Scope::Global);
                policies(global)
                    .iter()
                    .map(|p| (t(super::form::policy_key(*p)), Some(*p) == now))
                    .collect()
            }
            Choice::AddDriver => editor
                .drivers
                .iter()
                .map(|d| {
                    let name = if d.external {
                        format!("{} ({})", d.name, t("datasources.external"))
                    } else {
                        d.name.clone()
                    };
                    (name, false)
                })
                .collect(),
        }
    }

    /// List `choice` opened (its options in the shared picker, the chosen one in view; the
    /// drivers with their icons).
    pub fn open_popup(&mut self, choice: Choice) {
        let options = self.options(&choice);
        let chosen = options.iter().position(|(_, on)| *on);
        let icons: Vec<&'static str> = match (&choice, &self.editor) {
            (Choice::AddDriver, Some(editor)) => editor.drivers.iter().map(|d| d.icon).collect(),
            _ => Vec::new(),
        };
        let items = options
            .into_iter()
            .enumerate()
            .map(|(i, (label, _))| ui_components::picker::PickerItem {
                label,
                icon: icons.get(i).copied(),
                ..Default::default()
            })
            .collect();
        self.picker.open(items, chosen);
        self.popup = Some(choice);
    }

    /// The open list closed.
    pub fn close_popup(&mut self) {
        self.popup = None;
        self.picker.close();
    }

    /// What the open list said.
    fn picked(&mut self, event: Option<ui_components::picker::PickerEvent>) {
        use ui_components::picker::PickerEvent;
        match (event, self.popup.clone()) {
            (Some(PickerEvent::Chosen(at)), Some(choice)) => self.choose(&choice, at),
            (Some(PickerEvent::Closed), _) => self.close_popup(),
            _ => {}
        }
    }

    /// Option `at` of `choice` chosen.
    pub fn choose(&mut self, choice: &Choice, at: usize) {
        self.close_popup();
        let Some(editor) = &mut self.editor else { return };
        match choice {
            Choice::Spec(id) => {
                let values = match editor.field(id).map(|f| f.kind) {
                    Some(FieldKind::Choice(values)) => values,
                    _ => return,
                };
                if let Some((value, _)) = values.get(at) {
                    editor.choose(id, value);
                }
            }
            Choice::Policy => {
                let global = editor.scope() == Some(Scope::Global);
                if let Some(policy) = policies(global).get(at) {
                    editor.set_policy(*policy);
                }
            }
            Choice::AddDriver => {
                let Some(id) = editor.drivers.get(at).map(|d| d.id.clone()) else { return };
                editor.add(&id);
                self.focus = Some(Field::Name);
                self.form_scroll = 0.0;
            }
        }
        self.confirm_close = false;
    }

    // ─── Test Connection ───

    // ─── The footer's buttons ───

    // ─── Apply, OK, Cancel ───

    // ─── Hits ───

    pub(super) fn hit(&self, p: Point) -> Option<Hit> {
        self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| h.clone())
    }

    fn focused_input(&mut self) -> Option<&mut ui_kit::TextInput> {
        let field = self.focus.clone()?;
        self.editor.as_mut()?.input_mut(&field)
    }

    fn focus_step(&mut self, forward: bool) {
        let Some(editor) = &self.editor else { return };
        let texts = editor.texts();
        if texts.is_empty() {
            return;
        }
        let at = self.focus.as_ref().and_then(|f| texts.iter().position(|t| t == f));
        let next = match (at, forward) {
            (None, _) => 0,
            (Some(i), true) => (i + 1) % texts.len(),
            (Some(i), false) => (i + texts.len() - 1) % texts.len(),
        };
        self.focus = Some(texts[next].clone());
        self.close_popup();
    }

    /// The selection moved up or down the visible list.
    fn step_row(&mut self, down: bool) {
        let Some(editor) = &self.editor else { return };
        let visible = editor.visible();
        if visible.is_empty() {
            return;
        }
        let at = editor.selected.and_then(|s| visible.iter().position(|v| *v == s));
        let next = match (at, down) {
            (None, _) => 0,
            (Some(i), true) => (i + 1).min(visible.len() - 1),
            (Some(i), false) => i.saturating_sub(1),
        };
        let row = visible[next];
        let focus = self.focus.clone();
        self.select(row);
        self.focus = focus;
    }

    fn hover_point(&self) -> Point {
        self.last_hover.unwrap_or(Point::new(f32::MIN, f32::MIN))
    }
}

/// The policies offered: a global source has no project to keep a password in.
pub fn policies(global: bool) -> Vec<SecretPolicy> {
    POLICIES.into_iter().filter(|p| !(global && *p == SecretPolicy::ProjectVault)).collect()
}

fn scope_key(scope: Scope) -> &'static str {
    match scope {
        Scope::Project => "datasources.section_project",
        Scope::Global => "datasources.section_global",
    }
}

/// A field's problem, in words.
pub fn field_error_text(error: &database::FieldError) -> String {
    use database::FieldError as E;
    match error {
        E::Required => t("datasources.error_required"),
        E::NotANumber => t("datasources.error_number"),
        E::OutOfRange { min, max } => i18n::tr!("datasources.error_range", min = *min, max = *max),
        E::NotAChoice => t("datasources.error_choice"),
        E::NotAFlag => t("datasources.error_flag"),
    }
}

/// A URL's problem, in words.
pub fn url_error_text(error: &database::UrlError) -> String {
    use database::UrlError as E;
    match error {
        E::NoUrl => t("datasources.url_none"),
        E::Scheme { expected } => i18n::tr!("datasources.url_scheme", scheme = expected.as_str()),
        E::Malformed(_) => t("datasources.url_malformed"),
        E::Port => t("datasources.url_port"),
    }
}

/// What keeps Apply back, in words (with the source it's in).
fn problem_text(editor: &Editor, problem: &Problem) -> String {
    let name = |i: usize| editor.entries.get(i).map(|e| e.source.name.clone()).unwrap_or_default();
    match problem {
        Problem::NoName(_) => t("datasources.error_no_name"),
        Problem::Field(i, id, error) => {
            let label = editor.field(id).map(|f| t(&f.label)).unwrap_or_else(|| id.clone());
            i18n::tr!(
                "datasources.error_field",
                source = name(*i),
                field = label,
                why = field_error_text(error)
            )
        }
        Problem::SecretOption(_, option) => {
            i18n::tr!("datasources.error_secret_option", option = option.as_str())
        }
    }
}

#[cfg(feature = "snapshot")]
impl SourcesView {
    /// A snapshot's sample (`--snapshot`): a PostgreSQL source filled in (its password typed,
    /// masked), on a tab, or with a test's outcome shown — nothing connects; or a folder.
    fn snapshot(&mut self, sample: &str) -> bool {
        let Some(editor) = &mut self.editor else { return false };
        let at = editor.entries.iter().position(|e| e.source.driver == "postgres");
        editor.select(at);
        if let Some(entry) = editor.entries.get_mut(at.unwrap_or(0)) {
            entry.password = ui_kit::TextInput::new("not-a-real-password");
            entry.password.set_masked(true);
            entry.password_typed = true;
        }
        self.focus = None;
        let id = editor.current().map(|e| e.source.id);
        match sample {
            "general" => editor.tab = Tab::General,
            // The `+`'s drivers open (the shared picker, from a small button).
            "add" => {
                editor.tab = Tab::General;
                self.open_popup(Choice::AddDriver);
                return true;
            }
            // The folders, the one named prod selected.
            "folders" => {
                let id = editor.folders.iter().find(|f| f.name == "prod").map(|f| f.id);
                if let Some(id) = id {
                    editor.select_folder(id);
                }
            }
            // Down to its SSH part (the TLS fields above it).
            "ssl" => {
                editor.tab = Tab::Ssl;
                self.form_scroll = 10_000.0;
                self.form_max = 10_000.0;
            }
            "schemas" => {
                editor.tab = Tab::Schemas;
                if let Some(id) = id {
                    let found = ["billing", "public", "reporting", "staging"];
                    editor.schemas_found = vec![(id, found.map(str::to_owned).to_vec())];
                    let all = editor.schemas();
                    editor.toggle_schema("staging", &all);
                    // A server with several databases: one of them hidden.
                    let found = ["analytics", "postgres", "shop", "staging"];
                    editor.databases_found = vec![(id, found.map(str::to_owned).to_vec())];
                    let all = editor.databases();
                    editor.toggle_database("staging", &all);
                }
            }
            "test-ok" | "test-failed" => {
                editor.tab = Tab::General;
                self.tested = id;
                self.tester.set(if sample == "test-ok" {
                    Probe::Connected {
                        server: "PostgreSQL 16.4".into(),
                        millis: 12,
                        schemas: Vec::new(),
                        databases: Vec::new(),
                    }
                } else {
                    Probe::Failed(super::probe_error(&database::DbError::Auth(
                        "password authentication failed for user \"app\"".into(),
                    )))
                });
            }
            _ => return false,
        }
        true
    }
}
