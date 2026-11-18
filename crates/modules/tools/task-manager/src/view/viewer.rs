//! The Task Manager as a tab's view: what it reacts to — the mouse (its rows, headers,
//! cards, toolbar, popovers, scrollbars and the detail's separator), the keys (typing into
//! the filter, the rest through [`super::keys`]), its right-click menus and commands
//! ([`super::commands`]), the wheel — and what it
//! draws each frame, and the tooltips under the mouse.

use std::sync::Arc;

use i18n::tr;
use kernel::Point;
use ui_components::tooltip::Tip;
use ui_components::{
    KeyReach, Orientation, Viewer, ViewerAction, ViewerCursor, ViewerKey, ViewerPaint,
};

use super::summary::metrics;
use super::texts::value;
use super::{Hit, Popover, TaskManager};
use crate::collector::Metric;
use crate::model::{Item, ViewModel};

impl TaskManager {
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
        self.focus == super::Focus::Tool(crate::layout::Tool::Filter)
    }

    /// A character typed while the filter hasn't the keys: it goes there.
    fn key(&mut self, c: char) -> bool {
        let mut buffer = [0; 4];
        self.text_input(c.encode_utf8(&mut buffer))
    }

    /// Typing goes into the filter (it has the keys then).
    fn text_input(&mut self, text: &str) -> bool {
        let text: String = text.chars().filter(|c| !c.is_control()).collect();
        if text.is_empty() {
            return false;
        }
        self.focus_filter();
        self.filter.insert(&text);
        self.filtered();
        true
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        self.key(key, KeyReach::default())
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        self.key(key, reach)
    }

    /// ⌘C: what's selected in the filter, else the keys' row (tab-separated).
    fn selected_text(&self) -> Option<String> {
        let typed = self.filter.selected_text();
        if !typed.is_empty() && self.focus == super::Focus::Tool(crate::layout::Tool::Filter) {
            return Some(typed.to_owned());
        }
        self.row_text()
    }

    fn context_menu(&mut self, p: Point) -> Option<Vec<ui_components::ViewMenuEntry>> {
        self.context_entries(p)
    }

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        if self.mouse.is_some_and(|p| self.over_detail(p)) {
            let before = self.detail_scroll;
            self.detail_scroll = (self.detail_scroll + dy).clamp(0.0, self.detail_max);
            return self.detail_scroll != before;
        }
        let before = self.scroll;
        self.scroll = (self.scroll + dy).clamp(0.0, self.max_scroll);
        self.scroll != before
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
            self.focus = focus;
        } else if self.over_detail(p) {
            self.focus = super::Focus::Detail;
        }
        // A press outside an open popover only closes it.
        if self.popover.is_some() && !matches!(self.hit(p), Some(Hit::PopoverArea | Hit::Menu(_))) {
            self.popover = None;
            return None;
        }
        if self.splitter.begin_drag(p) {
            return None;
        }
        if let Some(offset) = self.bar.press(p) {
            self.scroll = offset;
            return None;
        }
        if let Some(offset) = self.detail_bar.press(p) {
            self.detail_scroll = offset;
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
        if let Some(center) = self.splitter.drag_to(p) {
            self.drag_splitter(center);
            return true;
        }
        if let Some(offset) = self.bar.drag(p) {
            let changed = offset != self.scroll;
            self.scroll = offset;
            return changed;
        }
        if let Some(offset) = self.detail_bar.drag(p) {
            let changed = offset != self.detail_scroll;
            self.detail_scroll = offset;
            return changed;
        }
        false
    }

    fn release(&mut self) -> bool {
        if self.end_resize() {
            return true;
        }
        if self.splitter.end_drag() {
            self.keep_detail_share();
            return true;
        }
        self.bar.release() | self.detail_bar.release()
    }

    /// How its actions went (toasts).
    fn take_notifications(&mut self) -> Vec<workbench_model::Notification> {
        std::mem::take(&mut self.notifications)
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        if self.resizing.is_some()
            || (self.splitter.contains(p) && self.splitter.orientation() == Orientation::Vertical)
        {
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
        self.splitter.hover(p);
        self.bar.hover(p);
        self.detail_bar.hover(p);
        self.hover = self.hit(p);
        matches!(self.hover, Some(h) if !matches!(h, Hit::Filter | Hit::PopoverArea | Hit::CardChart(_) | Hit::DetailChart(_)))
            || self.splitter.is_hovered()
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

    fn is_animating(&self) -> bool {
        self.splitter.is_animating()
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
        if self.wants_text() {
            let edited = match id {
                "select_all" => {
                    self.filter.select_all();
                    return true;
                }
                "undo" => self.filter.undo(),
                "redo" => self.filter.redo(),
                "cut" => {
                    self.filter.insert("");
                    true
                }
                _ => false,
            };
            if edited {
                self.filtered();
                return true;
            }
            if matches!(id, "undo" | "redo") {
                return false;
            }
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
    }
}
