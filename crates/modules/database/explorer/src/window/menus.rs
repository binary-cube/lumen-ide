//! The tree's menus and the commands their rows run.

use super::*;

impl DatabaseWindow {
    /// The `+` menu: Data Source › the drivers (the recent ones first), then a file.
    pub fn add_menu(&self) -> Vec<ViewMenuEntry> {
        let all = database::drivers::all();
        let item = |f: &database::DriverFactory| {
            let icon = driver_icon(&f.id);
            let entry =
                ViewMenuEntry::item(format!("add:{}", f.id), f.name.clone()).with_icon(icon);
            // A program installed apart, speaking the drivers' protocol: marked so.
            if f.external { entry.with_shortcut(i18n::t("datasources.external")) } else { entry }
        };
        let mut drivers = Vec::new();
        if let Some(sessions) = &self.sessions {
            let configured = sessions.catalog().all();
            let recent = database::recent_drivers(&configured, RECENT);
            let recent: Vec<&database::DriverFactory> =
                recent.iter().filter_map(|id| all.iter().find(|f| f.id == **id)).collect();
            if !recent.is_empty() {
                drivers.extend(recent.into_iter().map(item));
                drivers.push(ViewMenuEntry::Separator);
            }
        }
        let mut sorted: Vec<&database::DriverFactory> = all.iter().collect();
        sorted.sort_by_key(|f| f.name.to_lowercase());
        drivers.extend(sorted.into_iter().map(item));
        vec![
            ViewMenuEntry::Submenu {
                label: i18n::t("datasources.menu_data_source"),
                icon: Some("database"),
                entries: drivers,
            },
            ViewMenuEntry::item("import", i18n::t("datasources.menu_import"))
                .with_icon("folder-input"),
            ViewMenuEntry::Separator,
            ViewMenuEntry::item("sources", i18n::t("datasources.menu_sources"))
                .with_icon("settings"),
        ]
    }

    /// A driver chosen in the `+` menu: a file-based one asks for its file, another adds a
    /// source not configured yet (its connection is set in the data sources' window).
    pub(super) fn add_driver(&mut self, driver: &str) {
        let Some(sessions) = self.sessions.clone() else { return };
        let Some(factory) = database::drivers::find(driver) else { return };
        let file_based = sessions.driver(driver).is_some_and(|d| d.capabilities().file_based);
        if file_based {
            self.adding = Some(Adding::Driver(driver.to_owned()));
            self.pick_file(&sessions);
            return;
        }
        // A server's: a new source of it in the data sources' modal (nothing kept until
        // it's applied there).
        let _ = factory;
        self.open_sources(None, Some(driver.to_owned()), None);
    }

