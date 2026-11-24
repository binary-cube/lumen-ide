//! The structure's modal as a view: its form's fields and choices, the plan asked of the
//! source's worker as the form changes (only the latest kept), the confirmation of what's
//! lost, and the run.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use database::danger::{Danger, schema_dangers, script_dangers};
use database::transaction::AlterPreview;
use database::{DbError, FkAction, SchemaChange, Structure, TableRef};
use i18n::{t, tr};
use kernel::{Point, Rect};
use ui_components::control::{Control, Controls, Scrolled, Shown};
use ui_components::picker::Picker;
use ui_components::{
    EdgeScrollbar, KeyReach, Viewer, ViewerAction, ViewerCursor, ViewerKey, ViewerPaint,
};

use super::form::{ACTIONS, ColumnRow, Form, KeyRow, Kind, Text};
use super::{Done, SchemaRequest};
use crate::grid_source::message;
use crate::worker::{Client, Request, Response, Ticket};

mod choices;
mod controls;
mod plan;
mod viewer;

/// A box ticked or not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tick {
    NotNull(usize),
    Primary(usize),
    Auto(usize),
    Unique(usize),
    Stored(usize),
    WithoutRowid,
    IndexUnique,
    /// What's lost, confirmed.
    Confirm,
}

/// A list to choose from (a dropdown).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Choice {
    KeyColumns(usize),
    KeyTarget(usize),
    KeyReferences(usize),
    OnDelete(usize),
    OnUpdate(usize),
    /// Where a column moves to.
    Position,
}

impl Choice {
    /// Several may be chosen (in the order picked).
    pub fn multiple(self) -> bool {
        matches!(self, Self::KeyColumns(_) | Self::KeyReferences(_))
    }
}

/// What's open over the form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Popup {
    /// The types or collations completing a field.
    Suggest(Text),
    Choice(Choice),
}

/// The footer's buttons (the window draws them): their commands.
const SUBMIT: &str = "schema.footer.submit";
const CANCEL: &str = "schema.footer.cancel";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Hit {
    Field(Text),
    Tick(Tick),
    Choice(Choice),
    /// A column row of a new table: chosen (its details shown), or taken out.
    SelectColumn(usize),
    RemoveColumn(usize),
    AddKey,
    RemoveKey(usize),
    /// An index's column picked / let go, its order flipped.
    IndexColumn(String),
    IndexOrder(String),
}

/// The plan of the changes last asked, as the worker gave it.
type Planned = (Vec<SchemaChange>, Result<Option<AlterPreview>, DbError>);

/// The modal's view (see the module's doc).
#[derive(Default)]
pub struct SchemaView {
    pub(super) request: Option<SchemaRequest>,
    pub(super) form: Option<Form>,
    /// The worker's client, made with the app's waker (a reply wakes it).
    client: Option<Client>,
    pub(super) focus: Option<Text>,
    /// A new table's column whose details (check, collation, generation) are shown.
    pub(super) selected: usize,
    asked: Option<(Vec<SchemaChange>, Ticket)>,
    pub(super) planned: Option<Planned>,
    /// The tables keys point to, as read (and those asked).
    pub(super) structures: HashMap<TableRef, Structure>,
    structure_tickets: HashMap<Ticket, TableRef>,
    pub(super) confirmed: bool,
    pub(super) running: Option<Ticket>,
    /// Why the run failed (nothing changed).
    pub(super) failed: Option<String>,
    pub(super) popup: Option<Popup>,
    pub(super) picker: ui_components::picker::Picker,
    /// The suggestions the picker shows (what a row taken puts in the field).
    pub(super) suggested: Vec<&'static str>,
    pub(super) form_scroll: f32,
    pub(super) form_max: f32,
    pub(super) preview_scroll: f32,
    /// How far the preview scrolls (measured at the last paint).
    pub(super) preview_max: f32,
    pub(super) form_bar: EdgeScrollbar,
    pub(super) preview_bar: EdgeScrollbar,
    /// Where each choice's control and each field was drawn (popups open under them).
    pub(super) anchors: HashMap<Popup, Rect>,
    pub(super) form_area: Rect,
    pub(super) preview_area: Rect,
    /// Where the modal's content was drawn last (the form scrolls under the wheel anywhere
    /// but over the preview).
    pub(super) bounds: Rect,
    pub(super) hits: Vec<(Rect, Hit)>,
    /// What the abbreviated or icon-only parts under the mouse say (NN, PK, ×…).
    pub(super) tips: Vec<(Rect, String)>,
    pub(super) hover: Option<Hit>,
    last_hover: Option<Point>,
    action: Option<ViewerAction>,
    /// Its parts' state of use (see `with_controls`).
    controls: Controls,
    /// What the keys' parts heard, taken after each key or text.
    heard: Heard,
}

