//! The selected source's fields: the inputs by field, what an edit changes (the URL kept
//! in step both ways), what's loaded and stored back, the options, the files picked.

use super::*;

impl Editor {
    /// The selected source's values into the fields.
    pub(super) fn load(&mut self) {
        let Some(entry) = self.current().cloned() else {
            self.specs.clear();
            self.options.clear();
            return;
        };
        let spec = self.spec();
        self.name = TextInput::new(&entry.source.name);
        self.comment = TextInput::new(&entry.source.comment);
        self.specs = spec
            .fields
            .iter()
            .filter(|f| !f.is_secret())
            .map(|f| (f.id.clone(), TextInput::new(&fields::value(&entry.source, f))))
            .collect();
        self.startup = TextInput::new(
            entry.source.target.options.get(Target::STARTUP).map_or("", String::as_str),
        );
        self.options = entry
            .source
            .target
            .options
            .iter()
            .filter(|(k, _)| !own_option(&spec, k))
            .map(|(k, v)| (TextInput::new(k), TextInput::new(v)))
            .collect();
        self.revealed = false;
        self.url_error = None;
        self.write_url();
    }

    /// The URL written from the source as it is now.
    pub(super) fn write_url(&mut self) {
        let spec = self.spec();
        let url = self.current().and_then(|e| fields::to_url(&spec, &e.source)).unwrap_or_default();
        self.url = TextInput::new(&url);
    }

    /// Every field there is now, with its text, in one order that [`Self::inputs_mut`]
    /// keeps too: the filter, the source's own, its driver's, its options, its password
    /// (a source selected), the folder's name (a folder selected).
    pub fn inputs(&self) -> Vec<(Field, &TextInput)> {
        let mut out = vec![
            (Field::Filter, &self.filter.input),
            (Field::Name, &self.name),
            (Field::Comment, &self.comment),
            (Field::Url, &self.url),
            (Field::Startup, &self.startup),
        ];
        out.extend(self.specs.iter().map(|(id, input)| (Field::Spec(id.clone()), input)));
        for (i, (key, value)) in self.options.iter().enumerate() {
            out.extend([(Field::OptionKey(i), key), (Field::OptionValue(i), value)]);
        }
        if let Some(entry) = self.current() {
            out.push((Field::Password, &entry.password));
        }
        if self.selected_folder.is_some() {
            out.push((Field::FolderName, &self.folder_name));
        }
        out
    }

    /// [`Self::inputs`], to be edited (the same fields, in the same order): the filter
    /// whole (its × with it), the others' text.
    pub fn inputs_mut(&mut self) -> (&mut SearchField, Vec<(Field, &mut TextInput)>) {
        let Self {
            filter,
            name,
            comment,
            url,
            startup,
            specs,
            options,
            entries,
            selected,
            selected_folder,
            folder_name,
            ..
        } = self;
        let mut out = vec![
            (Field::Name, name),
            (Field::Comment, comment),
            (Field::Url, url),
            (Field::Startup, startup),
        ];
        out.extend(specs.iter_mut().map(|(id, input)| (Field::Spec(id.clone()), input)));
        for (i, (key, value)) in options.iter_mut().enumerate() {
            out.extend([(Field::OptionKey(i), key), (Field::OptionValue(i), value)]);
        }
        if let Some(entry) = selected.and_then(|i| entries.get_mut(i)) {
            out.push((Field::Password, &mut entry.password));
        }
        if selected_folder.is_some() {
            out.push((Field::FolderName, folder_name));
        }
        (filter, out)
    }

    pub fn input(&self, field: &Field) -> Option<&TextInput> {
        Some(match field {
            Field::Filter => &self.filter.input,
            Field::Name => &self.name,
            Field::Comment => &self.comment,
            Field::Url => &self.url,
            Field::Startup => &self.startup,
            Field::Spec(id) => &self.specs.iter().find(|(i, _)| i == id)?.1,
            Field::Password => &self.current()?.password,
            Field::OptionKey(i) => &self.options.get(*i)?.0,
            Field::OptionValue(i) => &self.options.get(*i)?.1,
            Field::FolderName => {
                self.selected_folder?;
                &self.folder_name
            }
        })
    }

