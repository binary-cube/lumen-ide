//! The status bar, and its items' windows (the file's details).

use super::locking::LockState;
use super::overlay_stack::{OverlayKind, OverlaySlot};
use super::*;
use render_api::Scene;
use std::collections::VecDeque;
use ui_workbench::chart::{Plot, Reduce, Samples, Series};
use ui_workbench::status_popover::{PopoverHit, PopoverRow};

/// The item standing for the file's details (its window lists them).
pub(super) const FILE_DETAILS: &str = "status.file_details";
/// The item standing for what the app costs (its window graphs it).
pub(super) const RESOURCES: &str = "status.resources";
/// The items with a window of their own.
const POPOVER_ITEMS: [&str; 2] = [FILE_DETAILS, RESOURCES];

/// The prefix of the `…` list's rows for the items with a window (they aren't commands:
/// chosen, the window opens over the `…`).
pub const STATUS_MENU: &str = "status-menu:";

/// A status bar item's window, floating: only the mouse on it reaches it.
pub(super) struct StatusPopoverSlot;

impl OverlaySlot for StatusPopoverSlot {
    fn kind(&self, _wb: &Workbench) -> OverlayKind {
        OverlayKind::Popover
    }

    fn is_open(&self, wb: &Workbench) -> bool {
        wb.status_popover.is_open()
    }

    fn covers_home(&self, _wb: &Workbench) -> bool {
        false
    }

    fn floats(&self, _wb: &Workbench) -> bool {
        true
    }

    fn holds(&self, wb: &Workbench, p: Point) -> bool {
        wb.status_popover.contains(p)
    }

    fn area(&self, wb: &Workbench) -> Option<Rect> {
        wb.status_popover.bounds()
    }

    /// A row with a command runs it (an unpinned window closes); the pin pins or unpins.
    fn click(&self, wb: &mut Workbench, _: &mut TextSystem, p: Point, _: u32) -> MouseResult {
        match wb.status_popover.hit(p) {
            Some(PopoverHit::Run(command)) => {
                wb.status_popover.close();
                MouseResult::Command(command)
            }
            // The resources pinned: they go to the editor's corner, as pills.
            Some(PopoverHit::Pin) => {
                wb.status_popover.close();
                wb.pin_resources(true);
                MouseResult::Redraw
            }
            Some(PopoverHit::Inside) | None => MouseResult::None,
        }
    }

    /// The mouse anywhere: away from it, its item and the way between, an unpinned one
    /// closes.
    fn hover(&self, wb: &mut Workbench, p: Point) -> (bool, CursorIcon) {
        if !wb.status_popover.keeps(p) {
            wb.status_popover.close();
            return (true, CursorIcon::Default);
        }
        let changed = wb.status_popover.hover(p);
        let icon =
            if wb.status_popover.over_button() { CursorIcon::Pointer } else { CursorIcon::Default };
        (changed, icon)
    }

    fn scroll(&self, _wb: &mut Workbench, _dx: f32, _dy: f32) -> Option<bool> {
        Some(false)
    }

    /// Its pin's: what a click does.
    fn tooltip_at(&self, wb: &Workbench, p: Point) -> Option<ui_workbench::Tip> {
        let at = wb.status_popover.pin_at(p)?;
        Some(ui_workbench::Tip::new(at, tr!("status.pin")))
    }

    fn paint(&self, wb: &mut Workbench, text: &mut TextSystem, scene: &mut Scene) {
        wb.paint_status_popover(text, scene);
    }

    fn animating(&self, wb: &Workbench) -> bool {
        wb.status_popover.is_animating()
    }
}

/// The resources pinned: pills in the editor's corner, under everything; their pin unpins
/// them.
pub(super) struct ResourcePillsSlot;

impl OverlaySlot for ResourcePillsSlot {
    fn kind(&self, _wb: &Workbench) -> OverlayKind {
        OverlayKind::Popover
    }

    fn is_open(&self, wb: &Workbench) -> bool {
        wb.resource_pills.is_open()
    }

    fn covers_home(&self, _wb: &Workbench) -> bool {
        false
    }

    fn floats(&self, _wb: &Workbench) -> bool {
        true
    }

    fn holds(&self, wb: &Workbench, p: Point) -> bool {
        wb.resource_pills.contains(p)
    }

    fn area(&self, wb: &Workbench) -> Option<Rect> {
        wb.resource_pills.bounds()
    }

