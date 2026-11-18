//! The parts' table: grouped (the groups fold, their totals on their header), virtualized
//! (only the rows in view are drawn: they're all as tall), its columns as the layout fitted
//! them under their headers (a click sorts; an edge between two is dragged to resize the
//! column on its left, double-clicked to fit it; the "⋯" at the end chooses them), a part's
//! programs under it once opened, the operations with their progress — or, on a narrow
//! page, a list of two-line or one-line rows. Its empty states: gathering (the first look),
//! nothing matching the filter (with "Clear the filter"), paused (a band with "Resume").

use std::sync::PoisonError;

use kernel::{Rect, Rgba};
use render_api::{Layer, Quad, Scene};
use text_layout::{Font, TextSystem};
use ui_components::LayerPainter;
use ui_components::chart::{Samples, Style, sparkline};
use ui_components::ds::{self, Ctx, radius};
use ui_components::tree;
use ui_kit::draw_text;
use workbench_model::Outcome;
use workbench_model::resources::{
    APP, INDEXING, LANGUAGE_SERVERS, MODULES, PROGRAMS, SERVICES, TERMINALS,
};

use super::{Focus, Hit, Popover, TaskManager, column_of, summary};
use crate::collector::{Line, Metric};
use crate::layout::{ColumnId, ListMode, PageLayout};
use crate::model::format::kind_icon;
use crate::model::{Column, Item, OperationRow, PartTexts, ProcessTexts, ViewModel};

/// The icon of a part's group.
pub(super) fn group_icon(group: &str) -> &'static str {
    match group {
        g if g == APP => "app-window",
        g if g == INDEXING => "index",
        g if g == TERMINALS => "terminal",
        g if g == PROGRAMS => "console",
        g if g == SERVICES => "database",
        g if g == LANGUAGE_SERVERS => "server",
        g if g == MODULES => "modules",
        _ => "cpu",
    }
}

/// The room kept at a row's right end (its scrollbar's) by what's written there.
const EDGE: f32 = 16.0;
/// How near a column's edge the mouse resizes it (logical pixels, each side).
pub(super) const GRIP: f32 = 4.0;

/// A row's place and what it may draw over.
struct RowArea {
    rect: Rect,
    /// The rows' visible area (a row's clicks stop at its edges).
    clip: Rect,
    item: usize,
}

