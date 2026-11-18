//! The shapes palette, on the sheet's left: a search field ("type to search": the shapes of
//! every section whose title holds it), then the sections, each folding, its shapes as
//! thumbnails (made only for the open ones, on a worker). A shape clicked lands in the
//! middle of the view; dragged, where it's let go (its outline shown on the way) — on the
//! grid, chosen, one step of the history.

use i18n::t;
use kernel::{Point, Rect};
use render_api::Quad;
use ui_components::ds::{self, Ctx};
use ui_components::{KeyReach, LayerPainter, ViewerKey};
use ui_kit::{TextInput, draw_text};

use super::DiagramView;
use super::gesture::Gesture;
use crate::palette::{SECTIONS, THUMB, Thumbnails};

/// The palette's width, a section header's height, a shape's cell (logical pixels).
pub(super) const WIDTH: f32 = 196.0;
const HEADER_H: f32 = 28.0;
const CELL: (f32, f32) = (46.0, 40.0);
const SEARCH_H: f32 = 30.0;

/// What a point of the palette is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Search,
    Section(usize),
    Entry(usize, usize),
}

/// The palette's state.
pub(super) struct Palette {
    /// The sections open (the first, General, at first).
    open: Vec<bool>,
    pub(super) search: TextInput,
    /// Its field has the keys.
    pub(super) searching: bool,
    scroll: f32,
    content_h: f32,
    hits: Vec<(Rect, Hit)>,
    pub(super) thumbs: Thumbnails,
    /// The mouse over it (the wheel scrolls it then).
    pub(super) over: bool,
}

impl Default for Palette {
    fn default() -> Self {
        let mut open = vec![false; SECTIONS.len()];
        open[0] = true;
        Self {
            open,
            search: TextInput::new(""),
            searching: false,
            scroll: 0.0,
            content_h: 0.0,
            hits: Vec::new(),
            thumbs: Thumbnails::default(),
            over: false,
        }
    }
}

/// A group of the palette's shapes: its section (`None`: what a search found), and its
/// shapes (their section, their place in it).
type Group = (Option<usize>, Vec<(usize, usize)>);

/// A shape's title (as the user reads it).
fn title(id: &str) -> String {
    t(&format!("diagram_shapes.{id}"))
}

impl DiagramView {
    /// The palette's area (the view's left).
    pub(super) fn palette_area(&self) -> Rect {
        let top = self.toolbar_area().max_y();
        let w = (WIDTH * self.scale).min(self.bounds.size.width / 3.0);
        Rect::new(self.bounds.min_x(), top, w, (self.bounds.max_y() - top).max(0.0))
    }