    fn click(&self, wb: &mut Workbench, _: &mut TextSystem, p: Point, _: u32) -> MouseResult {
        match wb.resource_pills.hit(p) {
            Some(PopoverHit::Pin) => {
                wb.resource_pills.close();
                wb.pin_resources(false);
                MouseResult::Redraw
            }
            _ => MouseResult::None,
        }
    }

    /// Pinned, they stay wherever the mouse goes.
    fn hover(&self, wb: &mut Workbench, p: Point) -> (bool, CursorIcon) {
        let changed = wb.resource_pills.hover(p);
        let icon =
            if wb.resource_pills.over_button() { CursorIcon::Pointer } else { CursorIcon::Default };
        (changed, icon)
    }

    fn scroll(&self, _wb: &mut Workbench, _dx: f32, _dy: f32) -> Option<bool> {
        Some(false)
    }

    fn tooltip_at(&self, wb: &Workbench, p: Point) -> Option<ui_workbench::Tip> {
        let at = wb.resource_pills.pin_at(p)?;
        Some(ui_workbench::Tip::new(at, tr!("status.unpin")))
    }

    fn paint(&self, wb: &mut Workbench, text: &mut TextSystem, scene: &mut Scene) {
        wb.paint_resource_pills(text, scene);
    }

    fn animating(&self, _wb: &Workbench) -> bool {
        false
    }
}

impl Workbench {
    /// What the modules' signs say now.
    fn indicators(&self) -> Vec<workbench_model::Indicator> {
        self.support.status_indicators.iter().filter_map(|i| i.indicator()).collect()
    }

    /// The sign the modules show outside the windows now (the first there is).
    pub fn system_sign(&self) -> Option<workbench_model::SystemSign> {
        self.support.system_indicators.iter().find_map(|i| i.sign())
    }

    /// A sign changed since the bar was drawn (told from a module's thread: the app woke).
    /// `true`: drawn again.
    pub(super) fn poll_indicators(&self) -> bool {
        self.indicators() != self.shown_indicators
    }

    pub(super) fn paint_status(&mut self, text: &mut TextSystem, scene: &mut Scene) {
        let mut left: Vec<StatusItem> = Vec::new();
        let mut right: Vec<StatusItem> = Vec::new();
        if let Some(tab) = self.tabs.get(self.active) {
            let file = tab.doc.path().filter(|p| !is_virtual(p)).map(|path| {
                let name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
                let (support, icons, palette) = (&self.support, &self.icons, &self.palette);
                self.file_icons.get(support, icons, palette, self.theme_generation, &name)
            });
            self.push_tab_items(tab, file, &mut left, &mut right);
        }
        // Work going on (a paste…): a click stops it.
        for (_, progress) in self.progress.iter().rev() {
            let item =
                StatusItem::named(&self.icons, progress.icon, progress.label.clone()).essential();
            let item = match progress.stop {
                Some(stop) => item.clickable(stop.0),
                None => item,
            };
            right.insert(0, item);
        }
        if self.indexing() {
            right.insert(0, StatusItem::named(&self.icons, "search", tr!("status.indexing")));
        }
        // The modules' signs (the computer kept awake): before the work going on.
        let indicators = self.indicators();
        for indicator in indicators.iter().rev() {
            let item = StatusItem::named(&self.icons, indicator.icon, indicator.label.clone())
                .with_tooltip(indicator.tooltip.clone());
            let item = match indicator.command {
                Some(command) => item.clickable(command.0),
                None => item,
            };
            right.insert(0, item);
        }
        self.shown_indicators = indicators;
        // Zoom: − 100% + (click on the percentage: back to 100%); the first to go when the
        // bar is narrow, the three together.
        if !self.tabs.is_empty() {
            use crate::commands::{ZOOM_IN, ZOOM_OUT, ZOOM_RESET, label};
            let pct = format!("{}%", (self.zoom_level() * 100.0).round());
            right.push(
                StatusItem::text("−".to_owned())
                    .clickable(ZOOM_OUT.0)
                    .grouped()
                    .spare()
                    .menu_label(label(ZOOM_OUT)),
            );
            right.push(
                StatusItem::text(pct.clone())
                    .reserved("000%")
                    .clickable(ZOOM_RESET.0)
                    .spare()
                    .joined()
                    .menu_label(format!("{} · {pct}", label(ZOOM_RESET))),
            );
            right.push(
                StatusItem::text("+".to_owned())
                    .clickable(ZOOM_IN.0)
                    .spare()
                    .joined()
                    .menu_label(label(ZOOM_IN)),
            );
        }
        self.push_resource_items(&mut right);
        // The padlock last, in the corner, quick to reach, always there (nothing moves when a
        // file opens): open while writable, closed while locked, a click toggles it; faint and
        // doing nothing when no tab can be locked.
        let toggle = crate::commands::TOGGLE_READ_ONLY;
        let padlock = match self.active_lock() {
            Some(lock) => {
                let (name, color) = match lock {
                    LockState::Writable => ("lock-open", self.palette.text_muted),
                    LockState::Locked => ("lock", self.palette.text),
                    LockState::ReadOnlyOnDisk => ("lock", self.palette.warning),
                };
                StatusItem::named(&self.icons, name, String::new())
                    .colored(color)
                    .clickable(toggle.0)
            }
            None => StatusItem::named(&self.icons, "lock-open", String::new())
                .colored(self.palette.text_muted.with_alpha(0.35))
                .with_tooltip(crate::commands::label(toggle)),
        };
        right.push(padlock.essential());
        self.status.set_still(!self.window_focused());
        self.status.paint(&left, &right, &self.icons, text, scene, &self.palette);
        // The resources pinned: their pills in the editor's corner, whether the bar has room
        // for their item or not.
        let pills = self.resources_pinned() && self.resources.is_some();
        if pills && !self.resource_pills.is_open() {
            self.resource_pills.open(RESOURCES, Rect::default());
            self.resource_pills.set_pinned(true);
        } else if !pills && self.resource_pills.is_open() {
            self.resource_pills.close();
        }
    }

