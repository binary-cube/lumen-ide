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

/// What an open list is over the form, for the keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Listing {
    /// A choice's list: every key, its search the text.
    Choice,
    /// A field's suggestions: only what moves in them, takes or closes them.
    Suggest,
}

/// The open list as a part, over everything while it's shown (until the next paint
/// closes it, a list no longer asked for takes nothing): its press, its hover, its cursor
/// and its wheel are the picker's; of the keys, a choice's list takes every one and, in
/// its search, the text; a field's suggestions ↑ ↓ ⇞ ⇟, ↵ ⇥ on the lit one (taken) and esc
/// (closed) — the other keys and the text go on to the field, which keeps the typing.
struct ListPart<'a> {
    picker: &'a mut Picker,
    shown: bool,
    /// What it is for the keys (`None`: no list open).
    listing: Option<Listing>,
    picked: &'a mut Option<usize>,
    closed: &'a mut bool,
}

impl ListPart<'_> {
    /// The picker as the part shown only while asked for.
    fn shown(&mut self) -> Shown<'_> {
        Shown { part: &mut *self.picker, shown: self.shown }
    }
}

impl Control for ListPart<'_> {
    fn area(&self) -> Option<Rect> {
        Control::area(&*self.picker).filter(|_| self.shown)
    }

    fn hit(&self, p: Point) -> bool {
        self.shown && Control::hit(&*self.picker, p)
    }

    fn press(&mut self, p: Point, clicks: u32) -> ui_components::control::Press {
        self.shown().press(p, clicks)
    }

    fn drag(&mut self, p: Point) -> bool {
        self.shown().drag(p)
    }

    fn release(&mut self) -> bool {
        self.shown().release()
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        self.shown().hover(p)
    }

    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        Control::cursor(&*self.picker, p).filter(|_| self.shown)
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        self.shown().scroll(dx, dy)
    }

    fn focusable(&self) -> bool {
        self.shown && Control::focusable(&*self.picker)
    }

    fn keeps_tab(&self) -> bool {
        self.shown && Control::keeps_tab(&*self.picker)
    }

    fn frame_deadline(&self) -> Option<std::time::Instant> {
        Control::frame_deadline(&*self.picker).filter(|_| self.shown)
    }

    fn captures_keys(&self) -> bool {
        self.listing.is_some()
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        match self.listing {
            Some(Listing::Choice) => Control::key(self.picker, key, reach),
            Some(Listing::Suggest) => match key {
                ViewerKey::Down | ViewerKey::Up | ViewerKey::PageDown | ViewerKey::PageUp => {
                    Picker::key(self.picker, key, reach);
                    true
                }
                ViewerKey::Enter | ViewerKey::Tab => {
                    let lit = self.picker.highlighted();
                    *self.picked = lit;
                    lit.is_some()
                }
                ViewerKey::Escape => {
                    *self.closed = true;
                    true
                }
                _ => false,
            },
            None => false,
        }
    }

    fn text(&mut self, typed: &str) -> bool {
        self.listing == Some(Listing::Choice) && self.picker.text_input(typed)
    }
}

/// The focused field as the part having the keys (focused from code; a press on it is the
/// view's, by its hit): the editing keys and the text typed, but none while a choice's
/// list is open over it. `edited`: its text changed.
struct FieldKeys<'a> {
    /// `None`: no field focused.
    field: Option<Text>,
    input: Option<&'a mut ui_kit::TextInput>,
    /// It takes the keys (no choice's list over it).
    open: bool,
    edited: &'a mut bool,
}

