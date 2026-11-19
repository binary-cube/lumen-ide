//! The overlays: settings, the projects menu, dialogs, tooltips, About.

use super::*;

impl Workbench {
    /// The space taken by the window buttons in the top bar (macOS, unified bar).
    pub fn set_titlebar_inset(&mut self, inset: f32) {
        self.chrome.titlebar_inset = inset;
    }

    /// The window's own buttons in the top bar (Windows), and whether it's maximized.
    pub fn set_window_controls(&mut self, controls: Option<bool>) {
        self.chrome.window_controls = controls;
    }

    /// The window is full screen (or not anymore). With the window's buttons ours, the top
    /// bar hides then: the content takes its place.
    pub fn set_full_screen(&mut self, full_screen: bool) {
        if self.chrome.full_screen != full_screen {
            self.chrome.full_screen = full_screen;
            self.chrome.top_bar_reveal.set(0.0);
            self.layout();
        }
    }

    /// The mouse at `p`: the hidden top bar comes at the top edge and goes once the mouse
    /// leaves it (not while its project list is open). `true` if that changed.
    pub(super) fn reveal_top_bar(&mut self, p: Point) -> bool {
        if !self.chrome.top_bar_hides() {
            return false;
        }
        let shown = self.chrome.top_bar_reveal.target() > 0.0;
        let show = if shown {
            p.y < TopBar::height(self.scale) || self.home.projects_menu.is_open()
        } else {
            p.y <= (2.0 * self.scale).max(1.0) && !self.modal_open()
        };
        self.chrome.top_bar_reveal.retarget(if show { 1.0 } else { 0.0 })
    }

    /// The mouse over the maximize button, as the system tells (it has it there).
    pub fn set_maximize_hovered(&mut self, hovered: bool) {
        self.chrome.maximize_hovered = hovered;
    }

    /// Where the maximize button is (physical pixels), when the top bar draws it.
    pub fn maximize_button(&self) -> Option<kernel::Rect> {
        self.chrome.top_bar.maximize_button()
    }

    /// Without recording opened projects in the recent list (e.g. `--snapshot`).
    pub fn forget_projects(&mut self) {
        self.remember_projects = false;
    }

    #[cfg(any(test, feature = "snapshot"))]
    /// Opens the project list (as when clicking the project name).
    pub fn open_projects_menu(&mut self) {
        self.home.recent = self.recent_projects();
        if !self.home.projects_menu.is_open() {
            self.home.projects_menu.open();
        }
    }

    pub(super) fn current_project(&self) -> Option<RecentProject> {
        self.project.as_ref()?;
        let mut current = self.data.described.clone()?;
        // What it is: as the list says (read with it, not on every frame).
        current.kind = self
            .home
            .recent
            .projects
            .iter()
            .find(|r| r.path == current.path)
            .and_then(|r| r.kind.clone());
        Some(current)
    }

    /// The recent projects (with their groups), each described by the modules that know it
    /// (a remote one).
    pub(super) fn recent_projects(&self) -> ui_workbench::RecentList {
        let mut list = crate::recent::list();
        // The past sessions stay as they were made (`sync_home_sessions`).
        list.sessions.clone_from(&self.home.recent.sessions);
        let paths: Vec<std::path::PathBuf> = list.projects.iter().map(|p| p.path.clone()).collect();
        let kinds = crate::recent::kinds(&paths, &self.support.project_services);
        for (project, kind) in list.projects.iter_mut().zip(kinds) {
            project.kind = kind;
        }
        list
    }

    // ─── Settings ───

    #[cfg(feature = "snapshot")]
    /// The text searched in settings (diagnostics / snapshots).
    pub fn settings_query(&mut self, query: &str) {
        self.settings.type_text(query);
    }

    /// The model a module finds texts by meaning with (semantic search's, if on and
    /// loaded), for searching in the settings; asked again at each opening.
    pub fn take_settings_embedder(&mut self) {
        let embedder = self.support.embeddings.iter().find_map(|e| e.embedder());
        self.settings.set_embedder(embedder);
    }

    pub fn open_settings(&mut self, model: SettingsModel) {
        self.take_settings_embedder();
        self.set_settings_model(model);
        self.settings.open();
    }