    /// The active tab's items: its file (left), its marks, its viewer's state, the
    /// occurrences, the cursor's position, the language level and the file's details.
    /// `file_icon`: its file's icon and color (`None`: not a file).
    fn push_tab_items(
        &self,
        tab: &Tab,
        file_icon: Option<(IconId, Rgba)>,
        left: &mut Vec<StatusItem>,
        right: &mut Vec<StatusItem>,
    ) {
        let doc = &tab.doc;
        // The canvas isn't a file: it shows its own state (on the right).
        if let Some(path) = doc.path().filter(|p| !is_virtual(p))
            && let Some((id, color)) = file_icon
        {
            let shown = self
                .project
                .as_ref()
                .and_then(|p| path.strip_prefix(p.root_path()).ok())
                .unwrap_or(path);
            left.push(
                StatusItem::with_icon(Some(id), home_relative(shown)).colored(color).as_path(),
            );
        }
        let (line, position) = self.cursor_position_shown(doc);
        self.push_mark_items(tab, line, left, right);
        if let Some(status) = tab.viewer.as_ref().and_then(|v| v.viewer.status()) {
            right.push(StatusItem::text(status));
        }
        let image =
            tab.viewer.as_ref().is_some_and(|v| v.kind == ui_workbench::ViewerKind::Replace);
        if !tab.occurrences.is_empty() {
            let count = tab.occurrences.len();
            right.push(StatusItem::text(tr!("status.occurrences", count = count)).spare());
        }
        if !image {
            right.push(StatusItem::text(position).reserved("0000:000").essential());
        }
        if let Some((language, level)) = self.level_at_cursor().filter(|_| !image) {
            let name = self.support.languages.get(&language).map_or("", |l| l.name);
            let key =
                if self.has_file_level(&language) { "status.level_file" } else { "status.level" };
            right.push(
                StatusItem::text(tr!(key, name = name, level = level.as_str()))
                    .clickable("project.language_level")
                    .spare(),
            );
        }
        if !image {
            // The file's details in one item (its language's icon, in its color): its window
            // tells them all (and switches the engine, where there's a choice).
            let indent = match doc.indent() {
                Indent::Spaces(n) => tr!("status.spaces_short", count = n),
                Indent::Tabs => tr!("status.tab_short"),
            };
            let details = format!(
                "{} · {} · {} · {indent}",
                self.language_label(tab),
                doc.encoding().label(),
                doc.line_ending().label()
            );
            let item = match file_icon {
                Some((id, color)) => StatusItem::with_icon(Some(id), details).colored(color),
                None => StatusItem::named(&self.icons, "braces", details),
            };
            right.push(
                StatusItem { menu_icon: Some("braces"), ..item }
                    .clickable(FILE_DETAILS)
                    .grouped()
                    .menu_label(tr!("status.file_details")),
            );
        }
    }