impl TaskManager {
    pub(super) fn paint_table(
        &mut self,
        scene: &mut Scene,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        page: &PageLayout,
        model: &ViewModel,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let mut rows = page.rows;
        // Paused: a thin band over the rows, with "Resume".
        if self.paused {
            let band_h = px(30.0);
            let band = Rect::new(rows.min_x(), rows.min_y(), rows.size.width, band_h);
            let layer = scene.push_layer(Some(band));
            layer.quad(Quad::filled(band, p.warning.with_alpha(0.10)).rounded(px(radius::SM)));
            let font = ds.ui_font(-1.0);
            let resume_w = ds::primary_width(text, ds, &[&self.texts.labels.resume]).min(px(110.0));
            let button = Rect::new(
                band.max_x() - resume_w - px(4.0),
                band.min_y() + px(3.0),
                resume_w,
                band_h - px(6.0),
            );
            let base = ds::optical_baseline(band.min_y(), band_h, &font);
            let room = (button.min_x() - band.min_x() - px(20.0)).max(0.0);
            ds::draw_ellipsized(
                layer,
                text,
                &self.texts.labels.paused,
                &font,
                (band.min_x() + px(10.0), base),
                room,
                p.text,
            );
            let hovered = self.hover == Some(Hit::Resume);
            ds::secondary_button(layer, text, ds, button, &self.texts.labels.resume, true, hovered);
            self.hits.push((button, Hit::Resume));
            rows = Rect::new(
                rows.min_x(),
                band.max_y() + px(4.0),
                rows.size.width,
                (rows.size.height - band_h - px(4.0)).max(0.0),
            );
        }
        if let Some(header) = page.header {
            self.paint_header(scene, text, icons, ds, page, header);
        }
        let row_h = page.row_height;
        let items = &model.items;
        let content = items.len() as f32 * row_h;
        self.max_scroll = (content - rows.size.height).max(0.0);
        self.scroll = self.scroll.clamp(0.0, self.max_scroll);
        let scroll = self.scroll.round();
        // When it scrolls, the scrollbar has a lane of its own: the rows (their bands, the
        // cursor's ring) end before it, never under it.
        let lane = if self.max_scroll > 0.0 {
            ui_components::scrollbar::Scrollbar::track_thickness(ds.scale)
        } else {
            0.0
        };
        let row_w = (rows.size.width - lane).max(0.0);
        let list = tree::TreeList::new(row_h, items.len());
        let visible = list.visible(scroll, rows.size.height);
        // The trend's plots: one a row in view (their room reused).
        if page.columns.shows(ColumnId::Trend) && self.plots.trend.len() < visible.len() {
            self.plots.trend.resize_with(visible.len(), Default::default);
        }
        // The table has the keys after its header's "⋯"; its keys' row gets the ring.
        self.stops.push((rows, Focus::Table));
        let cursor = self.cursor_index(model);
        let history = self.history.clone();
        let history = history.as_ref().map(|h| h.lock().unwrap_or_else(PoisonError::into_inner));
        let layer = scene.push_layer(Some(rows));
        // The cells' bars are a share of the busiest part's CPU.
        let highest = model.frame.lines.iter().map(|l| l.cpu).fold(1.0_f32, f32::max);
        for (slot, i) in visible.enumerate() {
            let top = rows.min_y() - scroll + i as f32 * row_h;
            let area =
                RowArea { rect: Rect::new(rows.min_x(), top, row_w, row_h), clip: rows, item: i };
            if cursor == Some(i) {
                self.cursor_rect = inset(area.rect, px(1.5)).intersect(&rows);
            }
            match &items[i] {
                Item::Group { key, title, cpu_text, memory_text, running, open, .. } => {
                    let hit = Hit::Group(*key);
                    let lit = self.hover == Some(hit);
                    let r = inset(area.rect, px(1.5));
                    tree::highlight(
                        layer,
                        r,
                        p.text.with_alpha(if lit { 0.07 } else { 0.035 }),
                        px(radius::SM),
                    );
                    let s = px(12.0);
                    let x = area.rect.min_x() + px(8.0);
                    let at = Rect::new(x, top + (row_h - s) / 2.0, s, s);
                    tree::chevron(layer, icons, at, *open, p.text_muted);
                    let font = ds.ui_font(-1.0).with_weight(Font::SEMIBOLD);
                    let base = ds::optical_baseline(top, row_h, &font);
                    let name_end = match page.list_mode {
                        ListMode::Table => page
                            .columns
                            .get(ColumnId::Name)
                            .map_or(area.rect.max_x(), |(x, w)| x + w),
                        _ => area.rect.max_x() - px(90.0),
                    };
                    let tx = x + px(18.0);
                    ds::draw_ellipsized(
                        layer,
                        text,
                        title,
                        &font,
                        (tx, base),
                        (name_end - tx).max(0.0),
                        p.text,
                    );
                    let mono = ds.code_font(-1.0);
                    let mbase = ds::optical_baseline(top, row_h, &mono);
                    let cpu_color = match running {
                        Some(n) if *n > 0 => p.accent,
                        _ => p.text_muted,
                    };
                    let cpu_font = if running.is_some() { ds.ui_font(-1.5) } else { mono.clone() };
                    match page.list_mode {
                        ListMode::Table => {
                            let cpu_at = if running.is_some() {
                                Some((area.rect.min_x(), area.rect.size.width - px(EDGE)))
                            } else {
                                page.columns.get(ColumnId::Cpu)
                            };
                            if let Some((cx, cw)) = cpu_at {
                                right_text(
                                    layer,
                                    text,
                                    cpu_text,
                                    &cpu_font,
                                    cx + cw,
                                    mbase,
                                    cpu_color,
                                );
                            }
                            if let (Some((mx, mw)), Some(memory)) =
                                (page.columns.get(ColumnId::Memory), memory_text)
                            {
                                right_text(
                                    layer,
                                    text,
                                    memory,
                                    &mono,
                                    mx + mw,
                                    mbase,
                                    p.text_muted,
                                );
                            }
                        }
                        _ => right_text(
                            layer,
                            text,
                            cpu_text,
                            &if running.is_some() { cpu_font } else { ds.ui_font(-1.0) },
                            area.rect.max_x() - px(EDGE),
                            mbase,
                            cpu_color,
                        ),
                    }
                    self.push_row_hit(&area, hit);
                }
                Item::Part { index, texts, others } => {
                    let line = &model.frame.lines[*index];
                    let trend = history
                        .as_deref()
                        .and_then(|h| h.part(&line.key))
                        .map(|t| Samples::from_slices(t.get(Metric::Cpu).as_slices()));
                    let generation =
                        history.as_deref().map_or(0, crate::collector::History::generation);
                    let slots = history.as_deref().map_or(60, crate::collector::History::capacity);
                    let part =
                        PartRow { line, texts, others, highest, trend, generation, slots, slot };
                    self.paint_part(
                        &mut LayerPainter::new(layer, text, icons, *ds),
                        page,
                        &area,
                        &part,
                        model,
                    );
                }
                Item::Process { part, process, texts } => {
                    let line = &model.frame.lines[*part];
                    let selected = self.selected.as_deref() == Some(line.key.as_str());
                    self.paint_process(
                        &mut LayerPainter::new(layer, text, icons, *ds),
                        page,
                        &area,
                        texts,
                        selected,
                        (line.processes[*process].cpu, highest),
                    );
                }
                Item::Operation(operation) => self.paint_operation(
                    &mut LayerPainter::new(layer, text, icons, *ds),
                    page,
                    &area,
                    operation,
                ),
            }
        }
        drop(history);
        if items.is_empty() {
            let layer = scene.push_layer(Some(rows));
            if model.frame.lines.is_empty() {
                let loader = ds::Loader::with_text(&self.texts.labels.collecting);
                let w = loader.width(text, ds);
                let x = (rows.min_x() + (rows.size.width - w) / 2.0).round();
                ds::loader(
                    layer,
                    text,
                    ds,
                    x,
                    rows.min_y() + (rows.size.height / 2.0).min(px(120.0)),
                    &loader,
                );
            } else {
                let mid = rows.min_y() + (rows.size.height / 2.0).min(px(120.0));
                let message = Rect::new(rows.min_x(), mid - px(30.0), rows.size.width, px(40.0));
                ds::empty_state(layer, text, ds, message, &self.texts.labels.nothing_matches, None);
                let w = ds::primary_width(text, ds, &[&self.texts.labels.clear_filter]);
                let button = Rect::new(
                    (rows.min_x() + (rows.size.width - w) / 2.0).round(),
                    mid + px(14.0),
                    w,
                    px(30.0),
                );
                let hovered = self.hover == Some(Hit::ClearFilter);
                ds::secondary_button(
                    layer,
                    text,
                    ds,
                    button,
                    &self.texts.labels.clear_filter,
                    true,
                    hovered,
                );
                self.hits.push((button, Hit::ClearFilter));
            }
        }
        self.bar.paint(scene, p, ds.scale, rows, content, scroll);
    }