    /// The state shown in settings (after a change made by the app), with this window's
    /// modules and its project's frameworks.
    pub fn set_settings_model(&mut self, mut model: SettingsModel) {
        self.fill_modules(&mut model);
        self.settings.set_sections(&model);
        self.settings_model = model;
    }

    /// The UI language changed: the computed texts (error messages) are redone.
    pub fn locale_changed(&mut self) {
        for tab in &mut self.tabs {
            tab.diagnostics.errors_version = None;
        }
    }

    pub fn close_settings(&mut self) {
        self.settings.close();
        // The other icon styles were only drawn for the previews.
        self.icons.release_unused();
    }

    // ─── Bottom window tabs ───

    /// The dialog with the name of the bottom window's tab `i`.
    pub(super) fn ask_tool_view_rename(&mut self, i: usize) {
        use ui_workbench::{ButtonKind, DialogButton, DialogSpec, ds::Tone};
        let Some((name, _)) = self.tool_view_at(i) else { return };
        let icon = self
            .tools
            .bottom
            .as_deref()
            .and_then(|id| self.tool(id))
            .map_or("file-pen", |t| t.icon());
        let len = name.len();
        self.open_dialog(DialogSpec {
            tone: Tone::Accent,
            icon,
            title: i18n::t("dialog.rename_view_title"),
            message: i18n::t("dialog.rename_view_message"),
            buttons: vec![
                DialogButton::new("cancel", i18n::t("dialog.cancel"), ButtonKind::Secondary),
                DialogButton::new("rename", i18n::t("dialog.rename"), ButtonKind::Primary),
            ],
            cancel: "cancel",
            input: Some((name, 0..len)),
            check: None,
            list: Vec::new(),
        });
        self.dialogs.tool_rename = Some(i);
    }

    /// Tab `i`'s view can be renamed (a session's terminal).
    pub fn tab_renamable(&self, i: usize) -> bool {
        self.tabs.get(i).and_then(|t| t.viewer.as_ref()).is_some_and(|v| v.viewer.renamable())
    }

    /// Asks a new name for tab `i`'s view (the file's dialog, its tab's name in it).
    pub fn ask_view_rename(&mut self, i: usize) {
        use ui_workbench::{ButtonKind, DialogButton, DialogSpec, ds::Tone};
        let Some(tab) = self.tabs.get(i) else { return };
        let name = tab.title();
        let id = tab.doc.id();
        let len = name.len();
        self.open_dialog(DialogSpec {
            tone: Tone::Accent,
            icon: "file-pen",
            title: i18n::t("dialog.rename_view_title"),
            message: i18n::t("dialog.rename_view_message"),
            buttons: vec![
                DialogButton::new("cancel", i18n::t("dialog.cancel"), ButtonKind::Secondary),
                DialogButton::new("rename", i18n::t("dialog.rename"), ButtonKind::Primary),
            ],
            cancel: "cancel",
            input: Some((name, 0..len)),
            check: None,
            list: Vec::new(),
        });
        self.dialogs.view_rename = Some(id);
    }

    /// An action of a bottom window tab's menu. `true` if something changed.
    pub fn tool_tab_action(&mut self, action: crate::commands::ToolTabAction, i: usize) -> bool {
        use crate::commands::ToolTabAction as A;
        if self.tool_view_at(i).is_none() {
            return false;
        }
        match action {
            A::Rename => self.ask_tool_view_rename(i),
            A::TogglePin => self.tools.toggle_pin_view(i),
            A::Close => self.close_tool_view(i),
            A::CloseOthers => self.tools.close_other_views(i),
            // On its group (the pinned ones stay, as with Close Others).
            A::CloseLeft | A::CloseRight | A::CloseAll => {
                let g = self.tools.window().map_or(0, |w| w.current);
                self.close_tool_views(g, |j, v| {
                    !v.pinned
                        && match action {
                            A::CloseLeft => j < i,
                            A::CloseRight => j > i,
                            _ => true,
                        }
                });
            }
            A::SplitRight => return self.split_tool_view(i, Split::Right, false),
            A::SplitDown => return self.split_tool_view(i, Split::Down, false),
            A::SplitMoveRight => return self.split_tool_view(i, Split::Right, true),
            A::SplitMoveDown => return self.split_tool_view(i, Split::Down, true),
        }
        true
    }