    /// The file's details as its window's rows: the language, what answers it (a row that
    /// switches it, where there's a choice), the encoding, the line endings, the indentation.
    pub(super) fn file_detail_rows(&self) -> Vec<PopoverRow> {
        let Some(tab) = self.tabs.get(self.active) else { return Vec::new() };
        let doc = &tab.doc;
        use crate::commands::{
            CHOOSE_ENCODING, CHOOSE_INDENT, CHOOSE_LANGUAGE, CHOOSE_LINE_ENDING,
        };
        // Each row chooses its value (a viewer's tab without text: only shown).
        let text = tab.holds_text();
        let value =
            |label: &str, value: String, command: command_model::CommandId| PopoverRow::Value {
                label: tr!(label),
                value,
                action: text.then_some(command.0),
                icon: None,
            };
        let mut rows = vec![value("status.language", self.language_label(tab), CHOOSE_LANGUAGE)];
        if let Some(engine) = self.engine_label(tab) {
            let choose = workbench_model::CHOOSE_ENGINE;
            rows.push(PopoverRow::Value {
                label: tr!("status.engine"),
                value: engine,
                action: workbench_model::presence::command(choose).then_some(choose.0),
                icon: None,
            });
        }
        rows.push(value("status.encoding", doc.encoding().label().to_owned(), CHOOSE_ENCODING));
        rows.push(value(
            "status.line_endings",
            doc.line_ending().label().to_owned(),
            CHOOSE_LINE_ENDING,
        ));
        let indent = match doc.indent() {
            Indent::Spaces(n) => tr!("status.spaces", count = n),
            Indent::Tabs => tr!("status.tab"),
        };
        rows.push(value("status.indentation", indent, CHOOSE_INDENT));
        rows
    }

    /// The mouse at `p` over the bar: an item with a window opens it (over that item); away
    /// from the item and its window, an unpinned one closes. `true`: a redraw.
    pub(super) fn hover_status_popover(&mut self, p: Point) -> bool {
        // The resources pinned: their pills say it already, no window over the meter (pinned
        // just now, before a frame opened them, too).
        let pills = self.resource_pills.is_open() || self.resources_pinned();
        let over = self
            .status
            .hit(p)
            .filter(|a| POPOVER_ITEMS.contains(a))
            .filter(|a| !(pills && *a == RESOURCES));
        if let Some(owner) = over
            && let Some(anchor) = self.status.action_rect(owner)
        {
            let changed = self.status_popover.owner() != Some(owner);
            self.status_popover.open(owner, anchor);
            return changed;
        }
        if self.status_popover.is_open() && !self.status_popover.keeps(p) {
            self.status_popover.close();
            return true;
        }
        false
    }

    /// A click on an item with a window: it opens (or closes, when it's open for it and not
    /// pinned).
    pub(super) fn click_status_popover(&mut self, owner: &'static str) {
        if self.status_popover.owner() == Some(owner) && !self.status_popover.pinned() {
            self.status_popover.close();
            return;
        }
        if let Some(anchor) = self.status.action_rect(owner) {
            self.status_popover.open(owner, anchor);
        }
    }

