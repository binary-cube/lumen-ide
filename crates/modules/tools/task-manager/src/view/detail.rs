//! A part's detail: its header (its group's icon, its name, whose it is and for which
//! project, its group and its state; × to close — or, as a page, "‹ Back"), a note when the
//! part is gone (its last values stay), its figures now / on average / at most, its charts
//! over the history (two by two on a wide panel), its threads with their CPU, the programs
//! it started and its operations. Scrolled when taller than its room.

use std::sync::{Arc, PoisonError};

use kernel::Rect;
use render_api::{Quad, Scene};
use text_layout::{Font, TextSystem};
use ui_components::LayerPainter;
use ui_components::chart::{ChartSeries, Samples, TimeChart, time_chart};
use ui_components::ds::{self, BadgeSpec, ButtonKind, ButtonSpec, Ctx, Tone, radius};
use ui_kit::draw_text;

use super::actions::{kind_icon, kind_index};
use super::table::{group_icon, quiet_icon, right_text};
use super::{Focus, Hit, TaskManager, summary};
use crate::collector::Line;
use crate::collector::Metric;
use crate::layout::{DetailLayout, DetailPlace, PageLayout};
use workbench_model::ActionTarget;

/// A stat's dot in its metric's color (logical pixels).
const STAT_DOT: f32 = 6.0;