    // ─── Dialogs ───

    /// Opens a dialog; the answer (the button id) comes through [`Self::take_dialog_answer`].
    pub fn open_dialog(&mut self, spec: ui_workbench::DialogSpec) {
        self.completion.close();
        self.tooltips.hide();
        self.dialogs.dialog.open(spec);
    }

    pub fn dialog_open(&self) -> bool {
        self.dialogs.dialog.is_open()
    }

    /// The answered dialog's checkbox was checked.
    pub fn take_dialog_checked(&mut self) -> bool {
        std::mem::take(&mut self.dialogs.checked)
    }

    pub fn take_dialog_value(&mut self) -> Option<String> {
        self.dialogs.value.take()
    }

    pub fn can_reopen(&self) -> bool {
        !self.data.closed.is_empty()
    }

    /// The root of the open project.
    pub fn project_root(&self) -> Option<PathBuf> {
        self.project.as_ref().map(|p| p.root_path().to_path_buf())
    }

    pub fn take_dialog_answer(&mut self) -> Option<&'static str> {
        self.dialogs.answer.take()
    }

    pub(crate) fn answer_dialog(&mut self, answer: Option<&'static str>) -> bool {
        let Some(answer) = answer else { return false };
        self.dialogs.answered();
        // The projects list's question (a group's name) stays here.
        if let Some(ask) = self.home.recent_ask.take() {
            self.answer_recent(ask, answer);
            return true;
        }
        // A module's question (`confirm`): its "yes" runs here.
        if let Some(Confirming(then)) = self.dialogs.confirming.take() {
            if answer == "yes" {
                then(self);
                // Asked from the settings (a module's button): they show what it did.
                if self.settings.is_open() {
                    self.settings_model.blocks = self.settings_blocks();
                }
            }
            return true;
        }
        // Renaming a tab's view (a session's terminal): its new name, one line.
        if let Some(id) = self.dialogs.view_rename.take() {
            if answer == "rename"
                && let Some(name) = self.dialogs.value.take()
            {
                let name: String = name.chars().filter(|c| !c.is_control()).collect();
                let name = name.trim();
                if let (false, Some(i)) = (name.is_empty(), self.tab_index(id))
                    && let Some(v) = self.tabs[i].viewer.as_mut()
                {
                    // What it asks then (its session renamed too) taken at the next turn.
                    if let Some(action) = v.viewer.rename(name) {
                        self.held_view_action = Some(action);
                    }
                }
            }
            return true;
        }
        // Renaming a bottom window's tab stays here (doesn't reach the app).
        if let Some(i) = self.dialogs.tool_rename.take() {
            if answer == "rename"
                && let Some(name) = self.dialogs.value.take()
            {
                self.tools.rename_view(i, &name);
            }
            return true;
        }
        self.dialogs.answer = Some(answer);
        true
    }

    /// The identifier of the document in tab `i` (stable while the tab is open).
    pub fn tab_id(&self, i: usize) -> kernel::DocumentId {
        self.tabs[i].doc.id()
    }

    /// The tab of the document with identifier `raw` (`DocumentId::get`, e.g. from a menu
    /// command).
    pub fn tab_index_raw(&self, raw: u64) -> Option<usize> {
        self.tabs.iter().position(|t| t.doc.id().get() == raw)
    }

    pub fn tab_index(&self, id: kernel::DocumentId) -> Option<usize> {
        self.tabs.iter().position(|t| t.doc.id() == id)
    }

    /// The tooltip of the element under the mouse: the bar buttons, the panel buttons, the
    /// tabs, the status bar and the viewer components (from modules).
    pub(super) fn tooltip_at(&self, p: Point) -> Option<ui_workbench::Tip> {
        use ui_workbench::{TabHit, Tip, TopBarHit};
        let with = |rect: Rect, key: &str, command: Option<command_model::CommandId>| {
            Tip::new(rect, i18n::t(&format!("tooltip.{key}")))
                .with_shortcut(command.and_then(crate::commands::shortcut_label))
        };
        // Nothing behind an open window (a modal, a context menu, a popover) shows a
        // tooltip: only the overlay on top may give one, of what it holds itself.
        if let Some(o) = self.active_overlay() {
            return o.slot().tooltip_at(self, p);
        }
        // On a floating window (a status bar item's): its own, or none.
        if let Some(o) = self.floating_at(p) {
            return o.slot().tooltip_at(self, p);
        }
        if let Some(tip) = self.annotation_tip(p) {
            return Some(tip);
        }
        // A side panel's header button: its tooltip.
        if let Some((rect, tooltip)) = self.side_button_at(p) {
            return Some(Tip::new(rect, tooltip));
        }
        // What a side panel says of what's under the mouse.
        if let Some(tip) = self
            .tools
            .sides
            .iter()
            .find(|s| s.content.contains(p))
            .and_then(|s| s.panel.tooltip_at(p))
        {
            return Some(tip);
        }
        if let Some((rect, id)) =
            self.tools.strip.hit_rect(p).or_else(|| self.tools.right_strip.hit_rect(p))
        {
            // A registered tool window: its title, its toggle's shortcut.
            if let Some(tool) = self.tool(id) {
                let command = tool.toggle_command().map(command_model::CommandId);
                // Its count says what it is (`Git · 3 files changed`).
                let title = match self.tool_badge(id) {
                    Some(badge) => format!("{} · {}", tool.title(), badge.label),
                    None => tool.title(),
                };
                return Some(
                    Tip::new(rect, title)
                        .with_shortcut(command.and_then(crate::commands::shortcut_label)),
                );
            }
            return Some(with(rect, id, None));
        }
        // The bottom window's size buttons and its hide.
        if let Some((rect, hit)) = self.tools.hits.iter().find(|(r, _)| r.contains(p)) {
            use tool_windows::{BottomSize, ToolHit};
            let key = match hit {
                ToolHit::Full => Some(self.bottom_size_tooltip(BottomSize::Full)),
                ToolHit::Half => Some(self.bottom_size_tooltip(BottomSize::Half)),
                ToolHit::Hide => Some("tooltip.bottom_hide"),
                ToolHit::More => Some("tooltip.window_options"),
                _ => None,
            };
            if let Some(key) = key {
                return Some(with(*rect, key.trim_start_matches("tooltip."), None));
            }
        }
        // What the bottom window's view says of what's under the mouse (as a side panel's).
        if let Some(tip) = self.tool_view_tooltip(p) {
            return Some(tip);
        }
        if let Some((rect, hit)) = self.chrome.top_bar.hit_rect(p) {
            return Some(match hit {
                TopBarHit::Menu => {
                    Tip::new(rect, i18n::t("tooltip.menu")).with_shortcut(self.menu_shortcut())
                }
                TopBarHit::Projects => with(rect, "projects", None),
                TopBarHit::Branch => {
                    let action = self.branch_action()?;
                    Tip::new(rect, i18n::t(action.tooltip))
                        .with_shortcut(crate::commands::shortcut_label(action.command))
                }
                TopBarHit::Badge => {
                    Tip::new(rect, self.chrome.badge.as_ref().map(|b| b.tooltip.clone())?)
                }
                TopBarHit::Action(id) => match self.tool(id) {
                    Some(tool) => Tip::new(rect, tool.title()).with_shortcut(
                        tool.toggle_command()
                            .map(command_model::CommandId)
                            .and_then(crate::commands::shortcut_label),
                    ),
                    None => with(rect, id, None),
                },
                TopBarHit::Minimize => with(rect, "minimize", None),
                TopBarHit::Maximize if self.chrome.full_screen => {
                    with(rect, "exit_full_screen", Some(crate::commands::FULL_SCREEN))
                }
                // The system shows its snap layouts there (Windows): no tooltip over them.
                TopBarHit::Maximize => return None,
                TopBarHit::Close => with(rect, "close_window", None),
                TopBarHit::Empty => return None,
            });
        }
        // Resting on a tab: its file's full path (`~` for the home directory).
        if let Some(TabHit::Activate(i)) = self.tab_bar.hit(p)
            && let (Some(rect), Some(path)) =
                (self.tab_bar.tab_rect(i), self.tabs.get(i).and_then(|t| t.doc.path()))
        {
            let mut shown = home_relative(path);
            let root = self.project.as_ref().map(|p| p.root_path());
            if super::paint::outside_project(path, root) {
                shown = format!("{} · {shown}", i18n::t("tooltip.outside_project"));
            }
            return Some(Tip::new(rect, shown).with_delay(std::time::Duration::from_millis(1000)));
        }
        if let Some((rect, hit)) = self.tab_bar.hit_rect(p) {
            return Some(match hit {
                TabHit::Back => with(rect, "back", Some(crate::commands::NAVIGATE_BACK)),
                TabHit::Forward => with(rect, "forward", Some(crate::commands::NAVIGATE_FORWARD)),
                TabHit::Maximize => {
                    let key = if self.editor_full() { "editor_restore" } else { "editor_full" };
                    with(rect, key, Some(crate::commands::EDITOR_FULL))
                }
                _ => with(rect, "close_tab", Some(crate::commands::CLOSE_TAB)),
            });
        }
        // The file's path cut short in the status bar: all of it.
        if let Some((rect, path)) = self.status.tooltip_at(p) {
            return Some(Tip::new(rect, path));
        }
        if let Some((rect, action)) = self.status.hit_rect(p) {
            // The padlock: what a click does now (or why it can't).
            if action == crate::commands::TOGGLE_READ_ONLY.0
                && let Some(tip) = self.lock_tooltip()
            {
                return Some(Tip::new(rect, tip));
            }
            let key = match action {
                "project.language_level" => "language_level",
                // The resources' meter: no tooltip (it shows its figures, its window says the
                // rest; one over it at every pass got in the way).
                a if a == workbench_model::SHOW_RESOURCES.0 || a == super::status::RESOURCES => {
                    return None;
                }
                // Its window open: it says it all (no tooltip over it).
                a if self.status_popover.owner() == Some(a) => return None,
                a if a == super::status::FILE_DETAILS => {
                    return Some(Tip::new(rect, i18n::t("status.file_details")));
                }
                a if a == ui_workbench::STATUS_MORE => {
                    return Some(Tip::new(rect, i18n::t("status.more_items")));
                }
                // A command (the zoom's): its name and its keys.
                a if crate::commands::by_id(a).is_some() => {
                    let id = command_model::CommandId(a);
                    return Some(
                        Tip::new(rect, crate::commands::label(id))
                            .with_shortcut(crate::commands::shortcut_label(id)),
                    );
                }
                other => other,
            };
            return Some(with(rect, key, None));
        }
        if let Some(tip) = self.toolbar_tooltip(p) {
            return Some(tip);
        }
        let tab = self.tabs.get(self.active)?;
        let viewer = tab.viewer.as_ref().filter(|v| v.viewer_visible() && v.bounds.contains(p))?;
        viewer.viewer.tooltip_at(p)
    }

    /// No tab is open: the empty screen shows the project list.
    pub(super) fn home_visible(&self) -> bool {
        self.tabs.is_empty()
            && self.editors.groups.len() <= 1
            && !Overlay::ALL
                .into_iter()
                .any(|o| o.slot().covers_home(self) && self.overlay_is_open(o))
    }

    /// Whether overlay `o` is open.
    pub(super) fn overlay_is_open(&self, o: Overlay) -> bool {
        o.slot().is_open(self)
    }

    /// The overlay on top (the one that gets the keyboard), if any; a floating one (the
    /// notifications) never is.
    pub(super) fn active_overlay(&self) -> Option<Overlay> {
        Overlay::ALL.into_iter().find(|o| !o.slot().floats(self) && self.overlay_is_open(*o))
    }

    /// The floating overlay open with `p` on it (a notification's card), if any.
    pub(super) fn floating_at(&self, p: Point) -> Option<Overlay> {
        Overlay::ALL.into_iter().find(|o| {
            let slot = o.slot();
            slot.floats(self) && self.overlay_is_open(*o) && slot.holds(self, p)
        })
    }

    /// The floating overlays' areas, as they show now (the notifications' cards).
    pub(super) fn floating_areas(&self) -> Vec<kernel::Rect> {
        Overlay::ALL
            .into_iter()
            .filter(|o| o.slot().floats(self) && self.overlay_is_open(*o))
            .filter_map(|o| o.slot().area(self))
            .collect()
    }

    /// A window is open over the editor (settings, search, projects, dialog, About,
    /// the color picker): keys go to it, not to the terminal.
    pub fn overlay_open(&self) -> bool {
        self.active_overlay().is_some()
    }

    /// A modal (About, a dialog) is open: nothing behind it gets the keyboard or the mouse.
    pub fn modal_open(&self) -> bool {
        self.active_overlay().is_some_and(|o| o.slot().kind(self) == OverlayKind::Modal)
    }

    /// The overlay on top has a text field with the keyboard (a dialog's, the settings' or
    /// the projects' search, a modal view's): it takes the clipboard commands.
    pub fn overlay_has_input(&self) -> bool {
        self.active_overlay().is_some_and(|o| o.slot().has_input(self))
    }

    /// Opens our context menu at `at` (right click).
    pub fn open_context_menu(&mut self, entries: Vec<ui_workbench::MenuEntry>, at: Point) {
        self.close_hovers_for_menu();
        self.context_menu.open(entries, at);
    }

    /// What floats over the editor goes as a context menu opens.
    pub(super) fn close_hovers_for_menu(&mut self) {
        self.code_hover.hide();
        self.completion.close();
        self.tooltips.hide();
    }

    /// Opens the About window.
    pub fn open_about(&mut self) {
        self.about.open();
    }

    /// The request produced by keys (e.g. Enter on a project), taken by the app.
    /// Esc while recording a shortcut: the recording given up.
    pub fn settings_escape(&mut self) {
        self.settings.cancel_text();
    }

    #[cfg(feature = "snapshot")]
    /// Settings → Shortcuts: `command` opened, recording its next keys (snapshots).
    pub fn open_shortcut(&mut self, command: &str) {
        self.settings.open_shortcut(command);
    }

    /// Settings → Shortcuts is recording the next keys pressed.
    pub fn recording_shortcut(&self) -> bool {
        self.settings.is_open() && self.settings.recording_shortcut()
    }

    /// Keys pressed while recording a shortcut (as written, as shown): the change to make,
    /// if any (otherwise the settings ask about a conflict, or keep recording).
    pub fn record_shortcut(
        &mut self,
        spec: &str,
        shown: &str,
    ) -> Option<ui_workbench::SettingsAction> {
        self.settings.record_shortcut(spec, shown)
    }

    pub fn take_request(&mut self) -> Option<MouseResult> {
        self.request.take()
    }
}

