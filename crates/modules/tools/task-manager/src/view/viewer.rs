//! The Task Manager as a tab's view: what it reacts to — the mouse (its rows, headers,
//! cards, toolbar, popovers, scrollbars and the detail's separator), the keys (typing,
//! pasting and editing in the filter through its part, the rest through
//! [`super::keys`]), its right-click menus and commands
//! ([`super::commands`]), the wheel — and what it
//! draws each frame, and the tooltips under the mouse.

use std::sync::Arc;

use i18n::tr;
use kernel::Point;
use ui_components::control::{Control, Controls, Divider, NOWHERE, Scrolled};
use ui_components::tooltip::Tip;
use ui_components::{KeyReach, Viewer, ViewerAction, ViewerCursor, ViewerKey, ViewerPaint};

use super::filter::{FilterField, typed_line};
use super::summary::metrics;
use super::texts::value;
use super::{Focus, Hit, Popover, TaskManager};
use crate::collector::Metric;
use crate::layout::Tool;
use crate::model::{Item, ViewModel};

/// The detail's separator, first among its parts (see [`TaskManager::with_controls`]).
const DIVIDER: usize = 0;

/// The filter's field, last among its parts (never hit: it only takes the keys).
const FILTER: usize = 3;

/// Everywhere a point can be (the list's wheel area: the wheel anywhere the detail isn't is
/// its, even before the mouse is first seen).
const EVERYWHERE: kernel::Rect =
    kernel::Rect::new(f32::MIN, f32::MIN, f32::INFINITY, f32::INFINITY);

impl TaskManager {
    /// Its parts, the topmost first, with their state of use: the detail's separator, the
    /// detail (while open) and the list, each scrolled by the wheel over it and by its
    /// scrollbar's thumb; the filter's field, which has the keys while the ring is on it.
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let detail_area = self.layouts.last().and_then(|l| l.detail).filter(|_| self.detail_open());
        let mut divider = Divider { splitter: &mut self.splitter, at: &mut self.split_at };
        let mut detail = Scrolled {
            bar: &mut self.detail_bar,
            offset: &mut self.detail_scroll,
            // Closed, it's nowhere: an empty area at the origin.
            area: detail_area.unwrap_or_default(),
            max: self.detail_max,
        };
        let mut list = Scrolled {
            bar: &mut self.bar,
            offset: &mut self.scroll,
            area: EVERYWHERE,
            max: self.max_scroll,
        };
        let mut filter = FilterField { input: &mut self.filter };
        let mut parts: [&mut dyn Control; 4] = [&mut divider, &mut detail, &mut list, &mut filter];
        f(&mut self.controls, &mut parts)
    }

    /// The keyboard moves to `focus`: reaching the filter's stop gives its part the parts'
    /// focus (the keys, the text, the commands), leaving it takes it away.
    pub(super) fn focus_on(&mut self, focus: Focus) {
        self.focus = focus;
        let keys = (focus == Focus::Tool(Tool::Filter)).then_some(FILTER);
        self.with_controls(|c, parts| c.focus(parts, keys));
    }

    /// The filter's part has the keys.
    pub(super) fn filter_has_keys(&self) -> bool {
        self.controls.focused() == Some(FILTER)
    }

    /// A key to the part with the keys (the filter's editing keys): `true` if used, the
    /// rows filtered again by what it changed.
    pub(super) fn part_key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        let used = self.with_controls(|c, parts| c.key(parts, key, reach)).is_some();
        if used {
            self.filtered();
        }
        used
    }

    /// Backspace / Delete with the keys elsewhere: they still edit the filter (what's typed
    /// anywhere goes there, so what undoes it does too), its focus left where it is.
    pub(super) fn erase_in_filter(&mut self, key: ViewerKey, reach: KeyReach) {
        self.with_controls(|_, parts| parts[FILTER].key(key, reach));
        self.filtered();
    }

    /// Over the detail (its own scroll) or the list.
    fn over_detail(&self, p: Point) -> bool {
        self.detail_open()
            && self.layouts.last().and_then(|l| l.detail).is_some_and(|d| d.contains(p))
    }

    /// The text of a row drawn cut (its whole name).
    fn row_name(model: &ViewModel, item: usize) -> Option<String> {
        match model.items.get(item)? {
            Item::Part { index, .. } => Some(model.frame.lines[*index].name.clone()),
            Item::Process { texts, .. } => Some(texts.name.clone()),
            // The whole of it (a query's text), when its subject is only its start.
            Item::Operation(o) => {
                Some(o.activity.text.clone().unwrap_or_else(|| o.activity.subject.clone()))
            }
            Item::Group { title, .. } => Some(title.clone()),
        }
    }

    /// A chart's reading at `x` as a tooltip's text (`12 s ago: 3.4%`).
    fn reading_tip(
        &self,
        plot: &ui_components::chart::Plot,
        area: kernel::Rect,
        x: f32,
        metric: Metric,
    ) -> Option<String> {
        let reading = plot.reading(area, x)?;
        let step = super::preferences().interval.max(1) as usize;
        let value = value(metric, reading.value);
        Some(if reading.ago == 0 {
            tr!("tasks.chart_now", value = value)
        } else {
            tr!("tasks.chart_ago", seconds = reading.ago * step, value = value)
        })
    }
}

