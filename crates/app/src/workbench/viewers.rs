//! Virtual tabs (the canvas, compare, the theme studio) and viewer actions.

use super::*;

impl Workbench {
    /// Opens `path` at a line and column as written (1-based; a `file:line:column` link),
    /// when given; the editor takes the keyboard.
    pub(super) fn open_file_at(
        &mut self,
        text: &mut TextSystem,
        path: &Path,
        line: Option<u32>,
        column: Option<u32>,
    ) -> MouseResult {
        if let Err(e) = self.open_file(path) {
            tracing::warn!(path = %path.display(), error = %e, "can't open");
            return MouseResult::Redraw;
        }
        if let Some(line) = line {
            let line = line.saturating_sub(1) as usize;
            match column {
                Some(column) => {
                    let offset = self.tabs.get(self.active).map(|t| {
                        let buffer = t.doc.buffer();
                        let line = line.min(buffer.line_count().saturating_sub(1));
                        let start = buffer.line_start(line);
                        let text = buffer.slice(start..buffer.line_end(line));
                        let skip = column.saturating_sub(1) as usize;
                        start + text.chars().take(skip).map(char::len_utf8).sum::<usize>()
                    });
                    if let Some(offset) = offset {
                        self.reveal_offset(text, offset);
                    }
                }
                None => {
                    self.reveal_line(text, line);
                }
            }
        }
        self.focus = Focus::Editor;
        MouseResult::Redraw
    }

