//! The view's popovers, under what opened them: the toolbar's "…" (what didn't fit), the
//! interval (how often it looks, how far back its graphs go), "Sort by…" (the lists) and
//! the columns' chooser (each column checked when shown; one left for want of room says
//! so, one kept says so; "By priority" forgets the choices). A press outside or Esc closes
//! one; a column chosen keeps the chooser open (several are chosen in a row).

use kernel::Rect;
use render_api::{Quad, Scene};
use text_layout::{Font, TextSystem};
use ui_components::columns::Choice;
use ui_components::ds::{self, Ctx, Elevation, radius};
use ui_kit::draw_text;

use super::toolbar::groupings;
use super::{Hit, MenuAction, Popover, TaskManager, column_of};
use crate::layout::{ColumnId, PageLayout, Tool, present};
use crate::model::{Column, Grouping};
use crate::settings::{HISTORIES, INTERVALS};

/// The sorting columns offered by "Sort by…".
const SORTS: [Column; 7] = [
    Column::Name,
    Column::Cpu,
    Column::Memory,
    Column::Net,
    Column::Disk,
    Column::Gpu,
    Column::Threads,
];

impl TaskManager {
    /// The open popover's rows (into its reused list).
    pub(super) fn menu_rows(&mut self, popover: Popover, page: &PageLayout) {
        let mut rows = std::mem::take(&mut self.menu);
        rows.clear();
        let timing = |rows: &mut Vec<MenuAction>| {
            rows.push(MenuAction::Heading(0));
            rows.extend(INTERVALS.map(MenuAction::Interval));
            rows.push(MenuAction::Heading(1));
            rows.extend(HISTORIES.map(MenuAction::History));
        };
        match popover {
            Popover::More => {
                for tool in &page.tools.collapsed {
                    match tool {
                        Tool::Scope => {
                            rows.extend([MenuAction::Scope(false), MenuAction::Scope(true)])
                        }
                        Tool::Grouping => rows
                            .extend(groupings(self.all).iter().map(|g| MenuAction::Grouping(*g))),
                        Tool::Pause => rows.push(MenuAction::Pause),
                        Tool::Interval => timing(&mut rows),
                        Tool::Filter | Tool::Sort => {}
                    }
                }
            }
            Popover::Interval => timing(&mut rows),
            Popover::Sort => {
                rows.push(MenuAction::Heading(2));
                let gpu = self.frame.gpu.is_some();
                rows.extend(
                    SORTS
                        .iter()
                        .filter(|c| gpu || **c != Column::Gpu)
                        .map(|c| MenuAction::Sort(*c)),
                );
            }
            Popover::Columns => {
                rows.push(MenuAction::Heading(3));
                let spec = self.spec(page.bounds, 1.0);
                rows.extend(
                    present(&spec, page.class)
                        .filter(|c| *c != ColumnId::Name)
                        .map(MenuAction::Column),
                );
                rows.push(MenuAction::ResetColumns);
            }
        }
        self.menu = rows;
    }

    /// A row's label, whether it's checked (`None`: not a choice) and its note.
    fn menu_label(
        &self,
        action: MenuAction,
        page: &PageLayout,
    ) -> (&str, Option<bool>, Option<&str>) {
        let labels = &self.texts.labels;
        let preferences = self.preferences;
        match action {
            MenuAction::Heading(i) => (&labels.headings[i], None, None),
            MenuAction::Scope(all) => {
                (&labels.scope[usize::from(all)], Some(self.all == all), None)
            }
            MenuAction::Grouping(g) => {
                let i = match g {
                    Grouping::Kind => 0,
                    Grouping::Project => 1,
                    Grouping::List => 2,
                };
                (&labels.grouping[i], Some(self.state.grouping.on(&self.frame) == g), None)
            }
            MenuAction::Pause => {
                (if self.paused { &labels.resume } else { &labels.pause }, None, None)
            }
            MenuAction::Interval(s) => {
                let i = INTERVALS.iter().position(|v| *v == s).unwrap_or(0);
                (&labels.intervals[i], Some(preferences.interval == s), None)
            }
            MenuAction::History(s) => {
                let i = HISTORIES.iter().position(|v| *v == s).unwrap_or(0);
                (&labels.histories[i], Some(preferences.history == s), None)
            }
            MenuAction::Sort(c) => {
                (labels.column(column_of(c)), Some(self.state.sort.0 == c), None)
            }
            MenuAction::Column(c) => {
                let choice = self.choices[c.index()];
                let dropped = page.columns.dropped.contains(&c);
                let shown = choice != Choice::Hidden && !dropped;
                let note = if choice == Choice::Pinned {
                    Some(labels.pinned.as_str())
                } else if dropped {
                    Some(labels.no_room.as_str())
                } else {
                    None
                };
                (labels.column(c), Some(shown), note)
            }
            MenuAction::ResetColumns => (&labels.columns_reset, None, None),
        }
    }