impl Control for FieldKeys<'_> {
    fn focusable(&self) -> bool {
        true
    }

    /// Focused from code only: the view moves among its fields with Tab and ↑ ↓.
    fn in_tab_order(&self) -> bool {
        false
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        let (true, Some(input)) = (self.open, self.input.as_deref_mut()) else { return false };
        match key {
            ViewerKey::Left => input.move_left(reach.word, reach.extend),
            ViewerKey::Right => input.move_right(reach.word, reach.extend),
            ViewerKey::Home => input.home(reach.extend),
            ViewerKey::End => input.end(reach.extend),
            ViewerKey::Backspace => input.backspace(reach.word),
            ViewerKey::Delete => input.delete(),
            _ => return false,
        }
        *self.edited |= matches!(key, ViewerKey::Backspace | ViewerKey::Delete);
        true
    }

    /// Typed into the field (control characters dropped).
    fn text(&mut self, typed: &str) -> bool {
        if !self.open || self.field.is_none() {
            return false;
        }
        let typed: String = typed.chars().filter(|c| !c.is_control()).collect();
        if let Some(input) = self.input.as_deref_mut() {
            input.insert(&typed);
        }
        *self.edited = true;
        true
    }
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

    /// Asks the worker what the form's changes would run, when they changed (the plan of
    /// changes no longer shown isn't waited for); and the structure of each table a key
    /// points to, once.
    pub fn replan(&mut self) {
        let Some(client) = &self.client else { return };
        if let Some(form) = &self.form {
            for row in &form.keys {
                if let Some(target) = &row.target
                    && !self.structures.contains_key(target)
                    && !self.structure_tickets.values().any(|t| t == target)
                    && form.ctx.tables.iter().any(|i| i.table == *target)
                {
                    let ticket = client.send(Request::Structure(target.clone()));
                    self.structure_tickets.insert(ticket, target.clone());
                }
            }
        }
        let Ok(changes) = self.changes() else {
            if let Some((_, ticket)) = self.asked.take() {
                client.cancel(ticket);
            }
            return;
        };
        let same = |c: &Vec<SchemaChange>| *c == changes;
        if self.asked.as_ref().is_some_and(|(c, _)| same(c))
            || self.planned.as_ref().is_some_and(|(c, _)| same(c))
        {
            return;
        }
        if let Some((_, ticket)) = self.asked.take() {
            client.cancel(ticket);
        }
        let ticket = client.send(Request::PlanAlter(changes.clone()));
        self.asked = Some((changes, ticket));
        self.confirmed = false;
    }

    /// What's lost, to confirm: the data dropped and what a rebuild doesn't make again.
    pub fn dangers(&self) -> Vec<Danger> {
        let (Ok(changes), Some(form)) = (self.changes(), &self.form) else { return Vec::new() };
        let ctx = &form.ctx;
        let mut dangers = schema_dangers(&changes, &|t| ctx.rows_of(t), &|t| {
            ctx.structure.as_ref().filter(|s| s.table == *t).cloned()
        });
        if let Some(Ok(Some(preview))) = self.plan() {
            dangers.extend(script_dangers(&preview.script));
        }
        dangers
    }

    /// Everything is there to submit: the form complete, its plan come back fine, what's
    /// lost confirmed, nothing running.
    pub fn can_submit(&self) -> bool {
        self.running.is_none()
            && matches!(self.plan(), Some(Ok(_)))
            && (self.confirmed || self.dangers().is_empty())
    }

    /// Runs exactly what was shown (when it can).
    pub fn submit(&mut self) -> bool {
        if !self.can_submit() {
            return false;
        }
        let (Ok(changes), Some(Ok(plan)), Some(client)) =
            (self.changes(), self.plan().cloned(), &self.client)
        else {
            return false;
        };
        let shown = plan.map(|p| p.script);
        self.running = Some(client.send(Request::AlterAsShown { changes, shown }));
        self.failed = None;
        self.popup = None;
        true
    }

    pub fn cancel(&mut self) {
        self.action = Some(ViewerAction::Close);
    }

    /// The worker's replies: plans, structures, the run's outcome.
    fn take_replies(&mut self) -> bool {
        let Some(client) = &self.client else { return false };
        let replies = client.poll();
        let mut changed = !replies.is_empty();
        for reply in replies {
            if let Some(table) = self.structure_tickets.remove(&reply.ticket) {
                if let Ok(Response::Structure(s)) = reply.result {
                    self.structures.insert(table, s);
                    self.fill_references();
                }
                continue;
            }
            if self.asked.as_ref().is_some_and(|(_, t)| *t == reply.ticket) {
                let Some((changes, _)) = self.asked.take() else { continue };
                match reply.result {
                    Err(DbError::Cancelled) => {}
                    Ok(Response::Plan(plan)) => self.planned = Some((changes, Ok(plan))),
                    Ok(_) => {}
                    Err(e) => self.planned = Some((changes, Err(e))),
                }
                continue;
            }
            if self.running == Some(reply.ticket) {
                self.running = None;
                match reply.result {
                    Ok(Response::Applied(applied)) => {
                        let changes = self.changes().unwrap_or_default();
                        if let Some(request) = &self.request {
                            request.done(Done { changes, applied });
                        }
                        self.action = Some(ViewerAction::Close);
                    }
                    Ok(_) => {}
                    Err(e) => {
                        self.failed = Some(tr!("schema.failed", why = message(&e)));
                        // Planned again: the structure may have changed.
                        self.planned = None;
                        changed = true;
                    }
                }
            }
        }
        self.replan();
        changed
    }

    /// A key's columns pointed to, when none were chosen: its table's primary key.
    fn fill_references(&mut self) {
        let Some(form) = &mut self.form else { return };
        for row in &mut form.keys {
            if !row.references.is_empty() {
                continue;
            }
            if let Some(s) = row.target.as_ref().and_then(|t| self.structures.get(t)) {
                row.references = s.primary_key();
            }
        }
    }

    /// The options of a choice: their labels and whether each is chosen.
    pub fn options(&self, choice: Choice) -> Vec<(String, bool)> {
        let Some(form) = &self.form else { return Vec::new() };
        let key = |i: usize| form.keys.get(i);
        match choice {
            Choice::KeyColumns(i) => {
                let chosen = key(i).map(|k| k.columns.clone()).unwrap_or_default();
                form.own_columns().into_iter().map(|c| (c.clone(), chosen.contains(&c))).collect()
            }
            Choice::KeyTarget(i) => {
                let chosen = key(i).and_then(|k| k.target.clone());
                form.targets()
                    .into_iter()
                    .map(|t| (t.to_string(), Some(&t) == chosen.as_ref()))
                    .collect()
            }
            Choice::KeyReferences(i) => {
                let Some(row) = key(i) else { return Vec::new() };
                self.target_columns(row)
                    .into_iter()
                    .map(|c| (c.clone(), row.references.contains(&c)))
                    .collect()
            }
            Choice::OnDelete(i) | Choice::OnUpdate(i) => {
                let now = key(i).map(|k| {
                    if matches!(choice, Choice::OnDelete(_)) { k.on_delete } else { k.on_update }
                });
                ACTIONS.iter().map(|a| (a.sql().to_owned(), Some(*a) == now)).collect()
            }
            Choice::Position => {
                let columns = form.own_columns();
                let Kind::MoveColumn(moved) = &form.kind else { return Vec::new() };
                let others: Vec<&String> = columns.iter().filter(|c| *c != moved).collect();
                let mut all = vec![(t("schema.position_first"), form.position == 0)];
                for (at, c) in others.iter().enumerate() {
                    all.push((
                        tr!("schema.position_after", column = c.as_str()),
                        form.position == at + 1,
                    ));
                }
                all
            }
        }
    }

    /// The columns of the table a key points to (a new table pointing to itself: its own).
    pub fn target_columns(&self, row: &KeyRow) -> Vec<String> {
        let Some(form) = &self.form else { return Vec::new() };
        let Some(target) = &row.target else { return Vec::new() };
        if let Some(s) = self.structures.get(target) {
            return s.columns.iter().map(|c| c.name.clone()).collect();
        }
        let own = form.ctx.table.as_ref() == Some(target)
            || (form.kind == Kind::CreateTable && target.name == form.table_name.text().trim());
        if own { form.own_columns() } else { Vec::new() }
    }

    /// Option `at` of `choice` chosen (or let go, in a list of several).
    pub fn choose(&mut self, choice: Choice, at: usize) {
        let options = self.options(choice);
        let Some((label, _)) = options.get(at).cloned() else { return };
        let targets = self.form.as_ref().map(Form::targets).unwrap_or_default();
        let Some(form) = &mut self.form else { return };
        let toggle =
            |list: &mut Vec<String>, name: String| match list.iter().position(|c| *c == name) {
                Some(i) => {
                    list.remove(i);
                }
                None => list.push(name),
            };
        match choice {
            Choice::KeyColumns(i) => {
                if let Some(row) = form.keys.get_mut(i) {
                    toggle(&mut row.columns, label);
                }
            }
            Choice::KeyReferences(i) => {
                if let Some(row) = form.keys.get_mut(i) {
                    toggle(&mut row.references, label);
                }
            }
            Choice::KeyTarget(i) => {
                if let Some(row) = form.keys.get_mut(i) {
                    let target = targets.get(at).cloned();
                    if row.target != target {
                        row.target = target;
                        row.references.clear();
                    }
                }
                self.fill_references();
            }
            Choice::OnDelete(i) | Choice::OnUpdate(i) => {
                let action = ACTIONS.get(at).copied().unwrap_or(FkAction::NoAction);
                if let Some(row) = form.keys.get_mut(i) {
                    if matches!(choice, Choice::OnDelete(_)) {
                        row.on_delete = action;
                    } else {
                        row.on_update = action;
                    }
                }
            }
            Choice::Position => form.position = at,
        }
        if !choice.multiple() {
            self.popup = None;
        }
        self.replan();
    }

    /// A box ticked or not (what follows it: auto-increment is the primary key).
    pub fn tick(&mut self, tick: Tick) {
        if tick == Tick::Confirm {
            self.confirmed = !self.confirmed;
            return;
        }
        let Some(form) = &mut self.form else { return };
        match tick {
            Tick::NotNull(i) => {
                if let Some(c) = form.columns.get_mut(i) {
                    c.not_null = !c.not_null;
                }
            }
            Tick::Primary(i) => {
                if let Some(c) = form.columns.get_mut(i) {
                    c.primary = !c.primary;
                    if !c.primary {
                        c.auto = false;
                    }
                }
            }
            Tick::Auto(i) => {
                if let Some(c) = form.columns.get_mut(i) {
                    c.auto = !c.auto;
                    // Numbered: the whole primary key, an integer.
                    if c.auto {
                        c.primary = true;
                        c.not_null = true;
                        if c.ty.text().trim().is_empty() {
                            c.ty.set_text("INTEGER");
                        }
                    }
                }
                if form.kind == Kind::CreateTable && form.columns.get(i).is_some_and(|c| c.auto) {
                    for (j, c) in form.columns.iter_mut().enumerate() {
                        if j != i {
                            c.primary = false;
                            c.auto = false;
                        }
                    }
                }
            }
            Tick::Unique(i) => {
                if let Some(c) = form.columns.get_mut(i) {
                    c.unique = !c.unique;
                }
            }
            Tick::Stored(i) => {
                if let Some(c) = form.columns.get_mut(i) {
                    c.stored = !c.stored;
                }
            }
            Tick::WithoutRowid => form.without_rowid = !form.without_rowid,
            Tick::IndexUnique => {
                let unique = !form.unique;
                form.set_unique(unique);
            }
            Tick::Confirm => {}
        }
        self.replan();
    }

    fn edited(&mut self, field: Text) {
        if let Some(form) = &mut self.form {
            form.edited(field);
        }
        if let Text::Name(i) | Text::Type(i) | Text::Default(i) = field {
            self.selected = i;
        }
        // Types and collations: suggested as they're typed.
        self.suggest(field, false);
        self.replan();
    }

    /// The suggestions completing `field` shown (the shared picker), or none: not a field
    /// that has them, or nothing left. Typed (`all` no): those starting as typed, the first
    /// lit (↵ takes it); clicked (`all`): every one, the field's value ticked and lit.
    fn suggest(&mut self, field: Text, all: bool) {
        use ui_components::picker::PickerItem;
        let typed = self.form.as_ref().and_then(|f| f.input(field)).map(|i| i.text().trim());
        let typed = typed.unwrap_or_default().to_owned();
        self.suggested = if all { self.options_of(field) } else { self.suggestions(field) };
        if self.suggested.is_empty() {
            if matches!(self.popup, Some(Popup::Suggest(_))) {
                self.popup = None;
                self.picker.close();
            }
            return;
        }
        let chosen = self.suggested.iter().position(|s| s.eq_ignore_ascii_case(&typed));
        let items = self
            .suggested
            .iter()
            .map(|s| PickerItem { label: (*s).to_owned(), ..PickerItem::default() })
            .collect();
        self.picker.open_suggestions(items, chosen.filter(|_| all), !all && !typed.is_empty());
        self.popup = Some(Popup::Suggest(field));
    }

    /// Every type or collation `field` may take (none for another field).
    fn options_of(&self, field: Text) -> Vec<&'static str> {
        let Some(form) = &self.form else { return Vec::new() };
        match field {
            Text::Type(_) => form.ctx.types(),
            Text::Collate(_) => form.ctx.collations(),
            _ => Vec::new(),
        }
    }

    /// A field's suggestions are open (the picker has the arrows, the clicks, the wheel).
    pub(super) fn suggesting(&self) -> bool {
        matches!(self.popup, Some(Popup::Suggest(_))) && self.picker.is_open()
    }

    /// What completes a type's or a collation's field (those starting as typed; all for
    /// an empty field).
    pub fn suggestions(&self, field: Text) -> Vec<&'static str> {
        let Some(form) = &self.form else { return Vec::new() };
        let all = match field {
            Text::Type(_) => form.ctx.types(),
            Text::Collate(_) => form.ctx.collations(),
            _ => return Vec::new(),
        };
        let typed = form.input(field).map(|i| i.text().trim().to_ascii_lowercase());
        let typed = typed.unwrap_or_default();
        all.into_iter()
            .filter(|s| {
                s.to_ascii_lowercase().starts_with(&typed) && !s.eq_ignore_ascii_case(&typed)
            })
            .collect()
    }

    /// Choice `choice`'s list opened (the shared picker; several to tick for the keys'
    /// columns and references).
    fn open_choice(&mut self, choice: Choice) {
        use ui_components::picker::{ItemRows, PickerItem};
        let options = self.options(choice);
        let chosen: Vec<usize> =
            options.iter().enumerate().filter(|(_, (_, on))| *on).map(|(i, _)| i).collect();
        let items = options
            .into_iter()
            .map(|(label, _)| PickerItem { label, ..PickerItem::default() })
            .collect();
        if choice.multiple() {
            self.picker.open_many(items, chosen, Box::new(ItemRows::default()));
        } else {
            self.picker.open(items, chosen.first().copied());
        }
        self.popup = Some(Popup::Choice(choice));
    }

    /// A choice's list is open (the picker has the keys, the clicks, the wheel).
    pub(super) fn choosing(&self) -> bool {
        matches!(self.popup, Some(Popup::Choice(_))) && self.picker.is_open()
    }

    /// What the open list said.
    fn picked(&mut self, event: Option<ui_components::picker::PickerEvent>) {
        use ui_components::picker::PickerEvent;
        match event {
            Some(PickerEvent::Chosen(at) | PickerEvent::Toggled(at)) => self.pick(at),
            Some(PickerEvent::Closed) => self.popup = None,
            None => {}
        }
    }

    /// The open list's rows: their labels and whether each is chosen.
    pub fn popup_rows(&self) -> Vec<(String, bool)> {
        match self.popup {
            Some(Popup::Suggest(_)) => {
                self.suggested.iter().map(|s| ((*s).to_owned(), false)).collect()
            }
            Some(Popup::Choice(choice)) => self.options(choice),
            None => Vec::new(),
        }
    }

    /// Row `at` of the popup taken.
    fn pick(&mut self, at: usize) {
        match self.popup {
            Some(Popup::Suggest(field)) => {
                let Some(value) = self.suggested.get(at).copied() else { return };
                if let Some(input) = self.form.as_mut().and_then(|f| f.input_mut(field)) {
                    input.set_text(value);
                }
                self.popup = None;
                self.picker.close();
                if let Some(form) = &mut self.form {
                    form.edited(field);
                }
                self.replan();
            }
            Some(Popup::Choice(choice)) => self.choose(choice, at),
            None => {}
        }
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

impl Viewer for SchemaView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        if let Some(request) = data.downcast_ref::<SchemaRequest>() {
            self.take(request.clone());
        }
    }

    fn subtitle(&self) -> Option<String> {
        let form = self.form.as_ref()?;
        Some(match form.table() {
            Some(table) => match &form.kind {
                Kind::AlterColumn(c) | Kind::DropColumn(c) | Kind::MoveColumn(c) => {
                    format!("{table}.{c}")
                }
                Kind::DropIndex(name) => format!("{table} · {name}"),
                _ => table.to_string(),
            },
            None => t("schema.subtitle_new_table"),
        })
    }

    fn modal_icon(&self) -> Option<&'static str> {
        Some("table")
    }

    /// The submit (in the danger color when it only takes away), Cancel.
    fn modal_buttons(&self) -> Vec<ui_components::ModalButton> {
        use ui_components::ModalButton;
        let drops = self.form.as_ref().is_some_and(|f| f.kind.drops());
        let submit = ModalButton::main(SUBMIT, self.submit_label(), self.can_submit());
        vec![submit.dangerous(drops), ModalButton::other(CANCEL, t("database.preview_cancel"))]
    }

    /// Running; else what runs is: SQL on the server, or a file written.
    fn modal_status(&self) -> Option<ui_components::ModalStatus> {
        use ui_components::ModalStatus;
        if self.running.is_some() {
            return Some(ModalStatus::Busy(t("schema.running")));
        }
        let sql = self.form.as_ref().is_some_and(|f| f.ctx.sql());
        let note = if sql { t("database.preview_sql_note") } else { super::file_note() };
        Some(ModalStatus::Note(note))
    }

    fn key_hints(&self) -> Vec<(Vec<&'static str>, String)> {
        let fields = self.form.as_ref().is_some_and(|f| !f.texts().is_empty());
        let mut hints = Vec::new();
        if fields {
            hints.push((vec!["tab"], t("schema.hint_next")));
        }
        hints.push((vec!["↵"], self.submit_label()));
        hints
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        // Woken by the app's waker (none in a snapshot: the replies are polled anyway).
        let wake = ctx.waker.clone().unwrap_or_else(|| Arc::new(|| {}));
        self.connect(wake);
        self.hits.clear();
        self.tips.clear();
        self.paint_modal(ctx);
    }

    fn tooltip_at(&self, p: Point) -> Option<ui_components::tooltip::Tip> {
        // Not through an open popup over them.
        if self.popup.is_some() {
            return None;
        }
        let (r, label) = self.tips.iter().rev().find(|(r, _)| r.contains(p))?;
        Some(ui_components::tooltip::Tip::new(*r, label.clone()))
    }

    fn poll(&mut self) -> bool {
        self.take_replies()
    }

    fn is_loading(&self) -> bool {
        self.running.is_some() || self.asked.is_some()
    }

    fn wants_text(&self) -> bool {
        self.focus.is_some() || (self.choosing() && self.picker.searches())
    }

    fn text_input(&mut self, text: &str) -> bool {
        // A choice's list's search, else the focused field (under its suggestions too).
        let used = self.with_controls(|c, parts| c.text(parts, text)).is_some();
        self.heard_keys();
        used
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        self.edit_key_with(key, KeyReach::default())
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        // An open list over everything first (a choice's has every key; a field's
        // suggestions are lit by the arrows, taken by ↵ or ⇥), then the focused field.
        let choosing = self.choosing();
        let used = self.with_controls(|c, parts| c.key(parts, key, reach)).is_some();
        if choosing {
            let event = self.picker.take_event();
            self.picked(event);
            return true;
        }
        if self.heard_keys() || used {
            return true;
        }
        match key {
            ViewerKey::Tab | ViewerKey::Down => self.focus_step(true),
            ViewerKey::Up => self.focus_step(false),
            ViewerKey::Enter => {
                self.popup = None;
                self.submit();
            }
            ViewerKey::Escape => self.cancel(),
            _ => return false,
        }
        true
    }

    fn command(&mut self, id: &str) -> bool {
        // The footer's buttons (the window draws them).
        match id {
            SUBMIT => return self.submit(),
            CANCEL => {
                self.cancel();
                return true;
            }
            _ => {}
        }
        // A choice's list's search gets the edit commands.
        if self.choosing() {
            return self.picker.command(id);
        }
        let field = self.focus;
        let used = match id {
            "submit" => self.submit(),
            "select_all" => self.focused_input().is_some_and(|i| {
                i.select_all();
                true
            }),
            "undo" | "redo" => {
                self.focused_input().is_some_and(|i| if id == "redo" { i.redo() } else { i.undo() })
            }
            "cut" => self.focused_input().is_some_and(|i| {
                let had = !i.selected_text().is_empty();
                if had {
                    i.insert("");
                }
                had
            }),
            #[cfg(feature = "snapshot")]
            id if id.starts_with("snapshot:") => self.snapshot(&id["snapshot:".len()..]),
            _ => return false,
        };
        if used
            && matches!(id, "undo" | "redo" | "cut")
            && let Some(field) = field
        {
            self.edited(field);
        }
        used
    }

    fn selected_text(&self) -> Option<String> {
        self.controls.selected_text_with(|i| self.part(i))
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }

    /// The part under the mouse (an open list over everything, the preview, the form), as
    /// it is now (a list may have opened under it since it last moved).
    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        let at = self.last_hover;
        self.with_controls(|c, parts| {
            c.hover(parts, at);
            c.scroll_hovered(parts, dx, dy)
        })
        .is_some()
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        let inside = self.picker.contains(p);
        let opener = self.popup;
        let pressed = self.with_controls(|c, parts| c.press(parts, p, 1));
        // A choice's list takes the click: a row taken; outside it, it closes — and the click
        // goes on, but on the field that opened it (that only closes it).
        if let Some((part, _)) = pressed {
            if part != LIST {
                return None;
            }
            let event = self.picker.take_event();
            self.picked(event);
            let same = matches!((&self.hit(p), opener), (Some(Hit::Choice(c)), Some(Popup::Choice(o))) if *c == o);
            if inside || same {
                return self.action.take();
            }
        }
        let hit = self.hit(p);

        match hit? {
            Hit::Field(field) => {
                self.focus = Some(field);
                if let Text::Name(i) | Text::Type(i) | Text::Default(i) = field {
                    self.selected = i;
                }
                if let Some(input) = self.focused_input() {
                    input.click(p.x);
                }
                // Clicked: every one, its value ticked (chosen once, it's offered again).
                self.suggest(field, true);
            }
            Hit::Tick(tick) => self.tick(tick),
            Hit::Choice(choice) => self.open_choice(choice),
            Hit::SelectColumn(i) => self.selected = i,
            Hit::RemoveColumn(i) => self.remove_column(i),
            Hit::AddKey => {
                if let Some(form) = &mut self.form {
                    form.keys.push(KeyRow::default());
                }
                self.replan();
            }
            Hit::RemoveKey(i) => {
                if let Some(form) = &mut self.form
                    && i < form.keys.len()
                {
                    form.keys.remove(i);
                }
                self.replan();
            }
            Hit::IndexColumn(name) => {
                if let Some(form) = &mut self.form {
                    form.toggle_index_column(&name);
                }
                self.replan();
            }
            Hit::IndexOrder(name) => {
                if let Some(form) = &mut self.form {
                    form.flip_index_order(&name);
                }
                self.replan();
            }
        }
        self.action.take()
    }

    fn drag(&mut self, p: Point) -> bool {
        if self.with_controls(|c, parts| c.drag(parts, p)).is_some() {
            return true;
        }
        match self.focused_input() {
            Some(input) if input.contains(p) => {
                input.extend_to(p.x);
                true
            }
            _ => false,
        }
    }

    fn release(&mut self) -> bool {
        self.with_controls(|c, parts| c.release(parts)).is_some_and(|(_, changed)| changed)
    }

    fn hover(&mut self, p: Point) -> bool {
        self.last_hover = Some(p);
        // An open list takes the mouse over everything.
        let parts = self.with_controls(|c, parts| c.hover(parts, Some(p)));
        if self.choosing() || self.suggesting() {
            return parts;
        }
        let hover = self.hit(p);
        let changed = hover != self.hover;
        self.hover = hover;
        changed | parts
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        // A part's (an open list's, never what's under it).
        if let Some(cursor) = self.controls.cursor_at(p) {
            return Some(cursor);
        }
        match self.hit(p)? {
            Hit::Field(_) => Some(ViewerCursor::Text),
            _ => Some(ViewerCursor::Pointer),
        }
    }
}

