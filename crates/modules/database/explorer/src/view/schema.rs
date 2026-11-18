//! The table's schema: its menus, its references, the diagram.

use super::*;

impl DataView {
    /// The structure changes this source's driver makes (none for a read-only source).
    pub(crate) fn schema_changes(&self) -> database::SchemaChanges {
        match &self.open {
            Some(open) if !open.worker.source().read_only => {
                open.worker.source().capabilities.schema_changes
            }
            _ => database::SchemaChanges::default(),
        }
    }

    /// The structure's modal for `kind`, on the table shown (its structure, the source's
    /// tables and their counts): opened by the app (the view leaves its request waiting).
    pub(crate) fn open_schema(&mut self, kind: crate::schema::Kind) -> bool {
        let Some(open) = &self.open else { return false };
        let source = open.worker.source();
        let creating = kind == crate::schema::Kind::CreateTable;
        let table = if creating { None } else { self.table().map(|t| t.table.clone()) };
        if !creating && table.is_none() {
            return false;
        }
        let structure = if creating {
            None
        } else {
            self.grid.as_ref().and_then(|g| g.source().structure().map(|s| (**s).clone()))
        };
        let rows = self
            .counts
            .exact()
            .filter_map(|(i, n)| self.tables.get(i).map(|t| (t.table.clone(), n)))
            .collect();
        let ctx = crate::schema::Context {
            table,
            schema: None,
            structure,
            tables: self.tables.clone(),
            rows,
            can: self.schema_changes(),
            dialect: source.dialect.clone(),
        };
        let answer: crate::schema::Answer = Arc::new(Mutex::new(None));
        let request = crate::schema::SchemaRequest {
            kind,
            ctx,
            worker: open.worker.clone(),
            answer: Arc::clone(&answer),
            wake: self.wake(),
        };
        self.schema_answer = Some(answer);
        self.action = Some(ViewerAction::OpenView(ui_components::OpenView::modal(
            request.kind.title(),
            crate::schema::EXTENSION,
            request,
        )));
        true
    }

    /// The structure's modal made its changes: told, the table it created
    /// or renamed shown next (the tables are read again by [`Self::restructured`]).
    pub(super) fn take_schema_answer(&mut self) -> bool {
        let Some(answer) = &self.schema_answer else { return false };
        let Some(done) = crate::schema::take_answer(answer) else { return false };
        self.schema_answer = None;
        self.note_own();
        for change in &done.changes {
            match change {
                database::SchemaChange::CreateTable(def) => self.reselect = Some(def.table.clone()),
                database::SchemaChange::RenameTable { table, to } => {
                    self.reselect =
                        Some(database::TableRef { schema: table.schema.clone(), name: to.clone() });
                }
                _ => {}
            }
        }
        self.notify(Notification::success(i18n::tr!("schema.done", count = done.applied.affected)));
        // Said on the bus too, unless the source has none (read again here either way).
        if let Some(open) = &self.open {
            open.restructured.store(true, Ordering::Release);
        }
        true
    }

    /// The structure changed: the tables listed again, the grid made again (its layout
    /// as kept, which followed the renames: the one in memory names what's gone).
    pub(super) fn restructured(&mut self) -> bool {
        let Some(open) = &self.open else { return false };
        if !open.restructured.swap(false, Ordering::AcqRel) {
            return false;
        }
        if self.reselect.is_none() {
            self.reselect = self.table().map(|t| t.table.clone());
        }
        self.grid = None;
        self.reload();
        true
    }