    /// The right-click menu of `key`'s row (`None`: the window's own, under the rows).
    pub(super) fn menu_of(&self, key: Option<&NodeKey>) -> Vec<ViewMenuEntry> {
        let item = |id: &str, key: &str| ViewMenuEntry::item(id, i18n::t(key));
        let Some(key) = key else {
            let mut entries = self.add_menu();
            entries.push(ViewMenuEntry::Separator);
            entries.push(item("refresh-all", "datasources.menu_refresh").with_icon("refresh"));
            entries
                .push(item("collapse", "datasources.menu_collapse").with_icon("chevrons-collapse"));
            return entries;
        };
        if let NodeKey::Folder(id) = key {
            let production = self.tree.folders().iter().find(|f| f.id == *id).map(|f| f.production);
            let mark = item(&format!("folder-production:{id}"), "datasources.menu_production");
            let mark = if production == Some(true) { mark.with_icon("check") } else { mark };
            return vec![
                item(&format!("folder:{id}"), "datasources.menu_folder").with_icon("folder"),
                mark,
                ViewMenuEntry::Separator,
                item("collapse", "datasources.menu_collapse").with_icon("chevrons-collapse"),
            ];
        }
        let Some(source) = key.source().and_then(|id| self.source(id)) else { return Vec::new() };
        let can = source.schema_changes;
        let mut entries = Vec::new();
        let kind = self.row(key).map(|r| r.kind.clone());
        let structure = |entries: &mut Vec<ViewMenuEntry>| {
            if !entries.is_empty() {
                entries.push(ViewMenuEntry::Separator);
            }
        };
        match (&kind, key) {
            (Some(RowKind::Table(table_kind)), NodeKey::Table(..)) => {
                entries.push(item("open", "datasources.menu_open").with_icon("table"));
                entries.push(item("structure", "datasources.menu_structure"));
                entries.push(item("diagram", "datasources.menu_diagram").with_icon("relations"));
                if *table_kind == TableKind::Table && can.any() {
                    structure(&mut entries);
                    if can.columns {
                        entries.push(
                            item("schema:add-column", "schema.menu_add_column").with_icon("plus"),
                        );
                    }
                    if can.indexes {
                        entries.push(item("schema:create-index", "schema.menu_create_index"));
                    }
                    if can.foreign_keys {
                        entries.push(item("schema:add-key", "schema.menu_add_key"));
                    }
                    if can.renames && can.tables {
                        entries.push(item("schema:rename-table", "schema.menu_rename_table"));
                    }
                    if can.tables {
                        entries.push(
                            item("schema:drop-table", "schema.menu_drop_table").with_icon("trash"),
                        );
                    }
                }
            }
            (_, NodeKey::Column(_, _, _)) => {
                entries.push(item("open", "datasources.menu_open").with_icon("table"));
                if can.alter_columns || can.renames {
                    structure(&mut entries);
                    entries.push(item("schema:alter-column", "schema.menu_alter_column"));
                }
                if can.columns {
                    entries.push(
                        item("schema:drop-column", "schema.menu_drop_column").with_icon("trash"),
                    );
                }
            }
            (_, NodeKey::Index(..)) if can.indexes => {
                entries
                    .push(item("schema:drop-index", "schema.menu_drop_index").with_icon("trash"));
            }
            (_, NodeKey::Key(..)) if can.foreign_keys => {
                entries.push(item("schema:drop-key", "schema.menu_drop_key").with_icon("trash"));
            }
            (_, NodeKey::Part(_, _, Part::Columns)) if can.columns => {
                entries.push(item("schema:add-column", "schema.menu_add_column").with_icon("plus"));
            }
            (_, NodeKey::Part(_, _, Part::Indexes)) if can.indexes => {
                entries.push(
                    item("schema:create-index", "schema.menu_create_index").with_icon("plus"),
                );
            }
            (_, NodeKey::Part(_, _, Part::Keys)) if can.foreign_keys => {
                entries.push(item("schema:add-key", "schema.menu_add_key").with_icon("plus"));
            }
            (
                _,
                NodeKey::Source(_)
                | NodeKey::Database(_)
                | NodeKey::Schema(..)
                | NodeKey::Group(_, _, Group::Tables),
            ) if can.tables && self.tree.tables(source.id).is_some() => {
                entries.push(
                    item("schema:create-table", "schema.menu_create_table").with_icon("plus"),
                );
            }
            _ => {}
        }
        // A source's (a schema's) tables and their relations.
        if matches!(
            key,
            NodeKey::Source(_)
                | NodeKey::Database(_)
                | NodeKey::Schema(..)
                | NodeKey::Group(_, _, Group::Tables)
        ) && source.configured
        {
            entries.insert(0, item("diagram", "datasources.menu_diagram").with_icon("relations"));
            if entries.len() > 1 {
                entries.insert(1, ViewMenuEntry::Separator);
            }
        }
        // Its query consoles (a source writing SQL): "Query Console", a new one beside.
        if source.configured
            && !matches!(key, NodeKey::Status(_) | NodeKey::Notice(_))
            && self
                .sessions
                .as_ref()
                .is_some_and(|s| s.driver(&source.driver).is_some_and(|d| d.dialect().is_some()))
        {
            let mut console = item("console", "datasources.menu_console").with_icon("console");
            if let Some(keys) = ui_components::shortcuts::label(database::console::JUMP_COMMAND) {
                console = console.with_shortcut(keys);
            }
            let mut rows = vec![console, item("new-console", "datasources.menu_new_console")];
            // A source's consoles, listed to rename or delete them.
            if matches!(key, NodeKey::Source(_)) {
                rows.push(item("consoles", "datasources.menu_consoles"));
            }
            rows.push(ViewMenuEntry::Separator);
            for (i, row) in rows.into_iter().enumerate() {
                entries.insert(i, row);
            }
            if matches!(entries.last(), Some(ViewMenuEntry::Separator)) {
                entries.pop();
            }
        }
        // Copying names.
        let copy = !matches!(key, NodeKey::Status(_) | NodeKey::Notice(_));
        if copy {
            structure(&mut entries);
            entries.push(item("copy-name", "datasources.menu_copy_name").with_icon("copy"));
            if key.table().is_some() || matches!(key, NodeKey::Schema(..)) {
                entries.push(item("copy-qualified", "datasources.menu_copy_qualified"));
            }
        }
        structure(&mut entries);
        let mut refresh = item("refresh", "datasources.menu_refresh").with_icon("refresh");
        if let Some(keys) = ui_components::shortcuts::label(REFRESH.0) {
            refresh = refresh.with_shortcut(keys);
        }
        entries.push(refresh);
        if matches!(key, NodeKey::Source(_)) && source.scope.is_some() {
            entries.push(item("properties", "datasources.menu_properties").with_icon("settings"));
        }
        if matches!(key, NodeKey::Source(_)) {
            if source.scope.is_none() {
                entries.push(item("keep", "datasources.menu_keep").with_icon("plus"));
            } else {
                entries.push(ViewMenuEntry::Separator);
                entries.push(item("remove", "datasources.menu_remove").with_icon("trash"));
            }
        }
        entries
    }