    pub fn input_mut(&mut self, field: &Field) -> Option<&mut TextInput> {
        Some(match field {
            Field::Filter => &mut self.filter.input,
            Field::Name => &mut self.name,
            Field::Comment => &mut self.comment,
            Field::Url => &mut self.url,
            Field::Startup => &mut self.startup,
            Field::Spec(id) => &mut self.specs.iter_mut().find(|(i, _)| i == id)?.1,
            Field::Password => &mut self.current_mut()?.password,
            Field::OptionKey(i) => &mut self.options.get_mut(*i)?.0,
            Field::OptionValue(i) => &mut self.options.get_mut(*i)?.1,
            Field::FolderName => {
                self.selected_folder?;
                &mut self.folder_name
            }
        })
    }

    /// `field` was edited: its text goes into the source (the URL into every field, and
    /// every field into the URL).
    pub fn edited(&mut self, field: &Field) {
        match field {
            Field::Filter => {
                let visible = self.visible();
                if self.selected.is_none_or(|s| !visible.contains(&s)) {
                    self.select(visible.first().copied());
                }
                return;
            }
            Field::Password => {
                if let Some(entry) = self.current_mut() {
                    entry.password_typed = true;
                }
                return;
            }
            Field::FolderName => {
                self.store_folder_name();
                return;
            }
            Field::Url => {
                let spec = self.spec();
                let text = self.url.text().to_owned();
                let Some(entry) = self.current_mut() else { return };
                let mut source = entry.source.clone();
                match fields::from_url(&spec, &text, &mut source) {
                    Ok(secret) => {
                        entry.source = source;
                        // A password in the URL goes to its field, never the source.
                        if let Some(secret) = secret {
                            entry.password = TextInput::new(secret.expose());
                            entry.password.set_masked(true);
                            entry.password_typed = true;
                        }
                        let url = std::mem::replace(&mut self.url, TextInput::new(""));
                        self.load();
                        self.url = url;
                        // The password never stays in the URL's text.
                        if self.current().is_some_and(|e| e.password_typed)
                            && text.contains('@')
                            && let Some(e) = self.current()
                            && let Some(clean) = fields::to_url(&spec, &e.source)
                            && clean.len() < text.len()
                        {
                            self.url = TextInput::new(&clean);
                        }
                        self.url_error = None;
                    }
                    Err(e) => self.url_error = Some(e),
                }
                return;
            }
            _ => {}
        }
        self.store();
        self.write_url();
    }

    /// The fields' texts into the selected source.
    pub(super) fn store(&mut self) {
        let spec = self.spec();
        let name = self.name.text().trim().to_owned();
        let comment = self.comment.text().trim().to_owned();
        let startup = self.startup.text().trim().to_owned();
        let specs: Vec<(String, String)> =
            self.specs.iter().map(|(id, input)| (id.clone(), input.text().to_owned())).collect();
        let options: Vec<(String, String)> = self
            .options
            .iter()
            .map(|(k, v)| (k.text().trim().to_owned(), v.text().trim().to_owned()))
            .collect();
        let Some(entry) = self.current_mut() else { return };
        let source = &mut entry.source;
        source.name = name;
        source.comment = comment;
        for (id, text) in specs {
            if let Some(field) = spec.field(&id) {
                fields::set_value(source, field, &text);
            }
        }
        // The advanced options: as listed (an empty name is none; a secret's never kept).
        source.target.options.retain(|k, _| own_option(&spec, k));
        for (k, v) in options {
            if !k.is_empty() && !database::source::is_secret_option(&k) && !own_option(&spec, &k) {
                source.target.options.insert(k, v);
            }
        }
        if startup.is_empty() {
            source.target.options.remove(Target::STARTUP);
        } else {
            source.target.options.insert(Target::STARTUP.to_owned(), startup);
        }
    }