    /// The menu of the tables' list (`row`: the table under the mouse).
    pub(super) fn list_menu(&self, row: Option<usize>) -> Option<Vec<ViewMenuEntry>> {
        if self.is_text() {
            return None;
        }
        let can = self.schema_changes();
        let mut entries = Vec::new();
        // Its source's query consoles (on the table under the mouse).
        if self.console_source().is_some() {
            let id = match row.filter(|&i| i < self.tables.len()) {
                Some(i) => format!("console:{i}"),
                None => "console".to_owned(),
            };
            let mut console =
                ViewMenuEntry::item(id, i18n::t("datasources.menu_console")).with_icon("console");
            if let Some(keys) = ui_components::shortcuts::label(database::console::JUMP_COMMAND) {
                console = console.with_shortcut(keys);
            }
            entries.push(console);
            entries
                .push(ViewMenuEntry::item("new-console", i18n::t("datasources.menu_new_console")));
            entries.push(ViewMenuEntry::Separator);
        }
        if can.any() {
            if can.tables {
                entries.push(
                    ViewMenuEntry::item("schema:create-table", i18n::t("schema.menu_create_table"))
                        .with_icon("plus"),
                );
            }
            let table = row.and_then(|i| self.tables.get(i)).filter(|t| t.kind == TableKind::Table);
            if let Some(info) = table {
                let name = &info.table.name;
                entries.push(ViewMenuEntry::Separator);
                if can.renames && can.tables {
                    entries.push(ViewMenuEntry::item(
                        format!("schema:rename-table:{name}"),
                        i18n::t("schema.menu_rename_table"),
                    ));
                }
                if can.tables {
                    entries.push(
                        ViewMenuEntry::item(
                            format!("schema:drop-table:{name}"),
                            i18n::t("schema.menu_drop_table"),
                        )
                        .with_icon("trash"),
                    );
                }
            }
        }
        // The tables and their relations (around the one under the mouse).
        if self.open.is_some() {
            if !entries.is_empty() && !matches!(entries.last(), Some(ViewMenuEntry::Separator)) {
                entries.push(ViewMenuEntry::Separator);
            }
            let id = match row.filter(|&i| i < self.tables.len()) {
                Some(i) => format!("diagram:{i}"),
                None => "diagram".to_owned(),
            };
            entries.push(
                ViewMenuEntry::item(id, i18n::t("datasources.menu_diagram")).with_icon("relations"),
            );
        }
        (!entries.is_empty()).then_some(entries)
    }

    /// What the cell at the grid's cursor points to, if its column is a key's: the table,
    /// its column, the value (never a null). The engine's keys first, then those accepted
    /// for the source (kept with it).
    pub(crate) fn reference(&self) -> Option<(database::TableRef, String, String)> {
        let grid = self.grid.as_ref()?;
        let cell = grid.selection().cursor()?;
        let source = grid.source();
        let column = source.columns().get(cell.col)?.name.clone();
        let structure = source.structure()?;
        let real = structure.foreign_keys.iter().find_map(|k| {
            let at = k.columns.iter().position(|c| *c == column)?;
            Some((k.table.clone(), k.references.get(at)?.clone()))
        });
        let (table, target) = match real {
            Some(found) => found,
            None => {
                let open = self.open.as_ref()?;
                let id = self.source.as_ref().map_or(open.worker.source().id, |(id, _)| *id);
                let kept = crate::diagram::configured(&open.sessions, id, self.file())?;
                let key = kept.relations.key_of(&structure.table, &column)?;
                let at = key.columns.iter().position(|c| c.eq_ignore_ascii_case(&column))?;
                (key.target.clone(), key.references.get(at)?.clone())
            }
        };
        use ui_components::data_grid::CellRef;
        let literal = match source.cell(cell.row, cell.col) {
            CellRef::Int(i) => i.to_string(),
            CellRef::Float(f) => f.to_string(),
            CellRef::Bool(b) => i64::from(b).to_string(),
            CellRef::Text(t) | CellRef::Other(t) => format!("'{}'", t.replace('\'', "''")),
            CellRef::Null | CellRef::Bytes(_) | CellRef::Json(_) => return None,
        };
        let simple = target.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && target.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        let name = if simple { target } else { format!("\"{}\"", target.replace('"', "\"\"")) };
        Some((table, name, literal))
    }

    /// The row the cell at the grid's cursor points to: its table shown, filtered to it.
    pub(super) fn go_to_reference(&mut self) -> bool {
        let Some((table, column, literal)) = self.reference() else { return false };
        let same = |t: &database::TableRef| {
            t.name.eq_ignore_ascii_case(&table.name)
                && (table.schema.is_none() || t.schema.is_none() || t.schema == table.schema)
        };
        let Some(i) = self.tables.iter().position(|t| same(&t.table)) else {
            self.notify(Notification::warning(i18n::tr!(
                "database.reference_missing",
                table = table.to_string()
            )));
            return false;
        };
        self.tab = Tab::Data;
        self.select(i);
        self.pending_where = Some(format!("{MENU_PREFIX}where:{column} = {literal}"));
        self.apply_pending_where();
        true
    }

    /// The rows shown filtered by `filter` (an SQL condition), once the table asked is the
    /// one shown.
    pub(super) fn filter_to(&mut self, filter: Option<String>) {
        if let Some(filter) = filter {
            self.pending_where = Some(format!("{MENU_PREFIX}where:{filter}"));
            self.apply_pending_where();
        }
    }

    /// The filter a key's cell led to, once the grid knows its columns (of the table asked:
    /// none still waits to be listed).
    pub(super) fn apply_pending_where(&mut self) -> bool {
        let Some(grid) = &mut self.grid else { return false };
        if self.pending_where.is_none()
            || grid.source().columns().is_empty()
            || self.reselect.is_some()
        {
            return false;
        }
        let filter = self.pending_where.take().unwrap_or_default();
        grid.command(&filter)
    }

