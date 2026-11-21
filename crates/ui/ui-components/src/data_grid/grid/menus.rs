//! The grid's right-click menus and the commands their rows run.

use super::*;

impl<S: GridSource> DataGrid<S> {
    /// The menu of a right click at `p` (`None`: not on the rows or the headers). What's
    /// under it gets selected first, unless it's selected already.
    pub fn context_menu(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        use i18n::t;
        let item =
            |id: String, key: &str| ViewMenuEntry::item(format!("{MENU_PREFIX}{id}"), t(key));
        let caps = self.source.capabilities();
        if self.in_header(p) {
            let (pos, _) = self.column_at(p.x)?;
            let col = *self.layout.visible().get(pos)?;
            self.menu_target = Some(MenuTarget::Header(col));
            let mut entries = Vec::new();
            if caps.sort {
                entries
                    .push(item(format!("sort-asc:{col}"), "grid.sort_asc").with_icon("arrow-up"));
                entries.push(
                    item(format!("sort-desc:{col}"), "grid.sort_desc").with_icon("arrow-down"),
                );
                if !self.query.sort.is_empty() {
                    entries.push(item("sort-clear".into(), "grid.sort_clear"));
                }
                entries.push(ViewMenuEntry::Separator);
            }
            entries.push(item(format!("select-column:{col}"), "grid.select_column"));
            entries.push(item(format!("fit:{col}"), "grid.fit_column"));
            entries.push(item(format!("hide:{col}"), "grid.hide_column").with_icon("eye-off"));
            entries.push(self.columns_menu());
            entries.push(ViewMenuEntry::Separator);
            entries.extend(export_menu(self.project));
            return Some(entries);
        }
        if !self.in_body(p) {
            return None;
        }
        let cell = if self.transpose {
            let row = self.record_row()?;
            let g = self.geometry;
            let i = ((p.y - g.body.min_y() + self.scroll_y) / g.row_h.max(1.0)).floor();
            (i >= 0.0 && (i as usize) < self.layout.visible().len())
                .then(|| CellPos::new(row, i as usize))?
        } else {
            self.cell_at(p, false)?
        };
        if !self.selection.contains(cell) {
            self.selection.set(cell);
            self.aggregates = None;
        }
        self.menu_target = Some(MenuTarget::Cell(cell));
        let copy_as = ViewMenuEntry::Submenu {
            label: t("grid.copy_as"),
            icon: Some("copy"),
            entries: extractor_entries(self.project, "copy", false, false),
        };
        let mut entries = vec![
            {
                let chosen = self.extractor();
                let copy = ViewMenuEntry::item(
                    format!("{MENU_PREFIX}copy:{}", chosen.id()),
                    i18n::tr!("grid.copy_with", name = chosen.name()),
                )
                .with_icon("copy");
                match crate::shortcuts::label("edit.copy") {
                    Some(keys) => copy.with_shortcut(keys),
                    None => copy,
                }
            },
            copy_as,
        ];
        if caps.filter {
            let shown = self.cell_value(cell).and_then(|v| filter_value_label(&v));
            let mut filters = Vec::new();
            if let Some(text) = shown {
                filters.push(ViewMenuEntry::item(
                    format!("{MENU_PREFIX}filter-eq"),
                    i18n::tr!("grid.filter_eq", value = text.as_str()),
                ));
                filters.push(ViewMenuEntry::item(
                    format!("{MENU_PREFIX}filter-ne"),
                    i18n::tr!("grid.filter_ne", value = text.as_str()),
                ));
            }
            filters.push(item("filter-null".into(), "grid.filter_null"));
            filters.push(item("filter-not-null".into(), "grid.filter_not_null"));
            if self.query.filter.is_some() {
                filters.push(ViewMenuEntry::Separator);
                filters.push(item("filter-clear".into(), "grid.filter_clear"));
            }
            entries.push(ViewMenuEntry::Separator);
            entries.push(ViewMenuEntry::Submenu {
                label: t("grid.filter"),
                icon: Some("filter"),
                entries: filters,
            });
        }
        if self.can_edit() {
            entries.push(ViewMenuEntry::Separator);
            entries.push(item("edit".into(), "grid.edit_cell").with_icon("file-pen"));
            entries.push(item("edit-big".into(), "grid.edit_big").with_icon("maximize-2"));
            entries.push(item("set-null".into(), "grid.set_null").with_icon("x-square"));
            entries.push(item("add-row".into(), "grid.add_row").with_icon("plus"));
            entries.push(item("clone-row".into(), "grid.clone_row").with_icon("copy"));
            entries.push(item("delete-rows".into(), "grid.delete_rows").with_icon("trash"));
        }
        if self.source.pending() > 0 {
            entries.push(item("revert".into(), "grid.revert_selected").with_icon("undo"));
            entries.push(item("revert-all".into(), "grid.revert_all").with_icon("rotate-ccw"));
        }
        entries.push(ViewMenuEntry::Separator);
        entries.push(item("select-row".into(), "grid.select_row").with_icon("rows"));
        entries.push(
            item(
                "transpose".into(),
                if self.transpose { "grid.untranspose" } else { "grid.transpose" },
            )
            .with_icon("transpose"),
        );
        entries.push(item("refresh".into(), "grid.refresh").with_icon("refresh"));
        Some(entries)
    }

