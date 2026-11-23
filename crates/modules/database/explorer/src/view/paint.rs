//! Drawing the data view: the tables' list (a SQLite file), the shown table's header (its
//! name, what it is, Data / Structure), the unsaved text's notice (a CSV), then the grid or
//! the structure.

use database::TableKind;
use kernel::Rect;
use render_api::Quad;
use text_layout::{Font, TextSystem};
use ui_components::data_grid::{GridPaint, grouped};
use ui_components::ds::{self, Ctx, Tone, radius};
use ui_components::{SplitterStyle, ViewerPaint};
use ui_kit::draw_text;

use super::{DataView, Hit, Tab};

/// The header's height over the grid (logical pixels).
const HEADER_H: f32 = 52.0;
/// A list row's height.
const ROW_H: f32 = 32.0;
/// The space over the list's first row: centered in the grid toolbar's band beside it.
const LIST_TOP: f32 = (ui_components::data_grid::TOOLBAR_HEIGHT - ROW_H) / 2.0;

/// `n` rows as the list shows them (`1.2k`, `3.4M`).
pub fn short_count(n: u64) -> String {
    match n {
        n if n < 10_000 => n.to_string(),
        n if n < 1_000_000 => format!("{:.1}k", n as f64 / 1000.0).replace(".0k", "k"),
        n => format!("{:.1}M", n as f64 / 1_000_000.0).replace(".0M", "M"),
    }
}