    /// The columns' headers (a click sorts by one; its arrow says which way) and the "⋯"
    /// choosing them.
    fn paint_header(
        &mut self,
        scene: &mut Scene,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        page: &PageLayout,
        header: Rect,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let layer = scene.push_layer(Some(header));
        let font = ds.ui_font(-1.5).with_weight(Font::SEMIBOLD);
        let base = ds::optical_baseline(header.min_y(), header.size.height, &font);
        let (sorted, descending) = self.state.sort;
        let sorted = column_of(sorted);
        for &(column, x, w) in &page.columns.shown {
            let label = self.texts.labels.column(column);
            let on = column == sorted;
            let color = if on { p.text } else { p.text_muted };
            let hovered = self.hover == Some(Hit::Header(column));
            let cell = Rect::new(
                x - px(4.0),
                header.min_y() + px(2.0),
                w + px(8.0),
                header.size.height - px(4.0),
            );
            if hovered && super::sort_column(column).is_some() {
                layer.quad(Quad::filled(cell, p.text.with_alpha(0.05)).rounded(px(radius::SM)));
            }
            let arrow = if on { if descending { "↓" } else { "↑" } } else { "" };
            let arrow_w = if on { text.shape_line(arrow, &font).width + px(3.0) } else { 0.0 };
            let room = (w - arrow_w).max(0.0);
            if column.numeric() {
                let label_w = text.shape_line(label, &font).width.min(room);
                let lx = x + w - arrow_w - label_w;
                ds::draw_ellipsized(layer, text, label, &font, (lx, base), room, color);
                if on {
                    draw_text(
                        layer,
                        &text.shape_line(arrow, &font),
                        x + w - arrow_w + px(3.0),
                        base,
                        p.accent,
                    );
                }
            } else {
                let (drawn, _) =
                    ds::draw_ellipsized(layer, text, label, &font, (x, base), room, color);
                if on {
                    draw_text(
                        layer,
                        &text.shape_line(arrow, &font),
                        x + drawn + px(3.0),
                        base,
                        p.accent,
                    );
                }
            }
            self.hits.push((cell, Hit::Header(column)));
        }
        // The edges between the columns, over the headers (dragged to resize, double-clicked
        // to fit): each a faint short line (where one can be taken), the one held or under
        // the mouse a taller line in the accent.
        let held = self.resizing.map(|(c, ..)| c);
        let faint = p.text_muted.with_alpha(0.22);
        for (column, x) in page.columns.edges() {
            let grip = Rect::new(x - px(GRIP), header.min_y(), px(GRIP) * 2.0, header.size.height);
            let lit =
                held == Some(column) || (held.is_none() && self.hover == Some(Hit::Edge(column)));
            let (inset, color) = if lit { (px(5.0), p.accent) } else { (px(10.0), faint) };
            let line = Rect::new(
                (x - ds.line() / 2.0).round(),
                header.min_y() + inset,
                ds.line().max(1.0),
                (header.size.height - 2.0 * inset).max(0.0),
            );
            layer.quad(Quad::filled(line, color));
            self.hits.push((grip, Hit::Edge(column)));
        }
        if let Some(chooser) = page.chooser {
            let hit = Hit::Open(Popover::Columns);
            let open = self.popover.is_some_and(|(p, _)| p == Popover::Columns);
            let drawn = icon_button(
                layer,
                icons,
                ds,
                chooser,
                "more-horizontal",
                self.hover == Some(hit),
                open,
            );
            self.hits.push((chooser, hit));
            self.stops.push((drawn, Focus::Chooser));
        }
        let line =
            Rect::new(header.min_x(), header.max_y() - ds.line(), header.size.width, ds.line());
        layer.quad(Quad::filled(line, p.border));
    }

