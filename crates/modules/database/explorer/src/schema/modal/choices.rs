//! The form's choices and suggestions: a choice's options and what picking one does, the
//! boxes ticked, a field's suggestions as it's written.

use super::*;

impl SchemaView {
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

    pub(super) fn edited(&mut self, field: Text) {
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
    pub(super) fn suggest(&mut self, field: Text, all: bool) {
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
    pub(in crate::schema) fn suggesting(&self) -> bool {
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
    pub(super) fn open_choice(&mut self, choice: Choice) {
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
    pub(in crate::schema) fn choosing(&self) -> bool {
        matches!(self.popup, Some(Popup::Choice(_))) && self.picker.is_open()
    }

    /// What the open list said.
    pub(super) fn picked(&mut self, event: Option<ui_components::picker::PickerEvent>) {
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
    pub(super) fn pick(&mut self, at: usize) {
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
}