impl DataView {
    pub(super) fn paint_view(&mut self, ctx: ViewerPaint<'_>) {
        let ViewerPaint { bounds: b, scale, text, scene, palette: p, icons, .. } = ctx;
        let ds = Ctx::new(p, scale);
        let px = |v: f32| ds.px(v);
        self.last_scale = scale;
        let layer = scene.push_layer(Some(b));
        layer.quad(Quad::filled(b, p.editor_background));

        if let Some(error) = &self.error {
            ds::empty_state(layer, text, &ds, b, &i18n::t("database.open_failed"), Some(error));
            return;
        }

        // The tables' list (a database of several; a CSV is one table).
        let mut right = b;
        if !self.is_text() {
            let width = px(self.list_width).min(b.size.width * 0.5);
            let list = Rect::new(b.min_x(), b.min_y(), width, b.size.height);
            self.list_area = list;
            self.paint_list(scene, text, icons, &ds, list);
            let strip = px(8.0);
            self.splitter.set_style(SplitterStyle::from_palette(p));
            self.splitter.set_bounds(
                Rect::new((list.max_x() - strip / 2.0).round(), b.min_y(), strip, b.size.height),
                scale,
            );
            right = Rect::new(list.max_x(), b.min_y(), b.max_x() - list.max_x(), b.size.height);
        } else {
            self.list_area = Rect::default();
            self.splitter.set_bounds(Rect::default(), scale);
        }

        // The header: the table, what it is, Data / Structure.
        let header = Rect::new(right.min_x(), right.min_y(), right.size.width, px(HEADER_H));
        let layer = scene.push_layer(Some(header));
        self.paint_header(layer, text, icons, &ds, header);
        let mut top = header.max_y();
        if self.is_text() && self.unsaved {
            let banner = Rect::new(right.min_x(), top, right.size.width, px(34.0));
            let layer = scene.push_layer(Some(banner));
            layer.quad(Quad::filled(banner, p.warning.with_alpha(0.10)));
            ds::divider(layer, &ds, banner.min_x(), banner.max_x(), banner.max_y() - ds.line());
            let s = px(14.0);
            if let Some(id) = icons.id("alert") {
                let ir = Rect::new(
                    banner.min_x() + px(14.0),
                    (banner.min_y() + (banner.size.height - s) / 2.0).round(),
                    s,
                    s,
                );
                ui_kit::draw_icon(layer, icons, id, ir, p.warning);
            }
            let font = ds.ui_font(-1.0);
            let line = ds::ellipsize(
                text,
                &i18n::t("database.save_first"),
                &font,
                banner.size.width - px(60.0),
            );
            let shaped = text.shape_line(&line, &font);
            draw_text(
                layer,
                &shaped,
                banner.min_x() + px(38.0),
                ds::optical_baseline(banner.min_y(), banner.size.height, &font),
                p.text,
            );
            top = banner.max_y();
        }
        // An export running: how far, and its Cancel.
        if self.export.is_some() {
            let banner = Rect::new(right.min_x(), top, right.size.width, px(40.0));
            let layer = scene.push_layer(Some(banner));
            self.paint_export(layer, text, icons, &ds, banner);
            top = banner.max_y();
        }
        let area = Rect::new(right.min_x(), top, right.size.width, (right.max_y() - top).max(0.0));
        self.structure_area = area;
        match self.tab {
            Tab::Data => match &mut self.grid {
                Some(grid) => {
                    let (grid_area, pane_area) = match &mut self.panes {
                        Some(panes) => {
                            panes.whole = area;
                            panes.host.split(area, scale)
                        }
                        None => (area, None),
                    };
                    grid.paint(GridPaint {
                        bounds: grid_area,
                        scale,
                        text,
                        scene,
                        palette: p,
                        icons,
                    });
                    if let (Some(rect), Some(pane)) =
                        (pane_area, self.panes.as_mut().and_then(|panes| panes.host.pane()))
                    {
                        let layer = scene.push_layer(Some(rect));
                        layer.quad(Quad::filled(rect, p.editor_background));
                        ds::divider(layer, &ds, rect.min_x(), rect.max_x(), rect.min_y());
                        let waker = self
                            .waker
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .clone();
                        pane.paint(ViewerPaint {
                            bounds: rect,
                            scale,
                            text,
                            scene,
                            palette: p,
                            icons,
                            focused: false,
                            waker,
                        });
                    }
                }
                None => {
                    let layer = scene.push_layer(Some(area));
                    if self.tables_loading {
                        let label = i18n::t("grid.loading");
                        let loader = ds::Loader::with_text(&label);
                        let w = loader.width(text, &ds);
                        ds::loader(
                            layer,
                            text,
                            &ds,
                            area.min_x() + (area.size.width - w) / 2.0,
                            area.min_y() + area.size.height / 2.0,
                            &loader,
                        );
                    } else {
                        ds::empty_state(
                            layer,
                            text,
                            &ds,
                            area,
                            &i18n::t("database.no_tables"),
                            None,
                        );
                    }
                }
            },
            Tab::Structure => self.paint_structure(scene, text, icons, &ds, area),
        }
        if !self.is_text() {
            self.splitter.paint(scene);
        }
    }