    /// The selected source's field `id` as the driver describes it.
    pub fn field(&self, id: &str) -> Option<FieldSpec> {
        self.spec().field(id).cloned()
    }

    /// The fields of the selected source shown in `tab` (its password, too, in General).
    pub fn fields_in(&self, tab: FieldTab) -> Vec<FieldSpec> {
        self.spec().fields_in(tab).cloned().collect()
    }

    /// The text of the selected source's field `id` (empty: its default meant).
    pub fn text_of(&self, id: &str) -> String {
        self.specs
            .iter()
            .find(|(i, _)| i == id)
            .map(|(_, t)| t.text().to_owned())
            .unwrap_or_default()
    }

    /// A choice field's current value (its default when none).
    pub fn choice_of(&self, field: &FieldSpec) -> String {
        let text = self.text_of(&field.id);
        if text.is_empty() { field.default.clone() } else { text }
    }

    /// The fields that take typed text, in order (for ⇥).
    pub fn texts(&self) -> Vec<Field> {
        if self.selected_folder.is_some() {
            return vec![Field::FolderName];
        }
        let mut out = vec![Field::Name, Field::Comment];
        let Some(entry) = self.current() else { return vec![Field::Filter] };
        let spec = self.spec();
        let typed = |f: &FieldSpec| !matches!(f.kind, FieldKind::Choice(_) | FieldKind::Flag);
        match self.tab {
            Tab::General => {
                for f in spec.fields_in(FieldTab::General).filter(|f| typed(f)) {
                    out.push(if f.is_secret() {
                        Field::Password
                    } else {
                        Field::Spec(f.id.clone())
                    });
                }
                if spec.url.is_some() {
                    out.push(Field::Url);
                }
            }
            Tab::Options => {
                out.extend(
                    spec.fields_in(FieldTab::Options)
                        .filter(|f| typed(f))
                        .map(|f| Field::Spec(f.id.clone())),
                );
                if !self.driver(&entry.source.driver).is_some_and(|d| d.file_based) {
                    out.push(Field::Startup);
                }
            }
            Tab::Ssl => {
                out.extend(
                    spec.fields_in(FieldTab::Ssl)
                        .filter(|f| typed(f))
                        .map(|f| Field::Spec(f.id.clone())),
                );
            }
            Tab::Schemas => {}
            Tab::Advanced => {
                for i in 0..self.options.len() {
                    out.push(Field::OptionKey(i));
                    out.push(Field::OptionValue(i));
                }
            }
        }
        out
    }

    /// An advanced option row added (empty), its name's field returned.
    pub fn add_option(&mut self) -> Field {
        self.options.push((TextInput::new(""), TextInput::new("")));
        Field::OptionKey(self.options.len() - 1)
    }

    pub fn remove_option(&mut self, i: usize) {
        if i < self.options.len() {
            self.options.remove(i);
            self.store();
            self.write_url();
        }
    }

    /// Where the selected source's file field points (a driver's `File` field: its id).
    pub fn file_fields(&self) -> Vec<String> {
        self.spec()
            .fields
            .iter()
            .filter(|f| matches!(f.kind, FieldKind::File { .. }))
            .map(|f| f.id.clone())
            .collect()
    }

    /// A file picked for field `id`.
    pub fn picked(&mut self, id: &str, path: &str) {
        let field = Field::Spec(id.to_owned());
        if let Some(input) = self.input_mut(&field) {
            input.set_text(path);
        }
        self.edited(&field);
    }

    /// The selected source's password binding's field (a server's).
    pub fn has_password(&self) -> bool {
        self.spec().fields.iter().any(|f| f.binding == Binding::Password)
    }
}
