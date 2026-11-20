//! The view's commands — the palette's (remappable: pause, find, sort by a metric, the
//! columns' chooser, the detail, copying) and its right-click menus' rows: on a row (show
//! it in the detail, copy it as tab-separated text, copy its programs' ids; a group folds
//! or opens), on a header (sort by it, hide it, choose the columns, back to their
//! priorities).

use i18n::t;
use ui_components::columns::Choice;
use ui_components::{AppRequest, ViewMenuEntry, ViewerAction};

use super::keys::is_row;
use super::{Hit, Popover, RowRef, TaskManager, sort_column};
use crate::layout::{COLUMNS, ColumnId};
use crate::model::{Column, Item};

/// The sorting a `sort:<id>` command names (by the columns' ids).
fn sorting(id: &str) -> Option<Column> {
    sort_column(ColumnId::from_id(id)?)
}

impl TaskManager {
    /// Command `id` run (`false`: not one of its own).
    pub(super) fn run_command(&mut self, id: &str) -> bool {
        // A part's action from a row's menu (the selected part's).
        if let Some((action, target)) = super::actions::decode(id) {
            let Some(key) = self.selected.clone() else { return false };
            let found = self.frame.line(&key).and_then(|l| {
                l.actions.iter().find(|a| a.id == action && a.target == target).copied()
            });
            match found {
                Some(action) => self.request(&key, action),
                None => self
                    .notifications
                    .push(workbench_model::Notification::warning(t("tasks.action_gone"))),
            }
            return true;
        }
        if let Some(column) = id.strip_prefix("sort:").and_then(sorting) {
            self.sort_by(column);
            return true;
        }
        if let Some(column) = id.strip_prefix("hide-column:").and_then(ColumnId::from_id) {
            if column == ColumnId::Name {
                return false;
            }
            self.choices[column.index()] = Choice::Hidden;
            self.save();
            return true;
        }
        match id {
            "pause" => self.set_paused(!self.paused),
            "find" => {
                self.ring = true;
                self.focus_filter();
            }
            "columns" => self.open_popover(Popover::Columns),
            "reset-columns" => self.reset_columns(),
            "detail" => {
                let model = self.model();
                let Some(i) = self.cursor_index(&model) else { return false };
                if matches!(model.items[i], Item::Group { .. }) {
                    return false;
                }
                self.go_to(i);
                self.open_detail();
            }
            "toggle-group" => {
                let model = self.model();
                let Some(i) = self.cursor_index(&model) else { return false };
                let Item::Group { key, .. } = &model.items[i] else { return false };
                self.toggle_group(*key);
            }
            "stop" => {
                let Some((key, action)) = self.stop_target() else { return false };
                self.request(&key, action);
            }
            "copy-row" => return self.copy(self.row_text()),
            "copy-pid" => return self.copy(self.pid_text()),
            _ => return false,
        }
        true
    }

    /// `text` put on the clipboard (through the app).
    fn copy(&mut self, text: Option<String>) -> bool {
        let Some(text) = text else { return false };
        self.action = Some(ViewerAction::Request(AppRequest::CopyText(text)));
        true
    }

    /// The keys' row as tab-separated text (its columns in their order, a dash for what's
    /// not known).
    pub(super) fn row_text(&self) -> Option<String> {
        let model = &self.model;
        let rows = 0..model.items.len();
        let i = match &self.cursor {
            Some(row) => rows.clone().find(|&i| is_row(model, i, row)),
            None => None,
        }
        .or_else(|| {
            let key = self.selected.as_deref()?;
            rows.into_iter().find(|&i| {
                matches!(&model.items[i], Item::Part { index, .. } if model.frame.lines[*index].key == key)
            })
        })?;
        let dash = |v: Option<&str>| v.unwrap_or("—").to_owned();
        let cells: Vec<String> = match model.items.get(i)? {
            Item::Group { title, cpu_text, memory_text, .. } => {
                vec![title.clone(), cpu_text.clone(), dash(memory_text.as_deref())]
            }
            Item::Part { index, texts, .. } => {
                let line = &model.frame.lines[*index];
                let project = line
                    .project
                    .and_then(|id| model.frame.projects.iter().find(|p| p.id == id))
                    .map(|p| p.name.clone());
                COLUMNS
                    .iter()
                    .filter_map(|c| {
                        Some(match c {
                            ColumnId::Name => line.name.clone(),
                            ColumnId::Project => dash(project.as_deref()),
                            ColumnId::Detail => line.detail.clone(),
                            ColumnId::Trend => return None,
                            c => dash(texts.get(sort_column(*c)?)),
                        })
                    })
                    .collect()
            }
            Item::Process { texts, .. } => {
                vec![
                    texts.name.clone(),
                    texts.cpu.clone(),
                    texts.memory.clone(),
                    texts.disk.clone(),
                ]
            }
            Item::Operation(o) => vec![
                o.activity.subject.clone(),
                o.kind.clone(),
                o.progress.clone(),
                o.status.clone(),
            ],
        };
        Some(cells.join("\t"))
    }