/// The open list's and the focused field's places among the modal's parts.
const LIST: usize = 0;
const FIELD: usize = 3;

impl SchemaView {
    /// Its parts (a choice's list or a field's suggestions while asked for, over
    /// everything, and its keys; the preview and its scrollbar; the form and its
    /// scrollbar: the rest of the modal; the focused field, having the keys), the topmost
    /// first, with their state of use. An open list's keys come first, those it doesn't
    /// use and the text go on to the field (its suggestions take ↑ ↓ ↵ ⇥ esc, the field
    /// keeps the typing). The view moves among its fields (Tab, ↑ ↓, a press on one): the
    /// field's part is always the focused one.
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let listing = if self.choosing() {
            Some(Listing::Choice)
        } else if self.suggesting() {
            Some(Listing::Suggest)
        } else {
            None
        };
        let field = self.focus;
        let shown = self.popup.is_some();
        let mut list = ListPart {
            picker: &mut self.picker,
            shown,
            listing,
            picked: &mut self.heard.picked,
            closed: &mut self.heard.closed,
        };
        let input = field.and_then(|f| self.form.as_mut()?.input_mut(f));
        let mut field_keys = FieldKeys {
            field,
            input,
            open: listing != Some(Listing::Choice),
            edited: &mut self.heard.edited,
        };
        let mut preview = Scrolled {
            bar: &mut self.preview_bar,
            offset: &mut self.preview_scroll,
            area: self.preview_area,
            max: self.preview_max,
        };
        let mut form = Scrolled {
            bar: &mut self.form_bar,
            offset: &mut self.form_scroll,
            area: self.bounds,
            max: self.form_max,
        };
        let mut parts: [&mut dyn Control; 4] =
            [&mut list, &mut preview, &mut form, &mut field_keys];
        self.controls.focus(&mut parts, Some(FIELD));
        let answer = f(&mut self.controls, &mut parts);
        // A press on the open list doesn't take the keys from the field.
        self.controls.focus(&mut parts, Some(FIELD));
        answer
    }

    /// Its part at `i`, read-only (the copy's selection asked from `&self`): the focused
    /// field's.
    fn part(&self, i: usize) -> Option<&dyn Control> {
        let field = self.focus.filter(|_| i == FIELD)?;
        Some(self.form.as_ref()?.input(field)? as &dyn Control)
    }

    /// What the keys' parts heard, acted on: a suggestion taken or closed, the field's
    /// text changed. `true` if a suggestion was taken or closed.
    fn heard_keys(&mut self) -> bool {
        let heard = std::mem::take(&mut self.heard);
        if heard.edited
            && let Some(field) = self.focus
        {
            self.edited(field);
        }
        if let Some(at) = heard.picked {
            self.pick(at);
            return true;
        }
        if heard.closed {
            self.popup = None;
            self.picker.close();
            return true;
        }
        false
    }

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