impl TaskManager {
    pub(super) fn paint_detail(
        &mut self,
        scene: &mut Scene,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        page: &PageLayout,
        rect: Rect,
    ) {
        let Some(key) = self.selected.as_deref() else { return };
        // The part's row now, or its last one (gone): borrowed from shared copies, nothing
        // cloned.
        let frame = Arc::clone(&self.frame);
        let last = self.last_selected.clone();
        let (line, gone) = match frame.line(key) {
            Some(line) => (line, false),
            None => match last.as_deref().filter(|l| l.key == key) {
                Some(line) => (line, true),
                None => return,
            },
        };
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let as_page = page.place == DetailPlace::Page;
        let layout = page.detail_layout.unwrap_or(DetailLayout {
            chart_columns: 1,
            stat_columns: 1,
            chart_height: px(96.0),
        });
        self.stops.push((rect, Focus::Detail));
        let layer = scene.push_layer(Some(rect));
        layer.quad(
            Quad::filled(rect, p.text.with_alpha(0.025))
                .rounded(px(radius::MD))
                .bordered(ds.line(), p.card_border),
        );
        let pad = px(14.0);
        let x0 = rect.min_x() + pad;
        let inner = (rect.size.width - 2.0 * pad).max(0.0);
        let mut y = rect.min_y() + px(10.0);
        // "‹ Back" on a page.
        if as_page {
            let font = ds.ui_font(-0.5);
            let w = text.shape_line(&self.texts.labels.back, &font).width + px(30.0);
            let back = Rect::new(x0 - px(6.0), y, w, px(28.0));
            let spec = ButtonSpec::new(ButtonKind::Ghost)
                .icon("chevron-left")
                .label(&self.texts.labels.back)
                .tone(Tone::Accent)
                .hovered(self.hover == Some(Hit::Back));
            ds::button(layer, text, icons, ds, back, spec);
            self.hits.push((back, Hit::Back));
            y = back.max_y() + px(8.0);
        }
        // The header: the icon, the name, ×.
        let icon = px(18.0);
        if let Some(id) = icons.id(group_icon(line.group)) {
            ui_kit::draw_icon(
                layer,
                icons,
                id,
                Rect::new(x0, y + px(3.0), icon, icon),
                p.text_muted,
            );
        }
        let size = px(ds::button_size::LARGE);
        let close = Rect::new(rect.max_x() - pad - size + px(6.0), y - px(2.0), size, size);
        let title_font = ds.ui_font(1.5).with_weight(Font::SEMIBOLD);
        let name_x = x0 + icon + px(10.0);
        let name_room =
            if as_page { x0 + inner - name_x } else { close.min_x() - px(6.0) - name_x };
        let base = ds::optical_baseline(y, px(24.0), &title_font);
        ds::draw_ellipsized(
            layer,
            text,
            &line.name,
            &title_font,
            (name_x, base),
            name_room.max(0.0),
            p.text,
        );
        if !as_page {
            let spec = quiet_icon("x").hovered(self.hover == Some(Hit::CloseDetail));
            ds::button(layer, text, icons, ds, close, spec);
            self.hits.push((close, Hit::CloseDetail));
        }
        y += px(28.0);
        // Whose, for which project; its group and its state.
        let small = ds.ui_font(-1.0);
        let owner_base = ds::optical_baseline(y, px(20.0), &small);
        let (owner_w, _) = ds::draw_ellipsized(
            layer,
            text,
            &self.texts.detail.owner,
            &small,
            (name_x, owner_base),
            (x0 + inner - name_x - px(150.0)).max(px(60.0)),
            p.text_muted,
        );
        let mid = y + px(10.0);
        let state = if gone {
            (&self.texts.labels.gone_state, Tone::Danger)
        } else if self.paused {
            (&self.texts.labels.paused_state, Tone::Warning)
        } else {
            (&self.texts.labels.running, Tone::Accent)
        };
        let mut bx = name_x + owner_w + px(10.0);
        for (label, tone) in [(&self.texts.detail.group, Tone::Neutral), (state.0, state.1)] {
            let r = ds::badge(
                layer,
                text,
                ds,
                BadgeSpec { label, x: bx, mid, align_right: false, tone },
            );
            bx = r.max_x() + px(6.0);
        }
        y += px(30.0);
        // What can be done to the part itself (stopping, closing, releasing): its buttons,
        // the one losing work in the danger color.
        if line.actions_on(ActionTarget::Part).next().is_some() {
            let mut bx = name_x;
            for action in line.actions_on(ActionTarget::Part) {
                let label = &self.texts.labels.actions[kind_index(action.kind)];
                let w = ds::primary_width(text, ds, &[label]);
                // What doesn't fit goes on the next row.
                if bx + w > x0 + inner && bx > name_x {
                    bx = name_x;
                    y += px(40.0);
                }
                let r = Rect::new(bx, y, w.min(x0 + inner - bx), px(30.0));
                let hovered = self.hover == Some(Hit::Action(*action));
                if action.confirm {
                    ds::danger_button(layer, text, ds, r, label, true, hovered);
                } else {
                    ds::secondary_button(layer, text, ds, r, label, true, hovered);
                }
                self.hits.push((r, Hit::Action(*action)));
                bx = r.max_x() + px(8.0);
            }
            y += px(40.0);
        }
        if gone {
            let banner = Rect::new(x0, y, inner, px(30.0));
            layer.quad(Quad::filled(banner, p.danger.with_alpha(0.10)).rounded(px(radius::SM)));
            if let Some(id) = icons.id("alert") {
                let s = px(14.0);
                ui_kit::draw_icon(
                    layer,
                    icons,
                    id,
                    Rect::new(
                        banner.min_x() + px(8.0),
                        banner.min_y() + (banner.size.height - s) / 2.0,
                        s,
                        s,
                    ),
                    p.danger,
                );
            }
            let b = ds::optical_baseline(banner.min_y(), banner.size.height, &small);
            ds::draw_ellipsized(
                layer,
                text,
                &self.texts.labels.gone,
                &small,
                (banner.min_x() + px(28.0), b),
                inner - px(36.0),
                p.text,
            );
            y = banner.max_y() + px(10.0);
        }
        ds::divider(layer, ds, x0, x0 + inner, y);
        y += px(8.0);

        // The rest scrolls.
        let body =
            Rect::new(rect.min_x(), y, rect.size.width, (rect.max_y() - y - px(6.0)).max(0.0));
        let content_top = body.min_y() - self.detail_scroll.round();
        let pen = &mut LayerPainter::new(scene.push_layer(Some(body)), text, icons, *ds);
        let mut y = content_top + px(4.0);
        y = self.paint_facts(pen, (x0, inner, y), line);
        y = self.paint_stats(pen, x0, inner, y, layout.stat_columns);
        y = self.paint_charts(pen, (x0, inner, y), &layout, body);
        y = self.paint_threads(pen, x0, inner, y);
        y = self.paint_programs(pen, (x0, inner, y), line, body);
        y = self.paint_part_operations(pen, (x0, inner, y), line, body);
        let content = y - content_top + px(8.0);
        self.detail_max = (content - body.size.height).max(0.0);
        self.detail_scroll = self.detail_scroll.clamp(0.0, self.detail_max);
        self.detail_bar.paint(scene, p, ds.scale, body, content, self.detail_scroll.round());
    }