    /// The width `column` needs for its content as the rows draw it (physical pixels;
    /// `None`: one with no text to fit, back to its spec's width): the names from their
    /// place in the row, the numbers' widest (already measured), the projects' names.
    pub(super) fn content_width(
        &self,
        text: &mut TextSystem,
        ds: &Ctx<'_>,
        model: &ViewModel,
        column: ColumnId,
    ) -> Option<f32> {
        let px = |v: f32| ds.px(v);
        let widest = match column {
            ColumnId::Name => {
                // From the column's left (the rows' inset): a part's chevron and icon, a
                // program's guide.
                let (part, program) = (ds.ui_font(-0.5), ds.code_font(-1.5));
                model
                    .items
                    .iter()
                    .filter_map(|item| match item {
                        Item::Part { index, .. } => {
                            let name = &model.frame.lines[*index].name;
                            Some(px(40.0) + text.shape_line(name, &part).width)
                        }
                        Item::Process { texts, .. } => {
                            Some(px(36.0) + text.shape_line(&texts.name, &program).width)
                        }
                        Item::Group { .. } | Item::Operation(_) => None,
                    })
                    .fold(0.0_f32, f32::max)
            }
            ColumnId::Project => {
                let small = ds.ui_font(-1.0);
                let dash = text.shape_line("—", &small).width;
                model
                    .frame
                    .projects
                    .iter()
                    .map(|pr| text.shape_line(&pr.name, &small).width)
                    .fold(dash, f32::max)
            }
            c if c.numeric() => {
                let w = self.texts.widths[c.index()];
                if w > 0.0 { w + px(12.0) } else { 0.0 }
            }
            _ => 0.0,
        };
        (widest > 0.0).then(|| (widest + px(4.0)).ceil())
    }

    /// A row's hit, only its part inside the rows' area.
    fn push_row_hit(&mut self, area: &RowArea, hit: Hit) {
        if let Some(r) = area.rect.intersect(&area.clip) {
            self.hits.push((r, hit));
        }
    }

    /// A row lit: selected (the accent, its edge) or under the mouse.
    fn row_background(&self, layer: &mut Layer, ds: &Ctx<'_>, area: &RowArea, selected: bool) {
        let p = ds.palette;
        let r = inset(area.rect, ds.px(1.0));
        if selected {
            layer.quad(
                Quad::filled(r, p.accent.with_alpha(0.12))
                    .rounded(ds.px(radius::SM))
                    .bordered(ds.line(), p.accent.with_alpha(0.55)),
            );
        } else if self.hover == Some(Hit::Row(area.item)) {
            tree::highlight(layer, r, p.text.with_alpha(0.05), ds.px(radius::SM));
        }
    }

