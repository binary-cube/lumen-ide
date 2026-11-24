//! The menus modules contribute (`Git ›`…): as submenus of the editor's and the tree's
//! right-click menus and behind tool strip buttons, with only the commands enabled there.
//! A tree file's menu acts on that file (the menu's target) without opening it.

use ui_workbench::MenuEntry;
use workbench_model::{ActionContext, MenuPlace, MenuRow};

use super::*;

impl Workbench {
    /// The right-click menu of `place`: the contributed rows (the modules' and the app's,
    /// by section), then the contributed submenus (`Git ›`…) after a separator.
    pub fn context_menu(&self, place: MenuPlace) -> Vec<MenuEntry> {
        let support = Arc::clone(&self.support);
        let file = self.menu_language();
        let enabled = |item: &workbench_model::ContextMenuItem| {
            self.item_enabled(item.command) && self.menu_when(item.when, file.as_deref())
        };
        let entry = |item: &workbench_model::ContextMenuItem| MenuEntry::Item {
            id: item.command.0.to_owned(),
            label: item.label_key.map_or_else(|| crate::commands::label(item.command), i18n::t),
            shortcut: match item.shortcut {
                Some(spec) => Some(crate::commands::display_shortcut(spec)),
                None => crate::commands::shortcut_label(item.command),
            },
            icon: crate::commands::menu_icon(item.command.0),
            on: None,
            look: Default::default(),
        };
        let mut entries: Vec<MenuEntry> =
            workbench_model::arrange(&support.context_items, place, &enabled)
                .into_iter()
                .map(|row| match row {
                    workbench_model::Arranged::Item(item) => entry(item),
                    workbench_model::Arranged::Submenu(section, items) => {
                        let (label, icon) = section.submenu().unwrap_or_default();
                        MenuEntry::Submenu {
                            label: i18n::t(label),
                            icon: Some(icon),
                            entries: items.into_iter().map(entry).collect(),
                        }
                    }
                    workbench_model::Arranged::Separator => MenuEntry::Separator,
                })
                .collect();
        let submenus = self.contributed_menus(place);
        if !submenus.is_empty() {
            entries.push(MenuEntry::Separator);
            entries.extend(submenus);
        }
        entries
    }

    /// The rows the modules list in `place`'s `section` for a menu opened on `path` (a
    /// tab's "Compare with ›": the other tabs), for a menu the app assembles.
    pub fn listed_items(
        &self,
        place: MenuPlace,
        section: workbench_model::MenuSection,
        path: Option<&Path>,
    ) -> Vec<platform_api::MenuItem> {
        let targets = path.map(|p| workbench_model::Target::new(p.to_path_buf(), false));
        let context = workbench_model::ActionContext::new(place, targets.into_iter().collect());
        let support = Arc::clone(&self.support);
        let listed = support.listed_items.iter();
        listed
            .filter(|l| l.section == section && l.places.contains(&place))
            .flat_map(|l| {
                let rows = l.list.rows(self, &context);
                rows.into_iter().map(|row| {
                    platform_api::MenuItem::command(&l.row_id(&row.argument), &row.label, None)
                })
            })
            .collect()
    }

    /// A row a module listed was clicked (`id`): what it runs. `None`: not such a row;
    /// `Some(true)`: something opened.
    pub fn run_listed(&mut self, id: &str) -> Option<bool> {
        let support = Arc::clone(&self.support);
        let (listed, argument) =
            support.listed_items.iter().find_map(|l| Some((l, l.argument(id)?)))?;
        Some(listed.list.run(self, argument))
    }

