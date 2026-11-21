//! File search in the project panel: the app's search field (pill, magnifier, × while
//! there is text; its press, drag, editing keys and edit commands) and, while there is text,
//! the result list (instead of the tree), with the matched letters highlighted.
//!
//! The component only displays and holds the field state; the search is the project's one
//! path search ([`workspace::search_paths`]), done by the caller: the matching
//! folders come first (a folder icon, a line under them), then the files.

use icons::{IconId, Icons};
use kernel::{Point, Rect, Rgba};
use render_api::Scene;
use text_layout::{Font, FontFamily, TextSystem};
use ui_components::control::Control;
use ui_components::ds;
use ui_components::search_field::SearchField;
use ui_components::{KeyReach, Palette, ViewerCursor, ViewerKey};
use ui_kit::{centered_baseline, draw_icon, draw_text};
use workspace::{FileMatch, PathResults};

/// A result row's highlight corners (logical pixels).
const ROW_RADIUS: f32 = 6.0;

/// The search's appearance; built from the theme or given individually. Sizes in logical
/// pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct FileSearchStyle {
    pub text: Rgba,
    /// The matched letters in the file name.
    pub highlight: Rgba,
    pub muted: Rgba,
    pub selection: Rgba,
    pub hover: Rgba,
    pub font_family: FontFamily,
    pub font_size: f32,
    pub field_height: f32,
    pub row_height: f32,
}

impl FileSearchStyle {
    pub fn from_palette(p: &Palette) -> Self {
        Self {
            text: p.text,
            highlight: p.accent,
            muted: p.text_muted,
            selection: p.tree_selection,
            hover: p.tree_hover,
            font_family: p.ui_font.clone(),
            font_size: p.ui_font_size,
            field_height: 34.0,
            row_height: 30.0,
        }
    }
}

impl Default for FileSearchStyle {
    fn default() -> Self {
        Self::from_palette(&Palette::dark())
    }
}

#[derive(Debug, Default)]
pub struct FileSearch {
    /// The field (the app's search field; its focus is the panel's).
    pub field: SearchField,
    style: FileSearchStyle,
    scale: f32,
    /// The field area (with surrounding margins), where the field is laid out in it, and
    /// the result list area.
    area: Rect,
    field_rect: Rect,
    list: Rect,
    /// The folders found, then the files.
    results: Vec<FileMatch>,
    /// How many of `results` are folders (the first ones).
    folders: usize,
    selected: usize,
    hovered: Option<usize>,
    scroll_y: f32,
    /// The file index is still being built.
    pub indexing: bool,
}

impl FileSearch {
    pub fn new(style: FileSearchStyle) -> Self {
        Self { style, scale: 1.0, ..Self::default() }
    }

    pub fn set_style(&mut self, style: FileSearchStyle) {
        self.style = style;
    }

    /// Height of the field area (with top / bottom margins), in physical pixels.
    pub fn field_area_height(&self, scale: f32) -> f32 {
        ((self.style.field_height + 10.0) * scale).round()
    }

    /// `area` = the field area (panel width), `list` = the results area.
    pub fn set_bounds(&mut self, area: Rect, list: Rect, scale: f32) {
        self.scale = scale;
        self.area = area;
        self.list = list;
        let px = |v: f32| (v * scale).round();
        let h = px(self.style.field_height);
        self.field_rect = Rect::new(
            area.min_x() + px(12.0),
            area.min_y() + px(2.0),
            (area.size.width - px(24.0)).max(0.0),
            h,
        );
    }

    /// The result list is shown (instead of the tree).
    pub fn is_active(&self) -> bool {
        !self.query().trim().is_empty()
    }

    pub fn query(&self) -> &str {
        self.field.text()
    }

    pub fn field_contains(&self, p: Point) -> bool {
        self.field_rect.contains(p)
    }

    /// A press on the field, the `clicks`-th of a series: the caret put (two a word, three
    /// all), held for a drag selecting; `Some(true)` when it was its × (the field emptied).
    pub fn press(&mut self, p: Point, clicks: u32) -> Option<bool> {
        if !self.field_contains(p) {
            return None;
        }
        Control::press(&mut self.field, p, clicks);
        Some(self.field.take_cleared() | self.field.take_edited())
    }

    /// The pointer moved while a press on the field holds it: the selection follows.
    pub fn drag(&mut self, p: Point) -> bool {
        Control::drag(&mut self.field, p)
    }

    /// `p` is on the field's × (shown while there is text).
    pub fn on_clear(&self, p: Point) -> bool {
        self.field_contains(p) && Control::cursor(&self.field, p) == Some(ViewerCursor::Pointer)
    }

