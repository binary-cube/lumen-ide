//! The bars modules put above a file's editor (a query console's: Run, Stop, the
//! transaction, the schema), from their [`EditorToolbarProvider`]s: asked when a file's tab
//! opens, laid out above its text (the text below the bar, never under it), given the
//! clicks, the hover and their menus' rows; their title, icon, color and breadcrumb taken
//! for the tab, their marks drawn in the gutter, asked before the tab closes. The app knows
//! nothing of what they do.
//!
//! [`EditorToolbarProvider`]: ui_workbench::editor_toolbar::EditorToolbarProvider

use super::*;
use ui_workbench::StatementLines;
use ui_workbench::editor_toolbar::{
    EditorState, EditorToolbar, ToolbarAction, ToolbarContext, ToolbarPaint,
};

/// The prefix of a bar's menu rows' ids among the app's menu commands.
pub const BAR_MENU: &str = "bar-menu:";

/// A tab's bar (contributed by a module) and what the app keeps for it.
pub(super) struct TabToolbar {
    pub bar: Box<dyn EditorToolbar>,
    /// Where it was laid out (above the text).
    pub bounds: Rect,
    /// What it was last told of the editor: the version, the cursor, the selection.
    told: Option<(u64, usize, Option<std::ops::Range<usize>>)>,
    /// Its gutter marks as lines, for (the document's version, the marks' generation).
    lines: Memo<(u64, u64), Vec<StatementLines>>,
    /// The mouse is over it.
    hovered: bool,
}

impl std::fmt::Debug for TabToolbar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TabToolbar").field("bounds", &self.bounds).finish_non_exhaustive()
    }
}

impl TabToolbar {
    pub(super) fn new(bar: Box<dyn EditorToolbar>) -> Self {
        Self { bar, bounds: Rect::default(), told: None, lines: Memo::default(), hovered: false }
    }

    /// Its height at `scale` (physical pixels).
    pub(super) fn height(&self, scale: f32) -> f32 {
        (self.bar.height() * scale).round()
    }

    /// Its marks as `doc`'s lines: only those whose text is still the one that ran.
    pub(super) fn statement_lines(&mut self, doc: &Document) -> &[StatementLines] {
        let (marks, generation) = self.bar.marks();
        let key = (doc.version(), generation);
        self.lines.get_or_update(key, || {
            let buffer = doc.buffer();
            let len = buffer.len();
            let mut out: Vec<StatementLines> = marks
                .iter()
                .filter(|m| m.range.start <= m.range.end && m.range.end <= len)
                .filter(|m| {
                    ui_workbench::editor_toolbar::text_hash(&buffer.slice(m.range.clone()))
                        == m.hash
                })
                .map(|m| StatementLines {
                    lines: buffer.line_of(m.range.start)..buffer.line_of(m.range.end) + 1,
                    state: m.state,
                })
                .collect();
            out.sort_by_key(|l| l.lines.start);
            out.dedup_by_key(|l| l.lines.start);
            out
        })
    }

    /// The place its focus points at in `doc` (a finding's), in its severity's color.
    pub(super) fn focus_of(
        &self,
        doc: &Document,
        palette: &ui_workbench::Palette,
    ) -> Option<(usize, usize, kernel::Rgba)> {
        let (range, level) = self.bar.focus()?;
        (range.start <= range.end && range.end <= doc.buffer().len())
            .then(|| (range.start, range.end, level.color(palette)))
    }

    /// The lines its highlight covers in `doc`.
    pub(super) fn highlight_lines(&self, doc: &Document) -> Option<(usize, usize)> {
        lines_of(doc, self.bar.highlight()?)
    }

    /// The lines of the statement its cursor is in (the bar beside them).
    pub(super) fn current_lines(&self, doc: &Document) -> Option<(usize, usize)> {
        lines_of(doc, self.bar.current()?)
    }
}

/// The lines `range` spans in `doc` (the first and the one after the last), if it's inside.
fn lines_of(doc: &Document, range: std::ops::Range<usize>) -> Option<(usize, usize)> {
    let buffer = doc.buffer();
    (range.start <= range.end && range.end <= buffer.len())
        .then(|| (buffer.line_of(range.start), buffer.line_of(range.end) + 1))
}