    /// The contributed rows of `place`'s `section` (a tab's "Compare with ›", after the
    /// listed rows), for a menu the app assembles.
    pub fn section_items(
        &self,
        place: MenuPlace,
        section: workbench_model::MenuSection,
    ) -> Vec<platform_api::MenuItem> {
        let file = self.menu_language();
        let enabled = |item: &workbench_model::ContextMenuItem| {
            item.section == section
                && self.item_enabled(item.command)
                && self.menu_when(item.when, file.as_deref())
        };
        let rows = workbench_model::arrange(&self.support.context_items, place, &enabled);
        let items = rows.into_iter().flat_map(|row| match row {
            workbench_model::Arranged::Item(item) => vec![item],
            workbench_model::Arranged::Submenu(_, items) => items,
            workbench_model::Arranged::Separator => Vec::new(),
        });
        items
            .map(|item| {
                let label =
                    item.label_key.map_or_else(|| crate::commands::label(item.command), i18n::t);
                let shortcut = item
                    .shortcut
                    .map(str::to_owned)
                    .or_else(|| crate::keymaps::shortcuts(item.command).into_iter().next());
                platform_api::MenuItem::command(item.command.0, &label, shortcut.as_deref())
            })
            .collect()
    }

    /// A context menu's row `id` can run now: a module's command when it's enabled; the
    /// app's own where it applies (its menus only open on something to act on).
    fn item_enabled(&self, id: command_model::CommandId) -> bool {
        if let Some(contributed) = self.support.commands.iter().find(|c| c.spec.id == id) {
            return contributed.handler.enabled(self);
        }
        let _ = id;
        true
    }

    /// The language of the file a menu acts on: the entry right-clicked, else the active
    /// tab's (its editor's). `None`: no file, or plain text.
    fn menu_language(&self) -> Option<String> {
        if let Some(path) = self.menu_target() {
            return files::language_id(&self.support, path);
        }
        let tab = self.tabs.get(self.active)?;
        let id = tab.syntax.language();
        (*id != kernel::LanguageId::PLAIN_TEXT).then(|| id.to_string())
    }

    /// Whether a row's condition holds for a menu on a file in `language`.
    fn menu_when(&self, when: workbench_model::MenuWhen, language: Option<&str>) -> bool {
        use workbench_model::MenuWhen;
        match when {
            MenuWhen::Always => true,
            MenuWhen::Language(id) => language == Some(id),
            MenuWhen::Provides(feature) => language.is_some_and(|language| {
                self.support
                    .intelligence
                    .iter()
                    .find(|i| i.language().as_str() == language)
                    .is_some_and(|i| i.features().contains(&feature))
            }),
            MenuWhen::With(command) => self.item_enabled(command),
        }
    }

    /// The contributed submenus for `place` (each with its enabled commands).
    pub fn contributed_menus(&self, place: MenuPlace) -> Vec<MenuEntry> {
        let support = Arc::clone(&self.support);
        support
            .menus
            .iter()
            .filter_map(|menu| {
                let entries = self.menu_entries(menu, place);
                (!entries.is_empty()).then(|| MenuEntry::Submenu {
                    label: i18n::t(menu.label_key),
                    icon: Some(menu.icon),
                    entries,
                })
            })
            .collect()
    }

    /// Whether the contributed command `id` can run now.
    fn command_enabled(&self, id: command_model::CommandId) -> bool {
        self.support
            .commands
            .iter()
            .find(|c| c.spec.id == id)
            .is_some_and(|c| c.handler.enabled(self))
    }

    /// How the setting command `id` turns on or off stands (`None`: a plain action).
    fn command_switched(&self, id: command_model::CommandId) -> Option<bool> {
        let command = self.support.commands.iter().find(|c| c.spec.id == id)?;
        command.handler.switched(self)
    }

    /// Menu `menu`'s rows in `place`, as menu entries.
    fn menu_entries(
        &self,
        menu: &workbench_model::MenuContribution,
        place: MenuPlace,
    ) -> Vec<MenuEntry> {
        menu.rows_in(place, &|id| self.command_enabled(id))
            .into_iter()
            .map(|row| match row {
                MenuRow::Separator => MenuEntry::Separator,
                MenuRow::Command(id, _) => MenuEntry::Item {
                    id: id.0.to_owned(),
                    label: crate::commands::label(id),
                    shortcut: crate::commands::shortcut_label(id),
                    icon: crate::commands::menu_icon(id.0),
                    on: self.command_switched(id),
                    look: Default::default(),
                },
            })
            .collect()
    }