    fn paint_part(
        &mut self,
        pen: &mut LayerPainter<'_>,
        page: &PageLayout,
        area: &RowArea,
        part: &PartRow<'_>,
        model: &ViewModel,
    ) {
        let ds = pen.ds;
        let ds = &ds;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let line = part.line;
        let top = area.rect.min_y();
        let row_h = area.rect.size.height;
        let selected = self.selected.as_deref() == Some(line.key.as_str());
        self.row_background(pen.layer, ds, area, selected);
        self.push_row_hit(area, Hit::Row(area.item));
        let font = ds.ui_font(-0.5);
        let mono = ds.code_font(-1.0);
        let x0 = area.rect.min_x() + px(8.0);
        // Its chevron (when it started programs), its group's icon, its name.
        let has_programs = !line.processes.is_empty();
        let chevron = Rect::new(x0, top + (row_h - px(12.0)) / 2.0, px(12.0), px(12.0));
        if has_programs {
            let open = self.state.expanded.contains(&line.key);
            tree::chevron(pen.layer, pen.icons, chevron, open, p.text_muted);
        }
        let icon = px(14.0);
        let icon_x = x0 + px(18.0);
        let first_line = match page.list_mode {
            ListMode::TwoLines => px(26.0),
            _ => row_h,
        };
        if let Some(id) = pen.icons.id(group_icon(line.group)) {
            let at = Rect::new(icon_x, top + (first_line - icon) / 2.0, icon, icon);
            ui_kit::draw_icon(pen.layer, pen.icons, id, at, p.text_muted);
        }
        let name_x = icon_x + icon + px(8.0);
        let base = ds::optical_baseline(top, first_line, &font);
        let sorted = self.state.sort.0;
        match page.list_mode {
            ListMode::Table => {
                let name_end =
                    page.columns.get(ColumnId::Name).map_or(area.rect.max_x(), |(x, w)| x + w);
                let room = (name_end - name_x).max(0.0);
                let (w, cut) = ds::draw_ellipsized(
                    pen.layer,
                    pen.text,
                    &line.name,
                    &font,
                    (name_x, base),
                    room,
                    p.text,
                );
                if cut {
                    self.cut.push((Rect::new(name_x, top, w, row_h), area.item));
                }
                let mbase = ds::optical_baseline(top, row_h, &mono);
                for &(column, x, w) in &page.columns.shown {
                    let right = x + w;
                    let value = |c: Column| part.texts.get(c);
                    match column {
                        ColumnId::Name => {}
                        ColumnId::Cpu => {
                            // A fine bar under the number: its share of the busiest.
                            if (line.threads > 0 || !line.processes.is_empty()) && line.cpu > 0.0 {
                                let share = (line.cpu / part.highest).clamp(0.0, 1.0);
                                let track = Rect::new(x, top + row_h - px(6.0), w, px(2.0));
                                pen.layer.quad(
                                    Quad::filled(track, p.text.with_alpha(0.07)).rounded(px(1.0)),
                                );
                                let bar = Rect::new(
                                    x,
                                    top + row_h - px(6.0),
                                    (w * share).max(ds.line()),
                                    px(2.0),
                                );
                                pen.layer.quad(
                                    Quad::filled(
                                        bar,
                                        summary::color(p, Metric::Cpu).with_alpha(0.7),
                                    )
                                    .rounded(px(1.0)),
                                );
                            }
                            cell(pen, value(Column::Cpu), &mono, right, mbase, p, line.cpu > 80.0);
                        }
                        ColumnId::Memory => {
                            cell(pen, value(Column::Memory), &mono, right, mbase, p, false)
                        }
                        ColumnId::Network => {
                            cell(pen, value(Column::Net), &mono, right, mbase, p, false)
                        }
                        ColumnId::Disk => {
                            cell(pen, value(Column::Disk), &mono, right, mbase, p, false)
                        }
                        ColumnId::Gpu => {
                            cell(pen, value(Column::Gpu), &mono, right, mbase, p, false)
                        }
                        ColumnId::Threads => {
                            cell(pen, value(Column::Threads), &mono, right, mbase, p, false)
                        }
                        ColumnId::Project => {
                            let name = line
                                .project
                                .and_then(|id| model.frame.projects.iter().find(|pr| pr.id == id))
                                .map_or("—", |pr| pr.name.as_str());
                            let small = ds.ui_font(-1.0);
                            let b = ds::optical_baseline(top, row_h, &small);
                            ds::draw_ellipsized(
                                pen.layer,
                                pen.text,
                                name,
                                &small,
                                (x, b),
                                w,
                                p.text_muted,
                            );
                        }
                        ColumnId::Trend => {
                            if let (Some(samples), Some(cache)) =
                                (part.trend, self.plots.trend.get_mut(part.slot))
                            {
                                let rect = Rect::new(x, top + px(7.0), w, row_h - px(14.0));
                                let key = part.generation.wrapping_mul(0x9E37_79B9_7F4A_7C15)
                                    ^ ((area.item as u64) << 40)
                                    ^ model.stamp.0
                                    ^ (model.stamp.1 << 20);
                                let plot = cache.get(
                                    key,
                                    samples,
                                    part.slots,
                                    w as usize,
                                    summary::reduce(Metric::Cpu),
                                );
                                let max = ui_components::chart::scale_for(plot.peak, 5.0);
                                sparkline(
                                    pen.layer,
                                    ds,
                                    rect,
                                    plot,
                                    summary::color(p, Metric::Cpu),
                                    Style::Steps,
                                    max,
                                );
                            }
                        }
                        ColumnId::Detail => {
                            let small = ds.ui_font(-1.5);
                            let b = ds::optical_baseline(top, row_h, &small);
                            ds::draw_ellipsized(
                                pen.layer,
                                pen.text,
                                &line.detail,
                                &small,
                                (x, b),
                                w,
                                p.text_muted,
                            );
                        }
                    }
                }
            }
            ListMode::TwoLines | ListMode::OneLine => {
                // The sorted metric on the right, large; the name before it.
                let metric = if sorted == Column::Name {
                    part.texts.cpu.as_deref()
                } else {
                    part.texts.get(sorted)
                };
                let metric = metric.unwrap_or("—");
                let big = ds.ui_font(0.0).with_weight(Font::SEMIBOLD);
                let mw = pen.text.shape_line(metric, &big).width;
                let right = area.rect.max_x() - px(EDGE);
                draw_text(
                    pen.layer,
                    &pen.text.shape_line(metric, &big),
                    right - mw,
                    ds::optical_baseline(top, first_line, &big),
                    p.text,
                );
                let room = (right - mw - px(10.0) - name_x).max(0.0);
                let (w, cut) = ds::draw_ellipsized(
                    pen.layer,
                    pen.text,
                    &line.name,
                    &font,
                    (name_x, base),
                    room,
                    p.text,
                );
                if cut {
                    self.cut.push((Rect::new(name_x, top, w, first_line), area.item));
                }
                if page.list_mode == ListMode::TwoLines && !part.others.is_empty() {
                    let small = ds.ui_font(-1.5);
                    let b = ds::optical_baseline(top + px(22.0), row_h - px(26.0), &small);
                    ds::draw_ellipsized(
                        pen.layer,
                        pen.text,
                        part.others,
                        &small,
                        (name_x, b),
                        (right - name_x).max(0.0),
                        p.text_muted,
                    );
                }
            }
        }
        if has_programs {
            // The chevron opens its programs (over the row's hit).
            let hit = inset(
                Rect::new(chevron.min_x() - px(4.0), top, chevron.size.width + px(8.0), first_line),
                0.0,
            );
            if let Some(r) = hit.intersect(&area.clip) {
                self.hits.push((r, Hit::Expand(area.item)));
            }
        }
    }