    pub(super) fn paint_popover(
        &mut self,
        scene: &mut Scene,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        page: &PageLayout,
    ) {
        let Some((popover, anchor)) = self.popover else { return };
        let anchor = if anchor.size.width > 0.0 {
            anchor
        } else {
            match popover {
                Popover::Columns => page.chooser.unwrap_or(page.toolbar),
                _ => page.toolbar,
            }
        };
        self.menu_rows(popover, page);
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let font = ds.ui_font(-0.5);
        let heading_font = ds.ui_font(-2.0).with_weight(Font::SEMIBOLD);
        let note_font = ds.ui_font(-2.0);
        let (row_h, heading_h) = (px(30.0), px(26.0));
        let mut width = px(200.0);
        let mut height = px(12.0);
        for i in 0..self.menu.len() {
            let action = self.menu[i];
            let (label, _, note) = self.menu_label(action, page);
            let heading = matches!(action, MenuAction::Heading(_));
            let f = if heading { &heading_font } else { &font };
            let note_w = note.map_or(0.0, |n| text.shape_line(n, &note_font).width + px(12.0));
            width = width.max(text.shape_line(label, f).width + note_w + px(56.0));
            height += if heading { heading_h } else { row_h };
        }
        let bounds = page.bounds;
        let width = width.min(bounds.size.width - px(16.0));
        let gap = px(6.0);
        let x = (anchor.max_x() - width).clamp(
            bounds.min_x() + px(8.0),
            (bounds.max_x() - width - px(8.0)).max(bounds.min_x()),
        );
        let below = anchor.max_y() + gap;
        let height = height.min(bounds.max_y() - below - px(8.0)).max(row_h);
        let rect = Rect::new(x.round(), below.round(), width.round(), height.round());
        let layer = scene.push_layer(None);
        ds::surface(layer, ds, rect, Elevation::Medium, radius::MD);
        self.hits.push((rect, Hit::PopoverArea));
        let layer = scene.push_layer(Some(rect));
        let mut y = rect.min_y() + px(6.0);
        for i in 0..self.menu.len() {
            let action = self.menu[i];
            if let MenuAction::Heading(_) = action {
                let (label, ..) = self.menu_label(action, page);
                let base = ds::optical_baseline(y + px(4.0), heading_h - px(4.0), &heading_font);
                draw_text(
                    layer,
                    &text.shape_line(label, &heading_font),
                    rect.min_x() + px(14.0),
                    base,
                    p.text_muted,
                );
                y += heading_h;
                continue;
            }
            let row = Rect::new(rect.min_x() + px(4.0), y, rect.size.width - px(8.0), row_h);
            let hit = Hit::Menu(action);
            self.hits.push((row, hit));
            let (label, checked, note) = self.menu_label(action, page);
            if self.hover == Some(hit) || self.menu_at == Some(i) {
                layer.quad(Quad::filled(row, p.hover_background).rounded(px(radius::SM)));
            }
            if checked == Some(true)
                && let Some(id) = icons.id("check")
            {
                let s = px(14.0);
                ui_kit::draw_icon(
                    layer,
                    icons,
                    id,
                    Rect::new(row.min_x() + px(8.0), row.min_y() + (row_h - s) / 2.0, s, s),
                    p.accent,
                );
            }
            let base = ds::optical_baseline(row.min_y(), row_h, &font);
            let note_w = note.map_or(0.0, |n| text.shape_line(n, &note_font).width + px(10.0));
            let room = (row.size.width - px(40.0) - note_w).max(0.0);
            ds::draw_ellipsized(
                layer,
                text,
                label,
                &font,
                (row.min_x() + px(30.0), base),
                room,
                p.text,
            );
            if let Some(note) = note {
                let nb = ds::optical_baseline(row.min_y(), row_h, &note_font);
                super::table::right_text(
                    layer,
                    text,
                    note,
                    &note_font,
                    row.max_x() - px(10.0),
                    nb,
                    p.text_muted,
                );
            }
            y += row_h;
        }
    }
}