/// What the parts taking the keys heard (the view acts on it once they're let go).
#[derive(Debug, Default)]
struct Heard {
    /// A suggestion taken (↵ or ⇥ on the lit one).
    picked: Option<usize>,
    /// The suggestions closed (esc).
    closed: bool,
    /// The focused field's text changed.
    edited: bool,
}

impl std::fmt::Debug for SchemaView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SchemaView").field("request", &self.request).finish_non_exhaustive()
    }
}

impl SchemaView {
    /// The modal for `request`, its client made now (woken by `wake`, when given; the app's
    /// view makes it at its first paint instead).
    pub fn open(request: SchemaRequest, wake: Option<Arc<dyn Fn() + Send + Sync>>) -> Self {
        let mut view = Self::default();
        view.take(request);
        view.connect(wake.unwrap_or_else(|| Arc::new(|| {})));
        view
    }

    fn take(&mut self, request: SchemaRequest) {
        let form = Form::new(request.kind.clone(), request.ctx.clone());
        self.focus = form.texts().first().copied();
        self.form = Some(form);
        self.request = Some(request);
        // The focused field has the keys from the start; a press on what isn't one keeps it.
        self.controls = Controls::sticky();
        self.with_controls(|_, _| ());
    }

    fn connect(&mut self, wake: Arc<dyn Fn() + Send + Sync>) {
        if self.client.is_none()
            && let Some(request) = &self.request
        {
            self.client = Some(request.worker.client(wake));
            self.replan();
        }
    }

    /// Why the last run failed (nothing changed), shown in the modal.
    pub fn failed_text(&self) -> Option<String> {
        self.failed.clone()
    }

    pub fn form(&self) -> Option<&Form> {
        self.form.as_ref()
    }

    pub fn form_mut(&mut self) -> Option<&mut Form> {
        self.form.as_mut()
    }

    /// The form's changes, or why not yet.
    pub fn changes(&self) -> Result<Vec<SchemaChange>, String> {
        self.form.as_ref().map_or_else(|| Err(String::new()), Form::changes)
    }

    /// The plan of the form's changes as they are now (none while it's asked, or when the
    /// form isn't complete).
    pub fn plan(&self) -> Option<&Result<Option<AlterPreview>, DbError>> {
        let changes = self.changes().ok()?;
        self.planned.as_ref().filter(|(c, _)| *c == changes).map(|(_, p)| p)
    }

    fn focus_step(&mut self, forward: bool) {
        let Some(form) = &self.form else { return };
        let texts = form.texts();
        if texts.is_empty() {
            return;
        }
        let at = self.focus.and_then(|f| texts.iter().position(|t| *t == f));
        let next = match (at, forward) {
            (None, _) => 0,
            (Some(i), true) => (i + 1) % texts.len(),
            (Some(i), false) => (i + texts.len() - 1) % texts.len(),
        };
        self.focus = Some(texts[next]);
        if let Text::Name(i) | Text::Type(i) | Text::Default(i) = texts[next] {
            self.selected = i;
        }
        self.popup = None;
    }

