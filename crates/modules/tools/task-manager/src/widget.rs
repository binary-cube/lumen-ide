//! The app's resources on calm screens (a [`ScreenWidget`] this module offers, in the
//! bottom right corner): its cards — CPU, memory, GPU when it's known, I/O — on glass, each
//! with its value now, its line and its sparkline, as the Task Manager shows them.
//!
//! Made when a screen opens, it subscribes to the window's collector ([`Hub`]: looking while
//! it's shown, as the Task Manager's own view does) and lets go when the screen closes:
//! nothing is measured for it otherwise. Its texts are written when a new look comes (a
//! generation), its sparklines aggregated again only then or for another width.

use std::sync::{Arc, Mutex, PoisonError};

use i18n::{t, tr};
use text_layout::TextSystem;
use ui_components::chart::{PlotCache, Samples, scale_for};
use ui_components::ds::Ctx;
use ui_components::metric_card::{CardSeries, MetricCard, glass_metric_card};
use ui_components::screen_widgets::{
    ScreenWidget, WIDGET_HEIGHT, WidgetContext, WidgetPaint, WidgetPlace, WidgetView,
};

use crate::collector::{FrameLabels, History, Hub, Metric, Subscription};
use crate::model::format::{percent, rate};

/// A card's width (logical pixels): as wide as there's room for, within these.
const CARD_MIN: f32 = 150.0;
const CARD_MAX: f32 = 220.0;
const GAP: f32 = 10.0;

#[derive(Debug)]
pub struct ResourcesWidget;

impl ScreenWidget for ResourcesWidget {
    fn id(&self) -> &'static str {
        "tasks.resources"
    }

    fn place(&self) -> WidgetPlace {
        WidgetPlace::BottomRight
    }

    /// Only where the app says what its parts cost.
    fn open(&self, ctx: &WidgetContext) -> Option<Box<dyn WidgetView>> {
        Some(Box::new(Resources::open(ctx)?))
    }
}

/// The words that don't change while it's open.
struct Labels {
    cards: [String; 4],
    network: String,
    disk: String,
}

impl Labels {
    fn new() -> Self {
        Self {
            cards: [t("tasks.cpu"), t("tasks.memory"), t("tasks.gpu"), t("tasks.card_io")],
            network: t("tasks.network"),
            disk: t("tasks.disk"),
        }
    }
}

/// A card as written: which (by [`Labels::cards`]), its value, its line, its series.
struct Card {
    which: usize,
    value: String,
    sub: String,
    metrics: &'static [Metric],
}

struct Resources {
    /// Held while it's open: the collector looks for it.
    _subscription: Subscription,
    history: Arc<Mutex<History>>,
    /// The history's generation its texts were written for.
    written: Option<u64>,
    cards: Vec<Card>,
    /// The sparklines' scale tops, by [`Metric::ALL`].
    scales: [f32; 5],
    plots: [PlotCache; 5],
    labels: Labels,
}

impl Resources {
    /// Subscribed to the window's collector (looking while it's open); `None`: the app
    /// doesn't say what its parts cost.
    fn open(ctx: &WidgetContext) -> Option<Self> {
        let source = ctx.resources.as_ref()?;
        let subscription = Hub::subscribe(source, FrameLabels::translated(), false);
        subscription.set_shown(true);
        if let Some(wake) = &ctx.wake {
            subscription.set_waker(Arc::clone(wake));
        }
        let history = subscription.history();
        Some(Self {
            _subscription: subscription,
            history,
            written: None,
            cards: Vec::new(),
            scales: [0.0; 5],
            plots: Default::default(),
            labels: Labels::new(),
        })
    }

