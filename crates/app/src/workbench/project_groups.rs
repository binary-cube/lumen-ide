//! The projects list's groups (the top bar's modal and the empty screen): collapsing and
//! expanding, moving a project or a group (a drop, the right-click menu), naming,
//! deleting, taking a project out of the list. Each change is made to the kept list
//! (`crate::recent::update`, read again first: another window may have changed it), then
//! the list is read again.

use std::path::PathBuf;

use ui_workbench::{ButtonKind, DialogButton, DialogSpec, MenuEntry, ProjectsTarget, ds::Tone};
use workspace::recent::{GroupId, Order, Place, RECENT, RecentProjects};

use super::*;

/// The prefix of the projects list's menu commands (`recent.move:…`), which the app
/// passes back to [`Workbench::recent_menu`].
pub(crate) const RECENT_MENU: &str = "recent.";

/// The question asked for the list: a group's name.
#[derive(Debug, Clone)]
pub(crate) enum RecentAsk {
    /// A new group in `parent` (`None`: at the top), with `project` moved into it.
    NewGroup {
        parent: Option<GroupId>,
        project: Option<PathBuf>,
    },
    RenameGroup(GroupId),
}

/// The "Delete…" dialog of the recent project `path`: taken out of the list; its checkbox
/// (unchecked on open) also sends its folder to the trash. `open`: it's open in a window,
/// so no checkbox (its folder stays).
pub(crate) fn delete_project_dialog(path: &std::path::Path, open: bool) -> DialogSpec {
    let name = kernel::paths::file_name(path);
    let shown = kernel::paths::tildify(path, dirs::home_dir().as_deref());
    let message = if open { "projects.delete_message_open" } else { "projects.delete_message" };
    DialogSpec {
        tone: Tone::Accent,
        icon: "trash",
        title: i18n::tr!("projects.delete_title", name = name.as_str()),
        message: i18n::t(message),
        buttons: vec![
            DialogButton::new("cancel", i18n::t("dialog.cancel"), ButtonKind::Secondary),
            DialogButton::new("remove", i18n::t("projects.forget"), ButtonKind::Primary),
        ],
        cancel: "cancel",
        input: None,
        check: (!open).then(|| ui_workbench::DialogCheck {
            label: i18n::t("projects.delete_disk"),
            warning: i18n::tr!("projects.delete_disk_warning", path = format!("**{shown}**")),
            checked_label: i18n::t("projects.delete_disk_confirm"),
        }),
        list: Vec::new(),
    }
}

fn group_command(id: GroupId) -> String {
    format!("{RECENT_MENU}move:{:032x}", id.0)
}

impl Workbench {
    /// The list shown: the modal's while it's open, else the empty screen's.
    fn shown_picker(&self) -> Option<&ProjectsPicker> {
        if self.home.projects_menu.is_open() {
            Some(&self.home.projects_menu)
        } else {
            self.home_visible().then_some(&self.home.picker)
        }
    }

    /// A choice from a projects list (`modal`: the top bar's): changes to the list are
    /// made here (the modal stays open); the rest closes the modal and goes on as a request.
    pub(super) fn picker_action(&mut self, action: ProjectsAction, modal: bool) -> MouseResult {
        match action {
            ProjectsAction::ToggleGroup(id) => {
                self.change_recent(|list| {
                    let collapsed = list.group(id).is_some_and(|g| g.collapsed);
                    list.set_collapsed(id, !collapsed)
                });
                MouseResult::Redraw
            }
            ProjectsAction::MoveProject { path, into, place } => {
                self.change_recent(|list| {
                    // Dropped on its own group's row: nothing to do.
                    let same = list.group_of(&path) == into && place == Place::Anywhere;
                    !same && list.move_to(&path, into, place)
                });
                MouseResult::Redraw
            }
            ProjectsAction::MoveGroup { id, parent, before } => {
                self.change_recent(|list| list.move_group(id, parent, before));
                MouseResult::Redraw
            }
            ProjectsAction::NewGroup => {
                self.ask_group_name(RecentAsk::NewGroup { parent: None, project: None });
                MouseResult::Redraw
            }
            ProjectsAction::ToggleSession(_) | ProjectsAction::ToggleSessionWindow(..) => {
                let picker =
                    if modal { &mut self.home.projects_menu } else { &mut self.home.picker };
                picker.toggle_session(&action);
                MouseResult::Redraw
            }
            other => {
                if modal {
                    self.home.projects_menu.close();
                } else {
                    self.note_home_place(&other);
                }
                if let ProjectsAction::EditProject(path) = &other {
                    self.edit_project(path);
                }
                picker_result(other)
            }
        }
    }

    /// Opens what the user changes of project `path` (its kind's settings: a remote one's
    /// connection), by the module that described it. `false`: none does.
    pub(super) fn edit_project(&mut self, path: &std::path::Path) -> bool {
        let support = Arc::clone(&self.support);
        support.project_services.iter().any(|s| s.edit(self, path))
    }