    pub(super) fn hit(&self, p: Point) -> Option<Hit> {
        self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| h.clone())
    }

    fn focused_input(&mut self) -> Option<&mut ui_kit::TextInput> {
        let field = self.focus?;
        self.form.as_mut()?.input_mut(field)
    }

    /// A new table's column row taken out (one row always stays).
    fn remove_column(&mut self, i: usize) {
        let Some(form) = &mut self.form else { return };
        if form.columns.len() > 1 && i < form.columns.len() {
            form.columns.remove(i);
            if form.columns.last().is_none_or(|c| !c.is_blank()) {
                form.columns.push(ColumnRow::default());
            }
        }
        self.focus = None;
        self.selected = self.selected.min(form.columns.len().saturating_sub(1));
        self.replan();
    }
}

impl Drop for SchemaView {
    fn drop(&mut self) {
        // What's still asked isn't waited for (a run goes on: the client's going drops
        // only what waits).
        if let (Some(client), Some((_, ticket))) = (&self.client, &self.asked) {
            client.cancel(*ticket);
        }
    }
}

impl SchemaView {
    /// The primary button's label.
    pub fn submit_label(&self) -> String {
        let kind = self.form.as_ref().map(|f| f.kind.clone());
        t(match kind {
            Some(Kind::CreateTable | Kind::CreateIndex) => "schema.create",
            Some(Kind::RenameTable) => "schema.rename",
            Some(Kind::AddColumn | Kind::AddForeignKey) => "schema.add",
            Some(Kind::MoveColumn(_)) => "schema.move",
            Some(k) if k.drops() => "schema.drop",
            _ => "schema.save",
        })
    }
}

/// A shared slot for the modal's outcome, as the view keeps it.
pub type Answer = Arc<Mutex<Option<Done>>>;

/// The outcome left in `answer`, taken.
pub fn take_answer(answer: &Answer) -> Option<Done> {
    answer.lock().unwrap_or_else(PoisonError::into_inner).take()
}

#[cfg(feature = "snapshot")]
impl SchemaView {
    /// A snapshot's sample (`--snapshot`): the form filled as a user would, no field focused
    /// (a caret blinks), its plan asked.
    fn snapshot(&mut self, sample: &str) -> bool {
        let Some(form) = &mut self.form else { return false };
        match sample {
            // A new table: a numbered key, a column pointing to `customers`, a default, a
            // check, and its foreign key.
            "create-table" => {
                form.table_name.set_text("invoices");
                form.columns.clear();
                let mut id = ColumnRow::new("id", "INTEGER");
                (id.primary, id.not_null, id.auto) = (true, true, true);
                let mut customer = ColumnRow::new("customer_id", "INTEGER");
                customer.not_null = true;
                let mut total = ColumnRow::new("total", "NUMERIC");
                total.default.set_text("0");
                total.check.set_text("total >= 0");
                total.not_null = true;
                let mut issued = ColumnRow::new("issued", "TEXT");
                issued.default.set_text("CURRENT_TIMESTAMP");
                let mut number = ColumnRow::new("number", "VARCHAR(20)");
                number.unique = true;
                number.collate.set_text("NOCASE");
                form.columns = vec![id, customer, total, issued, number, ColumnRow::default()];
                form.keys = vec![KeyRow {
                    columns: vec!["customer_id".into()],
                    target: Some(TableRef::new("customers")),
                    references: vec!["id".into()],
                    on_delete: FkAction::Cascade,
                    ..KeyRow::default()
                }];
                self.selected = 2;
            }
            // A column renamed and its type changed: a rebuild, the view naming it rewritten.
            "alter-column" => {
                let Some(row) = form.columns.first_mut() else { return false };
                row.name.set_text("amount");
                row.ty.set_text("NUMERIC(10, 2)");
            }
            // A column a view names, dropped: the view is lost, ticked to confirm.
            "drop-column" => {}
            // The last column's type field focused, its suggestions open (the shared picker).
            "types" => {
                let last = form.columns.len().saturating_sub(1);
                self.focus = Some(Text::Type(last));
                self.selected = last;
                self.suggest(Text::Type(last), false);
                return true;
            }
            // A partial index on two columns, one descending.
            "create-index" => {
                form.toggle_index_column("customer_id");
                form.toggle_index_column("total");
                form.flip_index_order("total");
                form.predicate.set_text("status <> 'cancelled'");
            }
            _ => return false,
        }
        self.focus = None;
        self.replan();
        true
    }
}