/// The windows over the editor, topmost first: the order they're drawn in (reversed) and
/// the order in which they get the keyboard, the text, the clicks, the scroll and the mouse.
/// What each one does is its [`OverlaySlot`](super::overlay_stack::OverlaySlot).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Overlay {
    /// Files dragged from the system's file manager: over everything, floating (see
    /// `file_drop`).
    FileDrop,
    /// The notifications: over every other, floating (see `toasts`).
    Toasts,
    /// A status bar item's window (the file's details, the resources): floating, under
    /// the notifications (see `status`).
    StatusPopover,
    /// The app's menus, from the window's bar (see `menu_drawer`).
    MenuDrawer,
    ContextMenu,
    ChoiceMenu,
    Dialog,
    ModalView,
    About,
    Settings,
    ProjectsMenu,
    /// The resources pinned, as pills in the editor's corner: under everything (see
    /// `status`).
    ResourcePills,
}

impl Overlay {
    pub const ALL: [Overlay; 12] = [
        Overlay::FileDrop,
        Overlay::Toasts,
        Overlay::StatusPopover,
        Overlay::MenuDrawer,
        Overlay::ContextMenu,
        Overlay::ChoiceMenu,
        Overlay::Dialog,
        Overlay::ModalView,
        Overlay::About,
        Overlay::Settings,
        Overlay::ProjectsMenu,
        Overlay::ResourcePills,
    ];
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/overlays.rs"]
mod tests;
