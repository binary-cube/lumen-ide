//! A chart pane over a result's rows: the chart (the shared component), the bar over it
//! (how many rows it's drawn from; all of them fetched on a press), and its series settings
//! on its right (shown at first, while nothing is chosen). The rows are prepared on a worker
//! each time they or the settings change (the last asked wins); the chart is laid out and
//! drawn from what was prepared, never from the rows.

use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, LazyLock};

use charts_model::{ChartSpec, ColumnKind, Prepared, SeriesSpec, Unready};
use kernel::{Point, Rect};
use render_api::Quad;
use ui_components::charts::{Chart, ChartLook};
use ui_components::control::{Control, Press};
use ui_components::ds::{self, Ctx};
use ui_components::picker::{ItemRows, Picker, PickerEvent, PickerItem};
use ui_components::result_pane::{Held, Lent, PaneRequest, ResultPane};
use ui_components::{Tip, ViewerCursor, ViewerPaint};

use crate::panel::{self, Field, Opt, PanelHit};

/// The charts' preparing, shared by all panes.
static PREPARE: LazyLock<kernel::Pool> =
    LazyLock::new(|| kernel::Pool::new(crate::PREPARE_THREAD, 2, kernel::work::IDLE));

type Wake = Arc<dyn Fn() + Send + Sync>;

/// Wakes the window with what's in `slot` now (taken out of the lock, called after it).
fn wake_from(slot: &std::sync::Mutex<Option<Wake>>) {
    let wake = slot.lock().ok().and_then(|w| w.clone());
    if let Some(wake) = wake {
        wake();
    }
}

/// What was prepared, for which asking.
type Made = (u64, Result<Prepared, Unready>);

/// A chart pane (see the module's doc).
pub struct ChartPane {
    spec: ChartSpec,
    rows: Option<Lent>,
    held: Held,
    chart: Chart,
    /// The last asking, the one being prepared, what came of it.
    asked: u64,
    preparing: Option<Receiver<Made>>,
    unready: Option<Unready>,
    /// The settings shown.
    settings: bool,
    /// The series being set.
    current: usize,
    /// The list open under a field.
    picker: Picker,
    choosing: Option<Field>,
    /// The fields' boxes, the panel's and the bar's parts, as drawn last.
    fields: Vec<(Field, Rect)>,
    hits: Vec<(Rect, PanelHit)>,
    hover: Option<PanelHit>,
    area: Rect,
    chart_area: Rect,
    /// The chart holds the pointer (a drag along X).
    chart_held: bool,
    request: Option<PaneRequest>,
    /// What wakes the window, as its paint last gave it: read by a job when it's done (one
    /// asked before the pane was first drawn still wakes it).
    waker: Arc<std::sync::Mutex<Option<Wake>>>,
    /// The look it was drawn in last (its export's colors).
    look: Option<ChartLook>,
    /// An export being written: how it went, to tell.
    saving: Option<Receiver<Result<String, String>>>,
    /// Drawn from the grid's selection: the chart as it was before (back with all the
    /// rows, the one kept), and as it was set for the selection (changed since: kept).
    before_pick: Option<(ChartSpec, ChartSpec)>,
    /// The latest asking, read by the jobs waiting: one asked again before it began is let
    /// go (never a chart of all the rows prepared before the selection's).
    latest: Arc<std::sync::atomic::AtomicU64>,
}

impl std::fmt::Debug for ChartPane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChartPane").field("spec", &self.spec).finish_non_exhaustive()
    }
}

impl Default for ChartPane {
    fn default() -> Self {
        Self::new()
    }
}

impl ChartPane {
    pub fn new() -> Self {
        let mut chart = Chart::new();
        chart.set_title(&i18n::t("charts.title"));
        Self {
            spec: ChartSpec { series: vec![SeriesSpec::default()], ..ChartSpec::default() },
            rows: None,
            held: Held::default(),
            chart,
            asked: 0,
            preparing: None,
            unready: Some(Unready::NotConfigured),
            settings: true,
            current: 0,
            picker: Picker::default(),
            choosing: None,
            fields: Vec::new(),
            hits: Vec::new(),
            hover: None,
            area: Rect::default(),
            chart_area: Rect::default(),
            chart_held: false,
            request: None,
            waker: Arc::default(),
            look: None,
            saving: None,
            before_pick: None,
            latest: Arc::default(),
        }
    }