    fn palette_hit(&self, p: Point) -> Option<Hit> {
        self.palette.hits.iter().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)
    }

    /// A press on the palette: its field, a section folded, a shape picked up. `true`: it
    /// was on the palette.
    pub(super) fn press_palette(&mut self, p: Point, clicks: u32) -> bool {
        if !self.palette_area().contains(p) {
            self.palette.searching = false;
            return false;
        }
        match self.palette_hit(p) {
            Some(Hit::Search) => {
                self.palette.searching = true;
                self.palette.search.press(p.x, clicks);
            }
            Some(Hit::Section(s)) => {
                self.palette.open[s] = !self.palette.open[s];
                self.palette.searching = false;
            }
            Some(Hit::Entry(s, e)) if !self.locked => {
                self.palette.searching = false;
                self.gesture = Some(Gesture::Place { section: s, entry: e, from: p, to: p });
            }
            _ => self.palette.searching = false,
        }
        true
    }

    /// A shape of the palette let go at `to` (from `from`): where it lands — on the sheet
    /// there; a click (not moved): the view's middle; elsewhere, nowhere.
    pub(super) fn finish_place(
        &mut self,
        section: usize,
        entry: usize,
        from: Point,
        to: Point,
    ) -> bool {
        let sheet = self.sheet();
        let clicked = (to.x - from.x).hypot(to.y - from.y) <= 3.0 * self.scale;
        let at = if clicked {
            sheet.center()
        } else if sheet.contains(to) {
            to
        } else {
            return false;
        };
        let at = self.camera.to_diagram(at, sheet, self.scale);
        self.add_entry(section, entry, at)
    }

    /// Shape `entry` of `section` added centered on `at` (diagram units, its corner on the
    /// grid), chosen.
    pub(super) fn add_entry(&mut self, section: usize, entry: usize, at: Point) -> bool {
        let Some(e) = SECTIONS.get(section).and_then(|s| s.entries.get(entry)).copied() else {
            return false;
        };
        let r = Rect::new(at.x - e.size.0 / 2.0, at.y - e.size.1 / 2.0, e.size.0, e.size.1);
        self.add_entry_in(section, entry, r)
    }

    /// Shape `entry` of `section` added in box `r` (diagram units: its corners on the grid,
    /// at least a grid step each way), chosen.
    pub(super) fn add_entry_in(&mut self, section: usize, entry: usize, r: Rect) -> bool {
        let Some(cell) = self.entry_cell(section, entry, r) else { return false };
        let id = cell.id.clone();
        let page = self.page;
        let Some(d) = self.diagram.as_mut() else { return false };
        let tx = diagram_model::ops::add(&d.doc, page, cell);
        let done = d.edit(tx, None);
        if done {
            self.selection = vec![id];
        }
        done
    }

    /// Shape `entry` of `section` as a new cell in box `r` (its corners on the grid, at
    /// least a grid step each way), on the page's first layer — made, not added.
    pub(super) fn entry_cell(
        &self,
        section: usize,
        entry: usize,
        r: Rect,
    ) -> Option<diagram_model::Cell> {
        let e = SECTIONS.get(section).and_then(|s| s.entries.get(entry)).copied()?;
        let least = self.grid().filter(|g| *g > 0.0).unwrap_or(10.0);
        let (a, b) = (
            self.grid_point(Point::new(r.min_x(), r.min_y())),
            self.grid_point(Point::new(r.max_x(), r.max_y())),
        );
        let (x, y) = (a.x, a.y);
        let (w, h) = ((b.x - x).max(least), (b.y - y).max(least));
        let page = self.diagram.as_ref()?.doc.pages.get(self.page)?;
        let layer = super::clipboard::first_layer(page)?;
        let id = super::clipboard::new_id();
        let mut cell = diagram_model::Cell::new(id, diagram_model::CellKind::Vertex, layer);
        cell.value = e.value.to_owned();
        cell.style = diagram_model::Style::parse(e.style);
        cell.geometry = Some(diagram_model::Geometry {
            x: f64::from(x),
            y: f64::from(y),
            width: f64::from(w),
            height: f64::from(h),
            ..Default::default()
        });
        Some(cell)
    }

    /// A key while the search has the keys: its field's (Esc lets go; ↩ adds the first
    /// shape found). `true`: used.
    pub(super) fn palette_key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        let s = &mut self.palette.search;
        match key {
            ViewerKey::Escape => {
                if s.text().is_empty() {
                    self.palette.searching = false;
                } else {
                    s.set_text("");
                }
            }
            ViewerKey::Enter => {
                let found = crate::palette::search(s.text(), |_, e| title(e.id));
                if let Some(&(sec, e)) = found.first() {
                    let at =
                        self.camera.to_diagram(self.sheet().center(), self.sheet(), self.scale);
                    self.add_entry(sec, e, at);
                }
            }
            ViewerKey::Backspace => s.backspace(reach.word),
            ViewerKey::Delete => s.delete(),
            ViewerKey::Left => s.move_left(reach.word, reach.extend),
            ViewerKey::Right => s.move_right(reach.word, reach.extend),
            ViewerKey::Home => s.home(reach.extend),
            ViewerKey::End => s.end(reach.extend),
            _ => return false,
        }
        self.palette.scroll = 0.0;
        true
    }

    /// The wheel over the palette: its sections scrolled. `true`: moved.
    pub(super) fn scroll_palette(&mut self, dy: f32) -> bool {
        let room = (self.palette_area().size.height - (SEARCH_H + 16.0) * self.scale).max(0.0);
        let max = (self.palette.content_h - room).max(0.0);
        let before = self.palette.scroll;
        self.palette.scroll = (self.palette.scroll + dy).clamp(0.0, max);
        self.palette.scroll != before
    }

    /// A shape's title under `p` (its tooltip).
    pub(super) fn palette_tip(&self, p: Point) -> Option<(Rect, String)> {
        self.palette.hits.iter().find_map(|(r, h)| match h {
            Hit::Entry(s, e) if r.contains(p) => Some((*r, title(SECTIONS[*s].entries[*e].id))),
            _ => None,
        })
    }

    /// The palette drawn.
    pub(super) fn paint_palette(
        &mut self,
        ctx: &mut ui_components::ViewerPaint<'_>,
        theme: &crate::drawing::Theme,
    ) {
        let area = self.palette_area();
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let p = ctx.palette;
        let px = |v: f32| ds.px(v);
        let layer = ctx.scene.push_layer(Some(area));
        layer.quad(Quad::filled(area, p.sidebar_background));
        layer.quad(Quad::filled(
            Rect::new(
                area.max_x() - ctx.scale.max(1.0),
                area.min_y(),
                ctx.scale.max(1.0),
                area.size.height,
            ),
            p.border,
        ));
        self.palette.hits.clear();
        let field = Rect::new(
            area.min_x() + px(8.0),
            area.min_y() + px(8.0),
            area.size.width - px(16.0),
            px(SEARCH_H),
        );
        ds::search_input(
            &mut LayerPainter::new(layer, ctx.text, ctx.icons, ds),
            field,
            &self.palette.search,
            self.palette.searching,
            &t("diagram_editor.search_shapes"),
        );
        self.palette.hits.push((field, Hit::Search));
        let list = Rect::new(
            area.min_x(),
            field.max_y() + px(8.0),
            area.size.width,
            (area.max_y() - field.max_y() - px(8.0)).max(0.0),
        );
        let layer = ctx.scene.push_layer(Some(list));
        let font = ds.ui_font(-1.0).with_weight(text_layout::Font::SEMIBOLD);
        let cols = ((list.size.width - px(16.0)) / px(CELL.0)).floor().max(1.0) as usize;
        let mut y = list.min_y() - self.palette.scroll;
        let query = self.palette.search.text().to_owned();
        // What to show: the sections, or one list of what's found.
        let groups: Vec<Group> = if query.trim().is_empty() {
            (0..SECTIONS.len())
                .map(|s| (Some(s), (0..SECTIONS[s].entries.len()).map(|e| (s, e)).collect()))
                .collect()
        } else {
            vec![(None, crate::palette::search(&query, |_, e| title(e.id)))]
        };
        let waker = self.waker.lock().ok().and_then(|w| w.clone());
        for (section, entries) in groups {
            if let Some(s) = section {
                let header = Rect::new(list.min_x(), y, list.size.width, px(HEADER_H));
                let open = self.palette.open[s];
                let chevron = ctx.icons.id(if open { "chevron-down" } else { "chevron-right" });
                let icon = Rect::new(
                    header.min_x() + px(8.0),
                    header.center().y - px(7.0),
                    px(14.0),
                    px(14.0),
                );
                if let Some(id) = chevron {
                    ui_kit::draw_icon(layer, ctx.icons, id, icon, p.text_muted);
                }
                let label = t(&format!("diagram_editor.section_{}", SECTIONS[s].id));
                let shaped = ctx.text.shape_line(&label, &font);
                draw_text(
                    layer,
                    &shaped,
                    (icon.max_x() + px(6.0)).round(),
                    (header.center().y + font.size * 0.35).round(),
                    p.text,
                );
                self.palette.hits.push((header, Hit::Section(s)));
                y = header.max_y();
                if !open {
                    continue;
                }
            }
            for (n, (s, e)) in entries.iter().copied().enumerate() {
                let (col, row) = (n % cols, n / cols);
                let cell = Rect::new(
                    list.min_x() + px(8.0) + px(CELL.0) * col as f32,
                    y + px(CELL.1) * row as f32,
                    px(CELL.0),
                    px(CELL.1),
                );
                if cell.max_y() >= list.min_y() && cell.min_y() <= list.max_y() {
                    self.palette.hits.push((cell, Hit::Entry(s, e)));
                    let corner = Point::new(
                        (cell.center().x - px(THUMB.0) / 2.0).round(),
                        (cell.center().y - px(THUMB.1) / 2.0).round(),
                    );
                    if let Some(thumbs) =
                        self.palette.thumbs.section(s, theme, ctx.scale, waker.as_ref())
                        && let Some(masks) = thumbs.get(e)
                    {
                        for (image, r) in masks.iter() {
                            layer.image(image, crate::palette::place(corner, *r));
                        }
                    }
                }
            }
            y += px(CELL.1) * entries.len().div_ceil(cols) as f32 + px(4.0);
        }
        self.palette.content_h = y + self.palette.scroll - list.min_y();
        // A shape dragged over the sheet: its outline where it would land.
        if let Some(Gesture::Place { section, entry, to, .. }) = &self.gesture
            && self.sheet().contains(*to)
            && let Some(e) = SECTIONS.get(*section).and_then(|s| s.entries.get(*entry))
        {
            let k = self.camera.k(self.scale);
            let (w, h) = (e.size.0 * k, e.size.1 * k);
            let r = Rect::new(to.x - w / 2.0, to.y - h / 2.0, w, h);
            let layer = ctx.scene.push_layer(Some(self.sheet()));
            let line = ctx.scale.max(1.0);
            for side in [
                Rect::new(r.min_x(), r.min_y(), r.size.width, line),
                Rect::new(r.min_x(), r.max_y() - line, r.size.width, line),
                Rect::new(r.min_x(), r.min_y(), line, r.size.height),
                Rect::new(r.max_x() - line, r.min_y(), line, r.size.height),
            ] {
                layer.quad(Quad::filled(side, p.accent));
            }
        }
    }
}

#[cfg(test)]
impl DiagramView {
    /// The shapes shown in the palette (their cells, their section and place).
    pub(super) fn palette_hits_for_tests(&self) -> Vec<(Rect, (usize, usize))> {
        self.palette
            .hits
            .iter()
            .filter_map(|(r, h)| match h {
                Hit::Entry(s, e) => Some((*r, (*s, *e))),
                _ => None,
            })
            .collect()
    }

    pub(super) fn palette_hits_for_search(&self) -> Point {
        self.palette
            .hits
            .iter()
            .find(|(_, h)| *h == Hit::Search)
            .map(|(r, _)| r.center())
            .unwrap_or_default()
    }

    pub(super) fn palette_section_for_tests(&self, s: usize) -> Point {
        self.palette
            .hits
            .iter()
            .find(|(_, h)| *h == Hit::Section(s))
            .map(|(r, _)| r.center())
            .unwrap_or_default()
    }
}