    /// Tool window `id`, as registered.
    pub(super) fn tool(&self, id: &str) -> Option<&Arc<dyn ui_workbench::ToolWindowProvider>> {
        self.support.tools.iter().find(|t| t.id() == id)
    }

    /// The tool windows of `dock` whose button shows now: a window of views only while its
    /// command is enabled (Git in a repository, Remote in a remote project).
    pub(super) fn docked(
        &self,
        dock: ui_workbench::Dock,
    ) -> Vec<Arc<dyn ui_workbench::ToolWindowProvider>> {
        self.support
            .tools
            .iter()
            .filter(|t| t.dock() == dock && t.available())
            .filter(|t| match t.content() {
                ui_workbench::ToolContent::Views { command, .. }
                | ui_workbench::ToolContent::Command { command, .. } => self.ready(command),
                ui_workbench::ToolContent::Panel => true,
            })
            .cloned()
            .collect()
    }

    /// Tool window `tool`'s button, lit while it shows: its panel open, its views at the
    /// bottom (or its menu open), or — a command — its tab active.
    pub(super) fn tool_button_of(
        &self,
        tool: &Arc<dyn ui_workbench::ToolWindowProvider>,
        bottom: bool,
    ) -> ToolButton {
        let id = tool.id();
        let active = match tool.content() {
            ui_workbench::ToolContent::Panel => self.tools.sides.iter().any(|s| s.id == id),
            ui_workbench::ToolContent::Views { .. } => {
                self.bottom_shows(id)
                    || (self.context_menu.is_open() && self.tools.menu_open == Some(id))
            }
            ui_workbench::ToolContent::Command { tab, .. } => {
                tool.active()
                    || tab.is_some_and(|ext| {
                        self.tabs
                            .get(self.active)
                            .and_then(|t| t.doc.path())
                            .is_some_and(|p| p.extension().is_some_and(|e| e == ext))
                    })
            }
        };
        // A dot while there's something (its count in the tooltip).
        let badge = self.tool_badge(id).filter(|b| b.count > 0).map(|b| match b.kind {
            workbench_model::BadgeKind::Count => ui_workbench::ds::BadgeKind::Count,
            workbench_model::BadgeKind::Warning => ui_workbench::ds::BadgeKind::Warning,
            workbench_model::BadgeKind::Error => ui_workbench::ds::BadgeKind::Error,
        });
        let (icon, group) = (self.icons.id_or_file(tool.icon()), tool.group());
        ToolButton { id, icon, active, bottom, badge, group, tone: tool.active_tone() }
    }

    /// The count a module put on tool window `id`'s button.
    pub(super) fn tool_badge(&self, id: &str) -> Option<&workbench_model::ToolBadge> {
        self.data.tool_badges.iter().find(|(t, _)| *t == id).map(|(_, b)| b)
    }

    /// Runs a tool window's command: a module's here, the app's own through the app (on the
    /// next turn of the loop).
    pub(super) fn run_tool_command(&mut self, id: command_model::CommandId) {
        if self.run_command(id).is_none() {
            self.request = Some(MouseResult::Command(id.0));
        }
    }

    /// Command `id` can run now: a module's, as it says; the app's own, always.
    fn ready(&self, id: &'static str) -> bool {
        let id = command_model::CommandId(id);
        // A workbench command a module implements: only with that module.
        if workbench_model::IMPLEMENTED.contains(&id) {
            return self.support.implementations.iter().any(|c| c.id == id);
        }
        !self.support.commands.iter().any(|c| c.spec.id == id) || self.command_enabled(id)
    }

    /// What tool window `id`'s button does with no view open yet (Git: "Show Git Log").
    pub(super) fn tool_command(&self, id: &str) -> Option<command_model::CommandId> {
        match self.tool(id)?.content() {
            ui_workbench::ToolContent::Views { command, .. }
            | ui_workbench::ToolContent::Command { command, .. } => {
                Some(command_model::CommandId(command))
            }
            ui_workbench::ToolContent::Panel => {
                self.tool(id)?.toggle_command().map(command_model::CommandId)
            }
        }
    }