impl Viewer for TaskManager {
    /// Hidden (another tab in its group, the window hidden): the hub pauses unless another
    /// view of it is shown, and lets go of the source (the window stops filling in its
    /// parts); shown again, both come back.
    fn set_shown(&mut self, shown: bool) {
        self.show(shown);
    }

    fn set_data(&mut self, data: &dyn std::any::Any) {
        if let Some(workbench_model::ResourceData(source)) = data.downcast_ref() {
            self.set_source(Arc::clone(source));
        } else if let Some(sizes) = data.downcast_ref::<workbench_model::LayoutSizes>() {
            self.take_sizes(sizes);
        }
    }

    /// "Clear and Rebuild Caches": what it listed, wrote, laid out and aggregated goes
    /// (built again when next drawn); the measured history stays.
    fn purge(&mut self) {
        self.model = Arc::new(ViewModel::default());
        self.texts = super::Texts::default();
        kernel::Purge::purge(&mut self.layouts);
        kernel::Purge::purge(&mut self.plots);
    }

    /// Its mirror in a split looks at the same window (the same hub).
    fn mirror_data(&self) -> Option<Box<dyn std::any::Any>> {
        let source = self.source.as_ref()?.upgrade()?;
        Some(Box::new(workbench_model::ResourceData(source)))
    }

    fn poll(&mut self) -> bool {
        // An action confirmed in the modal (also told at once through `CONFIRMED`).
        if self.confirmed() {
            return true;
        }
        let latest = self.subscription.as_ref().and_then(|s| s.tables.take());
        match latest {
            Some(tick) => {
                let shown = !self.paused;
                self.take(tick);
                shown
            }
            None => false,
        }
    }

    /// Its filter has the keys: typed text goes there (elsewhere a typed character opens
    /// it — [`Viewer::key`] — and its actions' keys work).
    fn wants_text(&self) -> bool {
        self.filter_has_keys()
    }

    /// A character typed while the filter hasn't the keys: it goes there.
    fn key(&mut self, c: char) -> bool {
        let mut buffer = [0; 4];
        self.text_input(c.encode_utf8(&mut buffer))
    }

    /// Typing goes into the filter: it gets the keys first (nothing to type: they stay).
    fn text_input(&mut self, text: &str) -> bool {
        if typed_line(text).is_none() {
            return false;
        }
        self.focus_filter();
        let typed = self.with_controls(|c, parts| c.text(parts, text)).is_some();
        self.filtered();
        typed
    }

    /// Pasted while the filter has the keys: into it, as typed (on one line).
    fn paste(&mut self, text: &str) -> bool {
        let pasted = self.with_controls(|c, parts| c.paste(parts, text)).is_some();
        self.filtered();
        pasted
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        self.key(key, KeyReach::default())
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        self.key(key, reach)
    }

    /// ⌘C: what's selected in the filter while it has the keys, else the keys' row
    /// (tab-separated).
    fn selected_text(&self) -> Option<String> {
        // Its part read as the field it lends (they select alike).
        let filter: &dyn Control = &self.filter;
        self.controls
            .selected_text_with(|i| (i == FILTER).then_some(filter))
            .or_else(|| self.row_text())
    }