    /// A row of its menus chosen.
    pub fn run(&mut self, id: &str) -> bool {
        self.refresh_rows();
        let key = self.selected.clone();
        match id {
            "import" => {
                if let Some(sessions) = self.sessions.clone() {
                    self.adding = Some(Adding::Any);
                    self.pick_file(&sessions);
                }
            }
            id if id.starts_with("add:") => self.add_driver(&id["add:".len()..]),
            id if id.starts_with("folder-production:") => {
                if let Some(folder) = database::FolderId::parse(&id["folder-production:".len()..]) {
                    self.toggle_production(folder);
                }
            }
            id if id.starts_with("folder:") => {
                if let Some(folder) = database::FolderId::parse(&id["folder:".len()..]) {
                    self.open_folder(folder);
                }
            }
            "refresh-all" => self.refresh_asked(None),
            "sources" => self.open_sources(key.as_ref().and_then(NodeKey::source), None, None),
            "properties" => {
                if let Some(source) = key.as_ref().and_then(NodeKey::source) {
                    self.open_sources(Some(source), None, None);
                }
            }
            "refresh" => self.refresh_asked(key.as_ref().and_then(NodeKey::source)),
            "collapse" => self.collapse_all(),
            "open" => {
                if let Some(key) = key {
                    self.open_table(&key, false);
                }
            }
            "structure" => {
                if let Some(key) = key {
                    self.open_table(&key, true);
                }
            }
            "diagram" => {
                if let Some(key) = key {
                    self.show_diagram(&key);
                }
            }
            "console" | "new-console" => self.console(key.as_ref(), id == "new-console"),
            "consoles" => self.console_listed(key.as_ref(), false, true),
            "copy-name" | "copy-qualified" => {
                if let Some(text) =
                    key.as_ref().and_then(|k| self.name_of(k, id == "copy-qualified"))
                {
                    self.action = Some(ToolPanelAction::View(ViewerAction::Request(
                        AppRequest::CopyText(text),
                    )));
                }
            }
            "keep" => {
                if let Some(source) = key.as_ref().and_then(NodeKey::source) {
                    self.keep_file(source);
                }
            }
            "remove" => {
                if let Some(source) = key.as_ref().and_then(NodeKey::source) {
                    self.remove(source);
                }
            }
            id if id.starts_with("schema:") => {
                if let Some(key) = key {
                    self.schema(&key, &id["schema:".len()..]);
                }
            }
            _ => return false,
        }
        self.stale = true;
        true
    }

    /// The name of `key`'s node, or its qualified one (`schema.table`, `table.column`).
    pub fn name_of(&self, key: &NodeKey, qualified: bool) -> Option<String> {
        let row = self.row(key)?;
        let table = key.table();
        Some(match (key, qualified) {
            (NodeKey::Source(_) | NodeKey::Database(_), _) => row.label.clone(),
            (NodeKey::Table(_, t), true) => t.to_string(),
            (NodeKey::Column(_, t, c), true) => format!("{t}.{c}"),
            (NodeKey::Index(_, t, i), true) => match &t.schema {
                Some(schema) => format!("{schema}.{i}"),
                None => i.clone(),
            },
            (NodeKey::Part(..) | NodeKey::Key(..), true) => table?.to_string(),
            _ => row.label.clone(),
        })
    }
}
