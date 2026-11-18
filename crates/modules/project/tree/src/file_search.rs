//! File search in the project panel: a "pill" field with a magnifier and, while there is
//! text, the result list (instead of the tree), with the matched letters highlighted.
//!
//! The component only displays and holds the field state; the search is the project's one
//! path search ([`workspace::search_paths`]), done by the caller: the matching
//! folders come first (a folder icon, a line under them), then the files.

use icons::{IconId, Icons};
use kernel::{Point, Rect, Rgba};
use render_api::{Quad, Scene};
use text_layout::{Font, FontFamily, TextSystem};
use ui_kit::{TextInput, TextInputStyle, centered_baseline, draw_icon, draw_text};
use workspace::{FileMatch, PathResults};

use ui_components::Palette;

/// The search's appearance; built from the theme or given individually. Sizes in logical
/// pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct FileSearchStyle {
    pub field_background: Rgba,
    pub field_border: Rgba,
    pub field_border_focused: Rgba,
    pub icon: Rgba,
    pub text: Rgba,
    pub placeholder: Rgba,
    /// The matched letters in the file name.
    pub highlight: Rgba,
    pub muted: Rgba,
    pub selection: Rgba,
    pub text_selection: Rgba,
    pub hover: Rgba,
    pub caret: Rgba,
    pub font_family: FontFamily,
    pub font_size: f32,
    pub field_height: f32,
    pub row_height: f32,
}