    /// "Columns ›": each column, shown or hidden (a click switches it), and "show all".
    pub(super) fn columns_menu(&self) -> ViewMenuEntry {
        let mut entries: Vec<ViewMenuEntry> = self
            .source
            .columns()
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let hidden = self.layout.is_hidden(i);
                let id = if hidden {
                    format!("{MENU_PREFIX}show:{i}")
                } else {
                    format!("{MENU_PREFIX}hide:{i}")
                };
                ViewMenuEntry::item(id, c.name.clone()).with_icon(if hidden {
                    "eye-off"
                } else {
                    "eye"
                })
            })
            .collect();
        entries.push(ViewMenuEntry::Separator);
        entries
            .push(ViewMenuEntry::item(format!("{MENU_PREFIX}show-all"), i18n::t("grid.show_all")));
        ViewMenuEntry::Submenu { label: i18n::t("grid.columns"), icon: Some("eye"), entries }
    }

    /// The value under the cell (by place), when its row came.
    pub(super) fn cell_value(&self, cell: CellPos) -> Option<super::super::source::CellValue> {
        let col = *self.layout.visible().get(cell.col)?;
        let row = self.row_at(cell.row);
        (self.source.row_state(row) == RowState::Ready)
            .then(|| self.source.cell(row, col).to_value())
    }

    /// A command: one of its menu rows (`grid:…`) or a key's (`find`, `refresh`,
    /// `transpose`, `count`, `cancel`). `true` if it was one.
    pub fn command(&mut self, id: &str) -> bool {
        let id = id.strip_prefix(MENU_PREFIX).unwrap_or(id);
        let (name, arg) = id.split_once(':').unwrap_or((id, ""));
        let col: Option<usize> = arg.parse().ok().filter(|c| *c < self.source.columns().len());
        match (name, col) {
            ("export" | "export-copy", _) => {
                let clipboard = name == "export-copy";
                let Some(format) = super::super::export::ExportFormat::find(self.project, arg)
                    .filter(|f| !clipboard || f.is_text())
                else {
                    return false;
                };
                self.action =
                    Some(GridAction::Export { format: format.id().to_owned(), clipboard });
            }
            ("copy", _) => {
                if let Some(text) = self.copy_as(arg) {
                    self.action = Some(GridAction::Copy(text));
                }
            }
            // The data extractor chosen (a menu, a snapshot), its popover opened.
            ("extractor", _) => return self.choose_extractor(arg),
            ("extractors", _) => return self.open_extractors(),
            ("find", _) => self.open_find(),
            ("more", _) => return self.open_more(),
            // A row of the "…" menu: its tool, as from its button.
            ("tool", _) => {
                let Some(tool) = Tool::from_id(arg) else { return false };
                return match tool {
                    Tool::PageSize => self.open_sizes(),
                    Tool::Extractor => self.open_extractors(),
                    _ => self.run_tool(tool),
                };
            }
            ("page-size", _) => {
                let size = match arg {
                    "all" => None,
                    n => match n.parse::<u32>() {
                        Ok(n) if super::super::paging::PAGE_SIZES.contains(&n) => Some(n),
                        _ => return false,
                    },
                };
                self.choose_page_size(size);
            }
            ("refresh", _) => self.refresh(),
            ("transpose", _) => {
                self.toggle_transpose();
            }
            ("count", _) => self.source.count(),
            ("cancel", _) => self.source.cancel(),
            ("sort-asc" | "sort-desc", Some(col)) => {
                let column = self.source.columns()[col].name.clone();
                self.set_sort(vec![SortKey { column, descending: name == "sort-desc" }]);
            }
            ("sort-clear", _) => self.set_sort(Vec::new()),
            ("fit", Some(col)) => self.fit_pending = Some(col),
            ("hide", Some(col)) => {
                if self.layout.hide(col) {
                    self.selection.clear();
                    self.layout_touched();
                }
            }
            ("show", Some(col)) => {
                self.layout.show(col);
                self.layout_touched();
            }
            ("show-all", _) => {
                self.layout.show_all();
                self.layout_touched();
            }
            ("select-column", Some(col)) => {
                if let (Some(pos), Some((first, last))) =
                    (self.layout.visible().iter().position(|&c| c == col), self.rows_span())
                {
                    self.selection.column(pos, first, last, false);
                    self.aggregates = None;
                }
            }
            ("select-row", _) => {
                let ncols = self.layout.visible().len();
                let rows: Vec<u64> = self.selection.row_set().into_iter().collect();
                if let (Some(&a), Some(&b)) = (rows.first(), rows.last()) {
                    self.selection.rows(a, b, ncols, false);
                    self.aggregates = None;
                }
            }
            ("filter-eq" | "filter-ne" | "filter-null" | "filter-not-null", _) => {
                self.quick_filter(name);
            }
            ("filter-clear", _) => self.set_filter(None),
            ("edit", _) => {
                self.start_edit(None);
            }
            // A cell selected from outside (`row,column` places: a link, a snapshot).
            ("select", _) => {
                let mut parts = arg.split(',').map(|n| n.trim().parse::<usize>());
                let (Some(Ok(row)), Some(Ok(col))) = (parts.next(), parts.next()) else {
                    return false;
                };
                let shown = self.shown_rows();
                if (row as u64) < shown && col < self.layout.visible().len() {
                    self.selection.set(CellPos::new(row as u64, col));
                    self.aggregates = None;
                    self.reveal_cursor();
                }
            }
            ("edit-big", _) => {
                self.start_big();
            }
            ("set-null", _) => self.set_null(),
            ("add-row", _) => {
                self.add_row(false);
            }
            ("clone-row", _) => {
                self.add_row(true);
            }
            ("delete-rows", _) => self.delete_rows(),
            ("revert", _) => self.revert(false),
            ("revert-all", _) => self.revert(true),
            ("submit", _) => {
                self.submit();
            }
            // The fields' text set from outside (a link to related rows, a snapshot).
            ("where", _) => {
                self.where_input.set_text(arg);
                self.apply_field(Field::Where);
                // Not understood: the field keeps it, saying why (as typed).
                if self.where_error.is_some() {
                    self.field = Some(Field::Where);
                }
            }
            ("order", _) => {
                self.order_input.set_text(arg);
                self.apply_field(Field::Order);
            }
            _ => return false,
        }
        true
    }

    /// "Only this value", "without it", `IS NULL`, `IS NOT NULL`: added to the filter.
    pub(super) fn quick_filter(&mut self, kind: &str) {
        let Some(MenuTarget::Cell(cell)) =
            self.menu_target.or_else(|| self.selection.cursor().map(MenuTarget::Cell))
        else {
            return;
        };
        let Some(&col) = self.layout.visible().get(cell.col) else { return };
        let column = self.source.columns()[col].name.clone();
        let value = self.cell_value(cell);
        let added = match kind {
            "filter-null" => GridFilter::IsNull(column),
            "filter-not-null" => GridFilter::NotNull(column),
            _ => match value {
                Some(super::super::source::CellValue::Null) | None => GridFilter::IsNull(column),
                Some(value) => GridFilter::Compare {
                    column,
                    op: if kind == "filter-eq" {
                        filter::Compare::Eq
                    } else {
                        filter::Compare::NotEq
                    },
                    value,
                },
            },
        };
        let filter = match self.query.filter.take() {
            Some(existing) => existing.and(added),
            None => added,
        };
        self.set_filter(Some(filter));
    }
}