    /// The texts written again when a new look came. `false`: nothing looked at yet.
    fn write(&mut self) -> bool {
        let history = self.history.lock().unwrap_or_else(PoisonError::into_inner);
        let generation = history.generation();
        if self.written == Some(generation) {
            return !self.cards.is_empty();
        }
        let total = history.total();
        let last = |m: Metric| total.get(m).last();
        let Some(cpu) = last(Metric::Cpu) else { return false };
        self.written = Some(generation);
        let peak = |m: Metric| total.get(m).iter().fold(0.0_f32, f32::max);
        let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
        let memory = last(Metric::Memory).unwrap_or(0.0);
        let mut cards = vec![
            Card {
                which: 0,
                value: percent(cpu),
                sub: tr!("tasks.cpu_sub", cores = cores, max = percent(peak(Metric::Cpu))),
                metrics: &[Metric::Cpu],
            },
            Card {
                which: 1,
                value: i18n::bytes(memory.max(0.0) as u64),
                sub: String::new(),
                metrics: &[Metric::Memory],
            },
        ];
        // The GPU's only when it's known.
        if let Some(gpu) = last(Metric::Gpu) {
            cards.push(Card {
                which: 2,
                value: i18n::bytes(gpu.max(0.0) as u64),
                sub: t("tasks.gpu_sub"),
                metrics: &[Metric::Gpu],
            });
        }
        let disk = last(Metric::Disk).unwrap_or(0.0);
        cards.push(Card {
            which: 3,
            value: format!("↓↑ {}", rate(last(Metric::Net).unwrap_or(0.0))),
            sub: tr!("tasks.io_sub", disk = rate(disk)),
            metrics: &[Metric::Net, Metric::Disk],
        });
        self.cards = cards;
        self.scales = Metric::ALL.map(|m| scale_for(peak(m), crate::view::floor(m)));
        true
    }

    /// The sparklines, aggregated again only for new values or another width.
    fn update_plots(&mut self, columns: usize) {
        let history = self.history.lock().unwrap_or_else(PoisonError::into_inner);
        let (generation, slots) = (history.generation(), history.capacity());
        for m in Metric::ALL {
            let reduce = crate::view::reduce(m);
            if self.plots[m as usize].is_stale(generation, slots, columns, reduce) {
                let samples = Samples::from_slices(history.total().get(m).as_slices());
                self.plots[m as usize].get(generation, samples, slots, columns, reduce);
            }
        }
    }

    /// A card's width for `room` (physical pixels), within its limits.
    fn card_width(&self, ds: &Ctx<'_>, room: f32) -> f32 {
        let n = self.cards.len().max(1) as f32;
        ((room - ds.px(GAP) * (n - 1.0)) / n).clamp(ds.px(CARD_MIN), ds.px(CARD_MAX)).floor()
    }
}

impl WidgetView for Resources {
    fn size(&mut self, _text: &mut TextSystem, ds: &Ctx<'_>, room: f32) -> Option<(f32, f32)> {
        if !self.write() {
            return None;
        }
        let n = self.cards.len() as f32;
        let w = self.card_width(ds, room);
        Some((w * n + ds.px(GAP) * (n - 1.0), ds.px(WIDGET_HEIGHT)))
    }

    fn paint(&mut self, p: WidgetPaint<'_, '_>) {
        if !self.write() {
            return;
        }
        let ds = p.ds;
        let w = self.card_width(ds, p.rect.size.width);
        self.update_plots((w - ds.px(24.0)).max(1.0) as usize);
        let mut x = p.rect.min_x();
        for card in &self.cards {
            let rect = kernel::Rect::new(x, p.rect.min_y(), w, p.rect.size.height);
            let series_of = |m: Metric| CardSeries {
                plot: self.plots[m as usize].plot(),
                color: crate::view::color(ds.palette, m),
                label: if m == Metric::Disk { &self.labels.disk } else { &self.labels.network },
            };
            let series: Vec<CardSeries<'_>> = card.metrics.iter().map(|m| series_of(*m)).collect();
            let max = card.metrics.iter().map(|m| self.scales[*m as usize]).fold(0.0, f32::max);
            let spec = MetricCard {
                label: &self.labels.cards[card.which],
                value: &card.value,
                sub: (!card.sub.is_empty()).then_some(card.sub.as_str()),
                warning: false,
                series: &series,
                max,
                hovered: false,
                active: false,
                focused: false,
            };
            glass_metric_card(p.layer, p.text, p.icons, ds, rect, &spec, p.look);
            x = rect.max_x() + ds.px(GAP);
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/widget.rs"]
mod tests;