    /// Opens the virtual tab `path` (the canvas, a comparison, the theme studio): brings it
    /// to front if it's already open, otherwise asks the module that shows it. `false` if
    /// no module does (or it failed).
    pub(super) fn open_virtual(&mut self, path: PathBuf) -> bool {
        if let Some(i) = self.tabs.iter().position(|t| t.doc.path() == Some(path.as_path())) {
            self.activate(i);
            return true;
        }
        let provider = self
            .support
            .viewers
            .iter()
            .find(|p| p.kind(&path) == Some(ui_workbench::ViewerKind::Replace));
        let Some(provider) = provider else { return false };
        let mut viewer = match provider.open(&path) {
            Ok(viewer) => viewer,
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "the virtual tab didn't open");
                return false;
            }
        };
        self.give_sizes(viewer.as_mut());
        let mut doc = Document::new("");
        doc.mark_saved(path);
        let mut tab = Tab::new(doc, self.code_font.clone(), None, LanguageId::PLAIN_TEXT);
        tab.seen_on_disk(&*self.ports.disk);
        tab.viewer = Some(TabViewer::new(
            viewer,
            provider.id(),
            ui_workbench::ViewerKind::Replace,
            ViewMode::Preview,
        ));
        self.push_tab(tab);
        true
    }

    /// Each tab's view told whether it's visible, only when that changed: the active tab of
    /// each group, its view shown (not its editor alone), the window not hidden. Worked out
    /// here, from the groups as they are, before each frame and when the window is hidden
    /// or shown again — whatever moved the tabs (activated, dragged, split, merged, closed).
    pub(super) fn sync_shown(&mut self) {
        fn tell(tabs: &mut [Tab], active: usize, visible: bool) {
            for (i, tab) in tabs.iter_mut().enumerate() {
                if let Some(v) = &mut tab.viewer {
                    let shown = visible && i == active && v.viewer_visible();
                    if v.shown != shown {
                        v.shown = shown;
                        v.viewer.set_shown(shown);
                    }
                }
            }
        }
        // Under a view drawing the whole window, none is seen.
        let visible = !self.occluded && !self.modal_view_covers();
        tell(&mut self.tabs, self.active, visible);
        let current = self.editors.current;
        for (g, group) in self.editors.groups.iter_mut().enumerate() {
            // The current group's slot holds nothing: its tabs are the fields'.
            if g != current {
                tell(&mut group.tabs, group.active, visible);
            }
        }
    }

    /// The pages the tabs' views have the system's web engine show (each with whether it's
    /// seen: its tab the active one of its group, its view shown, the window not hidden).
    /// A page under a floating overlay (a notification's card) counts as unseen: nothing the
    /// app draws goes over it.
    /// What an app page told: handed to the view that has it (a tab's, a side window's);
    /// `false` if none has it any more.
    pub fn web_event(&mut self, page: u128, event: ui_workbench::WebEvent) -> bool {
        let owns = |p: Option<ui_workbench::WebPage>| p.is_some_and(|p| p.id == page);
        for tab in all_tabs_mut(&mut self.tabs, &mut self.editors.groups) {
            if let Some(v) = &mut tab.viewer
                && owns(v.viewer.web_page())
            {
                v.viewer.web_event(page, event);
                return true;
            }
        }
        for side in &mut self.tools.sides {
            if owns(side.panel.web_page()) {
                side.panel.web_event(page, event);
                return true;
            }
        }
        tracing::debug!("a page's message for a view gone let go");
        false
    }

    /// What the views (the tabs', the side windows') have for their app pages.
    pub fn take_web_sends(&mut self) -> Vec<(u128, String)> {
        let mut sends = Vec::new();
        for tab in all_tabs_mut(&mut self.tabs, &mut self.editors.groups) {
            if let Some(v) = &mut tab.viewer {
                sends.extend(v.viewer.take_web_sends());
            }
        }
        for side in &mut self.tools.sides {
            sends.extend(side.panel.take_web_sends());
        }
        sends
    }

    /// The views' pages (the system's web engine's), each with where it stands: its view not
    /// seen, seen with a window over it (the window open over the editor isn't counted
    /// here), or seen.
    pub fn web_pages(&self) -> Vec<(ui_workbench::WebPage, Sight)> {
        let mut pages: Vec<(ui_workbench::WebPage, bool)> =
            all_tabs(&self.tabs, &self.editors.groups)
                .filter_map(|tab| {
                    let v = tab.viewer.as_ref()?;
                    Some((v.viewer.web_page()?, v.shown))
                })
                .collect();
        // A side or bottom window's (a Services tab's preview), while it's shown.
        pages.extend(
            self.tools
                .sides
                .iter()
                .filter(|s| s.parked_since.is_none())
                .filter_map(|s| Some((s.panel.web_page()?, true))),
        );
        if pages.is_empty() {
            return Vec::new();
        }
        let floating = self.floating_areas();
        // What floats over a page shows through it where pages can have holes (macOS,
        // Windows): nothing to make room for then, only what covers more (files dragged in)
        // hides it. Elsewhere a window that stays (the bar's, pinned; the resources' pills in the
        // corner) over a page's lower half makes it shorter, ending above it — hidden, it
        // would stay hidden as long as it's pinned — and any other window over a page hides
        // it while it's open (a page is the system's view, always over what the app draws).
        let held: Vec<kernel::Rect> = if self.page_holes_ok {
            [self.status_popover.bounds(), self.resource_pills.bounds(), self.toasts.bounds()]
                .into_iter()
                .flatten()
                .collect()
        } else {
            Vec::new()
        };
        let mut pinned: Vec<kernel::Rect> = Vec::new();
        if !self.page_holes_ok && self.status_popover.pinned() {
            pinned.extend(self.status_popover.bounds());
        }
        if !self.page_holes_ok && self.resource_pills.is_open() {
            pinned.extend(self.resource_pills.bounds());
        }
        let gap = ui_workbench::ds::POPOVER_GAP * self.scale;
        pages
            .into_iter()
            .map(|(mut page, seen)| {
                for r in &pinned {
                    page.area = above(page.area, *r, gap);
                }
                let under = floating
                    .iter()
                    .filter(|r| !pinned.contains(r) && !held.contains(r))
                    .any(|r| r.intersect(&page.area).is_some());
                let sight = match () {
                    _ if !seen || page.area.size.height <= 0.0 => Sight::Away,
                    _ if under => Sight::Under,
                    _ => Sight::Shown,
                };
                (page, sight)
            })
            .collect()
    }

    /// The corners of a page at `area` (physical) to round as the card it's in (the
    /// editor's, a side or bottom window's): those on a card's corner (within its border),
    /// and their radius — `(radius, corners)`, the corners as [`platform_api::Corners`]'
    /// bits (none: square).
    pub fn page_corners(&self, area: kernel::Rect) -> (f32, u8) {
        let radius = (CARD_RADIUS * self.scale).round();
        let near = self.scale.round().max(1.0) + 1.0;
        let cards = std::iter::once(self.card).chain(self.tools.sides.iter().map(|s| s.bounds));
        for card in cards {
            if card.intersect(&area).is_none() {
                continue;
            }
            let left = (area.min_x() - card.min_x()).abs();
            let right = (card.max_x() - area.max_x()).abs();
            let top = (area.min_y() - card.min_y()).abs();
            let bottom = (card.max_y() - area.max_y()).abs();
            let mut corners = 0;
            let mut inset: f32 = 0.0;
            for (bit, dx, dy) in [
                (platform_api::Corners::TOP_LEFT, left, top),
                (platform_api::Corners::TOP_RIGHT, right, top),
                (platform_api::Corners::BOTTOM_LEFT, left, bottom),
                (platform_api::Corners::BOTTOM_RIGHT, right, bottom),
            ] {
                if dx <= near && dy <= near {
                    corners |= bit;
                    inset = inset.max(dx.max(dy));
                }
            }
            if corners != 0 {
                return ((radius - inset).max(0.0), corners);
            }
        }
        (0.0, 0)
    }

    /// What floats over the pages and shows through them (each a rect with its corners'
    /// radius; physical): the bar's window or the resources' pills, the notifications, the
    /// tooltip. Nothing where pages can't have holes.
    pub fn page_holes(&self) -> Vec<(kernel::Rect, f32)> {
        if !self.page_holes_ok {
            return Vec::new();
        }
        let mut holes = Vec::new();
        holes.extend_from_slice(self.status_popover.shapes());
        holes.extend_from_slice(self.resource_pills.shapes());
        if let Some(r) = self.toasts.bounds() {
            holes.push((r, ui_workbench::ds::radius::LG * self.scale));
        }
        holes.extend_from_slice(self.tooltips.areas());
        holes
    }

    /// Whether the pages can have holes ([`Self::page_holes`]): the window says.
    pub fn set_page_holes(&mut self, ok: bool) {
        self.page_holes_ok = ok;
    }

    /// The pictures drawn in place of the pages a window is open over (page id, picture),
    /// for the next frames: drawn under the windows, where their pages were.
    pub fn set_page_pictures(&mut self, pictures: &[(u128, render_api::ImageData)]) {
        self.page_pictures.clear();
        self.page_pictures.extend(pictures.iter().cloned());
    }

    /// The pictures of the pages a window is open over, where those pages stand.
    pub(super) fn paint_page_pictures(&self, scene: &mut Scene) {
        if self.page_pictures.is_empty() {
            return;
        }
        for (page, _) in self.web_pages() {
            if let Some((_, picture)) = self.page_pictures.iter().find(|(id, _)| *id == page.id)
                && fits(picture, page.area)
            {
                scene.push_layer(Some(page.area)).image(picture, page.area);
            }
        }
    }

    /// The window hidden (occluded, minimized) or shown again: its views told at once (no
    /// frame is drawn while it's hidden).
    pub fn set_occluded(&mut self, occluded: bool) {
        if self.occluded != occluded {
            self.occluded = occluded;
            self.sync_shown();
        }
    }

    /// The tab of a module's view with `extension` brought to the front, in whichever group
    /// it is (that group made current). `false`: none is open.
    pub(super) fn focus_view_tab(&mut self, extension: &str) -> bool {
        let of = |t: &Tab| is_view_of(t, extension);
        if let Some(i) = self.tabs.iter().position(of) {
            self.activate(i);
            self.focus = Focus::Editor;
            return true;
        }
        let current = self.editors.current;
        let found = self.editors.groups.iter().enumerate().find_map(|(g, group)| {
            (g != current).then(|| group.tabs.iter().position(of).map(|i| (g, i))).flatten()
        });
        let Some((g, i)) = found else { return false };
        self.focus_group(g);
        self.activate(i);
        self.focus = Focus::Editor;
        true
    }

    /// The group a module's view `path` opens in, when not the current one: the group it's
    /// already open in, else — the current group having none of its kind — a group that has
    /// (another file's local history joins the first one's tabs instead of a new split
    /// growing under the editor for each).
    pub(super) fn group_for_view(&self, path: &Path) -> Option<usize> {
        let current = self.editors.current;
        let others = || (0..self.editors.groups.len()).filter(move |&g| g != current);
        let groups = &self.editors.groups;
        let open_in = |g: usize| groups[g].tabs.iter().any(|t| t.doc.path() == Some(path));
        if self.tabs.iter().any(|t| t.doc.path() == Some(path)) {
            return None;
        }
        if let Some(g) = others().find(|&g| open_in(g)) {
            return Some(g);
        }
        let extension = path.extension()?.to_str()?;
        if self.tabs.iter().any(|t| is_view_of(t, extension)) {
            return None;
        }
        others().find(|&g| groups[g].tabs.iter().any(|t| is_view_of(t, extension)))
    }

    /// A view just made gets the project's dragged sizes (it reads its own, and keeps it
    /// there when the user drags it again).
    pub(super) fn give_sizes(&self, viewer: &mut dyn ui_workbench::Viewer) {
        viewer.set_data(&self.data.sizes);
    }

    /// The active tab's viewer gets `data` (e.g. the comparison's two files).
    pub(super) fn set_viewer_data(&mut self, data: &dyn std::any::Any) {
        if let Some(v) = self.tabs.get_mut(self.active).and_then(|t| t.viewer.as_mut()) {
            v.viewer.set_data(data);
        }
    }

    /// What a viewer requested (click, keys, commands).
    pub(super) fn viewer_action(
        &mut self,
        text: &mut TextSystem,
        action: ui_workbench::ViewerAction,
    ) -> MouseResult {
        use ui_workbench::ViewerAction;
        match action {
            ViewerAction::OpenLink(link)
                if link.contains(':') && !self.exists(Path::new(&link)) =>
            {
                MouseResult::OpenUrl(link)
            }
            ViewerAction::OpenLink(link) => MouseResult::Open(PathBuf::from(link)),
            ViewerAction::OpenAt { path, line } => {
                self.focus_code_group();
                if let Err(e) = self.open_file(&path) {
                    tracing::warn!(path = %path.display(), error = %e, "can't open");
                } else {
                    self.reveal_line(text, line as usize);
                }
                MouseResult::Redraw
            }
            ViewerAction::OpenFile { path, line, column } => {
                self.focus_code_group();
                self.open_file_at(text, &path, line, column)
            }
            ViewerAction::Request(request) => MouseResult::ViewerRequest(request),
            ViewerAction::Close => {
                self.close_tab(self.active);
                MouseResult::Redraw
            }
            // A module's command runs here; the app's own (e.g. a panel's "hide", its
            // toggle) goes to the app, as from the menu.
            ViewerAction::RunCommand(id) => match self.run_command_named(&id) {
                Some(_) => MouseResult::Redraw,
                None => crate::commands::by_id(&id)
                    .map_or(MouseResult::Redraw, |command| MouseResult::Command(command.0)),
            },
            ViewerAction::SetText { path, text } => {
                self.set_file_text(&path, &text);
                MouseResult::Redraw
            }
            ViewerAction::SaveText { path, text } => {
                if let Err(e) = self.save_text(&path, &text) {
                    let name = path.display().to_string();
                    let message =
                        i18n::tr!("files.save_failed", name = name, reason = e.to_string());
                    workbench_model::WorkbenchHost::notify(
                        self,
                        workbench_model::Notification::error(message),
                    );
                }
                MouseResult::Redraw
            }
            ViewerAction::Compare(input, title) => {
                workbench_model::ViewHost::compare(self, *input, &title);
                MouseResult::Redraw
            }
            ViewerAction::CompareBeside(input, title) => {
                self.compare_beside(*input, &title);
                MouseResult::Redraw
            }
            ViewerAction::Ask(asked) => {
                asked.send(self);
                // What it changed may be shown by the settings under the view that asked.
                if self.settings.is_open() {
                    self.settings_model.blocks = self.settings_blocks();
                }
                MouseResult::Redraw
            }
            ViewerAction::OpenView(view) => {
                let data: &dyn std::any::Any = &*view.data;
                let opened = if view.modal {
                    workbench_model::ViewHost::open_modal(self, &view.title, view.extension, data)
                } else {
                    workbench_model::ViewHost::open_view(self, &view.title, view.extension, data)
                };
                if let (false, Some(why)) = (opened, view.unavailable) {
                    workbench_model::WorkbenchHost::notify(
                        self,
                        workbench_model::Notification::info(why),
                    );
                }
                MouseResult::Redraw
            }
            ViewerAction::WithHost(work) => {
                work.run(self);
                MouseResult::Redraw
            }
            ViewerAction::ApplyRefactoring(plan) => {
                // The preview's tab (the active one) closes once it's applied.
                let preview = self.active;
                match self.apply_refactoring(&plan) {
                    Ok(_) => self.close_tab(preview),
                    Err(e) => workbench_model::WorkbenchHost::notify(
                        self,
                        workbench_model::Notification::error(e),
                    ),
                }
                MouseResult::Redraw
            }
            ViewerAction::Replace { path, lines, text: replacement } => {
                // The view that asked rebuilds what it shows itself (the canvas).
                self.replace_lines(&path, lines, &replacement);
                MouseResult::Redraw
            }
        }
    }

    /// Replaces file `path`'s whole text in its tab (opened now if it isn't), left unsaved —
    /// one undo step brings the previous text back.
    /// File `path` gets `text` (in its tab, opened if needed) and is saved from there.
    /// Saved, the view that asked (the comparison) is done: it closes and the file's tab
    /// comes to front; not saved, the view stays in front (its work kept).
    pub(super) fn save_text(&mut self, path: &Path, text: &str) -> io::Result<()> {
        let asking = self.tabs.get(self.active).filter(|t| t.viewer.is_some()).map(|t| t.doc.id());
        self.set_file_text(path, text);
        let Some(i) = self.tabs.iter().position(|t| t.doc.path() == Some(path)) else {
            return Err(io::ErrorKind::NotFound.into());
        };
        let saved = self.save_tab(i, None);
        let view = asking.and_then(|id| self.tabs.iter().position(|t| t.doc.id() == id));
        match (&saved, view) {
            (Ok(()), Some(view)) => {
                self.close_tab(view);
                if let Some(file) = self.tabs.iter().position(|t| t.doc.path() == Some(path)) {
                    self.activate(file);
                }
            }
            (Err(_), Some(view)) => self.activate(view),
            _ => {}
        }
        saved
    }

    pub(super) fn set_file_text(&mut self, path: &Path, text: &str) {
        let open = all_tabs(&self.tabs, &self.editors.groups).any(|t| t.doc.path() == Some(path));
        if !open && let Err(e) = self.open_file(path) {
            tracing::warn!(path = %path.display(), error = %e, "can't open");
            return;
        }
        if let Some(tab) = all_tabs_mut(&mut self.tabs, &mut self.editors.groups)
            .find(|t| t.doc.path() == Some(path))
        {
            // The document keeps `\n` and writes its own line ending on save: a text with
            // `\r\n` (a comparison's result of a Windows file) would be saved as `\r\r\n`.
            let text: std::borrow::Cow<'_, str> =
                if text.contains('\r') { text.replace("\r\n", "\n").into() } else { text.into() };
            let len = tab.doc.buffer().len();
            tab.doc.apply(
                document::Transaction::single(0..len, text.as_ref()),
                None,
                document::EditKind::Command,
            );
        }
    }

    /// Replaces lines `lines` of file `path`: in its tab (from any group), or opened now
    /// in a new tab, left unsaved (one undo step).
    pub(super) fn replace_lines(
        &mut self,
        path: &Path,
        lines: std::ops::RangeInclusive<u32>,
        text: &str,
    ) {
        let apply = |doc: &mut Document| {
            let buffer = doc.buffer();
            let last = buffer.line_count().saturating_sub(1);
            let first = (*lines.start() as usize).min(last);
            let end_line = (*lines.end() as usize).min(last);
            let range = buffer.line_start(first)..buffer.line_end(end_line);
            doc.apply(
                document::Transaction::single(range, text),
                None,
                document::EditKind::Command,
            );
        };
        let open = all_tabs_mut(&mut self.tabs, &mut self.editors.groups)
            .find(|t| t.doc.path() == Some(path) && t.holds_text());
        if let Some(tab) = open {
            apply(&mut tab.doc);
            return;
        }
        // The file isn't open: it gets opened (without leaving the tab that asked).
        let asking = self.tabs.get(self.active).map(|t| t.doc.id());
        if let Err(e) = self.open_file(path) {
            tracing::warn!(path = %path.display(), error = %e, "can't open");
            return;
        }
        if let Some(tab) = self.tabs.get_mut(self.active) {
            apply(&mut tab.doc);
        }
        if let Some(back) = asking.and_then(|id| self.tabs.iter().position(|t| t.doc.id() == id)) {
            self.activate(back);
        }
    }

    /// A command for the active viewer (e.g. ⌘K on the canvas). `true` if it used it.
    pub fn viewer_command(&mut self, id: &str) -> bool {
        if let Some(used) = self.modal_view_command(id) {
            return used;
        }
        if self.focus == Focus::Panel
            && let Some(viewer) = self.tools.panel_viewer()
        {
            return viewer.command(id);
        }
        // The side panel having the keys, for its own commands (F5 in the "Database"
        // window); what it doesn't take goes on to the view.
        if let Focus::Side(dock) = self.focus
            && !self.keyboard_in_overlay()
            && let Some(side) = self.tools.sides.iter_mut().find(|s| s.dock == dock)
            && side.panel.keyed()
            && side.panel.command(id)
        {
            return true;
        }
        self.tabs
            .get_mut(self.active)
            .and_then(|t| t.viewer.as_mut())
            .is_some_and(|v| v.viewer.command(id))
    }

    /// Takes the action produced by the active viewer's keys.
    pub(crate) fn take_viewer_action(&mut self, text: &mut TextSystem) {
        let action = self.held_view_action.take().or_else(|| {
            self.tabs
                .get_mut(self.active)
                .and_then(|t| t.viewer.as_mut())
                .and_then(|v| v.viewer.take_action())
        });
        if let Some(action) = action {
            // A request for the app (e.g. the configurator's theme) reaches it.
            if let MouseResult::ViewerRequest(request) = self.viewer_action(text, action) {
                self.request = Some(MouseResult::ViewerRequest(request));
            }
        }
    }

    /// What the active view asked for with a command (from the palette, its keys): a request
    /// for the app (text for the clipboard) or a command, done as after a click; the rest
    /// (needing the text system) at the next action's turn.
    pub(super) fn take_command_action(&mut self) {
        use ui_workbench::ViewerAction;
        let action = self
            .tabs
            .get_mut(self.active)
            .and_then(|t| t.viewer.as_mut())
            .and_then(|v| v.viewer.take_action());
        match action {
            Some(ViewerAction::Request(request)) => {
                self.request = Some(MouseResult::ViewerRequest(request));
            }
            Some(ViewerAction::RunCommand(id)) => {
                if !workbench_model::WorkbenchHost::request_command_named(self, &id) {
                    self.held_view_action = Some(ViewerAction::RunCommand(id));
                }
            }
            Some(other) => self.held_view_action = Some(other),
            None => {}
        }
    }

    /// The work-in-progress theme the active view (the theme studio) asked to preview, taken
    /// (snapshots: the app applies it as it would).
    #[cfg(feature = "snapshot")]
    pub fn take_theme_preview(&mut self) -> Option<String> {
        self.take_command_action();
        match self.request.take() {
            Some(MouseResult::ViewerRequest(ui_workbench::AppRequest::PreviewTheme(src))) => {
                Some(src)
            }
            other => {
                self.request = other;
                None
            }
        }
    }

    /// File `path`'s text as the editor has it (unsaved changes included), else as on disk.
    pub(super) fn file_text(&self, path: &Path) -> Option<String> {
        let open = all_tabs(&self.tabs, &self.editors.groups)
            .find(|t| t.doc.path() == Some(path) && t.holds_text());
        match open {
            Some(tab) => Some(tab.doc.buffer().text()),
            None => read_text(&*self.ports.disk, path),
        }
    }

    #[cfg(feature = "snapshot")]
    /// Starts indexing the project files (diagnostics / snapshots).
    pub fn start_file_index(&mut self) {
        self.ensure_index();
    }

    #[cfg(feature = "snapshot")]
    /// The active viewer still waits for background work (diagnostics / snapshots, a graph).
    pub fn viewer_busy(&mut self) -> bool {
        let tab = self
            .tabs
            .get_mut(self.active)
            .and_then(|t| t.viewer.as_mut())
            .is_some_and(|v| v.viewer.poll());
        // The bottom window's view too (the Git log).
        tab | self.poll_tool_windows() | self.poll_modal_view()
    }

    #[cfg(feature = "snapshot")]
    /// The active tab's view still works in the background (snapshots wait for it).
    pub fn viewer_working(&self) -> bool {
        self.tabs
            .get(self.active)
            .and_then(|t| t.viewer.as_ref())
            .is_some_and(|v| v.viewer.working())
    }

    /// Viewers with new content after a wake (e.g. a decoded image). `true`: redraw.
    pub fn poll_viewers(&mut self) -> bool {
        let mut changed = self.poll_settings_blocks()
            | self.poll_tool_windows()
            | self.poll_modal_view()
            | self.poll_fetching()
            | self.poll_notifications()
            | self.poll_indicators()
            | self.poll_side_panel();
        // The Task Manager reads the app's parts: kept fresh while it's open.
        self.refresh_task_data_if_watched();
        // The project services' background work, back here (a message, a comparison).
        let support = Arc::clone(&self.support);
        for service in &support.project_services {
            #[cfg(test)]
            {
                changed |= crate::modules::shared(|| service.poll(self));
            }
            #[cfg(not(test))]
            {
                changed |= service.poll(self);
            }
        }
        for tab in all_tabs_mut(&mut self.tabs, &mut self.editors.groups) {
            if let Some(v) = &mut tab.viewer {
                // A hidden view keeps up (polled), but redraws the window only when what the
                // tab bar shows of it changed (its unsaved mark): a request streaming, a
                // console running in a tab behind another doesn't redraw what's shown.
                let dirty = v.viewer.is_dirty();
                let polled = v.viewer.poll();
                changed |= polled && (v.shown || v.viewer.is_dirty() != dirty);
            }
        }
        changed |= self.poll_toolbars();
        self.poll_view_notifications();
        changed
    }

    #[cfg(feature = "snapshot")]
    /// The project services' background work is done (snapshots wait for it): polled, so
    /// what it found shows. A project without a repository has no git work to wait for.
    /// Looked at before the poll: work ending between the two would otherwise count as
    /// settled before its result was taken.
    pub fn services_settled(&mut self) -> bool {
        let support = Arc::clone(&self.support);
        let id = workbench_model::WorkbenchHost::project_id(self);
        let busy = id.is_some_and(|id| support.project_services.iter().any(|s| s.busy(id)));
        self.poll_viewers();
        !busy
    }

    // ─── Editor groups (split) ───

    /// The project theme (from the session), if it has its own.
    pub fn project_theme(&self) -> Option<&str> {
        self.project_theme.as_deref()
    }

    pub fn set_project_theme(&mut self, theme: Option<String>) {
        self.project_theme = theme;
    }

    /// Closes the view that asked, done (the theme configurator, its theme saved or given
    /// up): the active tab, if it's a view.
    pub fn close_asking_view(&mut self) {
        if self.tabs.get(self.active).is_some_and(|t| t.viewer.is_some()) {
            self.close_tab(self.active);
        }
    }
}