    /// Changes the kept list; read again when it changed.
    pub(super) fn change_recent(&mut self, change: impl FnOnce(&mut RecentProjects) -> bool) {
        if crate::recent::update(change) {
            self.home.recent = self.recent_projects();
        }
    }

    /// ← / → on a selected group (`modal`: the top bar's list): collapses / expands it.
    /// `false`: no group is selected.
    pub(super) fn fold_selected_group(&mut self, modal: bool, collapse: bool) -> bool {
        let picker = if modal { &mut self.home.projects_menu } else { &mut self.home.picker };
        // A past session (or one of its windows) is folded in the list itself.
        if picker.fold_selected_session(collapse) {
            return true;
        }
        let Some((id, collapsed)) = picker.selected_group() else { return false };
        if collapsed != collapse {
            self.change_recent(|list| list.set_collapsed(id, collapse));
        }
        true
    }

    #[cfg(feature = "snapshot")]
    /// Shows `list` as the recent projects without reading or writing the kept one
    /// (snapshots).
    pub fn show_recent(&mut self, list: ui_workbench::RecentList) {
        self.home.recent = list;
        self.home.recent_generation = Some(crate::recent::generation());
    }

    #[cfg(feature = "snapshot")]
    /// Where project `path`'s row is in the top bar's projects list (snapshots).
    pub fn projects_row(&self, path: &std::path::Path) -> Option<kernel::Rect> {
        self.home.projects_menu.project_row(path)
    }

    #[cfg(feature = "snapshot")]
    /// Toggles the open dialog's checkbox (snapshots).
    pub fn toggle_dialog_check(&mut self) -> bool {
        self.dialogs.dialog.toggle_check()
    }

    #[cfg(feature = "snapshot")]
    /// Scrolls the open dialog's list `rows` rows (snapshots).
    pub fn scroll_dialog_list(&mut self, rows: f32) -> bool {
        self.dialogs.dialog.scroll_rows(rows)
    }

    /// A projects list holds a press (a click or a drag, known on release).
    pub(super) fn picker_pressed(&self) -> bool {
        self.home.projects_menu.pressed() || self.home.picker.pressed()
    }

    /// The mouse moved while a list's row is pressed. `None`: no list is pressed.
    pub(super) fn drag_picker(&mut self, p: Point) -> Option<bool> {
        let (picker, _) = self.home.pressed_picker()?;
        Some(picker.drag(p))
    }

    /// The press ends: the row's action (a click) or a move (a drop). `None`: no list is
    /// pressed.
    pub(super) fn release_picker(&mut self) -> Option<bool> {
        let (picker, modal) = self.home.pressed_picker()?;
        if let Some(action) = picker.release() {
            let result = self.picker_action(action, modal);
            self.request = Some(result);
        }
        Some(true)
    }