    pub fn spec(&self) -> &ChartSpec {
        &self.spec
    }

    /// The series being set.
    fn series(&mut self) -> &mut SeriesSpec {
        if self.spec.series.is_empty() {
            self.spec.series.push(SeriesSpec::default());
        }
        self.current = self.current.min(self.spec.series.len() - 1);
        &mut self.spec.series[self.current]
    }

    /// The chart prepared again from the rows (on a worker), for the settings now.
    fn prepare(&mut self) {
        self.chart.set_options(self.spec.options);
        let Some(rows) = self.rows.clone() else { return };
        self.asked += 1;
        let (asked, spec) = (self.asked, self.spec.clone());
        let (tx, rx) = std::sync::mpsc::channel();
        let wake = Arc::clone(&self.waker);
        let latest = Arc::clone(&self.latest);
        latest.store(asked, std::sync::atomic::Ordering::Release);
        PREPARE.spawn(kernel::Priority::Interactive, None, move || {
            // Asked again meanwhile: the newer one is prepared instead.
            if latest.load(std::sync::atomic::Ordering::Acquire) != asked {
                return;
            }
            let _ = tx.send((asked, charts_model::prepare(&spec, &*rows)));
            wake_from(&wake);
        });
        self.preparing = Some(rx);
    }

    /// A first chart for rows nothing was chosen for: the first text or moment column along
    /// X, the first number (another) as the values; a line over time, bars otherwise.
    /// Nothing measurable: nothing proposed.
    fn suggest(&mut self, rows: &dyn charts_model::Tabular) {
        let columns = rows.columns();
        let x = columns.iter().position(|c| matches!(c.kind, ColumnKind::Text | ColumnKind::Time));
        let y = columns
            .iter()
            .enumerate()
            .find(|(i, c)| Some(*i) != x && c.kind == ColumnKind::Number)
            .map(|(i, _)| i);
        let Some(y) = y else { return };
        let time = x.is_some_and(|x| columns[x].kind == ColumnKind::Time);
        self.spec.kind =
            if time { charts_model::ChartKind::Line } else { charts_model::ChartKind::Bar };
        let series = self.series();
        series.x = x.map(|x| columns[x].name.clone());
        series.y = vec![columns[y].name.clone()];
    }

    /// The rows' columns that fit `field` (values: measured ones; groups: text).
    fn columns_for(&self, field: Field) -> Vec<String> {
        let Some(rows) = &self.rows else { return Vec::new() };
        rows.columns()
            .iter()
            .filter(|c| match field {
                Field::Y => c.kind.measurable(),
                Field::Group => c.kind == ColumnKind::Text,
                Field::X if self.spec.kind.continuous_x() => c.kind.measurable(),
                _ => true,
            })
            .map(|c| c.name.clone())
            .collect()
    }

    /// The list for `field` opened under its box.
    fn open_picker(&mut self, field: Field) {
        if self.choosing == Some(field) {
            self.picker.close();
            self.choosing = None;
            return;
        }
        let series = self.spec.series.get(self.current).cloned().unwrap_or_default();
        let (items, chosen): (Vec<PickerItem>, Vec<usize>) = match field {
            Field::Aggregate => {
                let items =
                    panel::AGGREGATES.iter().map(|&a| item(panel::aggregate_name(a))).collect();
                let at = panel::AGGREGATES.iter().position(|&a| a == series.aggregate);
                (items, at.into_iter().collect())
            }
            Field::Y => {
                let names = self.columns_for(field);
                let chosen = names
                    .iter()
                    .enumerate()
                    .filter(|(_, n)| series.y.contains(n))
                    .map(|(i, _)| i)
                    .collect();
                (names.into_iter().map(item).collect(), chosen)
            }
            Field::X | Field::Group => {
                let none =
                    if field == Field::X { "charts.rows_in_order" } else { "charts.no_groups" };
                let names = self.columns_for(field);
                let current = if field == Field::X { &series.x } else { &series.group };
                let at = current
                    .as_ref()
                    .and_then(|c| names.iter().position(|n| n == c))
                    .map_or(0, |i| i + 1);
                let items = std::iter::once(item(i18n::t(none)))
                    .chain(names.into_iter().map(item))
                    .collect();
                (items, vec![at])
            }
        };
        if field == Field::Y {
            self.picker.open_many(items, chosen, Box::new(ItemRows::default()));
        } else {
            self.picker.open(items, chosen.first().copied());
        }
        self.choosing = Some(field);
    }