    fn paint_process(
        &mut self,
        pen: &mut LayerPainter<'_>,
        page: &PageLayout,
        area: &RowArea,
        texts: &ProcessTexts,
        selected: bool,
        (cpu, highest): (f32, f32),
    ) {
        let ds = pen.ds;
        let ds = &ds;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let top = area.rect.min_y();
        let row_h = area.rect.size.height;
        self.row_background(pen.layer, ds, area, false);
        self.push_row_hit(area, Hit::Row(area.item));
        // A guide from its part.
        let gx = area.rect.min_x() + px(14.0);
        pen.layer.quad(Quad::filled(Rect::new(gx, top, ds.line(), row_h), p.indent_guide));
        let x = area.rect.min_x() + px(44.0);
        let font = ds.code_font(-1.5);
        let base = ds::optical_baseline(top, row_h, &font);
        let mono = ds.code_font(-1.0);
        let mbase = ds::optical_baseline(top, row_h, &mono);
        let muted = if selected { p.text } else { p.text_muted };
        match page.list_mode {
            ListMode::Table => {
                let name_end =
                    page.columns.get(ColumnId::Name).map_or(area.rect.max_x(), |(x, w)| x + w);
                ds::draw_ellipsized(
                    pen.layer,
                    pen.text,
                    &texts.name,
                    &font,
                    (x, base),
                    (name_end - x).max(0.0),
                    muted,
                );
                for &(column, cx, w) in &page.columns.shown {
                    let value = match column {
                        ColumnId::Cpu => {
                            if cpu > 0.0 {
                                let share = (cpu / highest).clamp(0.0, 1.0);
                                let bar = Rect::new(
                                    cx,
                                    top + row_h - px(6.0),
                                    (w * share).max(ds.line()),
                                    px(2.0),
                                );
                                pen.layer.quad(
                                    Quad::filled(
                                        bar,
                                        summary::color(p, Metric::Cpu).with_alpha(0.45),
                                    )
                                    .rounded(px(1.0)),
                                );
                            }
                            &texts.cpu
                        }
                        ColumnId::Memory => &texts.memory,
                        ColumnId::Disk => &texts.disk,
                        _ => continue,
                    };
                    right_text(pen.layer, pen.text, value, &mono, cx + w, mbase, muted);
                }
            }
            ListMode::TwoLines | ListMode::OneLine => {
                let right = area.rect.max_x() - px(EDGE);
                let w = pen.text.shape_line(&texts.cpu, &mono).width;
                right_text(pen.layer, pen.text, &texts.cpu, &mono, right, mbase, muted);
                ds::draw_ellipsized(
                    pen.layer,
                    pen.text,
                    &texts.name,
                    &font,
                    (x, base),
                    (right - w - px(10.0) - x).max(0.0),
                    muted,
                );
            }
        }
    }