    /// A row of the `…` list for an item with a window chosen (`action`: the item's): what
    /// its click in the bar does — the resources pinned or unpinned, else its window, over
    /// the `…` (its item isn't shown). `None`: no command to run.
    pub fn status_more_chosen(&mut self, action: &str) -> Option<&'static str> {
        let owner = POPOVER_ITEMS.into_iter().find(|a| *a == action)?;
        if owner == RESOURCES {
            self.toggle_resources_pin();
            return None;
        }
        if let Some(anchor) = self.status_popover_anchor(owner) {
            self.status_popover.open(owner, anchor);
        }
        None
    }

    /// Where an item's window stands: over the item, or over the `…` while the item is in
    /// its list.
    fn status_popover_anchor(&self, owner: &str) -> Option<kernel::Rect> {
        self.status.action_rect(owner).or_else(|| {
            self.status
                .hidden()
                .iter()
                .any(|i| i.action == Some(owner))
                .then(|| self.status.action_rect(ui_workbench::STATUS_MORE))
                .flatten()
        })
    }

    /// A bar item's window opened by its name (`file`), as its click would (snapshots).
    #[cfg(feature = "snapshot")]
    pub fn open_status_popover(&mut self, which: &str) {
        let owner = match which {
            "file" => FILE_DETAILS,
            "resources" | "resources-pinned" => RESOURCES,
            _ => return,
        };
        if which == "resources-pinned" {
            // For this run only (the setting isn't written).
            self.resources_pinned = Some(true);
            return;
        }
        self.click_status_popover(owner);
    }

    fn paint_status_popover(&mut self, text: &mut TextSystem, scene: &mut Scene) {
        let Some(owner) = self.status_popover.owner() else { return };
        let rows = match owner {
            FILE_DETAILS => self.file_detail_rows(),
            RESOURCES => self.resource_rows(),
            _ => Vec::new(),
        };
        // Its item gone (another tab, a wider window's bar without it): it goes too.
        let Some(anchor) = self.status_popover_anchor(owner).filter(|_| !rows.is_empty()) else {
            self.status_popover.close();
            return;
        };
        self.status_popover.open(owner, anchor);
        // Over its item, its right edge on the item's, kept within the cards' corner (as far
        // from their right edge as from their bottom): over whatever is there, never moved
        // by the panels' sizes.
        // (The window keeps 8 px inside what it's given.)
        let width = (self.cards_corner().x + (8.0 * self.scale).round()).max(0.0);
        let viewport = Rect::new(0.0, 0.0, width, self.size.height as f32);
        let bar_top = self.status.bounds().min_y();
        self.status_popover.paint(
            scene,
            text,
            &self.icons,
            &self.palette,
            self.scale,
            viewport,
            bar_top,
            &rows,
            (owner == RESOURCES).then(|| tr!("status.resources")).as_deref(),
            owner == RESOURCES,
        );
    }

    /// The resources pinned: small, as pills in the window's bottom-right corner, over their
    /// meter in the bar (whether the bar shows it or not).
    fn paint_resource_pills(&mut self, text: &mut TextSystem, scene: &mut Scene) {
        let pills = self.resource_pills();
        if pills.is_empty() {
            return;
        }
        // In the cards' corner (their meter's right edge, when it's left of it), over
        // whatever is there (the editor, a bottom window): where their window opens too,
        // never moved by the panels' sizes.
        let mut corner = self.cards_corner();
        if let Some(item) = self.status_popover_anchor(RESOURCES) {
            corner.x = corner.x.min(item.max_x());
        }
        self.resource_pills.paint_pills(
            scene,
            text,
            &self.icons,
            &self.palette,
            self.scale,
            corner,
            &pills,
        );
    }

    /// What answers the tab's language now, as the servers' port names it (`None`: no
    /// server of it can answer — nothing to switch between).
    pub(super) fn engine_label(&self, tab: &Tab) -> Option<String> {
        self.project.as_ref()?;
        let servers = self.servers()?;
        servers.engine_label(self.data.id, tab.syntax.language().as_str())
    }

    /// Errors and warnings: their counts (right) and the message of the one on the cursor
    /// line (`line`, left).
    fn push_mark_items(
        &self,
        tab: &Tab,
        line: usize,
        left: &mut Vec<StatusItem>,
        right: &mut Vec<StatusItem>,
    ) {
        let errors = tab.diagnostics.marks.iter().filter(|m| !m.warning).count();
        if errors > 0 {
            let label = tr!("status.errors", count = errors);
            right.push(
                StatusItem::named(&self.icons, "x", label).colored(self.palette.danger).essential(),
            );
        }
        let warnings = tab.diagnostics.marks.iter().filter(|m| m.warning && !m.weak).count();
        if warnings > 0 {
            let label = tr!("status.warnings", count = warnings);
            right.push(
                StatusItem::named(&self.icons, "alert", label)
                    .colored(self.palette.warning)
                    .essential(),
            );
        }
        if tab.diagnostics.marks.is_empty() {
            return;
        }
        // The message of the first mark on the cursor line (errors before warnings): the
        // marks are sorted by their start, those past the text's end count on its last line.
        let buffer = tab.doc.buffer();
        let from = buffer.line_start(line);
        let to =
            if line + 1 < buffer.line_count() { buffer.line_start(line + 1) } else { usize::MAX };
        let first = tab.diagnostics.marks.partition_point(|m| m.range.start < from);
        let end = tab.diagnostics.marks.partition_point(|m| m.range.start < to);
        let here = tab.diagnostics.marks[first..end.max(first)]
            .iter()
            .min_by_key(|m| (m.warning, m.weak))
            .map(|m| (&m.message, &m.code));
        if let Some((message, code)) = here {
            // On one line: the status bar shapes a single line (a code may span several).
            let one_line = kernel::text::one_line;
            let text = match code {
                Some(code) => format!("{}: {}", one_line(message), one_line(code)),
                None => one_line(message).into_owned(),
            };
            left.push(StatusItem::text(text));
        }
    }

    /// The `…`'s list, above it: the items that didn't fit, each a row running what its
    /// click in the bar runs (one that runs nothing only tells: chosen, the list closes).
    pub fn open_status_more(&mut self) {
        let entries = self.status_more_entries();
        let Some(anchor) = self.status.action_rect(ui_workbench::STATUS_MORE) else { return };
        // Opened at the `…`'s top: it doesn't fit below, so it stands above it.
        self.open_context_menu(entries, kernel::Point::new(anchor.min_x(), anchor.min_y()));
    }

    /// The `…` list's rows: the items that didn't fit.
    fn status_more_entries(&self) -> Vec<ui_workbench::MenuEntry> {
        self.status
            .hidden()
            .iter()
            .map(|item| {
                let label = item
                    .menu_label
                    .clone()
                    .filter(|l| !l.is_empty())
                    .or_else(|| (!item.text.is_empty()).then(|| item.text.clone()))
                    .or_else(|| {
                        item.action.map(|a| crate::commands::label(command_model::CommandId(a)))
                    })
                    .unwrap_or_default();
                let action = item.action.unwrap_or_default();
                // An item with a window: its row opens it (over the `…`); the others run
                // their command.
                let id = if POPOVER_ITEMS.contains(&action) {
                    format!("{STATUS_MENU}{action}")
                } else {
                    action.to_owned()
                };
                ui_workbench::MenuEntry::Item {
                    id,
                    label,
                    shortcut: None,
                    icon: item.menu_icon,
                    on: None,
                    look: ui_workbench::RowLook::default(),
                }
            })
            .collect()
    }

    /// What the app costs (CPU, RAM, GPU) as a meter of three small bars, gliding as they
    /// change: over it, its window with the last minute's graphs; a click pins it (the pills
    /// in the editor's corner) or unpins it.
    fn push_resource_items(&self, right: &mut Vec<StatusItem>) {
        let Some((sample, gpu)) = &self.resources else { return };
        let palette = &self.palette;
        let color = |s| ui_workbench::chart::series_color(palette, s);
        let trail = &self.resource_trail;
        let mut bars = vec![
            ((sample.cpu / 100.0).clamp(0.0, 1.0), color(Series::Cpu)),
            (level(sample.ram_bytes as f32, &trail.ram), color(Series::Memory)),
        ];
        if let Some(gpu) = gpu {
            bars.push((level(gpu.total() as f32, &trail.gpu), color(Series::Gpu)));
        }
        let (cpu, ram, gpu) = resource_texts(sample, gpu.as_ref());
        right.push(
            StatusItem { menu_icon: Some("cpu"), ..StatusItem::meter(bars) }
                .clickable(RESOURCES)
                .grouped()
                .menu_label(tr!("status.resources_now", cpu = cpu, ram = ram, gpu = gpu)),
        );
    }

    /// The resources window's rows: the last minute of each, as a graph, and its value now.
    fn resource_rows(&self) -> Vec<PopoverRow> {
        let Some((sample, gpu)) = &self.resources else { return Vec::new() };
        let palette = &self.palette;
        let columns = (150.0 * self.scale) as usize;
        let (cpu, ram, gpu_text) = resource_texts(sample, gpu.as_ref());
        let series = |label: &str, values: &VecDeque<f32>, reduce, s, value: String, floor: f32| {
            let peak = values.iter().copied().fold(0.0_f32, f32::max);
            let icon = match s {
                Series::Cpu => "cpu",
                Series::Memory => "memory",
                Series::Gpu => "gpu",
                Series::Network => "globe",
                Series::Disk => "hard-drive",
            };
            PopoverRow::Series {
                label: tr!(label),
                plot: Plot::build(Samples::from_slices(values.as_slices()), TRAIL, columns, reduce),
                max: (peak * 1.2).max(floor),
                color: ui_workbench::chart::series_color(palette, s),
                value,
                icon: Some(icon),
            }
        };
        let trail = &self.resource_trail;
        let mb = 1_048_576.0;
        let mut rows = vec![
            series("status.cpu", &trail.cpu, Reduce::Max, Series::Cpu, cpu, 5.0),
            series("status.ram", &trail.ram, Reduce::Last, Series::Memory, ram, mb),
        ];
        if gpu.is_some() {
            rows.push(series("status.gpu", &trail.gpu, Reduce::Last, Series::Gpu, gpu_text, mb));
        }
        // What the system's monitor adds to the app's figure: the programs it started (the
        // terminals' shells and what runs in them, language servers, agents). A click: the
        // Task Manager, each one there. Shown even while none runs (`0`), so the window
        // keeps its shape.
        rows.push(PopoverRow::Value {
            label: tr!("status.programs"),
            value: tr!(
                "status.programs_value",
                count = sample.programs,
                memory = memory_text(sample.programs_bytes)
            ),
            action: Some(workbench_model::SHOW_RESOURCES.0),
            icon: Some("app-window"),
        });
        // The sum the system's monitor shows for the app (its own memory and its
        // programs'): the two rows above, added, so nothing looks hidden.
        rows.push(PopoverRow::Value {
            label: tr!("status.total"),
            value: memory_text(sample.ram_bytes.saturating_add(sample.programs_bytes)),
            action: None,
            icon: Some("layers"),
        });
        rows
    }

    /// The resources as the pinned pills say them: each value in its series' color.
    pub(super) fn resource_pills(&self) -> Vec<ui_workbench::status_popover::Pill> {
        let Some((sample, gpu)) = &self.resources else { return Vec::new() };
        let (cpu, ram, gpu_text) = resource_texts(sample, gpu.as_ref());
        let color = |s| ui_workbench::chart::series_color(&self.palette, s);
        let pill = |label: &str, value: String, s| ui_workbench::status_popover::Pill {
            label: tr!(label),
            value,
            color: color(s),
            more: None,
        };
        // The RAM: the app's, then the total the system shows for it (its programs' added).
        let total = memory_text(sample.ram_bytes.saturating_add(sample.programs_bytes));
        let ram = ui_workbench::status_popover::Pill {
            more: Some(total),
            ..pill("status.ram", ram, Series::Memory)
        };
        let mut pills = vec![pill("status.cpu", cpu, Series::Cpu), ram];
        if gpu.is_some() {
            pills.push(pill("status.gpu", gpu_text, Series::Gpu));
        }
        pills
    }

    /// The resources window pinned (the setting, read once).
    pub(super) fn resources_pinned(&mut self) -> bool {
        *self.resources_pinned.get_or_insert_with(|| {
            crate::config::settings().get(&crate::settings::RESOURCES_PINNED)
        })
    }

    /// The resources window pinned or not, kept for the next start.
    pub(super) fn pin_resources(&mut self, pinned: bool) {
        self.resources_pinned = Some(pinned);
        if let Err(e) = crate::config::settings().set(&crate::settings::RESOURCES_PINNED, pinned) {
            tracing::warn!(error = %e, "the resources window's pin wasn't saved");
        }
    }

    /// Where the bar's windows and the resources' pills end (bottom-right, physical): the
    /// bar's popover gap above it, and as far from the cards' right edge (the editor's, a
    /// side or bottom window's: left of the side's buttons) as from their bottom — so they
    /// seem to belong to the cards.
    pub(super) fn cards_corner(&self) -> kernel::Point {
        let gap = (ui_workbench::ds::POPOVER_GAP * self.scale).round();
        let bottom = self.status.bounds().min_y() - gap;
        let cards = std::iter::once(self.card)
            .chain(std::iter::once(self.tools.bottom_bounds))
            .chain(self.tools.sides.iter().map(|s| s.bounds))
            .filter(|r| r.size.width > 0.0 && r.size.height > 0.0);
        let (right, low) = cards.fold((self.card.max_x(), self.card.max_y()), |(r, b), c| {
            (r.max(c.max_x()), b.max(c.max_y()))
        });
        let inset = (low - bottom).max(gap);
        kernel::Point::new((right - inset).round(), bottom)
    }

    /// The resources' meter clicked: pinned if they weren't, unpinned if they were (what
    /// their window's pin and the pills' pin do), their window closed either way.
    pub(super) fn toggle_resources_pin(&mut self) {
        let pinned = !self.resources_pinned();
        self.status_popover.close();
        if !pinned {
            self.resource_pills.close();
        } else if self.resources.is_some() && !self.resource_pills.is_open() {
            // Their pills at once (not at the next frame: the mouse moving meanwhile would
            // open their window over the meter as well).
            self.resource_pills.open(RESOURCES, Rect::default());
            self.resource_pills.set_pinned(true);
        }
        self.pin_resources(pinned);
    }

    /// When the bar next needs a frame (its meter gliding).
    pub fn status_deadline(&self) -> Option<std::time::Instant> {
        self.status.next_frame()
    }
}