    /// The figures: each metric's now, on average, at most.
    fn paint_stats(
        &self,
        pen: &mut LayerPainter<'_>,
        x0: f32,
        inner: f32,
        y: f32,
        columns: usize,
    ) -> f32 {
        let ds = pen.ds;
        let ds = &ds;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let stats = &self.texts.detail.stats;
        if stats.is_empty() {
            return y;
        }
        let gap = px(10.0);
        let w = ((inner - gap * (columns - 1) as f32) / columns as f32).floor();
        let h = px(58.0);
        let label_font = ds.ui_font(-1.5).with_weight(Font::SEMIBOLD);
        let value_font = ds.ui_font(1.0).with_weight(Font::SEMIBOLD);
        let small = ds.ui_font(-1.5);
        let pieces_width = |text: &mut TextSystem, stat: &crate::view::texts::Stat| -> f32 {
            [(&self.texts.labels.avg, &stat.avg), (&self.texts.labels.max, &stat.max)]
                .iter()
                .map(|(l, v)| {
                    text.shape_line(l, &small).width + text.shape_line(v, &small).width + px(14.0)
                })
                .sum()
        };
        // "avg · max" beside every value, or under every one: the same in each figure.
        let inline = stats.iter().all(|stat| {
            let value = pen.text.shape_line(&stat.now, &value_font).width;
            px(10.0) + value + px(10.0) + pieces_width(pen.text, stat) <= w - px(10.0)
        });
        for (i, stat) in stats.iter().enumerate() {
            let (row, col) = (i / columns, i % columns);
            let r = Rect::new(x0 + col as f32 * (w + gap), y + row as f32 * (h + gap), w, h);
            pen.layer.quad(
                Quad::filled(r, p.text.with_alpha(0.03))
                    .rounded(px(radius::SM))
                    .bordered(ds.line(), p.card_border),
            );
            let ix = r.min_x() + px(10.0);
            let room = r.size.width - px(20.0);
            if let Some(metric) = stat.metric {
                let half = px(STAT_DOT) / 2.0;
                let center = kernel::Point::new(ix + half, r.min_y() + px(12.0) + half);
                ds::dot(pen.layer, ds, center, STAT_DOT, summary::color(p, metric));
            }
            ds::draw_ellipsized(
                pen.layer,
                pen.text,
                &stat.label,
                &label_font,
                (ix + px(12.0), ds::optical_baseline(r.min_y() + px(6.0), px(16.0), &label_font)),
                room - px(12.0),
                p.text_muted,
            );
            let vb = ds::optical_baseline(r.min_y() + px(22.0), px(22.0), &value_font);
            let (vw, _) = ds::draw_ellipsized(
                pen.layer,
                pen.text,
                &stat.now,
                &value_font,
                (ix, vb),
                room,
                p.text,
            );
            // "avg 1.2% · max 4.0%" beside the value when there's room, else under it.
            let pieces = [(&self.texts.labels.avg, &stat.avg), (&self.texts.labels.max, &stat.max)];
            let (sx, sb) = if inline {
                (ix + vw + px(10.0), vb)
            } else {
                (ix, ds::optical_baseline(r.min_y() + px(42.0), px(14.0), &small))
            };
            let mut sx = sx;
            for (label, value) in pieces {
                let lw = pen.text.shape_line(label, &small).width;
                draw_text(pen.layer, &pen.text.shape_line(label, &small), sx, sb, p.text_muted);
                let vw = pen.text.shape_line(value, &small).width;
                draw_text(
                    pen.layer,
                    &pen.text.shape_line(value, &small),
                    sx + lw + px(4.0),
                    sb,
                    p.text,
                );
                sx += lw + vw + px(14.0);
            }
        }
        let rows = stats.len().div_ceil(columns);
        y + rows as f32 * (h + gap) + px(6.0)
    }