    fn context_menu(&mut self, p: Point) -> Option<Vec<ui_components::ViewMenuEntry>> {
        self.context_entries(p)
    }

    /// The wheel: to the part under the mouse that scrolls (the detail or the list).
    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        let at = self.mouse.unwrap_or(NOWHERE);
        self.with_controls(|c, parts| c.scroll(parts, at, dx, dy)).is_some()
    }

    /// A press in its tab: how many clicks in a row (a column's edge double-clicked fits it).
    fn pointer_down(
        &mut self,
        p: Point,
        clicks: u32,
        _mods: ui_components::PressMods,
    ) -> Option<ViewerAction> {
        self.clicks = clicks;
        let action = self.click(p);
        self.clicks = 1;
        action
    }

    fn press(&mut self, p: Point, clicks: u32) -> Option<ViewerAction> {
        self.clicks = clicks;
        let action = self.click(p);
        self.clicks = 1;
        action
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        // A click shows no focus ring (a key does); what it pressed has the keys.
        self.ring = false;
        if let Some(focus) = self.hit(p).and_then(Self::focus_of) {
            self.focus_on(focus);
        } else if self.over_detail(p) {
            self.focus_on(Focus::Detail);
        }
        // A press outside an open popover only closes it.
        if self.popover.is_some() && !matches!(self.hit(p), Some(Hit::PopoverArea | Hit::Menu(_))) {
            self.popover = None;
            return None;
        }
        // Its parts: the separator held, a scrollbar's thumb held or jumped to.
        if self.with_controls(|c, parts| c.press(parts, p, 1)).is_some() {
            return None;
        }
        match self.hit(p) {
            // A column's edge: held to resize it, double-clicked to fit it to its content.
            Some(Hit::Edge(column)) if self.clicks >= 2 => {
                self.resizing = None;
                self.fit_pending = Some(column);
            }
            Some(Hit::Edge(column)) => self.begin_resize(column, p.x),
            Some(Hit::Filter) => self.filter.click(p.x),
            Some(hit) => self.click_hit(hit),
            None => {}
        }
        self.action.take()
    }

    fn drag(&mut self, p: Point) -> bool {
        if self.resizing.is_some() {
            return self.resize_to(p.x);
        }
        let Some((part, changed)) = self.with_controls(|c, parts| c.drag(parts, p)) else {
            return false;
        };
        // The separator moved: the detail's share follows.
        if part == DIVIDER {
            let Some(center) = self.split_at.take() else { return false };
            self.drag_splitter(center);
            return true;
        }
        changed
    }

    fn release(&mut self) -> bool {
        if self.end_resize() {
            return true;
        }
        let Some((part, changed)) = self.with_controls(|c, parts| c.release(parts)) else {
            return false;
        };
        // The separator let go: the share it was left at is kept.
        if part == DIVIDER && changed {
            self.keep_detail_share();
        }
        changed
    }

    /// How its actions went (toasts).
    fn take_notifications(&mut self) -> Vec<workbench_model::Notification> {
        std::mem::take(&mut self.notifications)
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        // Only a part's resize across (the separator beside the detail): the one above a
        // detail below keeps the view's own cursor.
        let part = self.controls.cursor_at(p).filter(|c| *c == ViewerCursor::ResizeHorizontal);
        if self.resizing.is_some() || part.is_some() {
            return Some(ViewerCursor::ResizeHorizontal);
        }
        match self.hit(p) {
            Some(Hit::Edge(_)) => Some(ViewerCursor::ResizeHorizontal),
            Some(Hit::Filter) => Some(ViewerCursor::Text),
            Some(Hit::PopoverArea | Hit::CardChart(_) | Hit::DetailChart(_)) | None => None,
            Some(_) => Some(ViewerCursor::Pointer),
        }
    }

    /// Whether something clickable is under the mouse (the app redraws on every move).
    fn hover(&mut self, p: Point) -> bool {
        self.mouse = Some(p);
        self.with_controls(|c, parts| c.hover(parts, Some(p)));
        self.hover = self.hit(p);
        matches!(self.hover, Some(h) if !matches!(h, Hit::Filter | Hit::PopoverArea | Hit::CardChart(_) | Hit::DetailChart(_)))
            || self.controls.hovered() == Some(DIVIDER)
    }

    fn tooltip_at(&self, p: Point) -> Option<Tip> {
        let (anchor, hit) = *self.hits.iter().rev().find(|(r, _)| r.contains(p))?;
        let labels = &self.texts.labels;
        let label = match hit {
            Hit::FilterIcon => labels.filter_open.clone(),
            Hit::FilterClear => labels.filter_clear.clone(),
            Hit::Pause => {
                if self.paused {
                    labels.resume.clone()
                } else {
                    labels.pause.clone()
                }
            }
            Hit::Open(Popover::More) => labels.more.clone(),
            Hit::Open(Popover::Interval) => self.texts.status.clone(),
            Hit::Open(Popover::Sort) => labels.headings[2].clone(),
            Hit::Open(Popover::Columns) => labels.columns_tip.clone(),
            Hit::Header(column) => labels.tips[column.index()].clone(),
            Hit::Card(card) | Hit::CardValue(card) => {
                let tip = &self.texts.cards[card as usize].tip;
                if tip.is_empty() { return None } else { tip.clone() }
            }
            Hit::CardChart(card) => {
                let metric = metrics(card)[0];
                self.reading_tip(self.plots.cards[metric as usize].plot(), anchor, p.x, metric)?
            }
            Hit::DetailChart(metric) => {
                self.reading_tip(self.plots.detail[metric as usize].plot(), anchor, p.x, metric)?
            }
            Hit::Row(item) => {
                let (_, row) = self.cut.iter().find(|(r, i)| *i == item && r.contains(p))?;
                Self::row_name(&self.model, *row)?
            }
            Hit::CloseDetail => labels.close.clone(),
            Hit::Back => labels.back.clone(),
            Hit::Resume => labels.resume.clone(),
            Hit::Scope(_)
            | Hit::Grouping(_)
            | Hit::Filter
            | Hit::Group(_)
            | Hit::Expand(_)
            | Hit::ClearFilter
            | Hit::Menu(_)
            | Hit::Edge(_)
            | Hit::PopoverArea => return None,
            Hit::Action(action) => labels.actions[super::actions::kind_index(action.kind)].clone(),
        };
        let anchor = match hit {
            Hit::CardChart(_) | Hit::DetailChart(_) => {
                kernel::Rect::new(p.x - 1.0, anchor.min_y(), 2.0, anchor.size.height)
            }
            _ => anchor,
        };
        Some(Tip::new(anchor, label))
    }

    /// The status bar says it's paused (what's shown no longer moves).
    fn status(&self) -> Option<String> {
        self.paused.then(|| i18n::t("tasks.state_paused"))
    }

    /// A part moving (the separator's handle easing), as of the last paint.
    fn is_animating(&self) -> bool {
        self.controls.is_animating()
    }

    /// The first look not come yet: its loader turns.
    fn is_loading(&self) -> bool {
        self.subscription.is_some() && self.frame.lines.is_empty() && self.generation == 0
    }

    /// Its actions (the palette's, remappable; its menus' rows).
    fn command(&mut self, id: &str) -> bool {
        #[cfg(any(test, feature = "snapshot"))]
        if let Some(scenario) = id.strip_prefix("demo:") {
            self.use_demo(scenario);
            return true;
        }
        if id == crate::confirm::CONFIRMED {
            return self.confirmed();
        }
        // The filter has the keys: the app's editing commands are its (⌘A, ⌘Z, ⌘⇧Z, ⌘X).
        if self.with_controls(|c, parts| c.command(parts, id)).is_some() {
            self.filtered();
            return true;
        }
        self.run_command(id)
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        // The app's waker, once: the hub wakes it when a look is ready.
        if let (Some(subscription), Some(waker)) = (&self.subscription, &ctx.waker)
            && !self.waker_given
        {
            subscription.set_waker(Arc::clone(waker));
            self.waker_given = true;
        }
        self.focused = ctx.focused;
        self.draw(ctx.scene, ctx.text, ctx.icons, ctx.palette, ctx.bounds, ctx.scale);
        self.with_controls(|c, parts| c.settle(parts));
    }
}
