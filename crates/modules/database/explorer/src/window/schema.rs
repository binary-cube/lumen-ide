//! The schema window of a source.

use super::*;

impl DatabaseWindow {
    /// A structure's modal for `key`'s node (`what`: the menu's action), on its source's
    /// worker: what needs the table's structure waits for it.
    pub(super) fn schema(&mut self, key: &NodeKey, what: &str) {
        use crate::schema::Kind;
        let Some(id) = key.source() else { return };
        let table = key.table().cloned();
        let kind = match (what, key) {
            ("create-table", _) => Kind::CreateTable,
            ("rename-table", _) => Kind::RenameTable,
            ("drop-table", _) => Kind::DropTable,
            ("add-column", _) => Kind::AddColumn,
            ("create-index", _) => Kind::CreateIndex,
            ("add-key", _) => Kind::AddForeignKey,
            ("alter-column", NodeKey::Column(_, _, c)) => Kind::AlterColumn(c.clone()),
            ("drop-column", NodeKey::Column(_, _, c)) => Kind::DropColumn(c.clone()),
            ("drop-index", NodeKey::Index(_, _, i)) => Kind::DropIndex(i.clone()),
            ("drop-key", NodeKey::Key(_, t, n)) => {
                match self.tree.structure(id, t).and_then(|s| s.foreign_keys.get(*n)) {
                    Some(fk) => Kind::DropForeignKey(fk.clone()),
                    None => return,
                }
            }
            _ => return,
        };
        let schema = match key {
            NodeKey::Schema(_, s) | NodeKey::Group(_, Some(s), _) => Some(s.clone()),
            _ => None,
        };
        if kind != Kind::CreateTable {
            let Some(table) = table else { return };
            if self.tree.structure(id, &table).is_none() {
                // Read first; the modal opens once it's here.
                if let Some(need) = self.tree.expand(&NodeKey::Table(id, table.clone())) {
                    self.ask(vec![need]);
                }
                self.waiting_schema = Some((id, table, kind));
                return;
            }
            self.open_schema(id, Some(table), None, kind);
        } else {
            self.open_schema(id, None, schema, kind);
        }
    }

    /// The structure's modal waiting for a table's structure, opened now that it's here.
    pub(super) fn open_waiting_schema(&mut self) {
        let Some((id, table, _)) = &self.waiting_schema else { return };
        if self.tree.structure(*id, table).is_none() {
            return;
        }
        if let Some((id, table, kind)) = self.waiting_schema.take() {
            self.open_schema(id, Some(table), None, kind);
        }
    }

    /// Opens the structure's modal (through the app: the module's command).
    pub(super) fn open_schema(
        &mut self,
        id: DataSourceId,
        table: Option<TableRef>,
        schema: Option<String>,
        kind: crate::schema::Kind,
    ) {
        let Some(sessions) = self.sessions.clone() else { return };
        let worker = match sessions.get(id).map(Ok).unwrap_or_else(|| sessions.source_worker(id)) {
            Ok(worker) => worker,
            Err(e) => {
                self.notify(workbench_model::Notification::error(message(&e)));
                return;
            }
        };
        let source = worker.source();
        let can = if source.read_only {
            database::SchemaChanges::default()
        } else {
            source.capabilities.schema_changes
        };
        let tables = self.tree.tables(id).map(<[_]>::to_vec).unwrap_or_default();
        let rows = tables.iter().filter_map(|t| t.rows.map(|n| (t.table.clone(), n))).collect();
        let structure = table.as_ref().and_then(|t| self.tree.structure(id, t)).cloned();
        // SQLite's own schema is the default: a table goes there unless another is named.
        let schema = schema.filter(|s| s != "main");
        let ctx = crate::schema::Context {
            table,
            schema,
            structure,
            tables,
            rows,
            can,
            dialect: source.dialect.clone(),
        };
        let answer: crate::schema::Answer = Arc::new(Mutex::new(None));
        let request = crate::schema::SchemaRequest {
            kind,
            ctx,
            worker: worker.clone(),
            answer: Arc::clone(&answer),
            wake: self.wake(),
        };
        self.schema_answer = Some(answer);
        self.action = Some(ToolPanelAction::View(ViewerAction::OpenView(
            ui_components::OpenView::modal(request.kind.title(), crate::schema::EXTENSION, request),
        )));
    }

    /// The structure's modal made its changes: told (the tree reads them again on the
    /// bus's word).
    pub(super) fn take_schema_answer(&mut self) -> bool {
        let Some(answer) = &self.schema_answer else { return false };
        let Some(done) = crate::schema::take_answer(answer) else { return false };
        self.schema_answer = None;
        self.notify(workbench_model::Notification::success(i18n::tr!(
            "schema.done",
            count = done.applied.affected
        )));
        true
    }
}