    /// The charts over the history: CPU, memory, GPU and I/O (network and disk), the ones
    /// the part reports.
    fn paint_charts(
        &mut self,
        pen: &mut LayerPainter<'_>,
        (x0, inner, y): (f32, f32, f32),
        layout: &DetailLayout,
        clip: Rect,
    ) -> f32 {
        let ds = pen.ds;
        let ds = &ds;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let reported = |m: Metric| self.texts.detail.stats.iter().any(|s| s.metric == Some(m));
        let mut charts: [Option<(Metric, Option<Metric>)>; 4] = [None; 4];
        let mut count = 0;
        for chart in [
            (Metric::Cpu, None),
            (Metric::Memory, None),
            (Metric::Gpu, None),
            (Metric::Net, Some(Metric::Disk)),
        ] {
            if reported(chart.0) || chart.1.is_some_and(reported) {
                charts[count] = Some(chart);
                count += 1;
            }
        }
        if count == 0 {
            return y;
        }
        let columns = layout.chart_columns.min(count);
        let gap = px(14.0);
        let w = ((inner - gap * (columns - 1) as f32) / columns as f32).floor();
        let title_h = px(20.0);
        let h = layout.chart_height + title_h;
        self.update_detail_plots(w.max(1.0) as usize);
        let title_font = ds.ui_font(-1.5).with_weight(Font::SEMIBOLD);
        let hover = match (self.hover, self.mouse) {
            (Some(Hit::DetailChart(m)), Some(mouse)) => Some((m, mouse.x)),
            _ => None,
        };
        for (i, chart) in charts.iter().flatten().enumerate() {
            let (row, col) = (i / columns, i % columns);
            let r = Rect::new(x0 + col as f32 * (w + gap), y + row as f32 * (h + gap), w, h);
            let (first, second) = *chart;
            let label = match first {
                Metric::Net => &self.texts.labels.cards[3],
                m => &self.texts.labels.cards[m as usize],
            };
            let base = ds::optical_baseline(r.min_y(), title_h - px(4.0), &title_font);
            draw_text(
                pen.layer,
                &pen.text.shape_line(label, &title_font),
                r.min_x(),
                base,
                p.text_muted,
            );
            let now = self
                .texts
                .detail
                .stats
                .iter()
                .find(|s| s.metric == Some(first))
                .map_or("", |s| s.now.as_str());
            right_text(pen.layer, pen.text, now, &ds.ui_font(-1.5), r.max_x(), base, p.text);
            let plot_a = self.plots.detail[first as usize].plot();
            let series_a = ChartSeries { plot: plot_a, color: summary::color(p, first) };
            let series_b = second.map(|m| ChartSeries {
                plot: self.plots.detail[m as usize].plot(),
                color: summary::color(p, m),
            });
            let both = [series_a, series_b.unwrap_or(series_a)];
            let series = if series_b.is_some() { &both[..] } else { &both[..1] };
            let scale = |m: Metric| self.texts.detail.scales[m as usize];
            let (max, max_label) = match second {
                Some(m) if scale(m) > scale(first) => {
                    (scale(m), &self.texts.detail.scale_labels[m as usize])
                }
                _ => (scale(first), &self.texts.detail.scale_labels[first as usize]),
            };
            let area = Rect::new(r.min_x(), r.min_y() + title_h, r.size.width, layout.chart_height);
            let chart_hover = hover.filter(|(m, _)| *m == first).map(|(_, x)| x);
            let plotted = time_chart(
                pen.layer,
                pen.text,
                ds,
                area,
                &TimeChart { series, max, max_label, hover: chart_hover },
            );
            // Its reading only where it's in view (the rest is scrolled out).
            if let Some(visible) = plotted.intersect(&clip) {
                self.hits.push((visible, Hit::DetailChart(first)));
            }
        }
        let rows = count.div_ceil(columns);
        y + rows as f32 * (h + gap) + px(4.0)
    }

