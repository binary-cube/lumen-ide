//! The side panel (Structure): breadcrumbs, usages, the panel's actions.

use super::*;

impl Workbench {
    /// The breadcrumb segments: the file, then the declarations containing the cursor.
    /// The breadcrumbs of the active tab (file › class › method), recomputed only when the
    /// document, the cursor, the symbols or the theme change.
    pub(super) fn crumbs(&mut self) -> &[Crumb] {
        let Some(tab) = self.tabs.get(self.active) else { return &[] };
        let key = (
            tab.doc.id(),
            tab.doc.version(),
            tab.doc.selections().primary().head,
            tab.symbols_version,
            tab.editor_visible(),
            tab.title(),
            self.theme_generation,
            self.icons.file_set(),
        );
        let (support, icons, palette, symbols) =
            (&self.support, &self.icons, &self.palette, &self.data.symbols);
        let (file_icons, theme) = (&mut self.file_icons, self.theme_generation);
        let (crumbs, _) = self.crumbs.get_or_update(key, || {
            let title = tab.title();
            let icon = file_icons.get(support, icons, palette, theme, &tab.icon_name());
            // A module's bar starts it (a console: `Database Consoles › shop › console`), the
            // file's icon on its last segment.
            let start = tab
                .toolbar
                .as_ref()
                .map(|t| t.bar.crumbs())
                .or_else(|| tab.viewer.as_ref().map(|v| v.viewer.crumbs()))
                .unwrap_or_default();
            let (mut out, mut targets) = if start.is_empty() {
                (vec![Crumb { label: title, icon: Some(icon) }], vec![None])
            } else {
                let last = start.len() - 1;
                let crumbs = start
                    .into_iter()
                    .enumerate()
                    .map(|(i, label)| Crumb { label, icon: (i == last).then_some(icon) })
                    .collect::<Vec<_>>();
                let targets = vec![None; crumbs.len()];
                (crumbs, targets)
            };
            if tab.editor_visible()
                && let Ok(index) = symbols.read()
                && let Some(file) = index.file(&Self::index_path(tab))
            {
                let head = tab.doc.selections().primary().head;
                for i in code_intelligence::scope_at(file, head) {
                    let symbol = &file.symbols[i];
                    let sigil = support
                        .intelligence
                        .iter()
                        .find(|i| i.language().as_str() == &*file.language)
                        .and_then(|i| i.naming().sigil);
                    out.push(Crumb { label: code_intelligence::label(symbol, sigil), icon: None });
                    targets.push(Some(symbol.offset));
                }
            }
            (out, targets)
        });
        crumbs
    }

    /// Opens / closes side panel `id` (on its side, in place of the one there). `false` if it
    /// doesn't exist.
    pub fn toggle_side_panel(&mut self, id: &str) -> bool {
        if self.tools.sides.iter().any(|s| s.id == id) {
            self.close_side_panel(id);
        } else if let Some(i) = self.tools.parked.iter().position(|s| s.id == id) {
            // Hidden before: back as it was (the tree's folders open, its selection).
            let mut side = self.tools.parked.remove(i);
            side.parked_since = None;
            let dock = side.dock;
            self.park_side(dock);
            self.tools.sides.push(side);
            self.ensure_index();
            if dock == ui_workbench::Dock::Bottom {
                self.set_bottom(Some(id));
            }
        } else if let Some((tool, dock, mut panel)) = self
            .tool(id)
            .filter(|t| {
                matches!(
                    t.dock(),
                    ui_workbench::Dock::Left
                        | ui_workbench::Dock::Right
                        | ui_workbench::Dock::Bottom
                )
            })
            .and_then(|t| Some((t.id(), t.dock(), t.create()?)))
        {
            // What a panel may need from the app (the Task Manager: the resources).
            panel.set_data(&workbench_model::ResourceData(
                Arc::clone(&self.task_data) as Arc<dyn workbench_model::ResourceSource>
            ));
            self.park_side(dock);
            let mut side = SidePanel {
                id: tool,
                dock,
                panel,
                bounds: Rect::default(),
                content: Rect::default(),
                header: PanelHeader::default(),
                key: None,
                project: None,
                dragging: false,
                parked_since: None,
            };
            // What it kept of itself last time (the project's session: a splitter's place).
            if let Some(memento) = self.tools.mementos.get(tool) {
                side.panel.set_data(&ui_workbench::PanelMemento(memento.clone()));
            }
            side.header.set_bottom(dock == ui_workbench::Dock::Bottom);
            // The project at once (what it's told before its first frame finds it).
            self.give_project(&mut side);
            self.tools.sides.push(side);
            self.ensure_index();
            // At the bottom: in place of the window shown there.
            if dock == ui_workbench::Dock::Bottom {
                self.set_bottom(Some(tool));
            }
        } else {
            return false;
        }
        self.layout();
        true
    }

