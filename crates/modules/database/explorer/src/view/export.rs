//! Exporting and submitting what changed.

use super::*;

impl DataView {
    /// Where every row goes: the system's "Save As", at the file's folder, the table's
    /// name with the format's extension filled in.
    pub(super) fn ask_export(&mut self, format: ExportFormat) {
        let Some(info) = self.table() else { return };
        let name = format!("{}.{}", info.table.name, format.extension());
        let start = self.path.parent().map(Path::to_path_buf);
        self.export_format = Some(format);
        // The path picked now is the export's (a pane's "Save As" cancelled before it isn't
        // waiting any more: a cancel sends nothing back).
        self.pane_saving = false;
        self.action = Some(ViewerAction::Request(AppRequest::PickSavePath { start, name }));
    }

    /// Every row of the table shown (its order and filter, the columns as shown) written
    /// to `target` in the background (one export at a time).
    pub(crate) fn start_export(&mut self, format: ExportFormat, target: crate::export::Target) {
        if self.export.is_some() {
            return;
        }
        let (Some(open), Some(grid), Some(info)) = (&self.open, &self.grid, self.table()) else {
            return;
        };
        let total = grid.source().total().known();
        if target == crate::export::Target::Clipboard
            && total.is_some_and(|n| n > crate::export::CLIPBOARD_ROWS)
        {
            let limit = crate::export::CLIPBOARD_ROWS;
            self.notify(Notification::warning(i18n::tr!(
                "database.export_clipboard_limit",
                count = limit
            )));
            return;
        }
        let columns = grid.source().columns();
        let shown = grid.shown_columns().into_iter().filter_map(|c| columns.get(c).cloned());
        let (order, filter) = grid.source().query();
        let request = crate::export::ExportRequest {
            table: info.table.clone(),
            columns: shown.collect(),
            order,
            filter,
            format,
            sql: grid.source().sql(),
            options: extractors::catalog().options(),
            target,
            total,
        };
        let log: Arc<dyn crate::export::ActivityLog> = open.sessions.clone();
        let export = crate::export::Export::start(
            &open.worker,
            open.sessions.runtime().clone(),
            request,
            self.wake(),
            Some(log),
        );
        self.export_seen = 0;
        self.export = Some(export);
    }

    /// An export shown midway (a snapshot of its progress).
    #[cfg(feature = "snapshot")]
    pub(super) fn hold_export(&mut self, rows: u64) -> bool {
        let (Some(grid), Some(info)) = (&self.grid, self.table()) else { return false };
        let name = format!("{}.csv", info.table.name);
        let request = crate::export::ExportRequest {
            table: info.table.clone(),
            columns: grid.source().columns().to_vec(),
            order: Vec::new(),
            filter: None,
            format: ExportFormat::find(None, "csv").unwrap_or(ExportFormat::Xlsx),
            sql: None,
            options: extractors::Options::default(),
            target: crate::export::Target::File(PathBuf::from(name)),
            total: grid.source().total().known(),
        };
        self.export = Some(crate::export::Export::held(request, rows));
        true
    }

    /// How the export is going: drawn again as rows are written; its end told (copied rows
    /// put on the clipboard).
    pub(super) fn follow_export(&mut self) -> bool {
        let Some(export) = &self.export else { return false };
        let Some(outcome) = export.take_outcome() else {
            let rows = export.rows();
            return std::mem::replace(&mut self.export_seen, rows) != rows;
        };
        self.export = None;
        let (told, copied) = outcome.told();
        if let Some(text) = copied {
            self.action = Some(ViewerAction::Request(AppRequest::CopyText(text)));
        }
        self.notify(told);
        true
    }

    /// The pending changes shown first (the statements that will run, or a line per change
    /// for a source that writes none), in the module's modal; run once it says yes.
    pub(super) fn ask_submit(&mut self) {
        let Some(source) = self.grid.as_ref().map(DataGrid::source) else { return };
        let Some(set) = source.change_set() else { return };
        let (lines, sql) = match source.preview(&set) {
            Ok(statements) if !statements.is_empty() => (statements, true),
            Ok(_) => (crate::edits::summary(&set), false),
            Err(e) => {
                self.notify(Notification::error(message(&e)));
                return;
            }
        };
        let answer = Arc::new(Mutex::new(None));
        let request = crate::preview::PreviewRequest {
            table: source.table().to_string(),
            lines,
            sql,
            changes: source.edits().count(),
            deletes: set.deletes(),
            confirm: crate::settings::confirm_delete(),
            answer: Arc::clone(&answer),
            wake: self.wake(),
        };
        self.asked = Some((set, answer));
        self.action = Some(ViewerAction::OpenView(ui_components::OpenView::modal(
            i18n::t("database.preview_title"),
            crate::preview::EXTENSION,
            request,
        )));
    }

    /// The preview's answer: yes, the changes run (in one transaction).
    pub(super) fn take_answer(&mut self) -> bool {
        let Some((_, answer)) = &self.asked else { return false };
        let Some(yes) = *answer.lock().unwrap_or_else(PoisonError::into_inner) else {
            return false;
        };
        let Some((set, _)) = self.asked.take() else { return false };
        if yes && let Some(grid) = &mut self.grid {
            grid.source_mut().submit(set);
        }
        true
    }

    /// How a submit ended, told (refused: every change kept).
    pub(super) fn take_outcome(&mut self) -> bool {
        let Some(outcome) = self.grid.as_mut().and_then(|g| g.source_mut().take_outcome()) else {
            return false;
        };
        let told = match outcome {
            Ok(applied) => {
                self.note_own();
                let mut text = i18n::tr!("database.submitted", count = applied.affected);
                if !applied.inserted.is_empty() {
                    let keys: Vec<String> = applied.inserted.iter().map(key_text).collect();
                    text.push_str(" · ");
                    text.push_str(&i18n::tr!("database.inserted_keys", keys = keys.join(", ")));
                }
                Notification::success(text)
            }
            Err(DbError::Conflict(why)) => {
                Notification::error(i18n::tr!("database.conflict", why = why.as_str()))
            }
            Err(e) => {
                // A console's transaction holding the file / the rows: said as such.
                let why = match self.open.as_ref() {
                    Some(open) => open.sessions.refusal(self.source_id(), &e),
                    None => message(&e),
                };
                Notification::error(i18n::tr!("database.submit_failed", why = why))
            }
        };
        self.notify(told);
        true
    }
}