impl Workbench {
    /// The right-click menu of the active tab's view at `p` (`None`: not over a view, or
    /// it has none): its rows' ids made the app's (`view-menu:` + the view's id).
    pub fn viewer_menu_at(&mut self, p: Point) -> Option<Vec<ui_workbench::MenuEntry>> {
        // A module's window over everything: its menu only (never what's under it).
        if let Some(menu) = self.modal_view_menu(p) {
            return menu.map(view_menu);
        }
        let v = self.tabs.get_mut(self.active)?.viewer.as_mut()?;
        if !v.viewer_visible() || !v.bounds.contains(p) {
            return None;
        }
        self.focus = Focus::Editor;
        let entries = v.viewer.context_menu(p)?;
        Some(view_menu(entries))
    }

    /// A row of a view's menu chosen: sent to the view as a command, what it asks done.
    pub fn run_view_menu(&mut self, text: &mut TextSystem, id: &str) -> bool {
        if let Some(used) = self.modal_view_command(id) {
            self.take_modal_action(text);
            return used;
        }
        let used = self
            .tabs
            .get_mut(self.active)
            .and_then(|t| t.viewer.as_mut())
            .is_some_and(|v| v.viewer.command(id));
        self.take_viewer_action(text);
        used
    }
}

/// The prefix of a view's menu rows' ids among the app's menu commands.
pub const VIEW_MENU: &str = "view-menu:";