    /// The export's strip: its icon, what's written where, the rows so far (of how many),
    /// a progress bar along its bottom, and Cancel.
    fn paint_export(
        &mut self,
        layer: &mut render_api::Layer,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        banner: Rect,
    ) {
        let Some(export) = &self.export else { return };
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        layer.quad(Quad::filled(banner, p.accent.with_alpha(0.07)));
        ds::divider(layer, ds, banner.min_x(), banner.max_x(), banner.max_y() - ds.line());
        let mid = banner.min_y() + banner.size.height / 2.0 - px(1.0);
        let s = px(14.0);
        if let Some(id) = icons.id("download") {
            let ir = Rect::new(banner.min_x() + px(14.0), (mid - s / 2.0).round(), s, s);
            ui_kit::draw_icon(layer, icons, id, ir, p.accent);
        }
        let request = export.request();
        let rows = export.rows();
        let target = match &request.target {
            crate::export::Target::File(path) => kernel::paths::file_name(path),
            crate::export::Target::Clipboard => i18n::t("database.export_clipboard"),
        };
        let line = if export.cancelling() {
            i18n::t("database.export_cancelling")
        } else {
            i18n::tr!(
                "database.exporting",
                table = request.table.name.as_str(),
                file = target.as_str()
            )
        };
        let count = match request.total {
            Some(total) => i18n::tr!(
                "database.export_rows_of",
                done = grouped(rows),
                count = total,
                total = grouped(total)
            ),
            None => i18n::tr!("database.export_rows", count = rows, rows = grouped(rows)),
        };
        // Cancel on the right.
        let label = i18n::t("database.export_cancel");
        let bw = (text.shape_line(&label, &ds::button_font(ds)).width + px(24.0)).round();
        let bh = px(26.0);
        let button = Rect::new(banner.max_x() - bw - px(12.0), (mid - bh / 2.0).round(), bw, bh);
        let hovered = self.hover == Some(Hit::ExportCancel);
        ds::secondary_button(layer, text, ds, button, &label, true, hovered);
        if !export.cancelling() {
            self.hits.push((button, Hit::ExportCancel));
        }
        let font = ds.ui_font(-1.0);
        let muted = ds.ui_font(-1.5);
        let count_shaped = text.shape_line(&count, &muted);
        let room = button.min_x() - px(16.0) - (banner.min_x() + px(38.0)) - count_shaped.width;
        let line = ds::ellipsize(text, &line, &font, room.max(px(40.0)));
        let shaped = text.shape_line(&line, &font);
        let x = banner.min_x() + px(38.0);
        draw_text(layer, &shaped, x, ds::optical_baseline(mid - px(10.0), px(20.0), &font), p.text);
        draw_text(
            layer,
            &count_shaped,
            x + shaped.width + px(10.0),
            ds::optical_baseline(mid - px(10.0), px(20.0), &muted),
            p.text_muted,
        );
        // The bar along the bottom (a sweep while the total isn't known).
        let fraction = request.total.filter(|t| *t > 0).map(|t| rows as f32 / t as f32);
        let bar = Rect::new(banner.min_x(), banner.max_y() - px(3.0), banner.size.width, px(3.0));
        ds::ProgressBar::from(fraction).paint_bar(layer, ds, bar);
    }