    /// What the list chose for the field it's open under.
    fn picked(&mut self, event: PickerEvent) {
        let Some(field) = self.choosing else { return };
        match event {
            PickerEvent::Closed => {
                self.choosing = None;
                return;
            }
            PickerEvent::Toggled(i) if field == Field::Y => {
                let Some(name) = self.columns_for(Field::Y).get(i).cloned() else { return };
                let y = &mut self.series().y;
                match y.iter().position(|n| *n == name) {
                    Some(at) => {
                        y.remove(at);
                    }
                    None => y.push(name),
                }
            }
            PickerEvent::Chosen(i) => {
                self.choosing = None;
                match field {
                    Field::Aggregate => {
                        if let Some(&a) = panel::AGGREGATES.get(i) {
                            self.series().aggregate = a;
                        }
                    }
                    Field::X | Field::Group => {
                        let name =
                            i.checked_sub(1).and_then(|i| self.columns_for(field).get(i).cloned());
                        let series = self.series();
                        if field == Field::X {
                            series.x = name;
                        } else {
                            series.group = name;
                        }
                    }
                    Field::Y => {}
                }
            }
            PickerEvent::Toggled(_) => return,
        }
        self.prepare();
    }

    fn press_panel(&mut self, hit: PanelHit) {
        match hit {
            PanelHit::Kind(kind) => {
                self.spec.kind = kind;
                self.chart.reset_zoom();
                self.prepare();
            }
            PanelHit::Field(field) => self.open_picker(field),
            PanelHit::Opt(opt) => {
                let o = &mut self.spec.options;
                let flag = match opt {
                    Opt::Stepped => &mut o.stepped,
                    Opt::Points => &mut o.points,
                    Opt::Percent => &mut o.percent,
                    Opt::Log => &mut o.log,
                };
                *flag = !*flag;
                self.prepare();
            }
            PanelHit::Close => {
                self.settings = false;
                self.picker.close();
                self.choosing = None;
            }
            PanelHit::Settings => self.settings = !self.settings,
            PanelHit::Export => {
                let name = format!("{}.png", i18n::t("charts.file_name"));
                self.request = Some(PaneRequest::SaveAs { name });
            }
            PanelHit::FetchAll => {
                self.request = Some(PaneRequest::AllRows { limit: crate::settings::fetch_rows() });
            }
            PanelHit::WholeRows => self.request = Some(PaneRequest::WholeRows),
            PanelHit::Series(i) => self.current = i.min(self.spec.series.len().saturating_sub(1)),
            PanelHit::AddSeries => {
                if self.spec.series.len() < panel::MAX_SERIES {
                    // As the one before it (its X, its aggregate), its values to choose.
                    let like = self.spec.series.get(self.current).cloned().unwrap_or_default();
                    self.spec.series.push(SeriesSpec { y: Vec::new(), ..like });
                    self.current = self.spec.series.len() - 1;
                }
            }
            PanelHit::RemoveSeries => {
                if self.spec.series.len() > 1 {
                    self.spec.series.remove(self.current);
                    self.current = self.current.min(self.spec.series.len() - 1);
                    self.prepare();
                }
            }
        }
    }