    /// The detail's plots (the selected part's), aggregated again only for new values,
    /// another part or another width.
    fn update_detail_plots(&mut self, columns: usize) {
        let Some(history) = self.history.clone() else { return };
        let Some(key) = self.selected.as_deref() else { return };
        let history = history.lock().unwrap_or_else(PoisonError::into_inner);
        let generation = history.generation().wrapping_mul(1_000_003) ^ self.selection_epoch;
        let slots = history.capacity();
        let stale = Metric::ALL.iter().any(|m| {
            self.plots.detail[*m as usize].is_stale(generation, slots, columns, summary::reduce(*m))
        });
        if !stale {
            return;
        }
        let trace = history.part(key);
        for m in Metric::ALL {
            let samples =
                trace.map_or_else(Samples::default, |t| Samples::from_slices(t.get(m).as_slices()));
            self.plots.detail[m as usize].get(
                generation,
                samples,
                slots,
                columns,
                summary::reduce(m),
            );
        }
    }

    /// What only the part's reporter knows (a language server's state, its latency, its
    /// last error): label on the left, value on the right, one per row.
    fn paint_facts(
        &self,
        pen: &mut LayerPainter<'_>,
        (x0, inner, y): (f32, f32, f32),
        line: &Line,
    ) -> f32 {
        let ds = pen.ds;
        let ds = &ds;
        if line.facts.is_empty() {
            return y;
        }
        let p = ds.palette;
        let row = ds.px(22.0);
        let font = ds.ui_font(-1.0);
        let mut y = y;
        for (label, value) in &line.facts {
            let base = ds::optical_baseline(y, row, &font);
            let label_w = pen.text.shape_line(label, &font).width;
            draw_text(pen.layer, &pen.text.shape_line(label, &font), x0, base, p.text_muted);
            let room = (inner - label_w - ds.px(16.0)).max(0.0);
            let shown = ds::ellipsize(pen.text, value, &font, room);
            right_text(pen.layer, pen.text, &shown, &font, x0 + inner, base, p.text);
            y += row;
        }
        ds::divider(pen.layer, ds, x0, x0 + inner, y + ds.px(4.0));
        y + ds.px(12.0)
    }

    /// A section's title; returns the y under it.
    fn section(&self, pen: &mut LayerPainter<'_>, x0: f32, y: f32, title: &str) -> f32 {
        let ds = pen.ds;
        let ds = &ds;
        let font = ds.ui_font(-1.0).with_weight(Font::SEMIBOLD);
        let base = ds::optical_baseline(y, ds.px(22.0), &font);
        draw_text(pen.layer, &pen.text.shape_line(title, &font), x0, base, ds.palette.text);
        y + ds.px(24.0)
    }

    /// Its threads, with their CPU.
    fn paint_threads(&self, pen: &mut LayerPainter<'_>, x0: f32, inner: f32, y: f32) -> f32 {
        let ds = pen.ds;
        let ds = &ds;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let mut y = self.section(pen, x0, y, &self.texts.labels.threads);
        let mono = ds.code_font(-1.5);
        let row = px(22.0);
        if self.texts.detail.threads.is_empty() {
            let small = ds.ui_font(-1.5);
            draw_text(
                pen.layer,
                &pen.text.shape_line(&self.texts.labels.no_threads, &small),
                x0,
                ds::optical_baseline(y, row, &small),
                p.text_muted,
            );
            return y + row + px(8.0);
        }
        for (name, cpu) in &self.texts.detail.threads {
            let base = ds::optical_baseline(y, row, &mono);
            let cw = pen.text.shape_line(cpu, &mono).width;
            ds::draw_ellipsized(
                pen.layer,
                pen.text,
                name,
                &mono,
                (x0, base),
                (inner - cw - px(12.0)).max(0.0),
                p.text,
            );
            right_text(pen.layer, pen.text, cpu, &mono, x0 + inner, base, p.text_muted);
            y += row;
        }
        y + px(8.0)
    }