fn view_menu(entries: Vec<ui_workbench::ViewMenuEntry>) -> Vec<ui_workbench::MenuEntry> {
    prefixed_menu(entries, VIEW_MENU)
}

/// A view's menu rows as the app's, their ids behind `prefix` (where they go back to).
pub(super) fn prefixed_menu(
    entries: Vec<ui_workbench::ViewMenuEntry>,
    prefix: &str,
) -> Vec<ui_workbench::MenuEntry> {
    use ui_workbench::{MenuEntry, ViewMenuEntry};
    entries
        .into_iter()
        .map(|e| match e {
            ViewMenuEntry::Item { id, label, shortcut, icon, look } => MenuEntry::Item {
                id: format!("{prefix}{id}"),
                label,
                shortcut,
                icon,
                on: None,
                look,
            },
            ViewMenuEntry::Submenu { label, icon, entries } => {
                MenuEntry::Submenu { label, icon, entries: prefixed_menu(entries, prefix) }
            }
            ViewMenuEntry::Separator => MenuEntry::Separator,
        })
        .collect()
}

#[cfg(any(test, feature = "snapshot"))]
impl Workbench {
    /// A point of the active tab's view, at fractions of its width and height.
    #[cfg(feature = "snapshot")]
    pub fn viewer_point(&self, x: f32, y: f32) -> Option<Point> {
        let b = self.tabs.get(self.active)?.viewer.as_ref()?.bounds;
        Some(Point::new(b.min_x() + b.size.width * x, b.min_y() + b.size.height * y))
    }