    fn hit(&self, p: Point) -> Option<PanelHit> {
        self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)
    }

    /// Drawn from a selection (`picked`) or from all the rows again: the chart set for all
    /// of them kept aside, the selection drawn with it when its columns are there (else as
    /// suggested for them); back, it's restored — unless it was changed meanwhile.
    fn follow_pick(&mut self, rows: &dyn charts_model::Tabular, picked: bool) {
        if !picked {
            if let Some((before, set)) = self.before_pick.take()
                && self.spec == set
            {
                self.spec = before;
            }
            return;
        }
        let before =
            self.before_pick.as_ref().map_or_else(|| self.spec.clone(), |(b, _)| b.clone());
        self.spec = if fits(&before, rows) {
            before.clone()
        } else {
            ChartSpec { series: vec![SeriesSpec::default()], ..ChartSpec::default() }
        };
        if !self.spec.is_configured() {
            self.suggest(rows);
        }
        self.current = 0;
        self.before_pick = Some((before, self.spec.clone()));
    }

    /// What the bar says of the rows the chart is drawn from.
    fn note(&self) -> String {
        let n = self.held.rows;
        if self.held.picked {
            return i18n::tr!("charts.from_selection", count = n);
        }
        if self.held.fetching {
            return i18n::tr!("charts.fetching", count = n);
        }
        if self.held.more && n < crate::settings::fetch_rows() {
            return i18n::tr!("charts.from_some", count = n);
        }
        i18n::tr!("charts.from_all", count = n)
    }

    /// Why nothing is drawn, in the user's words.
    fn unready_text(why: &Unready) -> String {
        match why {
            Unready::NotConfigured => i18n::t("charts.not_configured"),
            Unready::MissingColumn(c) => i18n::tr!("charts.missing_column", column = c.as_str()),
            Unready::NotMeasurable(c) => i18n::tr!("charts.not_measurable", column = c.as_str()),
        }
    }
}

/// Whether every column `spec` names is among `rows`' (a chart set for all the rows drawn
/// from a selection as it is).
fn fits(spec: &ChartSpec, rows: &dyn charts_model::Tabular) -> bool {
    spec.is_configured()
        && spec
            .series
            .iter()
            .all(|s| s.x.iter().chain(&s.group).chain(&s.y).all(|name| rows.column(name).is_some()))
}

fn item(label: String) -> PickerItem {
    PickerItem { label, ..PickerItem::default() }
}

impl ResultPane for ChartPane {
    /// The rows lent: prepared again, unless more are on their way (their count shown
    /// meanwhile; prepared once, when they've all come).
    fn set_rows(&mut self, rows: Lent, _epoch: u64, held: Held) {
        self.follow_pick(&*rows, held.picked);
        if !self.spec.is_configured() {
            self.suggest(&*rows);
        }
        self.rows = Some(rows);
        self.held = held;
        if !held.fetching {
            self.prepare();
        }
    }

    fn paint(&mut self, paint: ViewerPaint<'_>) {
        if paint.waker.is_some() {
            if let Ok(mut waker) = self.waker.lock() {
                waker.clone_from(&paint.waker);
            }
            self.chart.set_waker(&paint.waker);
        }
        let ViewerPaint { bounds, scale, text, scene, palette, icons, .. } = paint;
        let ctx = Ctx::new(palette, scale);
        let px = |v: f32| ctx.px(v);
        self.area = bounds;
        self.hits.clear();
        let panel_w =
            if self.settings { px(panel::PANEL_W).min(bounds.size.width * 0.6) } else { 0.0 };
        let left = Rect::new(
            bounds.min_x(),
            bounds.min_y(),
            bounds.size.width - panel_w,
            bounds.size.height,
        );
        let bar = Rect::new(left.min_x(), left.min_y(), left.size.width, px(panel::BAR_H));
        let pad = px(12.0);
        self.chart_area = Rect::new(
            left.min_x() + pad,
            bar.max_y() + pad,
            (left.size.width - 2.0 * pad).max(1.0),
            (left.max_y() - bar.max_y() - 2.0 * pad).max(1.0),
        );
        let layer = scene.push_layer(Some(bounds));
        layer.quad(Quad::filled(bounds, palette.editor_background));
        let working = self.preparing.is_some() || self.chart.busy();
        let note = if working { i18n::t("charts.preparing") } else { self.note() };
        let fetch = if self.held.picked {
            Some((i18n::t("charts.whole_rows"), PanelHit::WholeRows))
        } else {
            (self.held.more
                && !self.held.fetching
                && self.held.rows < crate::settings::fetch_rows())
            .then(|| (i18n::t("charts.fetch_all"), PanelHit::FetchAll))
        };
        let mut pen =
            panel::Pen { layer, text, icons, ctx: &ctx, hover: self.hover, hits: &mut self.hits };
        let can_export = self.unready.is_none();
        let fetch = fetch.as_ref().map(|(label, hit)| (label.as_str(), *hit));
        panel::paint_bar(&mut pen, bar, (&note, working), fetch, (self.settings, can_export));
        match &self.unready {
            Some(why) => {
                let message = Self::unready_text(why);
                ds::empty_state(pen.layer, pen.text, &ctx, self.chart_area, &message, None);
                self.chart.forget();
            }
            None => {
                let look = ChartLook::new(&ctx, palette.editor_background);
                self.chart.paint(pen.layer, pen.text, &ctx, &look, self.chart_area);
                self.look = Some(look);
            }
        }
        if self.settings {
            let rect = Rect::new(left.max_x(), bounds.min_y(), panel_w, bounds.size.height);
            self.fields =
                panel::paint_panel(&mut pen, rect, (&self.spec, self.current), self.choosing);
        } else {
            self.fields.clear();
        }
        // The list open under its field, over everything.
        if let Some(anchor) =
            self.choosing.and_then(|f| self.fields.iter().find(|(g, _)| *g == f)).map(|(_, r)| *r)
        {
            self.picker.paint(scene, text, icons, &ctx, (anchor, bounds), "");
        }
    }