    /// The programs it started: id and name, CPU, memory — and, where its owner offers it,
    /// a button stopping one (or closing its session).
    fn paint_programs(
        &mut self,
        pen: &mut LayerPainter<'_>,
        (x0, inner, y): (f32, f32, f32),
        line: &Line,
        clip: Rect,
    ) -> f32 {
        let ds = pen.ds;
        let ds = &ds;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let texts = std::mem::take(&mut self.texts.detail.programs);
        if texts.is_empty() {
            return y;
        }
        let mut y = self.section(pen, x0, y, &self.texts.labels.programs);
        let mono = ds.code_font(-1.5);
        let row = px(26.0);
        let acts = line
            .processes
            .iter()
            .any(|pr| line.actions_on(ActionTarget::Process(pr.pid)).next().is_some());
        let end = if acts { x0 + inner - px(34.0) } else { x0 + inner };
        for (program, process) in texts.iter().zip(&line.processes) {
            let base = ds::optical_baseline(y, row, &mono);
            if let Some(action) = line.actions_on(ActionTarget::Process(process.pid)).next() {
                let r = Rect::new(x0 + inner - px(28.0), y, px(28.0), row);
                let hit = Hit::Action(*action);
                let spec = quiet_icon(kind_icon(action.kind)).hovered(self.hover == Some(hit));
                ds::button(pen.layer, pen.text, pen.icons, ds, r, spec);
                if let Some(visible) = r.intersect(&clip) {
                    self.hits.push((visible, hit));
                }
            }
            let (x0, inner) = (x0, end - x0);
            right_text(pen.layer, pen.text, &program.memory, &mono, x0 + inner, base, p.text_muted);
            right_text(
                pen.layer,
                pen.text,
                &program.cpu,
                &mono,
                x0 + inner - px(90.0),
                base,
                p.text,
            );
            ds::draw_ellipsized(
                pen.layer,
                pen.text,
                &program.name,
                &mono,
                (x0, base),
                (inner - px(160.0)).max(0.0),
                p.text,
            );
            y += row;
        }
        self.texts.detail.programs = texts;
        y + px(8.0)
    }

    /// Its operations (running, and the last ended), with their progress — and "Stop" on
    /// those its owner can stop.
    fn paint_part_operations(
        &mut self,
        pen: &mut LayerPainter<'_>,
        (x0, inner, y): (f32, f32, f32),
        line: &Line,
        clip: Rect,
    ) -> f32 {
        let ds = pen.ds;
        let ds = &ds;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let model = Arc::clone(&self.model);
        let key = line.key.as_str();
        let mut operations = model.operations().filter(|o| o.owner == key).peekable();
        if operations.peek().is_none() {
            return y;
        }
        let mut y = self.section(pen, x0, y, &self.texts.labels.operations);
        let font = ds.ui_font(-1.0);
        let small = ds.ui_font(-1.5);
        for operation in operations {
            let a = &operation.activity;
            let base = ds::optical_baseline(y, px(20.0), &font);
            let stop = line.actions_on(ActionTarget::Activity(a.id)).next().filter(|_| a.running());
            let inner = match stop {
                Some(action) => {
                    let size = px(ds::button_size::LARGE);
                    let r = Rect::new(x0 + inner - size, y - px(4.0), size, size);
                    let hit = Hit::Action(*action);
                    let spec = quiet_icon(kind_icon(action.kind)).hovered(self.hover == Some(hit));
                    ds::button(pen.layer, pen.text, pen.icons, ds, r, spec);
                    if let Some(visible) = r.intersect(&clip) {
                        self.hits.push((visible, hit));
                    }
                    inner - px(36.0)
                }
                None => inner,
            };
            let sw = pen.text.shape_line(&operation.status, &small).width;
            right_text(
                pen.layer,
                pen.text,
                &operation.status,
                &small,
                x0 + inner,
                base,
                p.text_muted,
            );
            ds::draw_ellipsized(
                pen.layer,
                pen.text,
                &a.subject,
                &font,
                (x0, base),
                (inner - sw - px(12.0)).max(0.0),
                p.text,
            );
            let fraction = match (&a.ended, a.total) {
                (Some(_), _) => 1.0,
                (None, Some(total)) if total > 0 => a.done as f32 / total as f32,
                _ => 0.0,
            };
            let bar = Rect::new(x0, y + px(24.0), inner, px(3.0));
            ds::ProgressBar::of(fraction).paint_bar(pen.layer, ds, bar);
            y += px(36.0);
        }
        y + px(4.0)
    }
}