    fn paint_list(
        &mut self,
        scene: &mut render_api::Scene,
        text: &mut text_layout::TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        list: Rect,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let layer = scene.push_layer(Some(list));
        layer.quad(Quad::filled(list, p.text.with_alpha(0.015)));
        // Its title: the file, and how many tables.
        let head = Rect::new(list.min_x(), list.min_y(), list.size.width, px(HEADER_H));
        let title_font = ds.ui_font(-0.5).with_weight(Font::SEMIBOLD);
        let s = px(15.0);
        if let Some(id) = icons.id("table") {
            ui_kit::draw_icon(
                layer,
                icons,
                id,
                Rect::new(
                    head.min_x() + px(16.0),
                    (head.min_y() + (head.size.height - s) / 2.0).round(),
                    s,
                    s,
                ),
                p.accent,
            );
        }
        let name = self.title();
        let count = i18n::tr!("database.tables_count", count = self.tables.len());
        let small = ds.ui_font(-1.5);
        let count_shaped = text.shape_line(&count, &small);
        let room = head.size.width - px(46.0) - count_shaped.width - px(14.0);
        let name = ds::ellipsize_middle(text, &name, &title_font, room.max(0.0));
        let shaped = text.shape_line(&name, &title_font);
        let base = ds::optical_baseline(head.min_y(), head.size.height, &title_font);
        draw_text(layer, &shaped, head.min_x() + px(40.0), base, p.text);
        if !self.tables_loading {
            draw_text(
                layer,
                &count_shaped,
                (head.max_x() - px(14.0) - count_shaped.width).round(),
                base,
                p.text_muted,
            );
        }
        // Its line where the data header draws its own (just above its bottom), so the two
        // read as one across the splitter.
        ds::divider(layer, ds, list.min_x(), list.max_x(), head.max_y() - ds.line());

        let rows = Rect::new(
            list.min_x(),
            head.max_y(),
            list.size.width,
            (list.max_y() - head.max_y()).max(0.0),
        );
        let row_h = px(ROW_H);
        let content = self.tables.len() as f32 * row_h + px(2.0 * LIST_TOP);
        self.list_max = (content - rows.size.height).max(0.0);
        self.list_scroll = self.list_scroll.clamp(0.0, self.list_max);
        let layer = scene.push_layer(Some(rows));
        if self.tables_loading && self.tables.is_empty() {
            ds::loader(
                layer,
                text,
                ds,
                rows.min_x() + px(16.0),
                rows.min_y() + px(22.0),
                &ds::Loader::default(),
            );
        }
        let font = ds.ui_font(-0.5);
        // The first row centered in the band of the grid's toolbar beside it.
        let mut y = rows.min_y() + px(LIST_TOP) - self.list_scroll;
        for (i, info) in self.tables.iter().enumerate() {
            let r = Rect::new(rows.min_x() + px(6.0), y, rows.size.width - px(12.0), row_h);
            y += row_h;
            if r.max_y() < rows.min_y() || r.min_y() > rows.max_y() {
                continue;
            }
            let selected = self.selected == Some(i);
            ds::RowHighlight::new(selected, self.hover == Some(Hit::Table(i)))
                .selected_fill(p.tree_selection)
                .hover(ds::HoverFill::Tree)
                .radius(radius::SM)
                .paint(layer, ds, r);
            let icon = if info.kind == TableKind::View { "view" } else { "table" };
            let s = px(14.0);
            if let Some(id) = icons.id(icon) {
                let color = if selected { p.accent } else { p.text_muted };
                ui_kit::draw_icon(
                    layer,
                    icons,
                    id,
                    Rect::new(r.min_x() + px(10.0), (r.min_y() + (row_h - s) / 2.0).round(), s, s),
                    color,
                );
            }
            let count = self.counts.get(i).map(|c| c.label.as_str());
            let count_w = count.map_or(0.0, |c| text.shape_line(c, &small).width + px(10.0));
            let room = r.size.width - px(40.0) - count_w;
            let name = info.table.to_string();
            let name = ds::ellipsize(text, &name, &font, room.max(0.0));
            let shaped = text.shape_line(&name, &font);
            let base = ds::optical_baseline(r.min_y(), row_h, &font);
            draw_text(layer, &shaped, r.min_x() + px(32.0), base, p.text);
            if let Some(count) = count {
                let shaped = text.shape_line(count, &small);
                draw_text(
                    layer,
                    &shaped,
                    (r.max_x() - px(10.0) - shaped.width).round(),
                    base,
                    p.text_muted,
                );
            }
            if let Some(clip) = r.intersect(&rows) {
                self.hits.push((clip, Hit::Table(i)));
            }
        }
        self.list_bar.paint(scene, p, ds.scale, rows, content, self.list_scroll);
        self.list_area = list;
    }

