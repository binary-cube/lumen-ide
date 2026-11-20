//! The request editor's parts, as a form's controls: the list open over everything (a
//! method, a body's or an auth's kind, what a `{{` offers), the separator between the request
//! and its response, the form's fields with the response beside them. Each borrows the
//! editor in turn (one answers at a time): a field's typing opens the suggestions, a row
//! chosen in the list goes into the field.

use std::cell::RefCell;

use kernel::Point;
use ui_components::control::{Control, Divider, Press};
use ui_components::picker::PickerEvent;
use ui_components::tool_panel::KeyReach;
use ui_components::{MultilineField, PressMods, ViewerCursor, ViewerKey};
use ui_kit::TextInput;

use super::{Area, Focus, Hit, Pick, RequestEditor, Table};

/// The editor its parts borrow in turn.
pub(super) type Shared<'e> = RefCell<&'e mut RequestEditor>;

/// The least and the most of the width the request keeps beside its response.
const SHARE: (f32, f32) = (0.2, 0.8);

impl RequestEditor {
    /// The open list's suggestions (they leave the keys to the field typed in).
    pub(super) fn suggesting(&self) -> bool {
        matches!(self.picking, Some(Pick::Suggest(..))) && self.picker.is_open()
    }

    /// What the list told: a row chosen goes in, a list closed is let go.
    fn follow(&mut self, told: Option<PickerEvent>) {
        match told {
            Some(PickerEvent::Chosen(i)) => self.picked(i),
            Some(_) => self.picking = None,
            None => {}
        }
    }

    /// What the list told as a control, followed.
    fn follow_list(&mut self) {
        let told = self.picker.take_event();
        self.follow(told);
    }

