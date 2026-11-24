//! The source's choices: a dropdown or a switch set, read-only, production (its own or its
//! folder's), the password's policy, the schemas and databases shown.

use super::*;

/// `name` shown or not among `shown` (none: all of `all`): all shown, all but it then; the
/// last hidden one shown again, all again (none listed).
fn toggle_shown(shown: &mut Vec<String>, name: &str, all: &[String]) {
    if shown.is_empty() {
        *shown = all.iter().filter(|s| *s != name).cloned().collect();
    } else if let Some(at) = shown.iter().position(|s| s == name) {
        shown.remove(at);
    } else {
        shown.push(name.to_owned());
        if all.iter().all(|s| shown.contains(s)) {
            shown.clear();
        }
    }
}

impl Editor {
    /// A choice field's value chosen (an empty one: its default).
    pub fn choose(&mut self, id: &str, value: &str) {
        if let Some((_, input)) = self.specs.iter_mut().find(|(i, _)| i == id) {
            input.set_text(value);
        }
        self.store();
        self.write_url();
    }

    /// A switch field flipped (unset: its default first).
    pub fn flip(&mut self, id: &str) {
        let spec = self.spec();
        let Some(field) = spec.field(id) else { return };
        let now = self.specs.iter().find(|(i, _)| i == id).map(|(_, t)| t.text().to_owned());
        let now = now.filter(|t| !t.is_empty()).unwrap_or_else(|| field.default.clone());
        self.choose(id, if now == "true" { "false" } else { "true" });
    }

    pub fn set_read_only(&mut self, read_only: bool) {
        if let Some(entry) = self.current_mut() {
            entry.source.read_only = read_only;
        }
    }

    /// The selected source marked as production (its own mark; a folder's stays).
    pub fn set_production(&mut self, production: bool) {
        if let Some(entry) = self.current_mut() {
            entry.source.production = production;
        }
    }

    /// Entry `index` is production: marked so, or in a folder that is.
    pub fn production_of(&self, index: usize) -> bool {
        let Some(entry) = self.entries.get(index) else { return false };
        entry.source.production || self.inherited_production(index).is_some()
    }

    /// The folder entry `index` takes production from (the nearest one up marked so).
    pub fn inherited_production(&self, index: usize) -> Option<&SourceFolder> {
        let source = &self.entries.get(index)?.source;
        let folder = self.folder_of(source)?;
        folders::production_folder(&self.folders, folder)
    }

    /// Folder `id` marked as production, or not.
    pub fn set_folder_production(&mut self, id: FolderId, production: bool) {
        if let Some(f) = self.folders.iter_mut().find(|f| f.id == id) {
            f.production = production;
        }
    }

    /// Folder `id` is production (marked so, or inside one that is).
    pub fn folder_production(&self, id: FolderId) -> bool {
        folders::folder_production(&self.folders, id)
    }

    pub fn set_policy(&mut self, policy: SecretPolicy) {
        if let Some(entry) = self.current_mut() {
            entry.source.secrets = policy;
        }
    }

    /// The selected source's schema `name` shown or not (all shown: all but it, then).
    pub fn toggle_schema(&mut self, name: &str, all: &[String]) {
        if let Some(entry) = self.current_mut() {
            toggle_shown(&mut entry.source.schemas, name, all);
        }
    }

    /// The selected source's database `name` shown or not (all shown: all but it, then).
    pub fn toggle_database(&mut self, name: &str, all: &[String]) {
        if let Some(entry) = self.current_mut() {
            toggle_shown(&mut entry.source.databases, name, all);
        }
    }

    /// Every database shown again.
    pub fn show_all_databases(&mut self) {
        if let Some(entry) = self.current_mut() {
            entry.source.databases.clear();
        }
    }

    /// The databases known for the selected source: those the last test found, and those
    /// it shows.
    pub fn databases(&self) -> Vec<String> {
        let Some(entry) = self.current() else { return Vec::new() };
        let mut all: Vec<String> = self
            .databases_found
            .iter()
            .find(|(id, _)| *id == entry.source.id)
            .map(|(_, s)| s.clone())
            .unwrap_or_default();
        for d in &entry.source.databases {
            if !all.contains(d) {
                all.push(d.clone());
            }
        }
        all
    }

    /// A database is shown in the tree.
    pub fn shows_database(&self, name: &str) -> bool {
        self.current().is_some_and(|e| {
            e.source.databases.is_empty() || e.source.databases.iter().any(|s| s == name)
        })
    }

    /// Every schema shown again.
    pub fn show_all_schemas(&mut self) {
        if let Some(entry) = self.current_mut() {
            entry.source.schemas.clear();
        }
    }

    /// The schemas known for the selected source: those the last test found, and those it
    /// shows.
    pub fn schemas(&self) -> Vec<String> {
        let Some(entry) = self.current() else { return Vec::new() };
        let mut all: Vec<String> = self
            .schemas_found
            .iter()
            .find(|(id, _)| *id == entry.source.id)
            .map(|(_, s)| s.clone())
            .unwrap_or_default();
        for s in &entry.source.schemas {
            if !all.contains(s) {
                all.push(s.clone());
            }
        }
        all
    }

    /// A schema is shown in the tree.
    pub fn shows_schema(&self, name: &str) -> bool {
        self.current().is_some_and(|e| {
            e.source.schemas.is_empty() || e.source.schemas.iter().any(|s| s == name)
        })
    }
}