    /// The cursor over the field: the text's, a hand on its ×.
    pub fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        self.field_contains(p).then(|| Control::cursor(&self.field, p)).flatten()
    }

    /// A key for the field (its editing ones), what's typed, an edit command: `(used,
    /// edited)`, edited when its text changed (the search asked again).
    pub fn key(&mut self, key: ViewerKey, reach: KeyReach) -> (bool, bool) {
        self.editing(|field| Control::key(field, key, reach))
    }

    pub fn text(&mut self, typed: &str) -> (bool, bool) {
        self.editing(|field| Control::text(field, typed))
    }

    pub fn command(&mut self, id: &str) -> (bool, bool) {
        self.editing(|field| Control::command(field, id))
    }

    pub fn selected_text(&self) -> Option<String> {
        Control::selected_text(&self.field)
    }

    /// `edit` done on the field, and whether its text changed.
    fn editing(&mut self, edit: impl FnOnce(&mut SearchField) -> bool) -> (bool, bool) {
        let used = edit(&mut self.field);
        (used, self.field.take_edited())
    }

    pub fn list_contains(&self, p: Point) -> bool {
        self.is_active() && self.list.contains(p)
    }

    pub fn set_results(&mut self, results: PathResults) {
        self.folders = results.folders.len();
        self.results = results.folders;
        self.results.extend(results.files);
        self.selected = 0;
        self.scroll_y = 0.0;
        self.hovered = None;
    }

    pub fn clear(&mut self) {
        self.field.set_text("");
        self.set_results(PathResults::default());
    }

    pub fn selected_index(&self) -> usize {
        self.selected
    }

    pub fn selected(&self) -> Option<&FileMatch> {
        self.results.get(self.selected)
    }

    pub fn result(&self, index: usize) -> Option<&FileMatch> {
        self.results.get(index)
    }

    /// Result `index` is a folder.
    pub fn is_folder(&self, index: usize) -> bool {
        index < self.folders
    }

    fn row_height(&self) -> f32 {
        (self.style.row_height * self.scale).round()
    }

    /// Moves the selection by `delta` rows and keeps it visible.
    pub fn move_selection(&mut self, delta: isize) {
        if self.results.is_empty() {
            return;
        }
        self.selected = ui_kit::step_index(self.selected, delta, self.results.len());
        let row = self.row_height();
        let top = self.selected as f32 * row;
        if top < self.scroll_y {
            self.scroll_y = top;
        } else if top + row > self.scroll_y + self.list.size.height {
            self.scroll_y = top + row - self.list.size.height;
        }
    }

    pub fn result_at(&self, p: Point) -> Option<usize> {
        if !self.list_contains(p) {
            return None;
        }
        let index = ((p.y - self.list.min_y() + self.scroll_y) / self.row_height()) as usize;
        (index < self.results.len()).then_some(index)
    }

    pub fn hover(&mut self, p: Point) -> bool {
        let hovered = self.result_at(p);
        let field = Control::hover(&mut self.field, Some(p));
        let changed = field || hovered != self.hovered;
        self.hovered = hovered;
        changed
    }

    pub fn scroll_by(&mut self, dy: f32) -> bool {
        let before = self.scroll_y;
        let content = self.results.len() as f32 * self.row_height();
        let max = (content - self.list.size.height).max(0.0);
        self.scroll_y = (self.scroll_y + dy).clamp(0.0, max).round();
        self.scroll_y != before
    }

    /// The field: the app's search field (its caret while `focused`, × with text).
    pub fn paint_field(
        &mut self,
        focused: bool,
        icons: &Icons,
        text: &mut TextSystem,
        scene: &mut Scene,
        palette: &Palette,
    ) {
        if self.field_rect.size.width <= 0.0 {
            return;
        }
        let ctx = ui_components::ds::Ctx::new(palette, self.scale);
        let layer = scene.push_layer(Some(self.area));
        let placeholder = i18n::t("files.placeholder");
        self.field.paint(layer, text, icons, &ctx, (self.field_rect, focused), &placeholder);
    }

    /// The results, in the list area. `file_icon(name)` gives the file's icon and color.
    pub fn paint_results(
        &self,
        icons: &Icons,
        file_icon: &dyn Fn(&str) -> (IconId, Rgba),
        text: &mut TextSystem,
        scene: &mut Scene,
        palette: &Palette,
    ) {
        let st = &self.style;
        let px = |v: f32| (v * self.scale).round();
        let ctx = ds::Ctx::new(palette, self.scale);
        let b = self.list;
        let font = Font::new(st.font_family.clone(), px(st.font_size));
        let metrics = text.metrics(&font);
        let row = self.row_height();
        let layer = scene.push_layer(Some(b));
        let inset = px(8.0);
        let padding = px(14.0);

        if self.results.is_empty() {
            let message = i18n::t(if self.indexing { "files.indexing" } else { "files.none" });
            let shaped = text.shape_line(&message, &font);
            let baseline = centered_baseline(b.min_y(), row, &metrics);
            draw_text(layer, &shaped, b.min_x() + padding, baseline, st.muted);
            return;
        }

        let first = (self.scroll_y / row) as usize;
        let count = (b.size.height / row).ceil() as usize + 1;
        // Row backgrounds in the list layer; text in per-row layers clipped to the pill.
        let mut rows = Vec::with_capacity(count);
        for (index, m) in self.results.iter().enumerate().skip(first).take(count) {
            let top = b.min_y() + index as f32 * row - self.scroll_y;
            let pill = Rect::new(
                b.min_x() + inset,
                top + px(1.0),
                b.size.width - 2.0 * inset,
                row - px(2.0),
            );
            let lit = if index == self.selected {
                Some(st.selection)
            } else {
                (self.hovered == Some(index)).then_some(st.hover)
            };
            if let Some(color) = lit {
                ui_components::tree::highlight(layer, pill, color, px(ROW_RADIUS));
            }
            // A line between the folders and the files.
            if index == self.folders && self.folders > 0 {
                let (x0, x1) = (b.min_x() + padding, b.max_x() - padding);
                let color = st.muted.with_alpha(FOLDERS_LINE);
                ds::divider_in(layer, &ctx, (x0, x1, top.round()), color);
            }
            rows.push((index, m, top, pill));
        }
        for (index, m, top, pill) in rows {
            let clip = Rect::new(
                pill.min_x(),
                pill.min_y(),
                (pill.size.width - px(10.0)).max(0.0),
                pill.size.height,
            );
            let layer = scene.push_layer(Some(clip.intersect(&b).unwrap_or_default()));
            let baseline = centered_baseline(top, row, &metrics);
            let mut x = b.min_x() + padding;
            let icon = px(16.0);
            let (id, color) = match icons.id("folder").filter(|_| index < self.folders) {
                Some(folder) => (folder, st.highlight),
                None => file_icon(m.name()),
            };
            draw_icon(
                layer,
                icons,
                id,
                Rect::new(x, (top + (row - icon) / 2.0).round(), icon, icon),
                color,
            );
            x += icon + px(8.0);

            // The name, in segments: matched letters accented, the rest normal.
            let name = m.name();
            let mut start = 0;
            let mut segments: Vec<(&str, bool)> = Vec::new();
            for &p in &m.name_positions {
                let Some(c) = name[p..].chars().next() else { continue };
                if p > start {
                    segments.push((&name[start..p], false));
                }
                segments.push((&name[p..p + c.len_utf8()], true));
                start = p + c.len_utf8();
            }
            if start < name.len() {
                segments.push((&name[start..], false));
            }
            // Too long for the row: cut in the middle, its extension kept (unlit).
            let room = (clip.max_x() - x - px(6.0)).max(0.0);
            let fitted = ui_components::ds::ellipsize_file(text, name, &font, room);
            if fitted != name {
                segments = vec![(fitted.as_str(), false)];
            }
            for (segment, matched) in segments {
                let shaped = text.shape_line(segment, &font);
                draw_text(
                    layer,
                    &shaped,
                    x.round(),
                    baseline,
                    if matched { st.highlight } else { st.text },
                );
                x += shaped.width;
            }
            if !m.dir().is_empty() {
                // A long directory is shortened from the left (the end of the path matters).
                let x = x + px(10.0);
                let room = clip.max_x() - x;
                let mut dir = m.dir().to_owned();
                let mut shaped = text.shape_line(&dir, &font);
                let mut rest = m.dir();
                while shaped.width > room {
                    let Some((_, tail)) = rest.split_once('/') else { break };
                    rest = tail;
                    dir = format!("…/{rest}");
                    shaped = text.shape_line(&dir, &font);
                }
                draw_text(layer, &shaped, x.round(), baseline, st.muted);
            }
        }
    }
}

/// The line between the folders and the files: its alpha of the muted text.
const FOLDERS_LINE: f32 = 0.25;

#[cfg(test)]
#[path = "../tests/unit/file_search.rs"]
mod tests;