    fn poll(&mut self) -> bool {
        let mut changed = self.chart.poll();
        if let Some(rx) = &self.saving
            && let Ok(saved) = rx.try_recv()
        {
            self.saving = None;
            self.request = Some(match saved {
                Ok(name) => PaneRequest::Tell {
                    text: i18n::tr!("charts.saved", name = name.as_str()),
                    ok: true,
                },
                Err(why) => PaneRequest::Tell {
                    text: i18n::tr!("charts.save_failed", why = why.as_str()),
                    ok: false,
                },
            });
            changed = true;
        }
        let Some(rx) = &self.preparing else { return changed };
        match rx.try_recv() {
            Ok((asked, prepared)) => {
                if asked == self.asked {
                    self.preparing = None;
                    match prepared {
                        Ok(p) => {
                            self.unready = None;
                            self.chart.set_data(Some(Arc::new(p)), asked);
                        }
                        Err(why) => {
                            self.unready = Some(why);
                            self.chart.set_data(None, asked);
                        }
                    }
                    changed = true;
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => self.preparing = None,
        }
        changed
    }

    fn busy(&self) -> bool {
        self.preparing.is_some() || self.chart.busy() || self.saving.is_some()
    }

    fn take_request(&mut self) -> Option<PaneRequest> {
        self.request.take()
    }

    /// The chart as set for all the rows (drawn from a selection: as it was before).
    fn saved(&self) -> Option<String> {
        let spec = self.before_pick.as_ref().map_or(&self.spec, |(before, _)| before);
        spec.is_configured().then(|| serde_json::to_string(spec).ok()).flatten()
    }

    fn restore(&mut self, saved: &str) {
        if let Ok(spec) = serde_json::from_str::<ChartSpec>(saved) {
            self.spec = spec;
            // Set already: the chart first, its settings a press away.
            self.settings = !self.spec.is_configured();
            self.prepare();
        }
    }

    fn hidden(&mut self) {
        self.chart.forget();
        self.picker.close();
        self.choosing = None;
    }
}

impl Control for ChartPane {
    fn area(&self) -> Option<Rect> {
        Some(self.area).filter(|a| a.size.width > 0.0)
    }

    fn press(&mut self, p: Point, clicks: u32) -> Press {
        if self.picker.is_open() {
            if self.picker.contains(p) {
                if let Some(event) = self.picker.press_with(p, clicks) {
                    self.picked(event);
                }
                return Press::Took;
            }
            // A press beside the list closes it (and does what it's on, unless that's its
            // own field, which would open it again).
            self.picker.close();
            let field = self.choosing.take();
            if field.is_some() && self.hit(p) == field.map(PanelHit::Field) {
                return Press::Took;
            }
        }
        if let Some(hit) = self.hit(p) {
            self.press_panel(hit);
            return Press::Took;
        }
        match self.chart.press(p, clicks) {
            Press::Held => {
                self.chart_held = true;
                Press::Held
            }
            other => other,
        }
    }

    fn drag(&mut self, p: Point) -> bool {
        if self.picker.dragging() {
            return self.picker.drag(p);
        }
        self.chart_held && self.chart.drag(p)
    }

    fn release(&mut self) -> bool {
        if self.picker.dragging() {
            return self.picker.release();
        }
        std::mem::take(&mut self.chart_held) && self.chart.release()
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        let over_list = p.is_some_and(|p| self.picker.is_open() && self.picker.contains(p));
        let mut changed = match p {
            Some(p) if over_list => self.picker.hover(p),
            _ => false,
        };
        let hover = p.filter(|_| !over_list).and_then(|p| self.hit(p));
        changed |= hover != self.hover;
        self.hover = hover;
        let on_chart = p.filter(|p| !over_list && self.chart_area.contains(*p));
        changed | self.chart.hover(on_chart)
    }

    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        if self.picker.is_open() && self.picker.contains(p) {
            return self.picker.cursor_at(p);
        }
        if self.hit(p).is_some() {
            return Some(ViewerCursor::Pointer);
        }
        self.chart.cursor(p)
    }

    /// The path its "Save As" chose: the chart written there on a worker (an `.svg` as SVG,
    /// anything else as PNG), how it went told.
    fn command(&mut self, id: &str) -> bool {
        let Some(path) = id.strip_prefix("picked-save:") else { return false };
        let Some(svg) = self.look.as_ref().and_then(|look| self.chart.svg(look)) else {
            return false;
        };
        let path = std::path::PathBuf::from(path);
        let (tx, rx) = std::sync::mpsc::channel();
        let wake = Arc::clone(&self.waker);
        PREPARE.spawn(kernel::Priority::Interactive, None, move || {
            let _ = tx.send(crate::export::write(&path, &svg));
            wake_from(&wake);
        });
        self.saving = Some(rx);
        true
    }

    fn animating(&self) -> bool {
        self.chart.animating()
    }

    /// Its open list takes the keys, the text and the commands, whatever has the focus.
    fn captures_keys(&self) -> bool {
        self.picker.is_open()
    }

    fn key(&mut self, key: ui_components::ViewerKey, reach: ui_components::KeyReach) -> bool {
        if !self.picker.is_open() {
            return false;
        }
        if let Some(event) = self.picker.key(key, reach) {
            self.picked(event);
        }
        true
    }

    fn text(&mut self, typed: &str) -> bool {
        self.picker.is_open() && self.picker.text_input(typed)
    }

    /// An open list keeps the wheel even at its ends (nothing under it scrolls instead).
    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        if self.picker.is_open() {
            self.picker.scroll(dy);
            return true;
        }
        self.chart.scroll(0.0, dy)
    }

    fn tooltip(&self, p: Point) -> Option<Tip> {
        if let Some((r, hit)) = self.hits.iter().rev().find(|(r, _)| r.contains(p)) {
            let label = match hit {
                PanelHit::Kind(kind) => panel::kind_name(*kind),
                PanelHit::Settings => i18n::t("charts.settings"),
                PanelHit::Close => i18n::t("charts.close_settings"),
                PanelHit::Export => i18n::t("charts.export"),
                PanelHit::FetchAll => i18n::tr!(
                    "charts.fetch_all_detail",
                    count = crate::settings::grouped(crate::settings::fetch_rows() as i64)
                ),
                PanelHit::WholeRows => i18n::t("charts.whole_rows_detail"),
                PanelHit::AddSeries => i18n::t("charts.add_series"),
                PanelHit::RemoveSeries => i18n::t("charts.remove_series"),
                PanelHit::Series(i) => i18n::tr!("charts.series_n", n = i + 1),
                _ => return None,
            };
            return Some(Tip::new(*r, label));
        }
        self.chart.tooltip(p)
    }
}

#[cfg(test)]
#[path = "../tests/unit/pane.rs"]
mod tests;