    /// Hides side panel `id`: kept aside (its state too) until shown again.
    pub(super) fn close_side_panel(&mut self, id: &str) {
        let Some(i) = self.tools.sides.iter().position(|s| s.id == id) else { return };
        let mut side = self.tools.sides.remove(i);
        if self.focus == Focus::Side(side.dock) {
            self.focus = Focus::Editor;
        }
        side.dragging = false;
        side.panel.parked();
        side.parked_since = Some(std::time::Instant::now());
        if side.dock == ui_workbench::Dock::Bottom && self.tools.bottom.as_deref() == Some(id) {
            self.tools.bottom = None;
        }
        self.tools.parked.push(side);
    }

    /// The panel open on `dock` set aside (another takes its place).
    fn park_side(&mut self, dock: ui_workbench::Dock) {
        if let Some(id) = self.tools.side_on(dock).map(|s| s.id) {
            self.close_side_panel(id);
        }
    }

    /// Shows side panel `id` (opened if it isn't). `false` if it doesn't exist.
    pub fn show_side_panel(&mut self, id: &str) -> bool {
        if self.tools.sides.iter().any(|s| s.id == id) {
            return true;
        }
        self.toggle_side_panel(id)
    }

    /// The side panel having the keyboard.
    pub(super) fn focused_side(&mut self) -> Option<&mut SidePanel> {
        let Focus::Side(dock) = self.focus else { return None };
        if self.keyboard_in_overlay() {
            return None;
        }
        self.tools.sides.iter_mut().find(|s| s.dock == dock)
    }

    /// An editing command (`copy`, `cut`, `paste`, `delete`, `select_all`) for the side panel
    /// having the keyboard. `true` if it took it.
    pub fn side_command(&mut self, text: &mut TextSystem, id: &str) -> bool {
        let Some(side) = self.focused_side() else { return false };
        if !side.panel.command(id) {
            return false;
        }
        if let Some(action) = side.panel.take_action() {
            self.side_action(text, action);
        }
        true
    }

    /// A side panel's header button under `p`, with its area (its tooltip).
    pub(super) fn side_button_at(&self, p: Point) -> Option<(Rect, String)> {
        let side = self.tools.sides.iter().find(|s| s.header.contains(p))?;
        let (rect, id) = side.header.hit_rect(p)?;
        // A bottom panel's size and hide buttons (the app's, as the bottom windows').
        let own = match id {
            BOTTOM_HIDE => Some("tooltip.hide"),
            super::window_menu::WINDOW_MORE => Some("tooltip.window_options"),
            BOTTOM_HALF => Some(self.bottom_size_tooltip(super::tool_windows::BottomSize::Half)),
            BOTTOM_FULL => Some(self.bottom_size_tooltip(super::tool_windows::BottomSize::Full)),
            _ => None,
        };
        if let Some(key) = own {
            return Some((rect, i18n::t(key)));
        }
        let tooltip = side.panel.header()?.into_iter().find(|b| b.id == id)?.tooltip;
        Some((rect, tooltip))
    }

    /// The bottom area's height (logical pixels), as dragged by its separator (a snapshot's).
    #[cfg(feature = "snapshot")]
    pub fn set_bottom_height(&mut self, height: f32) {
        self.data.sizes.set(workbench_model::sizes::DOCK_BOTTOM, height);
    }

    /// The side panels' background work. `true`: a redraw.
    pub fn poll_side_panel(&mut self) -> bool {
        let mut changed = false;
        for side in &mut self.tools.sides {
            changed |= side.panel.poll();
        }
        changed
    }

    /// Gives the side panels the editor state, if it changed since their last update.
    pub(super) fn sync_side_panel(&mut self) {
        let mut sides = std::mem::take(&mut self.tools.sides);
        for side in &mut sides {
            self.give_project(side);
            self.update_side_panel(side);
        }
        self.tools.sides = sides;
    }