    /// The relations diagram of the view's source (around table `around`, by its place in
    /// the list), opened by the diagram's command.
    pub(super) fn show_diagram(&mut self, around: Option<usize>) -> bool {
        let Some(open) = &self.open else { return false };
        let sessions = Arc::clone(&open.sessions);
        let focus = around.and_then(|i| self.tables.get(i)).map(|t| t.table.clone());
        let access = match &self.source {
            Some((id, name)) => crate::diagram::SourceSchema::open(&sessions, *id, name, "", None),
            None => {
                let name = self
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let id = open.worker.source().id;
                crate::diagram::SourceSchema::open(
                    &sessions,
                    id,
                    &name,
                    &self.files.driver,
                    Some(&self.path),
                )
            }
        };
        match access {
            Ok(access) => {
                let (title, request) = crate::diagram::request(&sessions, access, None, focus);
                self.action = Some(ViewerAction::OpenView(crate::diagram::view(title, request)));
                true
            }
            Err(e) => {
                self.notify(Notification::error(message(&e)));
                false
            }
        }
    }

    /// The menu of a structure's row: change it, move it, drop it.
    pub(super) fn row_menu(&self, part: structure::Part, i: usize) -> Option<Vec<ViewMenuEntry>> {
        let can = self.schema_changes();
        let s = self.grid.as_ref()?.source().structure()?.clone();
        if self.table().is_some_and(|t| t.kind == TableKind::View) {
            return None;
        }
        let item = |id: String, key: &str| ViewMenuEntry::item(id, i18n::t(key));
        let mut entries = Vec::new();
        match part {
            structure::Part::Columns => {
                let column = s.columns.get(i)?;
                let name = &column.name;
                if can.alter_columns || can.renames {
                    entries.push(item(
                        format!("schema:alter-column:{name}"),
                        "schema.menu_alter_column",
                    ));
                }
                if can.reorder && s.columns.len() > 1 {
                    entries.push(item(
                        format!("schema:move-column:{name}"),
                        "schema.menu_move_column",
                    ));
                }
                if can.columns && s.columns.len() > 1 {
                    entries.push(
                        item(format!("schema:drop-column:{name}"), "schema.menu_drop_column")
                            .with_icon("trash"),
                    );
                }
            }
            structure::Part::Indexes => {
                let index = s.indexes.get(i)?;
                if can.indexes && !index.implicit {
                    entries.push(
                        item(format!("schema:drop-index:{}", index.name), "schema.menu_drop_index")
                            .with_icon("trash"),
                    );
                }
            }
            structure::Part::Keys => {
                if can.foreign_keys && s.foreign_keys.get(i).is_some() {
                    entries.push(
                        item(format!("schema:drop-key:{i}"), "schema.menu_drop_key")
                            .with_icon("trash"),
                    );
                }
            }
        }
        (!entries.is_empty()).then_some(entries)
    }

    /// A structure command (`schema:…`, from a menu or a button).
    pub(super) fn schema_command(&mut self, command: &str) -> bool {
        use crate::schema::Kind;
        let (what, arg) = command.split_once(':').unwrap_or((command, ""));
        // A table's own command from the list: that table shown first.
        if matches!(what, "rename-table" | "drop-table") && !arg.is_empty() {
            match self.tables.iter().position(|t| t.table.name == arg) {
                Some(i) => self.select(i),
                None => return false,
            }
        }
        let structure = self.grid.as_ref().and_then(|g| g.source().structure().cloned());
        let kind = match what {
            "create-table" => Kind::CreateTable,
            "rename-table" => Kind::RenameTable,
            "drop-table" => Kind::DropTable,
            "add-column" => Kind::AddColumn,
            "alter-column" => Kind::AlterColumn(arg.to_owned()),
            "drop-column" => Kind::DropColumn(arg.to_owned()),
            "move-column" => Kind::MoveColumn(arg.to_owned()),
            "create-index" => Kind::CreateIndex,
            "drop-index" => Kind::DropIndex(arg.to_owned()),
            "add-key" => Kind::AddForeignKey,
            "drop-key" => {
                let key = arg
                    .parse::<usize>()
                    .ok()
                    .and_then(|i| structure.as_ref()?.foreign_keys.get(i).cloned());
                match key {
                    Some(key) => Kind::DropForeignKey(key),
                    None => return false,
                }
            }
            _ => return false,
        };
        // What needs the table's structure waits for it.
        let needs = !matches!(kind, Kind::CreateTable | Kind::RenameTable | Kind::DropTable);
        if needs && structure.is_none() {
            return false;
        }
        self.open_schema(kind)
    }
}