impl FileSearchStyle {
    pub fn from_palette(p: &Palette) -> Self {
        Self {
            field_background: p.search_field_background,
            field_border: p.search_field_border,
            field_border_focused: p.search_field_border_focused,
            icon: p.text_muted,
            text: p.text,
            placeholder: p.text_muted,
            highlight: p.accent,
            muted: p.text_muted,
            selection: p.tree_selection,
            text_selection: p.selection,
            hover: p.tree_hover,
            caret: p.caret,
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
    pub input: TextInput,
    style: FileSearchStyle,
    scale: f32,
    /// The field area (with surrounding margins) and the result list area.
    area: Rect,
    field: Rect,
    list: Rect,
    /// The folders found, then the files.
    results: Vec<FileMatch>,
    /// How many of `results` are folders (the first ones).
    folders: usize,
    selected: usize,
    hovered: Option<usize>,
    /// The mouse is on the × button (clears the field).
    clear_hovered: bool,
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
        self.field = Rect::new(
            area.min_x() + px(12.0),
            area.min_y() + px(2.0),
            (area.size.width - px(24.0)).max(0.0),
            h,
        );
    }

    /// The result list is shown (instead of the tree).
    pub fn is_active(&self) -> bool {
        !self.input.text().trim().is_empty()
    }

    pub fn query(&self) -> &str {
        self.input.text()
    }

    /// The × button on the right of the field (only with text).
    pub fn clear_button(&self) -> Option<Rect> {
        if self.input.text().is_empty() || self.field.size.width <= 0.0 {
            return None;
        }
        let px = |v: f32| (v * self.scale).round();
        let size = px(18.0);
        let cx = self.field.max_x() - px(8.0) - size / 2.0;
        let cy = self.field.min_y() + self.field.size.height / 2.0;
        Some(Rect::new((cx - size / 2.0).round(), (cy - size / 2.0).round(), size, size))
    }

    pub fn field_contains(&self, p: Point) -> bool {
        self.field.contains(p)
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
        self.input.set_text("");
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
        let clear = self.clear_button().is_some_and(|r| r.contains(p));
        let changed = hovered != self.hovered || clear != self.clear_hovered;
        self.hovered = hovered;
        self.clear_hovered = clear;
        changed
    }

    pub fn scroll_by(&mut self, dy: f32) -> bool {
        let before = self.scroll_y;
        let content = self.results.len() as f32 * self.row_height();
        let max = (content - self.list.size.height).max(0.0);
        self.scroll_y = (self.scroll_y + dy).clamp(0.0, max).round();
        self.scroll_y != before
    }

    /// The field: pill with magnifier, text or placeholder.
    pub fn paint_field(
        &self,
        focused: bool,
        icons: &Icons,
        text: &mut TextSystem,
        scene: &mut Scene,
    ) {
        let st = &self.style;
        let px = |v: f32| (v * self.scale).round();
        let f = self.field;
        if f.size.width <= 0.0 {
            return;
        }
        let line = self.scale.round().max(1.0);
        let layer = scene.push_layer(Some(self.area));
        let border = if focused { st.field_border_focused } else { st.field_border };
        layer.quad(
            Quad::filled(f, st.field_background)
                .rounded(f.size.height / 2.0)
                .bordered(line, border),
        );
        let icon = px(15.0);
        let icon_x = f.min_x() + px(13.0);
        if let Some(id) = icons.id("search") {
            let r =
                Rect::new(icon_x, (f.min_y() + (f.size.height - icon) / 2.0).round(), icon, icon);
            draw_icon(layer, icons, id, r, st.icon);
        }
        let input_style = TextInputStyle {
            font: Font::new(st.font_family.clone(), px(st.font_size)),
            background: Rgba::TRANSPARENT,
            border: Rgba::TRANSPARENT,
            border_focused: Rgba::TRANSPARENT,
            text: st.text,
            placeholder: st.placeholder,
            selection: st.text_selection,
            caret: st.caret,
            radius: 0.0,
            border_width: 0.0,
            padding: 0.0,
            caret_width: px(1.5).max(1.0),
            lead: 0.0,
        };
        let text_x = icon_x + icon + px(9.0);
        let mut right = f.max_x() - px(14.0);
        // With text: the × button (subtle circle, small x) clears the field.
        if let Some(r) = self.clear_button() {
            let fill = st.text.with_alpha(if self.clear_hovered { 0.12 } else { 0.06 });
            layer.quad(Quad::filled(r, fill).rounded(r.size.width / 2.0));
            if let Some(id) = icons.id("x") {
                let s = px(9.0);
                let ir = Rect::new(
                    (r.min_x() + (r.size.width - s) / 2.0).round(),
                    (r.min_y() + (r.size.height - s) / 2.0).round(),
                    s,
                    s,
                );
                draw_icon(layer, icons, id, ir, if self.clear_hovered { st.text } else { st.icon });
            }
            right = r.min_x() - px(6.0);
        }
        let inner = Rect::new(text_x, f.min_y(), (right - text_x).max(0.0), f.size.height);
        let layer = scene.push_layer(Some(inner));
        self.input.paint(layer, text, inner, focused, &i18n::t("files.placeholder"), &input_style);
    }

    /// The results, in the list area. `file_icon(name)` gives the file's icon and color.
    pub fn paint_results(
        &self,
        icons: &Icons,
        file_icon: &dyn Fn(&str) -> (IconId, Rgba),
        text: &mut TextSystem,
        scene: &mut Scene,
    ) {
        let st = &self.style;
        let px = |v: f32| (v * self.scale).round();
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
            if index == self.selected {
                layer.quad(Quad::filled(pill, st.selection).rounded(px(6.0)));
            } else if self.hovered == Some(index) {
                layer.quad(Quad::filled(pill, st.hover).rounded(px(6.0)));
            }
            // A line between the folders and the files.
            if index == self.folders && self.folders > 0 {
                let line = scale_line(self.scale);
                layer.quad(Quad::filled(
                    Rect::new(b.min_x() + padding, top.round(), b.size.width - 2.0 * padding, line),
                    st.muted.with_alpha(0.25),
                ));
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

/// A hairline, in physical pixels.
fn scale_line(scale: f32) -> f32 {
    scale.round().max(1.0)
}

#[cfg(test)]
#[path = "../tests/unit/file_search.rs"]
mod tests;