    /// Gives a panel the project's queries, when the project or its files changed since.
    fn give_project(&mut self, side: &mut SidePanel) {
        // By the project's identity: the same folder opened again is another project.
        let key = self.project.as_ref().map(|_| {
            let folders = self.data.indexer.as_ref().and_then(|i| i.peek_folders());
            let index = folders.map_or(0, |f| Arc::as_ptr(&f) as usize);
            (self.data.id, self.data.generations.files(), index)
        });
        if side.project == Some(key) {
            return;
        }
        let view = workbench_model::ProjectView {
            project: key.map(|(id, ..)| id),
            queries: self.quiet_queries(),
            events: self.data.events.clone(),
            indexing: self.data.indexer.as_ref().is_some_and(|i| i.paths_first_build()),
        };
        side.panel.set_data(&view);
        // The project's dragged sizes (a panel's splitter, the views it holds).
        side.panel.set_data(&self.data.sizes);
        side.project = Some(key);
    }

    /// What `language` writes properties after (`$` in PHP).
    fn sigil_of(&self, language: &str) -> Option<char> {
        let intelligence =
            self.support.intelligence.iter().find(|i| i.language().as_str() == language);
        intelligence.and_then(|i| i.naming().sigil)
    }

    pub(super) fn update_side_panel(&mut self, side: &mut SidePanel) {
        // A language without a structure of ours: its server's (asked in the background).
        let served = self.served_outline();
        let Some(tab) = self.tabs.get(self.active) else {
            if side.key.take().is_some() {
                let line_of = |_: usize| 0;
                side.panel.update(&ui_workbench::PanelContext {
                    path: None,
                    file: None,
                    sigil: None,
                    cursor: 0,
                    line_of: &line_of,
                    header: None,
                    problems: &[],
                });
            }
            return;
        };
        let head = tab.doc.selections().primary().head;
        let key = (
            tab.doc.id(),
            tab.doc.version(),
            head,
            tab.symbols_version,
            self.data.served.outline_generation,
        );
        if side.key == Some(key) {
            return;
        }
        let symbols = Arc::clone(&self.data.symbols);
        // On every caret move: never waiting for the indexer (tried again next time; the key
        // isn't kept until it's done).
        let Ok(index) = symbols.try_read() else { return };
        let path = Self::index_path(tab);
        let file = index.file(&path).filter(|f| !f.symbols.is_empty()).or(served.as_deref());
        let current =
            file.and_then(|f| code_intelligence::scope_at(f, head).last().map(|&i| &f.symbols[i]));
        let buffer = tab.doc.buffer();
        let line_of = |o: usize| buffer.line_of(o.min(buffer.len())) as u32;
        // The declaration's header (up to `{` / `;`), with syntax colors.
        let header = current.filter(|s| !s.range.is_empty()).and_then(|s| {
            let start = s.range.start.min(buffer.len());
            // A few lines at most (line ends are always character boundaries).
            let last = (buffer.line_of(start) + 5).min(buffer.line_count().saturating_sub(1));
            let mut end = s.range.end.min(buffer.line_end(last));
            let raw = buffer.slice(start..end);
            if let Some(cut) = raw.find(['{', ';']) {
                end = start + cut;
            }
            let raw = &raw[..end - start];
            let mut colors = Vec::new();
            if let Some(snapshot) = tab.syntax.snapshot() {
                let text_snapshot = buffer.snapshot();
                self.highlighter.highlight(snapshot, &text_snapshot, start..end, &mut self.spans);
                for span in &self.spans {
                    let color = *self.scope_colors.entry(span.scope).or_insert_with(|| {
                        scope_color(&self.support, self.theme.as_ref(), span.scope)
                    });
                    if let Some(color) = color.map(|s| s.color) {
                        let r =
                            span.range.start.max(start) - start..span.range.end.min(end) - start;
                        if r.start < r.end {
                            colors.push((r, color));
                        }
                    }
                }
            }
            let (header, colors) = collapse_whitespace(raw, &colors);
            (!header.is_empty()).then_some((header, colors))
        });
        let problems: Vec<ui_workbench::Problem> = tab
            .diagnostics
            .marks
            .iter()
            .map(|Mark { range, message, code, .. }| ui_workbench::Problem {
                range: range.clone(),
                line: line_of(range.start),
                message: match code {
                    Some(code) => format!("{message} {code}"),
                    None => message.clone(),
                },
            })
            .collect();
        side.panel.update(&ui_workbench::PanelContext {
            path: tab.doc.path(),
            file,
            sigil: file.and_then(|f| self.sigil_of(&f.language)),
            cursor: head,
            line_of: &line_of,
            header: header.as_ref().map(|(h, c)| (h.as_str(), c.as_slice())),
            problems: &problems,
        });
        side.key = Some(key);
    }

    /// Moves the cursor to `offset` in the active document (and scrolls it into view).
    pub(super) fn reveal_offset(&mut self, text: &mut TextSystem, offset: usize) -> bool {
        self.focus = Focus::Editor;
        self.edit(text, |doc| commands::click(doc, offset.min(doc.buffer().len()), false, false))
    }

