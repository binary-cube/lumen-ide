//! The sources added, copied, removed and moved between lists, what's wrong with them, and
//! what Apply makes of it all ([`Plan`]).

use super::*;

impl Editor {
    /// The names taken (a new source's is made unique among them).
    fn names(&self) -> Vec<String> {
        self.entries.iter().map(|e| e.source.name.clone()).collect()
    }

    /// A new source of `driver` (the project's), selected; its id.
    pub fn add(&mut self, driver: &str) -> Option<DataSourceId> {
        let info = self.driver(driver)?.clone();
        let names = self.names();
        let name = database::unique_name(&info.name, names.iter().map(String::as_str));
        let mut source = DataSource::new(name, driver, Scope::Project);
        if info.file_based {
            source.secrets = SecretPolicy::Never;
        }
        let id = source.id;
        self.entries.push(Entry::new(source, None));
        self.sort();
        let at = self.entries.iter().position(|e| e.source.id == id);
        self.filter.set_text("");
        self.select(at);
        self.tab = Tab::General;
        Some(id)
    }

    /// The selected source copied (a new identity, its name unique), selected.
    pub fn duplicate(&mut self) {
        let Some(entry) = self.current().cloned() else { return };
        let names = self.names();
        let mut source = entry.source.clone();
        source.id = DataSourceId::new();
        source.name = database::unique_name(&entry.source.name, names.iter().map(String::as_str));
        let mut copy = Entry::new(source, None);
        if entry.password_typed {
            copy.password = entry.password;
            copy.password_typed = true;
        } else {
            copy.copied_from = entry.original;
        }
        let id = copy.source.id;
        let after = self.selected.unwrap_or(self.entries.len()) + 1;
        self.entries.insert(after.min(self.entries.len()), copy);
        self.sort();
        let at = self.entries.iter().position(|e| e.source.id == id);
        self.select(at);
    }

    /// The selected source removed (the catalog's on Apply).
    pub fn remove(&mut self) {
        let Some(at) = self.selected else { return };
        let entry = self.entries.remove(at);
        if let Some(original) = entry.original {
            self.removed.push(original);
        }
        let visible = self.visible();
        let next = visible.iter().copied().find(|i| *i >= at).or_else(|| visible.last().copied());
        self.select(next);
    }

    /// The selected source moved to the other list (the project's ↔ every project's), at
    /// its top (its folder is the other list's no more).
    pub fn move_scope(&mut self) {
        let Some(entry) = self.current_mut() else { return };
        entry.source.scope = match entry.source.scope {
            Scope::Project => Scope::Global,
            Scope::Global => Scope::Project,
        };
        entry.source.folder = None;
        self.sort();
    }

    /// The selected source is in the project's list.
    pub fn scope(&self) -> Option<Scope> {
        self.current().map(|e| e.source.scope)
    }

    /// The problems keeping Apply back.
    pub fn problems(&self) -> Vec<Problem> {
        let mut out = Vec::new();
        for (i, entry) in self.entries.iter().enumerate().filter(|(_, e)| e.changed()) {
            if entry.source.name.trim().is_empty() {
                out.push(Problem::NoName(i));
            }
            if let Some(driver) = self.driver(&entry.source.driver) {
                if self.selected == Some(i) {
                    // The selected one's fields as typed (a port that isn't one never
                    // reaches the source).
                    for (id, input) in &self.specs {
                        if let Some(field) = driver.spec.field(id)
                            && let Err(error) = fields::validate(field, input.text())
                        {
                            out.push(Problem::Field(i, id.clone(), error));
                        }
                    }
                } else {
                    for (field, error) in driver.spec.problems(&entry.source) {
                        out.push(Problem::Field(i, field, error));
                    }
                }
            }
        }
        // The selected one's options as typed (one named like a secret is never kept).
        if let Some(at) = self.selected {
            for (k, _) in &self.options {
                if database::source::is_secret_option(k.text().trim()) {
                    out.push(Problem::SecretOption(at, k.text().trim().to_owned()));
                }
            }
        }
        out
    }

    /// The selected source's field `id`'s problem, if it has one.
    pub fn field_problem(&self, id: &str) -> Option<fields::FieldError> {
        let at = self.selected?;
        self.problems().into_iter().find_map(|p| match p {
            Problem::Field(i, f, e) if i == at && f == id => Some(e),
            _ => None,
        })
    }

    /// What Apply makes of the changes.
    pub fn plan(&self) -> Plan {
        let mut plan = Plan::default();
        let mut typed = Vec::new();
        let mut copies = Vec::new();
        for entry in &self.entries {
            let secret = entry.password_typed.then(|| Secret::new(entry.password.text()));
            match &entry.original {
                None => {
                    plan.add.push(entry.source.clone());
                    match (&secret, &entry.copied_from) {
                        (Some(s), _) => typed.push(SecretOp::Set(entry.source.clone(), s.clone())),
                        (None, Some(from)) => copies
                            .push(SecretOp::Copy { from: from.clone(), to: entry.source.clone() }),
                        _ => {}
                    }
                }
                Some(original) => {
                    if *original != entry.source {
                        plan.update.push(entry.source.clone());
                        if original.secrets != entry.source.secrets
                            || original.scope != entry.source.scope
                        {
                            plan.secrets.push(SecretOp::Move {
                                from: original.clone(),
                                to: entry.source.clone(),
                            });
                        }
                    }
                    if let Some(s) = secret {
                        typed.push(SecretOp::Set(entry.source.clone(), s));
                    }
                }
            }
        }
        plan.folders = self.changed_folders();
        plan.secrets.extend(typed);
        plan.secrets.extend(copies);
        for removed in &self.removed {
            plan.remove.push(removed.clone());
            plan.secrets.push(SecretOp::Remove(removed.clone()));
        }
        plan
    }

    /// Applied: what the catalog has now is the edited lists (nothing left to apply).
    pub fn applied(&mut self) {
        self.removed.clear();
        self.original_folders = self.folders.clone();
        for entry in &mut self.entries {
            entry.original = Some(entry.source.clone());
            entry.password = TextInput::new("");
            entry.password.set_masked(true);
            entry.password_typed = false;
            entry.copied_from = None;
        }
    }
}