impl Workbench {
    /// The bar a module puts above file `path` of `language` (none: no module knows it).
    pub(super) fn toolbar_for(&self, path: &Path, language: Option<&str>) -> Option<TabToolbar> {
        let wake: Arc<dyn Fn() + Send + Sync> = match &self.waker {
            Some(w) => Arc::clone(&w.0),
            None => Arc::new(|| {}),
        };
        let root = self.project.as_ref().map(|p| p.root_path().to_path_buf());
        let context = ToolbarContext {
            path,
            language,
            project: workbench_model::WorkbenchHost::project_id(self),
            root: root.as_deref(),
            wake,
        };
        let bar = self.support.toolbars.iter().find_map(|p| p.open(&context))?;
        tracing::debug!(path = %path.display(), "file opened with a bar");
        Some(TabToolbar::new(bar))
    }

    /// The active tab's bar told of its editor while it follows it (the statement under the
    /// cursor): only when the text, the cursor or the selection moved.
    pub(super) fn sync_toolbar_editor(&mut self) {
        self.tell_toolbar_editor(false);
    }

    /// The active tab's bar told of its editor (`always`: even when it doesn't follow it — a
    /// press on it, its menu's rows acting where the cursor is).
    fn tell_toolbar_editor(&mut self, always: bool) {
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let Some(toolbar) = tab.toolbar.as_mut() else { return };
        if !always && !toolbar.bar.follows_editor() {
            toolbar.told = None;
            return;
        }
        let primary = tab.doc.selections().primary();
        let selection = (!primary.is_empty()).then(|| primary.range());
        let key = (tab.doc.version(), primary.head, selection.clone());
        if toolbar.told.as_ref() == Some(&key) {
            return;
        }
        let state =
            EditorState { text: tab.text(), version: key.0, cursor: primary.head, selection };
        let Some(toolbar) = tab.toolbar.as_mut() else { return };
        toolbar.bar.sync_editor(&state);
        toolbar.told = Some(key);
    }

    /// The active tab's bar, with its color's strip along its top.
    pub(super) fn paint_toolbar(&mut self, text: &mut TextSystem, scene: &mut Scene) {
        let scale = self.scale;
        let (palette, icons) = (&self.palette, &self.icons);
        let Some(toolbar) = self.tabs.get_mut(self.active).and_then(|t| t.toolbar.as_mut()) else {
            return;
        };
        let bounds = toolbar.bounds;
        if bounds.size.height <= 0.0 {
            return;
        }
        toolbar.bar.paint(ToolbarPaint { bounds, scale, text, scene, palette, icons });
        if let Some(accent) = toolbar.bar.accent(palette) {
            let strip =
                Rect::new(bounds.min_x(), bounds.min_y(), bounds.size.width, (2.0 * scale).round());
            scene.push_layer(None).quad(Quad::filled(strip, accent));
        }
    }

    /// A press on the active tab's bar: what it asks done. `None`: not on it.
    pub(super) fn click_toolbar(&mut self, text: &mut TextSystem, p: Point) -> Option<MouseResult> {
        let toolbar = self.tabs.get_mut(self.active)?.toolbar.as_mut()?;
        if !toolbar.bounds.contains(p) {
            return None;
        }
        self.focus = Focus::Editor;
        self.tell_toolbar_editor(true);
        let toolbar = self.tabs.get_mut(self.active)?.toolbar.as_mut()?;
        let action = toolbar.bar.pointer_down(p);
        Some(match action {
            Some(action) => self.toolbar_action(text, action),
            None => MouseResult::Redraw,
        })
    }

    /// The mouse at `p`: the active tab's bar told it's over it, or that it left. `Some`: on
    /// the bar (whether to redraw, the cursor).
    pub(super) fn hover_toolbar(&mut self, p: Point) -> Option<(bool, CursorIcon)> {
        let toolbar = self.tabs.get_mut(self.active)?.toolbar.as_mut()?;
        let inside = toolbar.bounds.contains(p);
        if !inside {
            if std::mem::take(&mut toolbar.hovered) {
                toolbar.bar.hover(None);
            }
            return None;
        }
        toolbar.hovered = true;
        let redraw = toolbar.bar.hover(Some(p));
        let icon = match toolbar.bar.cursor_at(p) {
            Some(cursor) => super::cursor_icon(cursor),
            None => CursorIcon::Default,
        };
        // The highlight it shows follows the editor from now.
        self.sync_toolbar_editor();
        Some((redraw, icon))
    }