    /// The menu of the top bar's badge under `p` (a right click): the one its module named.
    /// `false` if it isn't there or has none.
    pub fn top_badge_menu_at(&mut self, p: Point) -> bool {
        let Some((rect, ui_workbench::TopBarHit::Badge)) = self.chrome.top_bar.hit_rect(p) else {
            return false;
        };
        let Some(menu) = self.chrome.badge.as_ref().and_then(|b| b.menu) else { return false };
        let below = Rect::new(rect.min_x() - 6.0 * self.scale, rect.max_y(), 0.0, 0.0);
        self.open_tool_menu(menu, below)
    }

    /// The menu of the tool strip button under `p` (a right click). `false` if there's none.
    pub fn tool_strip_menu_at(&mut self, p: Point) -> bool {
        if let Some((rect, id)) = self.tools.strip.hit_rect(p) {
            let id = id.to_owned();
            return self.open_tool_menu(&id, rect);
        }
        // A right strip's button: its menu beside it, on the left.
        let Some((rect, id)) = self.tools.right_strip.hit_rect(p) else { return false };
        let id = id.to_owned();
        let Some(entries) = self.tool_menu_entries(&id) else { return false };
        self.close_hovers_for_menu();
        self.context_menu.open_left_of(entries, rect, 6.0 * self.scale);
        self.tools.menu_open = menu_id(&Arc::clone(&self.support), &id);
        true
    }

    /// Opens tool strip menu `id` beside its button (`at`: the button). `false` if there's
    /// no such menu.
    pub(super) fn open_tool_menu(&mut self, id: &str, at: Rect) -> bool {
        let Some(entries) = self.tool_menu_entries(id) else { return false };
        self.open_context_menu(entries, Point::new(at.max_x() + 6.0 * self.scale, at.min_y()));
        self.tools.menu_open = menu_id(&Arc::clone(&self.support), id);
        true
    }

    /// The rows of tool strip menu `id`; `None`: no such menu, or nothing in it.
    pub(super) fn tool_menu_entries(&mut self, id: &str) -> Option<Vec<ui_workbench::MenuEntry>> {
        let support = Arc::clone(&self.support);
        let menu = support.menus.iter().find(|m| m.id == id)?;
        Some(self.menu_entries(menu, MenuPlace::ToolStrip)).filter(|e| !e.is_empty())
    }

    /// What the next menu's commands act on (what was right-clicked), with the modules'
    /// facts; `None`: the keyboard's context.
    pub fn set_menu_context(&mut self, context: Option<ActionContext>) {
        self.menu_context = context.map(|c| self.with_facts(c));
        *self.menu_text.get_mut() = None;
        *self.menu_probe.get_mut() = None;
    }

    /// The menu acts on entry `path` (a tab's file, the tree's entry); `None`: the keyboard's
    /// context.
    pub fn set_menu_target(&mut self, path: Option<PathBuf>) {
        let context = path.and_then(|p| self.entry_context(p));
        self.set_menu_context(context);
    }

    pub fn clear_menu_target(&mut self) {
        self.menu_context = None;
        *self.menu_text.get_mut() = None;
        *self.menu_probe.get_mut() = None;
    }

    /// A right click at `p` on a side panel's entries (the project tree's rows): what it's
    /// on, now the menu's context (its facts added). `None`: no entry there.
    pub fn entries_menu_at(&mut self, p: Point) -> Option<ActionContext> {
        let side = self.tools.side_at(p).filter(|s| s.content.contains(p))?;
        let context = side.panel.context_at(p)?;
        self.set_menu_context(Some(context));
        self.menu_context.clone()
    }

    /// What the menu acts on, if it was opened on something.
    pub(super) fn menu_target(&self) -> Option<&Path> {
        self.menu_context.as_ref()?.path()
    }