    /// An operation: its kind's icon, what it's on, where it is (on the right), how far (a
    /// thin bar along the row's foot).
    fn paint_operation(
        &mut self,
        pen: &mut LayerPainter<'_>,
        page: &PageLayout,
        area: &RowArea,
        operation: &OperationRow,
    ) {
        let ds = pen.ds;
        let ds = &ds;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let a = &operation.activity;
        let top = area.rect.min_y();
        let row_h = area.rect.size.height;
        self.row_background(pen.layer, ds, area, false);
        self.push_row_hit(area, Hit::Row(area.item));
        let outcome = a.ended.as_ref().map(|(_, o)| o);
        let color = match outcome {
            None => p.accent,
            Some(Outcome::Done) => summary::color(p, Metric::Memory),
            Some(Outcome::Failed(_)) => p.danger,
            Some(Outcome::Stopped) => p.warning,
        };
        let x0 = area.rect.min_x() + px(26.0);
        let s = px(13.0);
        if let Some(id) = pen.icons.id(kind_icon(a.kind)) {
            let at = Rect::new(x0, top + (row_h - s) / 2.0, s, s);
            ui_kit::draw_icon(
                pen.layer,
                pen.icons,
                id,
                at,
                if a.running() { color } else { color.with_alpha(0.7) },
            );
        }
        let right = area.rect.max_x() - px(EDGE);
        let state_font = ds.ui_font(-1.5);
        let state_w = pen.text.shape_line(&operation.status, &state_font).width;
        right_text(
            pen.layer,
            pen.text,
            &operation.status,
            &state_font,
            right,
            ds::optical_baseline(top, row_h, &state_font),
            if a.running() { p.text } else { color },
        );
        let font = ds.ui_font(-0.5);
        let name_x = x0 + s + px(8.0);
        let subject_end = match (page.list_mode, page.columns.get(ColumnId::Name)) {
            (ListMode::Table, Some((x, w))) => (x + w).min(right - state_w - px(12.0)),
            _ => right - state_w - px(12.0),
        };
        let (w, _) = ds::draw_ellipsized(
            pen.layer,
            pen.text,
            &a.subject,
            &font,
            (name_x, ds::optical_baseline(top, row_h, &font)),
            (subject_end - name_x).max(0.0),
            if a.running() { p.text } else { p.text_muted },
        );
        // Its kind and how much, after the subject when there's room.
        let small = ds.ui_font(-1.5);
        let detail_x = name_x + w + px(10.0);
        let room = right - state_w - px(14.0) - detail_x;
        if room > px(60.0) {
            let b = ds::optical_baseline(top, row_h, &small);
            let (kw, _) = ds::draw_ellipsized(
                pen.layer,
                pen.text,
                &operation.kind,
                &small,
                (detail_x, b),
                room,
                p.text_muted,
            );
            let px_ = detail_x + kw + px(8.0);
            if right - state_w - px(14.0) - px_ > px(40.0) {
                ds::draw_ellipsized(
                    pen.layer,
                    pen.text,
                    &operation.progress,
                    &small,
                    (px_, b),
                    right - state_w - px(14.0) - px_,
                    p.text_muted,
                );
            }
        }
        let fraction = match (outcome, a.total) {
            (Some(Outcome::Done), _) => 1.0,
            (_, Some(total)) if total > 0 => (a.done as f32 / total as f32).clamp(0.0, 1.0),
            _ => 0.0,
        };
        let track = Rect::new(name_x, top + row_h - px(4.0), (right - name_x).max(0.0), px(2.0));
        pen.layer.quad(Quad::filled(track, p.text.with_alpha(0.07)).rounded(px(1.0)));
        if fraction > 0.0 {
            let fill = Rect::new(
                track.min_x(),
                track.min_y(),
                track.size.width * fraction,
                track.size.height,
            );
            pen.layer.quad(
                Quad::filled(fill, color.with_alpha(if a.running() { 0.9 } else { 0.45 }))
                    .rounded(px(1.0)),
            );
        }
    }
}