/// The values a meter's bar stands for: how many the trail keeps (a minute, one a second).
pub(super) const TRAIL: usize = 60;

/// What the app cost over the last minute (one sample a second): the CPU (percent of a
/// core), the memory and the GPU's (bytes).
/// Kept by time, not by how often the app wakes (a terminal's output, indexing): a value told
/// again within its second replaces that second's, a value held for seconds (the monitor
/// tells only changes) fills them.
#[derive(Debug, Default)]
pub(super) struct ResourceTrail {
    cpu: VecDeque<f32>,
    ram: VecDeque<f32>,
    gpu: VecDeque<f32>,
    /// When the last second began.
    second: Option<std::time::Instant>,
}

impl ResourceTrail {
    pub(super) fn push(&mut self, at: std::time::Instant, cpu: f32, ram: f32, gpu: Option<f32>) {
        let seconds = self.second.map_or(1, |s| at.saturating_duration_since(s).as_secs());
        let seconds = usize::try_from(seconds).unwrap_or(TRAIL).min(TRAIL);
        if seconds > 0 {
            let began =
                self.second.map_or(at, |s| s + std::time::Duration::from_secs(seconds as u64));
            self.second = Some(began);
        }
        let add = |ring: &mut VecDeque<f32>, v: f32| {
            if seconds == 0 {
                ring.pop_back();
            }
            // The seconds between held the value before (the newest of them: this one).
            let held = ring.back().copied().unwrap_or(v);
            for i in 0..seconds.max(1) {
                if ring.len() == TRAIL {
                    ring.pop_front();
                }
                ring.push_back(if i + 1 == seconds.max(1) { v } else { held });
            }
        };
        add(&mut self.cpu, cpu);
        add(&mut self.ram, ram);
        if let Some(gpu) = gpu {
            add(&mut self.gpu, gpu);
        }
    }
}