    /// The tooltip of the active tab's bar's control under `p`.
    pub(super) fn toolbar_tooltip(&self, p: Point) -> Option<ui_workbench::Tip> {
        let toolbar = self.tabs.get(self.active)?.toolbar.as_ref()?;
        toolbar.bounds.contains(p).then(|| toolbar.bar.tooltip_at(p)).flatten()
    }

    /// What a bar asked: a view's action, or a menu under its button.
    pub(super) fn toolbar_action(
        &mut self,
        text: &mut TextSystem,
        action: ToolbarAction,
    ) -> MouseResult {
        match action {
            ToolbarAction::View(action) => self.viewer_action(text, action),
            ToolbarAction::Menu { anchor, entries } => {
                let gap = (4.0 * self.scale).round();
                let at = Point::new(anchor.min_x(), anchor.max_y() + gap);
                self.open_context_menu(bar_menu(entries), at);
                MouseResult::Redraw
            }
            ToolbarAction::List { anchor, rows, chosen } => {
                self.close_hovers_for_menu();
                let (ids, items): (Vec<String>, Vec<_>) = rows.into_iter().unzip();
                self.dropdown_list.open(items, chosen);
                self.dropdown_anchor = anchor;
                self.dropdown_ids = ids;
                MouseResult::Redraw
            }
        }
    }

    /// A row of a bar's menu chosen: sent to the active tab's bar, what it asks done.
    pub fn run_bar_menu(&mut self, text: &mut TextSystem, id: &str) -> bool {
        let Some(toolbar) = self.tabs.get_mut(self.active).and_then(|t| t.toolbar.as_mut()) else {
            return false;
        };
        let used = toolbar.bar.command(id);
        self.take_toolbar_action(text);
        used
    }

    /// What the active tab's bar asks on its own (after a poll, a command).
    pub fn take_toolbar_action(&mut self, text: &mut TextSystem) {
        let action = self
            .tabs
            .get_mut(self.active)
            .and_then(|t| t.toolbar.as_mut())
            .and_then(|t| t.bar.take_action());
        if let Some(action) = action
            && let MouseResult::ViewerRequest(request) = self.toolbar_action(text, action)
        {
            self.request = Some(MouseResult::ViewerRequest(request));
        }
    }

    /// The bars' background work, back here (a statement ended); what they tell the user
    /// shown. `true`: redraw.
    pub(super) fn poll_toolbars(&mut self) -> bool {
        let mut changed = false;
        let mut said = Vec::new();
        for tab in all_tabs_mut(&mut self.tabs, &mut self.editors.groups) {
            if let Some(toolbar) = &mut tab.toolbar {
                changed |= toolbar.bar.poll();
                said.extend(toolbar.bar.take_notifications());
            }
        }
        for notification in said {
            workbench_model::WorkbenchHost::notify(self, notification);
            changed = true;
        }
        changed
    }

    /// A bar's work is going on (its spinner turns).
    pub(super) fn toolbars_loading(&self) -> bool {
        self.shown_tabs().any(|t| t.toolbar.as_ref().is_some_and(|b| b.bar.is_loading()))
    }

    /// The bar of tab `i` lets it close (`false`: it asks first, and closes it itself).
    pub fn toolbar_may_close(&mut self, i: usize) -> bool {
        self.tabs.get_mut(i).and_then(|t| t.toolbar.as_mut()).is_none_or(|t| t.bar.may_close())
    }

    #[cfg(test)]
    /// The active tab's bar's area (tests).
    pub fn toolbar_bounds(&self) -> Option<Rect> {
        self.tabs.get(self.active)?.toolbar.as_ref().map(|t| t.bounds)
    }
}

/// A bar's menu as the app's: its rows' ids made the app's (`bar-menu:` + the bar's id).
fn bar_menu(entries: Vec<ui_workbench::ViewMenuEntry>) -> Vec<ui_workbench::MenuEntry> {
    use ui_workbench::{MenuEntry, ViewMenuEntry};
    entries
        .into_iter()
        .map(|e| match e {
            ViewMenuEntry::Item { id, label, shortcut, icon, look } => MenuEntry::Item {
                id: format!("{BAR_MENU}{id}"),
                label,
                shortcut,
                icon,
                on: None,
                look,
            },
            ViewMenuEntry::Submenu { label, icon, entries } => {
                MenuEntry::Submenu { label, icon, entries: bar_menu(entries) }
            }
            ViewMenuEntry::Separator => MenuEntry::Separator,
        })
        .collect()
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/editor_toolbars.rs"]
mod tests;
