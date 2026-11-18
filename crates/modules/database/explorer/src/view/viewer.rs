//! The data view as a viewer.

use super::*;

impl Viewer for DataView {
    /// Its source's color (its own, else its folder's); none for a file shown by itself
    /// (no source kept for it).
    fn accent(&self, palette: &ui_components::Palette) -> Option<kernel::Rgba> {
        let open = self.open.as_ref()?;
        let sessions = &open.sessions;
        let color = self.color.get(sessions.catalog(), |catalog| {
            let id = match &self.source {
                Some((id, _)) => *id,
                // A file: the source kept for it, if there's one.
                None => catalog.source_of_file(sessions.root(), &self.path)?.id,
            };
            catalog.color_of(id)
        })?;
        ui_components::tag_colors::named(&color.name(), palette)
    }

    fn memory(&self) -> ViewMemory {
        let ram = self.grid.as_ref().map_or(0, |g| g.source().memory());
        ViewMemory { ram: ram as u64, gpu: 0 }
    }

    fn set_data(&mut self, data: &dyn std::any::Any) {
        if let Some(restored) = data.downcast_ref::<Restored>() {
            self.project = restored.id;
        } else if let Some(sizes) = data.downcast_ref::<workbench_model::LayoutSizes>() {
            // As wide as it was last dragged in the project.
            if let Some(width) = sizes.get(LIST_SIZE) {
                self.list_width = width.clamp(LIST_MIN, LIST_MAX);
            }
            self.sizes = Some(sizes.clone());
        } else if let Some(asked) = data.downcast_ref::<FileTable>() {
            self.show_asked(asked);
        } else if let Some(asked) = data.downcast_ref::<OpenSource>() {
            // Another source than the one shown (a tab of the same name): started again.
            if self.source_id().is_some_and(|id| id != asked.source) {
                self.keep_layout();
                self.open = None;
                self.grid = None;
                self.tables.clear();
                self.selected = None;
                self.error = None;
            }
            self.project = Some(asked.project);
            self.source = Some((asked.source, asked.name.clone()));
            if let Some(table) = &asked.table {
                self.show_table(table.clone());
            }
            if asked.structure {
                self.tab = Tab::Structure;
                self.point_at(asked.column.clone());
            }
            self.filter_to(asked.filter.clone());
        }
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        if let Some(waker) = &ctx.waker {
            *self.waker.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::clone(waker));
        }
        self.start();
        self.hits.clear();
        self.tips.clear();
        self.paint_view(ctx);
    }

    fn poll(&mut self) -> bool {
        let mut changed = self.take_replies();
        changed |= self.file_changed();
        changed |= self.follow_settings();
        changed |= self.take_answer();
        changed |= self.take_schema_answer();
        changed |= self.restructured();
        // The text has unsaved changes: the data can't be edited (they'd write over
        // each other).
        let blocked = (self.files.text && self.unsaved).then(|| i18n::t("database.ro_unsaved"));
        if let Some(grid) = &mut self.grid {
            grid.source_mut().block(blocked);
            changed |= grid.poll();
        }
        changed |= self.follow_grid_total();
        changed |= self.apply_pending_where();
        self.take_source_events();
        changed |= self.take_outcome();
        changed |= self.follow_export();
        self.follow_layout();
        changed
    }

    fn is_loading(&self) -> bool {
        self.tables_loading || self.grid.as_ref().is_some_and(DataGrid::is_loading)
    }

    fn working(&self) -> bool {
        self.is_loading() || self.export.is_some()
    }

    fn is_animating(&self) -> bool {
        self.splitter.is_animating()
    }

    fn moved(&mut self, path: &Path) {
        // A configured source's view has no file of its own.
        if self.file().is_none() {
            return;
        }
        let old = std::mem::replace(&mut self.path, path.to_path_buf());
        // Its session opens again at the new path (the sessions follow it: the project's
        // heard it on its bus, a view's own are told here), everything read again from
        // there; the disk's changes are those of the new file.
        let wake = self.wake();
        if let Some(open) = &mut self.open
            && open._watch.is_some()
        {
            open.sessions.file_moved(&old, path);
            open._watch = Some(watch_file(&open.sessions, &open.changed, &wake, path));
        }
        self.reload();
    }

    fn reads_file(&self) -> bool {
        self.files.text
    }

    fn opens_alone(&self) -> bool {
        self.files.text
    }

    /// The grid beside a CSV's text takes the keys once clicked.
    fn keys_in_split(&self) -> bool {
        true
    }

    fn set_unsaved(&mut self, unsaved: bool) {
        self.unsaved = unsaved;
    }

    fn pointer_down(&mut self, p: Point, clicks: u32, mods: PressMods) -> Option<ViewerAction> {
        if self.splitter.begin_drag(p) {
            return None;
        }
        // The tables' or the structure's scrollbar (when shown).
        if let Some(offset) = self.list_bar.press(p) {
            self.list_scroll = offset;
            return None;
        }
        if self.tab == Tab::Structure
            && let Some(offset) = self.structure_bar.press(p)
        {
            self.structure_scroll = offset;
            return None;
        }
        match self.hit(p) {
            Some(Hit::Table(i)) => {
                self.select(i);
                return None;
            }
            Some(Hit::Tab(tab)) => {
                self.tab = tab;
                return None;
            }
            Some(Hit::ExportCancel) => {
                if let Some(export) = &self.export {
                    export.cancel();
                }
                return None;
            }
            Some(Hit::Add(part)) => {
                let command = match part {
                    structure::Part::Columns => "add-column",
                    structure::Part::Indexes => "create-index",
                    structure::Part::Keys => "add-key",
                };
                self.schema_command(command);
                return self.action.take();
            }
            // A column double-clicked: changed.
            Some(Hit::Row(structure::Part::Columns, i)) if clicks == 2 => {
                let name = self
                    .grid
                    .as_ref()
                    .and_then(|g| g.source().structure()?.columns.get(i).map(|c| c.name.clone()));
                let can = self.schema_changes();
                if let Some(name) = name
                    && (can.alter_columns || can.renames)
                {
                    self.schema_command(&format!("alter-column:{name}"));
                }
                return self.action.take();
            }
            Some(Hit::Row(..)) => return None,
            Some(Hit::Console) => {
                self.console(None, false);
                return self.action.take();
            }
            None => {}
        }
        if self.in_grid(p)
            && let Some(grid) = &mut self.grid
        {
            grid.press(p, clicks, mods);
        }
        self.take_grid_action();
        self.action.take()
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        self.pointer_down(p, 1, PressMods::default())
    }

    fn drag(&mut self, p: Point) -> bool {
        if self.splitter.is_dragging() {
            if let Some(x) = self.splitter.drag_to(p) {
                let left = self.list_area.min_x();
                let width = (x - left) / self.scale().max(0.01);
                self.list_width = width.clamp(LIST_MIN, LIST_MAX).round();
                // The project's: its next data views (and its next session) as wide.
                if let Some(sizes) = &self.sizes {
                    sizes.set(LIST_SIZE, self.list_width);
                }
            }
            return true;
        }
        if let Some(offset) = self.list_bar.drag(p) {
            self.list_scroll = offset;
            return true;
        }
        if let Some(offset) = self.structure_bar.drag(p) {
            self.structure_scroll = offset;
            return true;
        }
        self.grid.as_mut().is_some_and(|g| g.drag(p))
    }

    /// Its scrollbars fading: a frame when they next change.
    fn frame_deadline(&self) -> Option<std::time::Instant> {
        [self.list_bar.next_frame(), self.structure_bar.next_frame()].into_iter().flatten().min()
    }

    fn release(&mut self) -> bool {
        if self.splitter.end_drag() | self.list_bar.release() | self.structure_bar.release() {
            return true;
        }
        let released = self.grid.as_mut().is_some_and(DataGrid::release);
        self.follow_layout();
        released
    }

    /// Its hover says when what it shows changed.
    fn hover_tells_changes(&self) -> bool {
        true
    }

    fn hover(&mut self, p: Point) -> bool {
        self.last_hover = p;
        let hover = self.hit(p);
        let mut changed = hover != self.hover;
        // A table under the mouse: counted exactly (unless huge), its estimate until then.
        if changed && let Some(Hit::Table(i)) = hover {
            self.counts.want(i, self.open.as_ref().map(|o| &o.client), &self.tables);
        }
        self.hover = hover;
        changed |= self.splitter.hover(p) | self.list_bar.hover(p);
        if self.tab == Tab::Structure {
            changed |= self.structure_bar.hover(p);
        }
        if let Some(grid) = &mut self.grid {
            changed |= grid.hover(p);
        }
        changed
    }

    fn tooltip_at(&self, p: Point) -> Option<ui_components::Tip> {
        use ui_components::Tip;
        if let Some((r, label)) = self.tips.iter().rev().find(|(r, _)| r.contains(p)) {
            return Some(Tip::new(*r, label.clone()));
        }
        let found = self.hits.iter().rev().find(|(r, _)| r.contains(p));
        if let Some(&(r, hit)) = found {
            return match hit {
                Hit::Tab(tab) => Some(Tip::new(
                    r,
                    i18n::t(match tab {
                        Tab::Data => "database.tip_tab_data",
                        Tab::Structure => "database.tip_tab_structure",
                    }),
                )),
                Hit::Console => {
                    Some(Tip::new(r, i18n::t("datasources.tooltip_console")).with_shortcut(
                        ui_components::shortcuts::label(database::console::JUMP_COMMAND),
                    ))
                }
                Hit::Table(i) => {
                    let info = self.tables.get(i)?;
                    let kind = i18n::t(if info.kind == TableKind::View {
                        "database.kind_view"
                    } else {
                        "database.kind_table"
                    });
                    let label = match self.counts.get(i) {
                        Some(c) => i18n::tr!(
                            if c.exact {
                                "database.tip_table_rows"
                            } else {
                                "database.tip_table_rows_estimate"
                            },
                            table = info.table.to_string(),
                            kind = kind,
                            count = c.rows,
                            rows = ui_components::data_grid::grouped(c.rows)
                        ),
                        None => i18n::tr!(
                            "database.tip_table",
                            table = info.table.to_string(),
                            kind = kind
                        ),
                    };
                    Some(Tip::new(r, label))
                }
                _ => None,
            };
        }
        if self.in_grid(p) {
            return self.grid.as_ref().and_then(|g| g.tooltip_at(p));
        }
        None
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        if self.splitter.contains(p) || self.splitter.is_dragging() {
            return Some(ViewerCursor::ResizeHorizontal);
        }
        if self.hit(p).is_some() {
            return Some(ViewerCursor::Pointer);
        }
        if self.in_grid(p) {
            return self.grid.as_ref().and_then(|g| g.cursor_at(p));
        }
        None
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        let at = self.hover_point();
        if self.list_area.contains(at) {
            self.list_scroll = (self.list_scroll + dy).max(0.0);
            return true;
        }
        if self.tab == Tab::Structure {
            self.structure_scroll = (self.structure_scroll + dy).max(0.0);
            return true;
        }
        self.grid.as_mut().is_some_and(|g| g.scroll(dx, dy))
    }

    fn wants_text(&self) -> bool {
        self.tab == Tab::Data && self.grid.as_ref().is_some_and(DataGrid::wants_text)
    }

    fn text_input(&mut self, text: &str) -> bool {
        self.grid.as_mut().is_some_and(|g| g.text_input(text))
    }

    /// A character typed on the grid (no field has the keys): the cell's editor starts.
    fn key(&mut self, c: char) -> bool {
        self.tab == Tab::Data && self.grid.as_mut().is_some_and(|g| g.type_char(c))
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        self.edit_key_with(key, KeyReach::default())
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        if self.tab != Tab::Data {
            return false;
        }
        self.grid.as_mut().is_some_and(|g| g.edit_key(key, reach))
    }

    fn select_all(&mut self) -> bool {
        self.tab == Tab::Data && self.grid.as_mut().is_some_and(DataGrid::select_all)
    }

    fn selected_text(&self) -> Option<String> {
        self.grid.as_ref().filter(|_| self.tab == Tab::Data).and_then(DataGrid::selected_text)
    }

    fn context_menu(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        match self.hit(p) {
            Some(Hit::Table(i)) => return self.list_menu(Some(i)),
            Some(Hit::Row(part, i)) => return self.row_menu(part, i),
            _ => {}
        }
        if self.list_area.contains(p) {
            return self.list_menu(None);
        }
        if !self.in_grid(p) {
            return None;
        }
        let mut entries = self.grid.as_mut()?.context_menu(p)?;
        // A key's cell (the engine's, or one accepted in the relations diagram): its row.
        if self.reference().is_some() {
            entries.push(ViewMenuEntry::Separator);
            entries.push(
                ViewMenuEntry::item("goto-ref", i18n::t("database.goto_reference"))
                    .with_icon("link"),
            );
        }
        Some(entries)
    }

    fn command(&mut self, id: &str) -> bool {
        let used = match id {
            "find" if self.tab == Tab::Data => self.grid.as_mut().is_some_and(|g| {
                g.open_find();
                true
            }),
            "refresh" => {
                self.reload();
                true
            }
            "submit" => self.grid.as_mut().is_some_and(DataGrid::submit),
            // The text field having the keys (⌘A, ⌘Z, ⇧⌘Z, ⌘X): its own.
            "select_all" if self.tab == Tab::Data => {
                self.grid.as_mut().is_some_and(DataGrid::select_all)
            }
            "undo" | "redo" if self.tab == Tab::Data => {
                self.grid.as_mut().is_some_and(|g| g.undo(id == "redo"))
            }
            "cut" if self.tab == Tab::Data => {
                self.grid.as_mut().is_some_and(|g| g.wants_text() && g.cut().is_some())
            }
            "goto-ref" => self.go_to_reference(),
            "console" => self.console(None, false),
            "new-console" => self.console(None, true),
            id if id.starts_with("console:") => {
                let at = id["console:".len()..].parse().ok();
                self.console(at, false)
            }
            "diagram" => self.show_diagram(None),
            id if id.starts_with("diagram:") => {
                let at = id["diagram:".len()..].parse().ok();
                self.show_diagram(at)
            }
            id if id.starts_with("schema:") => self.schema_command(&id["schema:".len()..]),
            // A snapshot: an export of the table shown, midway (`rows` of the total).
            #[cfg(feature = "snapshot")]
            id if id.starts_with("snapshot-export:") => {
                let rows = id["snapshot-export:".len()..].parse().unwrap_or(0);
                self.hold_export(rows)
            }
            // A snapshot: where a toolbar's button is (its middle), to rest the mouse on it.
            #[cfg(feature = "snapshot")]
            id if id.starts_with("snapshot-point:") => {
                let tool = match &id["snapshot-point:".len()..] {
                    "delete" => Tool::Delete,
                    "refresh" => Tool::Refresh,
                    _ => Tool::More,
                };
                let area = self
                    .grid
                    .as_ref()
                    .and_then(|g| g.tool_area(tool).or_else(|| g.tool_area(Tool::More)));
                *SNAPSHOT_POINT.lock().unwrap_or_else(PoisonError::into_inner) = area.map(|r| {
                    Point::new(r.min_x() + r.size.width / 2.0, r.min_y() + r.size.height / 2.0)
                });
                area.is_some()
            }
            "export-cancel" => self.export.as_ref().is_some_and(|e| {
                e.cancel();
                true
            }),
            id if id.starts_with("picked-save:") => match self.export_format.take() {
                Some(chosen) => {
                    // Chosen (a snapshot gives it directly): the dialog isn't asked again.
                    if matches!(
                        self.action,
                        Some(ViewerAction::Request(AppRequest::PickSavePath { .. }))
                    ) {
                        self.action = None;
                    }
                    let path = PathBuf::from(&id["picked-save:".len()..]);
                    let format =
                        crate::export::format_of(&path, self.project, &chosen).unwrap_or(chosen);
                    self.start_export(format, crate::export::Target::File(path));
                    true
                }
                None => false,
            },
            "data" => {
                self.tab = Tab::Data;
                true
            }
            "structure" => {
                self.tab = Tab::Structure;
                true
            }
            id if id.starts_with("table:") => {
                let name = &id["table:".len()..];
                match self.tables.iter().position(|t| t.table.name == name) {
                    Some(i) => {
                        self.select(i);
                        true
                    }
                    None => false,
                }
            }
            id if id.starts_with(MENU_PREFIX) || id == "transpose" => {
                self.grid.as_mut().is_some_and(|g| g.command(id))
            }
            _ => false,
        };
        self.take_grid_action();
        self.follow_layout();
        used
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }

    fn take_notifications(&mut self) -> Vec<Notification> {
        std::mem::take(&mut self.told)
    }

    fn purge(&mut self) {
        // The rows kept go: what's in view is read again.
        if let Some(grid) = &mut self.grid {
            grid.refresh();
        }
        // Which file each path names is looked up again (a link may point elsewhere now).
        if let Some(open) = &self.open {
            open.sessions.identities().clear();
        }
    }

    fn status(&self) -> Option<String> {
        let info = self.table()?;
        let rows = self.grid.as_ref().and_then(|g| g.source().total().known());
        Some(match rows {
            Some(n) => i18n::tr!("database.status", table = info.table.to_string(), count = n),
            None => info.table.to_string(),
        })
    }
}