/// A part's row as drawn.
struct PartRow<'a> {
    line: &'a Line,
    texts: &'a PartTexts,
    others: &'a str,
    /// The busiest part's CPU (the cells' bars are its share).
    highest: f32,
    trend: Option<Samples<'a>>,
    generation: u64,
    slots: usize,
    /// Its place among the rows in view (its trend's plot).
    slot: usize,
}

fn inset(r: Rect, by: f32) -> Rect {
    Rect::new(r.min_x(), r.min_y() + by, r.size.width, (r.size.height - 2.0 * by).max(0.0))
}

/// `value` ending at `right`.
pub(super) fn right_text(
    layer: &mut Layer,
    text: &mut TextSystem,
    value: &str,
    font: &Font,
    right: f32,
    base: f32,
    color: Rgba,
) {
    let shaped = text.shape_line(value, font);
    draw_text(layer, &shaped, (right - shaped.width).round(), base, color);
}

/// A number's cell (a dash when not known; hot ones in the warning color).
fn cell(
    pen: &mut LayerPainter<'_>,
    value: Option<&str>,
    font: &Font,
    right: f32,
    base: f32,
    p: &ui_components::Palette,
    hot: bool,
) {
    let color = match value {
        None => p.text_muted.with_alpha(0.6),
        Some(_) if hot => p.warning,
        Some(_) => p.text,
    };
    right_text(pen.layer, pen.text, value.unwrap_or("—"), font, right, base, color);
}

/// A small square button with an icon: lit under the mouse, tinted while `on`. Returns
/// the button as drawn (its focus ring goes around it).
pub(super) fn icon_button(
    layer: &mut Layer,
    icons: &icons::Icons,
    ds: &Ctx<'_>,
    rect: Rect,
    icon: &str,
    hovered: bool,
    on: bool,
) -> Rect {
    let p = ds.palette;
    let fill = if on {
        p.accent.with_alpha(0.14)
    } else if hovered {
        p.text.with_alpha(0.07)
    } else {
        Rgba::TRANSPARENT
    };
    let r = Rect::new(
        rect.min_x(),
        (rect.min_y() + (rect.size.height - ds.px(28.0)) / 2.0).round(),
        rect.size.width,
        ds.px(28.0).min(rect.size.height),
    );
    layer.quad(Quad::filled(r, fill).rounded(ds.px(radius::SM)));
    if let Some(id) = icons.id(icon) {
        let s = ds.px(16.0);
        let at = Rect::new(
            (r.min_x() + (r.size.width - s) / 2.0).round(),
            (r.min_y() + (r.size.height - s) / 2.0).round(),
            s,
            s,
        );
        let color = if on {
            p.accent
        } else if hovered {
            p.text
        } else {
            p.text_muted
        };
        ui_kit::draw_icon(layer, icons, id, at, color);
    }
    r
}