    /// Moves the cursor to the start of the text on line `line` (after the indentation).
    /// The cursor's line and column (0-based, the column in characters) and the line count.
    pub fn cursor_position(&self) -> Option<(usize, usize, usize)> {
        let tab = self.tabs.get(self.active).filter(|t| t.editor_visible())?;
        let buffer = tab.doc.buffer();
        let head = tab.doc.selections().primary().head;
        let line = buffer.line_of(head);
        let column = buffer.slice(buffer.line_start(line)..head).chars().count();
        Some((line, column, buffer.line_count()))
    }

    /// Moves the cursor to `line`, `column` (0-based, clamped) and shows it ("Go to Line").
    pub fn goto_position(&mut self, text: &mut TextSystem, line: usize, column: usize) -> bool {
        self.focus = Focus::Editor;
        self.jump(text, |doc| {
            let buffer = doc.buffer();
            let line = line.min(buffer.line_count().saturating_sub(1));
            let start = buffer.line_start(line);
            let content = buffer.slice(start..buffer.line_end(line));
            let offset = content.char_indices().nth(column).map_or(content.len(), |(i, _)| i);
            commands::click(doc, start + offset, false, false);
        })
    }

    pub(super) fn reveal_line(&mut self, text: &mut TextSystem, line: usize) -> bool {
        self.focus = Focus::Editor;
        self.jump(text, |doc| {
            let buffer = doc.buffer();
            let line = line.min(buffer.line_count().saturating_sub(1));
            let start = buffer.line_start(line);
            let indent = buffer
                .slice(start..buffer.line_end(line))
                .chars()
                .take_while(|c| c.is_whitespace())
                .map(char::len_utf8)
                .sum::<usize>();
            commands::click(doc, start + indent, false, false);
        })
    }

    /// What a click in the side panel requested.
    pub(super) fn side_action(
        &mut self,
        text: &mut TextSystem,
        action: ui_workbench::ToolPanelAction,
    ) {
        match action {
            ui_workbench::ToolPanelAction::Reveal(offset) => {
                self.reveal_offset(text, offset);
            }
            ui_workbench::ToolPanelAction::View(action) => {
                // As a view's (a file at a line, a command, a request for the app).
                match self.viewer_action(text, action) {
                    MouseResult::None | MouseResult::Redraw => {}
                    request => self.request = Some(request),
                }
            }
            // It searches the project: the files indexed (if they aren't), a fresh view of
            // the project given once they are.
            ui_workbench::ToolPanelAction::RefreshProject => {
                self.ensure_index();
                for side in &mut self.tools.sides {
                    side.project = None;
                }
            }
            ui_workbench::ToolPanelAction::Move { from, into } => {
                self.move_entry(&from, &into);
            }
            ui_workbench::ToolPanelAction::Rename { from, to } => {
                self.rename_entry(&from, &to);
            }
            // A menu asked for by a key or a command: at the panel's top.
            ui_workbench::ToolPanelAction::Menu(entries) => {
                let side = self.focused_side().map(|s| (s.id, s.content));
                if let Some((tool, content)) = side {
                    self.open_panel_menu(
                        tool,
                        entries,
                        Point::new(content.min_x(), content.min_y()),
                    );
                }
            }
            // A menu of a button of its own (a row's "⋮"): under it.
            ui_workbench::ToolPanelAction::MenuUnder { anchor, entries } => {
                let side = self.tools.sides.iter().find(|s| s.content.contains(anchor.origin));
                if let Some(tool) = side.map(|s| s.id) {
                    let at = Point::new(anchor.min_x(), anchor.max_y() + 4.0 * self.scale);
                    self.open_panel_menu(tool, entries, at);
                }
            }
            ui_workbench::ToolPanelAction::Open { path, line } => {
                let Some(root) = self.project.as_ref().map(|p| p.root_path().to_path_buf()) else {
                    return;
                };
                let path = root.join(path);
                if let Err(e) = self.open_file(&path) {
                    tracing::warn!(path = %path.display(), error = %e, "can't open");
                    return;
                }
                self.reveal_line(text, line as usize);
            }
        }
    }

    /// Side panel `tool`'s own menu at `at`: its rows' ids made the app's (`panel-menu:`),
    /// coming back to it.
    pub(super) fn open_panel_menu(
        &mut self,
        tool: &'static str,
        entries: Vec<ui_workbench::ViewMenuEntry>,
        at: Point,
    ) {
        self.tools.menu_panel = Some(tool);
        self.open_context_menu(panel_menu(entries), at);
    }