    /// The right-click menu of the projects list's row under `p` (the modal's or the empty
    /// screen's); its commands come back through [`Self::recent_menu`].
    pub fn projects_menu_at(&mut self, p: Point) -> Option<Vec<MenuEntry>> {
        let target = self.shown_picker()?.target_at(p)?;
        let item = |id: &str, key: &str, icon: &'static str| MenuEntry::Item {
            id: format!("{RECENT_MENU}{id}"),
            label: i18n::t(key),
            shortcut: None,
            icon: Some(icon),
            on: None,
            look: Default::default(),
        };
        let model = &self.home.recent.model;
        let entries = match &target {
            ProjectsTarget::Project { path, current } => {
                let here = model.group_of(path);
                let shown = |name: &str| {
                    if name.is_empty() { i18n::t("home.recent_group") } else { name.to_owned() }
                };
                let mut groups: Vec<(String, GroupId)> = model
                    .groups
                    .iter()
                    .filter(|g| g.id != here)
                    .map(|g| {
                        let names: Vec<String> =
                            model.path_of(g.id).into_iter().map(shown).collect();
                        (names.join(" / "), g.id)
                    })
                    .collect();
                // The recent group first, then the user's by name.
                groups.sort_by_key(|(name, id)| (*id != RECENT, name.clone()));
                let mut into = Vec::new();
                into.extend(groups.into_iter().map(|(label, id)| MenuEntry::Item {
                    id: group_command(id),
                    label,
                    shortcut: None,
                    icon: Some("folder"),
                    on: None,
                    look: Default::default(),
                }));
                if !into.is_empty() {
                    into.push(MenuEntry::Separator);
                }
                into.push(item("move:new", "projects.new_group_with", "plus"));
                let mut entries = Vec::new();
                if !current {
                    entries.push(item("open", "projects.open", "folder-open"));
                }
                // What its kind lets the user change (a remote project's connection).
                let kind = self.home.recent.projects.iter().find(|p| p.path == *path);
                if let Some(edit) = kind.and_then(|p| p.kind.as_ref()).and_then(|k| k.edit.as_ref())
                {
                    entries.push(MenuEntry::Item {
                        id: format!("{RECENT_MENU}edit"),
                        label: edit.menu_label.clone(),
                        shortcut: None,
                        icon: Some(edit.icon),
                        on: None,
                        look: Default::default(),
                    });
                }
                entries.push(MenuEntry::Submenu {
                    label: i18n::t("projects.move_to"),
                    icon: Some("folder-input"),
                    entries: into,
                });
                entries.push(MenuEntry::Separator);
                entries.push(item("forget", "projects.forget", "x"));
                entries.push(item("delete", "projects.delete", "trash"));
                entries
            }
            // The recent group: moved and collapsed only.
            ProjectsTarget::Group(id) if *id == RECENT => {
                vec![item("new_group", "home.new_group", "plus")]
            }
            ProjectsTarget::Group(id) => {
                let mut entries = vec![
                    item("rename_group", "projects.rename_group", "file-pen"),
                    item("new_group", "projects.new_subgroup", "plus"),
                ];
                if model.group(*id).is_some_and(|g| g.order == Order::Manual) {
                    entries.push(item("sort_opening", "projects.sort_opening", "clock"));
                }
                entries.push(MenuEntry::Separator);
                entries.push(item("delete_group", "projects.delete_group", "trash"));
                entries
            }
            ProjectsTarget::History => vec![item("new_group", "home.new_group", "plus")],
        };
        self.home.recent_target = Some(target);
        Some(entries)
    }

    /// The project the list's menu acts on (the app asks before deleting it).
    pub fn recent_menu_project(&self) -> Option<PathBuf> {
        match &self.home.recent_target {
            Some(ProjectsTarget::Project { path, .. }) => Some(path.clone()),
            _ => None,
        }
    }

    /// A command of the projects list's menu (`action`: after [`RECENT_MENU`]), on what
    /// was right-clicked. `false`: nothing to do.
    pub fn recent_menu(&mut self, action: &str) -> bool {
        let Some(target) = self.home.recent_target.clone() else { return false };
        let group = match &target {
            ProjectsTarget::Group(id) => Some(*id),
            _ => None,
        };
        if let ProjectsTarget::Project { path, .. } = target {
            match action {
                "open" => {
                    self.home.projects_menu.close();
                    self.request = Some(MouseResult::OpenProject(path));
                }
                "forget" => self.change_recent(|list| list.forget(&path)),
                "edit" => {
                    self.home.projects_menu.close();
                    self.edit_project(&path);
                }
                "move:new" => {
                    self.ask_group_name(RecentAsk::NewGroup { parent: None, project: Some(path) })
                }
                other => {
                    let Some(id) = other
                        .strip_prefix("move:")
                        .and_then(|id| u128::from_str_radix(id, 16).ok())
                    else {
                        return false;
                    };
                    self.change_recent(|list| list.move_to(&path, GroupId(id), Place::Anywhere));
                }
            }
            return true;
        }
        match (action, group) {
            ("rename_group", Some(id)) => self.ask_group_name(RecentAsk::RenameGroup(id)),
            ("sort_opening", Some(id)) => self.change_recent(|list| list.sort_by_opening(id)),
            ("new_group", parent) => {
                let parent = parent.filter(|p| *p != RECENT);
                self.ask_group_name(RecentAsk::NewGroup { parent, project: None })
            }
            ("delete_group", Some(id)) => self.change_recent(|list| list.delete_group(id)),
            _ => return false,
        }
        true
    }

    /// Asks for a group's name (a new one, or a new name).
    fn ask_group_name(&mut self, ask: RecentAsk) {
        let (title, name, button) = match &ask {
            RecentAsk::NewGroup { .. } => {
                ("projects.new_group_title", String::new(), "projects.create")
            }
            RecentAsk::RenameGroup(id) => {
                let name =
                    self.home.recent.model.group(*id).map(|g| g.name.clone()).unwrap_or_default();
                ("projects.rename_group_title", name, "dialog.rename")
            }
        };
        let len = name.len();
        self.open_dialog(DialogSpec {
            tone: Tone::Accent,
            icon: "folder",
            title: i18n::t(title),
            message: i18n::t("projects.group_name_message"),
            buttons: vec![
                DialogButton::new("cancel", i18n::t("dialog.cancel"), ButtonKind::Secondary),
                DialogButton::new("ok", i18n::t(button), ButtonKind::Primary),
            ],
            cancel: "cancel",
            input: Some((name, 0..len)),
            check: None,
            list: Vec::new(),
        });
        self.home.recent_ask = Some(ask);
    }

    /// The group name's answer.
    pub(super) fn answer_recent(&mut self, ask: RecentAsk, answer: &str) {
        let name = self.dialogs.value.take().unwrap_or_default();
        if answer != "ok" || name.trim().is_empty() {
            return;
        }
        match ask {
            RecentAsk::NewGroup { parent, project } => self.change_recent(|list| {
                let id = list.new_group(&name, parent);
                if let Some(path) = &project {
                    list.move_to(path, id, Place::Anywhere);
                }
                // Where it was made shows it.
                if let Some(parent) = parent {
                    list.set_collapsed(parent, false);
                }
                true
            }),
            RecentAsk::RenameGroup(id) => self.change_recent(|list| list.rename_group(id, &name)),
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/project_groups.rs"]
mod tests;