    fn paint_header(
        &mut self,
        layer: &mut render_api::Layer,
        text: &mut text_layout::TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        header: Rect,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let mid = header.min_y() + header.size.height / 2.0;
        ds::divider(layer, ds, header.min_x(), header.max_x(), header.max_y() - ds.line());
        // Data / Structure, on the right.
        let labels = [i18n::t("database.tab_data"), i18n::t("database.tab_structure")];
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        let hovered = match self.hover {
            Some(Hit::Tab(Tab::Data)) => Some(0),
            Some(Hit::Tab(Tab::Structure)) => Some(1),
            _ => None,
        };
        let selected = usize::from(self.tab == Tab::Structure);
        let segments = ds::segmented(
            layer,
            text,
            ds,
            // A text file's view has the app's Editor / Split / Preview switch in its corner.
            ds::SegmentedSpec {
                right: header.max_x() - px(if self.is_text() { 132.0 } else { 12.0 }),
                mid,
                labels: &labels,
                selected,
                hovered,
            },
        );
        let mut left_of_tabs = segments.first().map_or(header.max_x(), |r| r.min_x());
        for (i, r) in segments.into_iter().enumerate() {
            self.hits.push((r, Hit::Tab(if i == 0 { Tab::Data } else { Tab::Structure })));
        }
        // The panes offered over its rows, before Data / Structure (its data shown).
        if self.tab == Tab::Data
            && let Some(panes) = &self.panes
        {
            let hovered = match self.hover {
                Some(Hit::Pane(i)) => Some(i),
                _ => None,
            };
            let mut hits = Vec::new();
            let side = px(ds::button_size::LARGE);
            left_of_tabs = panes.host.paint_buttons(
                (layer, text, icons, ds),
                (left_of_tabs - px(8.0), mid, side),
                hovered,
                &mut hits,
            ) + px(2.0);
            self.hits.extend(hits.into_iter().map(|(r, i)| (r, Hit::Pane(i))));
        }
        // "Jump to Query Console…", before Data / Structure (a source writing SQL).
        if self.has_console() {
            let size = px(ds::button_size::LARGE);
            let r =
                Rect::new(left_of_tabs - px(8.0) - size, (mid - size / 2.0).round(), size, size);
            let spec = ds::ButtonSpec::new(ds::ButtonKind::Ghost)
                .icon("console")
                .hovered(self.hover == Some(Hit::Console));
            ds::button(layer, text, icons, ds, r, spec);
            self.hits.push((r, Hit::Console));
            left_of_tabs = r.min_x();
        }
        // The table's name and what it is.
        let Some(info) = self.table().cloned() else { return };
        let mut x = header.min_x() + px(16.0);
        let s = px(16.0);
        let icon = if info.kind == TableKind::View { "view" } else { "table" };
        if let Some(id) = icons.id(icon) {
            ui_kit::draw_icon(
                layer,
                icons,
                id,
                Rect::new(x, (mid - s / 2.0).round(), s, s),
                p.text_muted,
            );
            x += s + px(10.0);
        }
        let font = ds.ui_font(1.0).with_weight(Font::SEMIBOLD);
        // The name cut before it reaches Data / Structure (the badges only where they fit
        // after it).
        let room = left_of_tabs - x - px(12.0);
        if room < px(24.0) {
            return;
        }
        let full = info.table.to_string();
        let name = ds::ellipsize(text, &full, &font, room);
        let shaped = text.shape_line(&name, &font);
        if name != full {
            self.tips.push((
                Rect::new(x, header.min_y(), shaped.width, header.size.height),
                full.clone(),
            ));
        }
        draw_text(
            layer,
            &shaped,
            x,
            ds::optical_baseline(header.min_y(), header.size.height, &font),
            p.text,
        );
        x += shaped.width + px(10.0);
        let kind = if self.is_text() {
            i18n::t("database.kind_file")
        } else if info.kind == TableKind::View {
            i18n::t("database.kind_view")
        } else {
            i18n::t("database.kind_table")
        };
        if x + px(80.0) < left_of_tabs {
            let r = ds::badge(
                layer,
                text,
                ds,
                ds::BadgeSpec { label: &kind, x, mid, align_right: false, tone: Tone::Neutral },
            );
            x = r.max_x() + px(8.0);
        }
        let read_only = self.open.as_ref().is_some_and(|o| o.worker.source().read_only);
        if read_only && x + px(80.0) < left_of_tabs {
            ds::badge(
                layer,
                text,
                ds,
                ds::BadgeSpec {
                    label: &i18n::t("database.read_only"),
                    x,
                    mid,
                    align_right: false,
                    tone: Tone::Accent,
                },
            );
        }
    }
}