    /// The own right-click menu of the side panel at `p` (`None`: not over one, or it has
    /// none there).
    pub fn side_menu_at(&mut self, p: Point) -> Option<Vec<ui_workbench::MenuEntry>> {
        let side = self.tools.sides.iter_mut().find(|s| s.content.contains(p))?;
        let entries = side.panel.menu_at(p)?;
        let (tool, dock, takes) = (side.id, side.dock, side.panel.takes_keys());
        if takes {
            self.focus = Focus::Side(dock);
        }
        self.tools.menu_panel = Some(tool);
        Some(panel_menu(entries))
    }

    /// A row of a side panel's own menu chosen: back to that panel, what it asks done.
    pub fn run_panel_menu(&mut self, text: &mut TextSystem, id: &str) -> bool {
        let Some(tool) = self.tools.menu_panel else { return false };
        let Some(side) = self.tools.sides.iter_mut().find(|s| s.id == tool) else { return false };
        let used = side.panel.menu_command(id);
        // It asks for text now (a name to type in place): the keyboard is its, at once,
        // whichever menu it came from (its header's `+` too, not only a right click).
        if side.panel.wants_text() {
            self.focus = Focus::Side(side.dock);
        }
        if let Some(action) = side.panel.take_action() {
            self.side_action(text, action);
        }
        used
    }

    /// A file the system's picker gave (`picked-file:<path>`): to the side panel that asked,
    /// else to the active view.
    pub fn picked_file(&mut self, path: &Path) -> bool {
        let command = format!("picked-file:{}", path.display());
        let mut work = None;
        let mut taken = false;
        for side in &mut self.tools.sides {
            if side.panel.command(&command) {
                // What it does with it that needs the host (writing it in the background,
                // opening what it made).
                if let Some(ui_workbench::ToolPanelAction::View(
                    ui_workbench::ViewerAction::WithHost(job),
                )) = side.panel.take_action()
                {
                    work = Some(job);
                }
                taken = true;
                break;
            }
        }
        if let Some(work) = work {
            work.run(self);
        }
        taken || self.viewer_command(&command)
    }

    /// The system's picker closed with no file: every side panel told
    /// (`picked-file-cancelled`), so one that asked doesn't take the next file another asks for.
    pub fn pick_cancelled(&mut self) {
        for side in &mut self.tools.sides {
            side.panel.command("picked-file-cancelled");
        }
    }

    /// Side panel `tool`'s header button `id`, where it's drawn (snapshots press it).
    #[cfg(any(test, feature = "snapshot"))]
    pub fn side_header_button(&self, tool: &str, id: &str) -> Option<Rect> {
        self.tools.sides.iter().find(|s| s.id == tool)?.header.bounds_of(id)
    }

    /// Side panel `tool`'s content area and the scale (snapshots point into it).
    #[cfg(feature = "snapshot")]
    pub fn side_content(&self, tool: &str) -> Option<(Rect, f32)> {
        Some((self.tools.sides.iter().find(|s| s.id == tool)?.content, self.scale))
    }

    /// The side panel having the keyboard, when its own commands work there
    /// ([`ui_workbench::ToolPanel::keyed`]).
    pub(super) fn keyed_side(&self) -> Option<&SidePanel> {
        let Focus::Side(dock) = self.focus else { return None };
        self.tools.side_on(dock).filter(|s| s.panel.keyed())
    }

    // ─── Colors in code ───
}

/// The buttons the app adds to a bottom panel's header (as the bottom windows have): half
/// the height, all of it, hide.
pub const BOTTOM_HALF: &str = "bottom-half";
pub const BOTTOM_FULL: &str = "bottom-full";
pub const BOTTOM_HIDE: &str = "bottom-hide";

/// The prefix of a side panel's own menu rows' ids among the app's menu commands.
pub const PANEL_MENU: &str = "panel-menu:";

fn panel_menu(entries: Vec<ui_workbench::ViewMenuEntry>) -> Vec<ui_workbench::MenuEntry> {
    use ui_workbench::{MenuEntry, ViewMenuEntry};
    entries
        .into_iter()
        .map(|e| match e {
            ViewMenuEntry::Item { id, label, shortcut, icon, look } => MenuEntry::Item {
                id: format!("{PANEL_MENU}{id}"),
                label,
                shortcut,
                icon,
                on: None,
                look,
            },
            ViewMenuEntry::Submenu { label, icon, entries } => {
                MenuEntry::Submenu { label, icon, entries: panel_menu(entries) }
            }
            ViewMenuEntry::Separator => MenuEntry::Separator,
        })
        .collect()
}