    /// The active tab's preview beside its text (snapshots: a CSV's data and text).
    pub fn set_viewer_mode_split(&mut self) {
        let (area, scale) = (self.editor_area, self.scale);
        if let Some(tab) = self.tabs.get_mut(self.active)
            && let Some(v) = &mut tab.viewer
            && v.kind == ui_workbench::ViewerKind::Preview
        {
            v.mode = ViewMode::Split;
            tab.sync_bounds(area, scale);
        }
    }
}

/// Tab `t` shows a module's view of files with `extension`.
fn is_view_of(t: &Tab, extension: &str) -> bool {
    t.viewer.is_some()
        && t.doc
            .path()
            .is_some_and(|p| is_virtual(p) && p.extension().is_some_and(|e| e == extension))
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/viewers.rs"]
mod tests;

/// Whether `picture` is a page's at `area`'s size (a pixel or two apart, rounding): one
/// taken at another size isn't drawn, stretched or squeezed (its page moved under a window).
pub(crate) fn fits(picture: &render_api::ImageData, area: kernel::Rect) -> bool {
    (picture.width as f32 - area.size.width).abs() <= 2.0
        && (picture.height as f32 - area.size.height).abs() <= 2.0
}

/// Where a view's page stands ([`Workbench::web_pages`]): its view not seen, seen with a
/// window over it (its picture drawn in its place), or seen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sight {
    Away,
    Under,
    Shown,
}

impl From<bool> for Sight {
    fn from(seen: bool) -> Self {
        if seen { Sight::Shown } else { Sight::Away }
    }
}

/// `page` made to end `gap` above `window` when that stands over its lower half (else as
/// it is): a window that stays never hides a page.
pub(crate) fn above(page: kernel::Rect, window: kernel::Rect, gap: f32) -> kernel::Rect {
    let middle = page.min_y() + page.size.height / 2.0;
    if window.intersect(&page).is_none() || window.min_y() < middle {
        return page;
    }
    let height = (window.min_y() - gap - page.min_y()).max(0.0);
    kernel::Rect::new(page.min_x(), page.min_y(), page.size.width, height)
}