    /// The context of entry `path`: in the tree's place for a file or a folder, with its mark
    /// (as the tree shows it). `None`: neither on disk nor known to the tree.
    pub(super) fn entry_context(&self, path: PathBuf) -> Option<ActionContext> {
        let node = self.project.as_ref().and_then(|t| Some((t, t.find(&path)?)));
        if node.is_none() && !self.exists(&path) {
            return None;
        }
        let is_dir = match node {
            Some((tree, id)) => tree.kind(id) == NodeKind::Directory,
            None => self.is_dir(&path),
        };
        let mut target = workbench_model::Target::new(path, is_dir);
        if let Some((tree, id)) = node {
            target = target.with_mark(tree.mark(id), tree.is_absent(id));
        }
        let place = if is_dir { MenuPlace::TreeFolder } else { MenuPlace::TreeFile };
        Some(ActionContext::new(place, vec![target]))
    }

    /// The context with what the modules know of its targets.
    fn with_facts(&self, mut context: ActionContext) -> ActionContext {
        let project = self.project.as_ref().map(|p| (self.data.id, p.root_path()));
        for provider in &self.support.context_providers {
            provider.add_facts(project, &mut context);
        }
        context
    }

    /// What a command acts on: the menu's, else the keyboard's — the focused side panel's
    /// selection, else the active tab's file.
    pub(super) fn action_context(&self) -> ActionContext {
        if let Some(context) = &self.menu_context {
            return context.clone();
        }
        let side = match self.focus {
            Focus::Side(dock) if !self.keyboard_in_overlay() => {
                self.tools.side_on(dock).and_then(|s| s.panel.context())
            }
            _ => None,
        };
        let context = side.unwrap_or_else(|| {
            let path = self
                .tabs
                .get(self.active)
                .and_then(|t| t.doc.path().filter(|p| !is_virtual(p)).map(Path::to_path_buf));
            let targets = path.map(|p| workbench_model::Target::new(p, false));
            ActionContext::new(MenuPlace::Editor, targets.into_iter().collect())
        });
        self.with_facts(context)
    }

    /// The menu's target as an active file: its text from its tab if open, else from disk
    /// (only a text file).
    pub(super) fn menu_target_file(&self) -> Option<workbench_model::ActiveFile> {
        let path = self.menu_target_text_path()?;
        let path = &path;
        let open = all_tabs(&self.tabs, &self.editors.groups)
            .find(|t| t.doc.path() == Some(path.as_path()) && t.editor_visible());
        let text: Arc<str> = match open {
            Some(tab) => tab.text(),
            None => {
                let cached = self.menu_text.borrow().clone().filter(|(p, _)| p == path);
                match cached {
                    Some((_, text)) => text,
                    None => {
                        let text: Arc<str> = Arc::from(read_text(&*self.ports.disk, path)?);
                        *self.menu_text.borrow_mut() = Some((path.clone(), Arc::clone(&text)));
                        text
                    }
                }
            }
        };
        Some(workbench_model::ActiveFile {
            path: path.clone(),
            text,
            line: 0,
            cursor: 0,
            selection: None,
        })
    }

    /// The menu's target when it's a text file (open in a tab, or its start looked at once
    /// per menu: a few kilobytes, never the whole file) — what the menu's `enabled` checks
    /// ask, its text left to the command that runs.
    pub(super) fn menu_target_text_path(&self) -> Option<PathBuf> {
        let path = self.menu_target()?;
        let open = all_tabs(&self.tabs, &self.editors.groups)
            .any(|t| t.doc.path() == Some(path) && t.editor_visible());
        if open {
            return Some(path.to_path_buf());
        }
        let known = self.menu_probe.borrow().clone().filter(|(p, _)| p == path);
        let text = match known {
            Some((_, text)) => text,
            None => {
                let check = document::probe_text(path, &*self.ports.disk).ok();
                let text = check == Some(document::TextCheck::Text);
                *self.menu_probe.borrow_mut() = Some((path.to_path_buf(), text));
                text
            }
        };
        text.then(|| path.to_path_buf())
    }
}

/// The contributed menu's static id for `id`.
fn menu_id(support: &LanguageSupport, id: &str) -> Option<&'static str> {
    support.menus.iter().find(|m| m.id == id).map(|m| m.id)
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/menus.rs"]
mod tests;