    /// The program ids of the keys' row: a program's own, a part's (every one it started,
    /// space-separated); `None` when it has none.
    pub(super) fn pid_text(&self) -> Option<String> {
        let model = &self.model;
        let row = self.cursor.clone().or_else(|| self.selected.clone().map(RowRef::Part))?;
        match row {
            RowRef::Process(_, pid) => Some(pid.to_string()),
            RowRef::Part(key) => {
                let line = model.frame.line(&key)?;
                let pids: Vec<String> = line.processes.iter().map(|p| p.pid.to_string()).collect();
                (!pids.is_empty()).then(|| pids.join(" "))
            }
            RowRef::Group(_) | RowRef::Operation(_) => None,
        }
    }

    /// The right-click menu at `p`: a row's (the keys move to it first) or a header's.
    pub(super) fn context_entries(&mut self, p: kernel::Point) -> Option<Vec<ViewMenuEntry>> {
        self.popover = None;
        let hit = self.hit(p)?;
        let row = match hit {
            Hit::Header(column) => return Some(self.header_menu(column)),
            Hit::Row(i) | Hit::Expand(i) => i,
            Hit::Group(key) => {
                let model = self.model();
                model
                    .items
                    .iter()
                    .position(|i| matches!(i, Item::Group { key: k, .. } if *k == key))?
            }
            _ => return None,
        };
        self.focus_on(super::Focus::Table);
        self.go_to(row);
        Some(self.row_menu(row))
    }

    /// A row's menu.
    fn row_menu(&mut self, i: usize) -> Vec<ViewMenuEntry> {
        let model = self.model();
        let mut entries = Vec::new();
        // The part's actions first: on a program's row its own, on an operation's its own,
        // on the part's row all of them (each program's named).
        let (line, only) = match model.items.get(i) {
            Some(Item::Part { index, .. }) => (Some(&model.frame.lines[*index]), None),
            Some(Item::Process { part, process, .. }) => {
                let line = &model.frame.lines[*part];
                let pid = line.processes[*process].pid;
                (Some(line), Some(workbench_model::ActionTarget::Process(pid)))
            }
            Some(Item::Operation(o)) => (
                model.frame.line(&o.owner),
                Some(workbench_model::ActionTarget::Activity(o.activity.id)),
            ),
            _ => (None, None),
        };
        if let Some(line) = line {
            let labels = &self.texts.labels.actions;
            for action in line.actions.iter().filter(|a| only.is_none_or(|t| a.target == t)) {
                let kind = labels[super::actions::kind_index(action.kind)].as_str();
                let label = match action.target {
                    workbench_model::ActionTarget::Part => kind.to_owned(),
                    _ if only.is_some() => kind.to_owned(),
                    target => format!("{kind}: {}", super::actions::subject(line, target)),
                };
                let label = if action.confirm { format!("{label}…") } else { label };
                entries.push(
                    ViewMenuEntry::item(super::actions::encode(action), label)
                        .with_icon(super::actions::kind_icon(action.kind)),
                );
            }
            if !entries.is_empty() {
                entries.push(ViewMenuEntry::Separator);
            }
        }
        let shortcut = |id: &str| ui_components::shortcuts::label(id);
        let with_keys = |entry: ViewMenuEntry, id: &str| match shortcut(id) {
            Some(keys) => entry.with_shortcut(keys),
            None => entry,
        };
        match model.items.get(i) {
            Some(Item::Group { open, .. }) => {
                let label = if *open { t("tasks.menu_fold") } else { t("tasks.menu_unfold") };
                entries.push(ViewMenuEntry::item("toggle-group", label));
            }
            Some(_) => {
                entries.push(with_keys(
                    ViewMenuEntry::item("detail", t("tasks.menu_detail")).with_icon("info"),
                    "tasks.detail",
                ));
            }
            None => {}
        }
        entries.push(ViewMenuEntry::Separator);
        entries.push(with_keys(
            ViewMenuEntry::item("copy-row", t("tasks.menu_copy_row")).with_icon("copy"),
            "tasks.copy_row",
        ));
        if self.pid_text().is_some() {
            entries.push(with_keys(
                ViewMenuEntry::item("copy-pid", t("tasks.menu_copy_pid")),
                "tasks.copy_pid",
            ));
        }
        entries
    }

    /// A header's menu: sort by it, hide it, the chooser, back to the priorities.
    fn header_menu(&self, column: ColumnId) -> Vec<ViewMenuEntry> {
        let mut entries = Vec::new();
        let label = self.texts.labels.column(column);
        if sort_column(column).is_some() {
            entries.push(ViewMenuEntry::item(
                format!("sort:{}", column.id()),
                i18n::tr!("tasks.menu_sort", column = label),
            ));
        }
        if column != ColumnId::Name {
            entries.push(ViewMenuEntry::item(
                format!("hide-column:{}", column.id()),
                i18n::tr!("tasks.menu_hide", column = label),
            ));
        }
        entries.push(ViewMenuEntry::Separator);
        entries.push(ViewMenuEntry::item("columns", t("tasks.menu_columns")));
        entries.push(ViewMenuEntry::item("reset-columns", t("tasks.columns_reset")));
        entries
    }
}

#[cfg(test)]
#[path = "../../tests/unit/view/commands.rs"]
mod tests;