/// A level's bar (0–1): `value` against the minute's peak (with room above it), so a change
/// shows.
fn level(value: f32, trail: &VecDeque<f32>) -> f32 {
    let peak = trail.iter().copied().fold(value, f32::max);
    if peak <= 0.0 { 0.0 } else { (value / (peak * 1.25)).clamp(0.0, 1.0) }
}

/// The CPU, the memory and the GPU's as the bar writes them.
fn resource_texts(
    sample: &Sample,
    gpu: Option<&render_api::GpuMemory>,
) -> (String, String, String) {
    let cpu = format!("{:.1}%", sample.cpu).replace('.', ",");
    (
        cpu,
        memory_text(sample.ram_bytes),
        gpu.map_or_else(|| "—".to_owned(), |g| memory_text(g.total())),
    )
}

/// An amount of memory as the resources say it: whole MB under a GB (`250 MB`), GB with
/// one decimal from there on (`1,2 GB`), the comma as the CPU's.
pub(super) fn memory_text(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    let mb = (bytes as f64 / MB).round();
    if mb < 1024.0 {
        format!("{mb} MB")
    } else {
        format!("{:.1} GB", bytes as f64 / (MB * 1024.0)).replace('.', ",")
    }
}

impl Workbench {
    /// [`cursor_position`], kept while the document, its cursors and its tab width stay the
    /// same (the status bar is drawn on every frame; counting a large selection isn't free).
    fn cursor_position_shown(&self, doc: &Document) -> (usize, String) {
        let selections = doc.selections();
        let primary = selections.primary();
        let key = (
            doc.id(),
            doc.version(),
            primary.head,
            primary.anchor,
            selections.len(),
            doc.tab_width(),
        );
        self.status_cursor.borrow_mut().get_or_update(key, || cursor_position(doc)).clone()
    }
}

/// The cursor's line and its position as shown ("12:5"), with the number of cursors or
/// of selected characters.
fn cursor_position(doc: &Document) -> (usize, String) {
    let head = doc.selections().primary().head;
    let buffer = doc.buffer();
    let line = buffer.line_of(head);
    let prefix = buffer.slice(buffer.line_start(line)..head);
    let column = visual_width(&prefix, doc.tab_width()) + 1;
    let mut position = format!("{}:{}", line + 1, column);
    let selected: usize = doc.selections().iter().map(|s| s.range().len()).sum();
    if doc.selections().len() > 1 {
        let cursors = tr!("status.cursors", count = doc.selections().len());
        position.push_str(&format!("  ({cursors})"));
    } else if selected > 0 {
        let chars = buffer.char_count(doc.selections().primary().range());
        position.push_str(&format!("  ({})", tr!("status.selected", count = chars)));
    }
    (line, position)
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/status.rs"]
mod tests;