    /// What the form draws topmost at `p`.
    fn hit_at(&self, p: Point) -> Option<Hit> {
        self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)
    }

    /// A press on the form: what's under it pressed (a field takes the caret and is held
    /// for the drag that selects), on nothing the keys leave the fields.
    fn form_press(&mut self, p: Point, clicks: u32) -> Press {
        match self.hit_at(p) {
            Some(hit) => self.press_hit(hit, p, clicks),
            None => self.focus = None,
        }
        if self.dragging.is_some() { Press::Held } else { Press::Took }
    }

    /// The pressed field selects to `p`.
    fn field_drag(&mut self, p: Point) -> bool {
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

    /// The wheel: in the text of many lines typed in, else the response.
    fn form_scroll(&mut self, dx: f32, dy: f32) -> bool {
        if let Some(Focus::Area(area)) = self.focus
            && let Some(field) = self.area(area)
        {
            return field.scroll_by(dy);
        }
        if let Some((_, _, view)) = self.response.as_mut() {
            return ui_components::Viewer::scroll(view, dx, dy);
        }
        false
    }

    /// A key in the field typed in: the suggestions take ↑ ↓ ↵ ⇥ Esc (another key closes
    /// them and goes on in the field); ⇥ and ↵ go to the next field (↵ in the URL sends),
    /// Esc leaves the fields.
    fn field_key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        let list_key = matches!(
            key,
            ViewerKey::Up | ViewerKey::Down | ViewerKey::Enter | ViewerKey::Tab | ViewerKey::Escape
        );
        if self.suggesting() {
            if list_key {
                let told = self.picker.key(key, reach);
                self.follow(told);
                return true;
            }
            self.picker.close();
            self.picking = None;
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

    /// Text typed into the field typed in (one line in a field of one), then what it offers
    /// after a `{{`.
    fn field_text(&mut self, typed: &str) -> bool {
        let Some(focus) = self.focus else { return false };
        match focus {
            Focus::Area(area) => {
                let Some(field) = self.area(area) else { return false };
                field.insert(typed);
            }
            _ => {
                let one_line: String = typed.chars().filter(|c| !c.is_control()).collect();
                let Some(input) = self.input(focus) else { return false };
                input.insert(&one_line);
            }
        }
        self.typed_in(focus);
        self.suggest(focus);
        true
    }

    /// The field typed in, as a control read from `&self` (what it has selected).
    pub(super) fn typed_field(&self) -> Option<&dyn Control> {
        let focus = self.focus?;
        let form = self.form.as_ref()?;
        Some(match focus {
            Focus::Title => &form.title,
            Focus::Url => &form.url,
            Focus::Cell(table, row, value) => {
                let rows = match table {
                    Table::Params => &form.params,
                    Table::Headers => &form.headers,
                    Table::Form => &form.form,
                    Table::Multipart => &form.parts,
                };
                let row = rows.get(row)?;
                if value { &row.value } else { &row.name }
            }
            Focus::Auth(i) => form.auth.get(i)?,
            Focus::File => &form.file,
            Focus::Timeout => &form.timeout,
            Focus::ConnectTimeout => &form.connect_timeout,
            Focus::Area(Area::Bulk) => self.bulk.as_ref()?,
            Focus::Area(Area::Body) => &form.body,
            Focus::Area(Area::Variables) => &form.variables,
            Focus::Area(Area::Pre) => &form.pre,
            Focus::Area(Area::Post) => &form.post,
            Focus::Area(Area::Docs) => &form.docs,
        })
    }

    /// What the field typed in has selected.
    pub(super) fn field_selected_text(&self) -> Option<String> {
        self.typed_field()?.selected_text()
    }

    /// An edit command in the field typed in: ⌘A its text, undo and redo its typing.
    fn field_command(&mut self, id: &str) -> bool {
        let Some(focus) = self.focus else { return false };
        match id {
            "select_all" => match focus {
                Focus::Area(area) => self.area(area).map(MultilineField::select_all).is_some(),
                _ => self.input(focus).map(TextInput::select_all).is_some(),
            },
            "undo" | "redo" => {
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

    /// The separator as the app's divider (`None`: not drawn yet).
    fn divider(&mut self) -> Option<Divider<'_>> {
        Some(Divider { splitter: self.splitter.as_mut()?, at: &mut self.split_at })
    }
}

/// The open list as a part: over everything while open (a press outside it closes it),
/// each press as one (a quick series selects no word in its search), what it told followed
/// at once. It has the keys while it's open, but for the suggestions (they're the field's).
pub(super) struct ListPart<'s, 'e>(pub &'s Shared<'e>);

impl Control for ListPart<'_, '_> {
    fn area(&self) -> Option<kernel::Rect> {
        Control::area(&self.0.borrow().picker)
    }

    fn hit(&self, p: Point) -> bool {
        Control::hit(&self.0.borrow().picker, p)
    }

    fn press(&mut self, p: Point, _clicks: u32) -> Press {
        let mut editor = self.0.borrow_mut();
        let press = Control::press(&mut editor.picker, p, 1);
        editor.follow_list();
        press
    }

    fn drag(&mut self, p: Point) -> bool {
        Control::drag(&mut self.0.borrow_mut().picker, p)
    }

    fn release(&mut self) -> bool {
        Control::release(&mut self.0.borrow_mut().picker)
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        Control::hover(&mut self.0.borrow_mut().picker, p)
    }

    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        Control::cursor(&self.0.borrow().picker, p)
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        Control::scroll(&mut self.0.borrow_mut().picker, dx, dy)
    }

    fn focusable(&self) -> bool {
        let editor = self.0.borrow();
        editor.picker.is_open() && !editor.suggesting()
    }

    fn keeps_tab(&self) -> bool {
        true
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        let mut editor = self.0.borrow_mut();
        Control::key(&mut editor.picker, key, reach);
        editor.follow_list();
        true
    }

    fn text(&mut self, typed: &str) -> bool {
        Control::text(&mut self.0.borrow_mut().picker, typed)
    }

    fn selected_text(&self) -> Option<String> {
        self.0.borrow().picker.selected_text()
    }

    /// Its search's (⌘A, cut, undo); an undo or redo it has nothing for is the field's.
    fn command(&mut self, id: &str) -> bool {
        let mut editor = self.0.borrow_mut();
        editor.picker.command(id) || (matches!(id, "undo" | "redo") && editor.field_command(id))
    }
}

/// The separator between the request and its response (once drawn) as a part: held, the
/// request's share follows it.
pub(super) struct SplitPart<'s, 'e>(pub &'s Shared<'e>);

impl Control for SplitPart<'_, '_> {
    fn hit(&self, p: Point) -> bool {
        let editor = self.0.borrow();
        !editor.picker.is_open() && editor.splitter.as_ref().is_some_and(|s| s.contains(p))
    }

    fn press(&mut self, p: Point, clicks: u32) -> Press {
        let mut editor = self.0.borrow_mut();
        editor.divider().map_or(Press::Ignored, |mut d| d.press(p, clicks))
    }

    fn drag(&mut self, p: Point) -> bool {
        let mut editor = self.0.borrow_mut();
        let changed = editor.divider().is_some_and(|mut d| d.drag(p));
        let Some(x) = editor.split_at.take() else { return changed };
        let b = editor.split_area;
        if b.size.width > 0.0 {
            editor.share = ((x - b.min_x()) / b.size.width).clamp(SHARE.0, SHARE.1);
        }
        true
    }

    fn release(&mut self) -> bool {
        self.0.borrow_mut().divider().is_some_and(|mut d| d.release())
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        self.0.borrow_mut().divider().is_some_and(|mut d| d.hover(p))
    }

    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        self.0.borrow_mut().divider()?.cursor(p)
    }

    fn animating(&self) -> bool {
        self.0.borrow().splitter.as_ref().is_some_and(|s| s.is_animating())
    }
}

/// The form's fields and the response beside them as a part: under everything (while no
/// list is open), what a press lands on pressed; the field typed in has the keys (its own
/// ⇥ and ↵ order), its text, its selection and its edit commands; the wheel scrolls the
/// text of many lines typed in, else the response.
pub(super) struct FormPart<'s, 'e>(pub &'s Shared<'e>);

impl Control for FormPart<'_, '_> {
    fn hit(&self, _p: Point) -> bool {
        !self.0.borrow().picker.is_open()
    }

    fn press(&mut self, p: Point, clicks: u32) -> Press {
        self.0.borrow_mut().form_press(p, clicks)
    }

    fn press_with(&mut self, p: Point, clicks: u32, _mods: PressMods) -> Press {
        self.press(p, clicks)
    }

    fn drag(&mut self, p: Point) -> bool {
        self.0.borrow_mut().field_drag(p)
    }

    fn release(&mut self) -> bool {
        self.0.borrow_mut().dragging = None;
        false
    }

    /// What's under the mouse lights up; left for a list opened over it, it stays as it was.
    fn hover(&mut self, p: Option<Point>) -> bool {
        let mut editor = self.0.borrow_mut();
        let hover = match p {
            Some(p) => editor.hit_at(p),
            None if editor.picker.is_open() => return false,
            None => None,
        };
        std::mem::replace(&mut editor.hover, hover) != hover
    }

    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        Some(match self.0.borrow().hit_at(p)? {
            Hit::Field(_) => ViewerCursor::Text,
            _ => ViewerCursor::Pointer,
        })
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        self.0.borrow_mut().form_scroll(dx, dy)
    }

    fn focusable(&self) -> bool {
        true
    }

    fn keeps_tab(&self) -> bool {
        true
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        self.0.borrow_mut().field_key(key, reach)
    }

    fn text(&mut self, typed: &str) -> bool {
        self.0.borrow_mut().field_text(typed)
    }

    fn selected_text(&self) -> Option<String> {
        self.0.borrow().field_selected_text()
    }

    fn command(&mut self, id: &str) -> bool {
        self.0.borrow_mut().field_command(id)
    }
}
