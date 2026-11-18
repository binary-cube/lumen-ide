//! The summary: always the whole app's — four metric cards (CPU, memory, GPU, I/O: network
//! and disk), a band of pills on a narrow or short page, a line of text on the narrowest.
//! A click on a card sorts the table by its metric; its value's tooltip says the least, the
//! average and the most over the history, its chart's says the value under the mouse and
//! when.

use std::sync::PoisonError;

use render_api::Scene;
use text_layout::TextSystem;
use ui_components::chart::{Reduce, Samples, series_color};
use ui_components::ds::{self, Ctx};
use ui_components::metric_card::{CardForm, CardSeries, MetricCard, metric_card};

use super::{Card, Focus, Hit, TaskManager};
use crate::collector::Metric;
use crate::layout::{PageLayout, SummaryForm};

/// The cards shown, in their order (the GPU's only when it's known).
pub(super) fn cards(has_gpu: bool) -> &'static [Card] {
    if has_gpu {
        &[Card::Cpu, Card::Memory, Card::Gpu, Card::Io]
    } else {
        &[Card::Cpu, Card::Memory, Card::Io]
    }
}

/// The series a card draws.
pub(super) fn metrics(card: Card) -> &'static [Metric] {
    match card {
        Card::Cpu => &[Metric::Cpu],
        Card::Memory => &[Metric::Memory],
        Card::Gpu => &[Metric::Gpu],
        Card::Io => &[Metric::Net, Metric::Disk],
    }
}

/// A metric's series color.
pub(crate) fn color(palette: &ui_components::Palette, metric: Metric) -> kernel::Rgba {
    use ui_components::chart::Series;
    let series = match metric {
        Metric::Cpu => Series::Cpu,
        Metric::Memory => Series::Memory,
        Metric::Gpu => Series::Gpu,
        Metric::Net => Series::Network,
        Metric::Disk => Series::Disk,
    };
    series_color(palette, series)
}

/// How a metric's values become one per pixel column (a level keeps its last; a rate,
/// its spike).
pub(crate) fn reduce(metric: Metric) -> Reduce {
    match metric {
        Metric::Memory | Metric::Gpu => Reduce::Last,
        Metric::Cpu | Metric::Net | Metric::Disk => Reduce::Max,
    }
}

impl TaskManager {
    pub(super) fn paint_summary(
        &mut self,
        scene: &mut Scene,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        page: &PageLayout,
    ) {
        let summary = &page.summary;
        let p = ds.palette;
        match summary.form {
            SummaryForm::None => {}
            SummaryForm::Line => {
                let layer = scene.push_layer(Some(summary.area));
                let font = ds.ui_font(-0.5).with_weight(text_layout::Font::SEMIBOLD);
                let base =
                    ds::optical_baseline(summary.area.min_y(), summary.area.size.height, &font);
                let room = summary.area.size.width;
                let x = summary.area.min_x();
                ds::draw_ellipsized(layer, text, &self.texts.line, &font, (x, base), room, p.text);
            }
            SummaryForm::Cards | SummaryForm::Pills => {
                let form = if summary.form == SummaryForm::Cards {
                    CardForm::Card
                } else {
                    CardForm::Pill
                };
                self.update_card_plots(page, form, ds);
                let layer = scene.push_layer(Some(page.bounds));
                let shown = cards(summary.cards.len() == 4);
                for (&card, &rect) in shown.iter().zip(&summary.cards) {
                    let texts = &self.texts.cards[card as usize];
                    let metrics = metrics(card);
                    let plots = &self.plots.cards;
                    let series_of = |i: usize| {
                        let m = metrics[i.min(metrics.len() - 1)];
                        CardSeries {
                            plot: plots[m as usize].plot(),
                            color: color(p, m),
                            label: if m == Metric::Disk {
                                &self.texts.labels.disk
                            } else {
                                &self.texts.labels.network
                            },
                        }
                    };
                    let both = [series_of(0), series_of(1)];
                    let series = &both[..metrics.len()];
                    let hovered = matches!(
                        self.hover,
                        Some(Hit::Card(c) | Hit::CardValue(c) | Hit::CardChart(c)) if c == card
                    );
                    let max = self.texts.scales[metrics[0] as usize]
                        .max(metrics.get(1).map_or(0.0, |m| self.texts.scales[*m as usize]));
                    let spec = MetricCard {
                        label: &self.texts.labels.cards[card as usize],
                        value: &texts.value,
                        sub: (!texts.sub.is_empty()).then_some(texts.sub.as_str()),
                        warning: texts.warning,
                        series,
                        max,
                        hovered,
                        active: self.sorts_by(card),
                        focused: self.shows_ring() && self.focus == Focus::Card(card),
                    };
                    let areas = metric_card(layer, text, icons, ds, rect, &spec, form);
                    self.hits.push((rect, Hit::Card(card)));
                    self.stops.push((rect, Focus::Card(card)));
                    if areas.chart.size.width > 0.0 {
                        self.hits.push((areas.chart, Hit::CardChart(card)));
                    }
                    self.hits.push((areas.value, Hit::CardValue(card)));
                }
                // The reading under the mouse over a card's chart.
                if let (Some(Hit::CardChart(card)), Some(mouse)) = (self.hover, self.mouse)
                    && let Some(&(chart, _)) =
                        self.hits.iter().rev().find(|(_, h)| *h == Hit::CardChart(card))
                {
                    let metric = metrics(card)[0];
                    if let Some(reading) =
                        self.plots.cards[metric as usize].plot().reading(chart, mouse.x)
                    {
                        let line = ds.line();
                        let guide =
                            kernel::Rect::new(reading.x, chart.min_y(), line, chart.size.height);
                        layer.quad(render_api::Quad::filled(guide, p.text.with_alpha(0.35)));
                    }
                }
            }
        }
    }

    /// The cards' plots, aggregated again only for new values or another width.
    fn update_card_plots(&mut self, page: &PageLayout, form: CardForm, ds: &Ctx<'_>) {
        let Some(history) = self.history.clone() else { return };
        let width = page.summary.cards.first().map_or(0.0, |r| r.size.width);
        let columns = match form {
            CardForm::Card => (width - ds.px(24.0)).max(1.0) as usize,
            CardForm::Pill => ds.px(28.0) as usize,
        };
        let generation = history.lock().unwrap_or_else(PoisonError::into_inner).generation();
        let slots = history.lock().unwrap_or_else(PoisonError::into_inner).capacity();
        let stale = Metric::ALL.iter().any(|m| {
            self.plots.cards[*m as usize].is_stale(generation, slots, columns, reduce(*m))
        });
        if !stale {
            return;
        }
        let history = history.lock().unwrap_or_else(PoisonError::into_inner);
        for m in Metric::ALL {
            let samples = Samples::from_slices(history.total().get(m).as_slices());
            self.plots.cards[m as usize].get(generation, samples, slots, columns, reduce(m));
        }
    }
}
