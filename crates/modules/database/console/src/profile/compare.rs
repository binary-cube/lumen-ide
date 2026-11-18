//! Two profiles of a console compared (plan 39h6, "Compare ▾" in the profile's bar): the
//! profile shown and another the console keeps — before and after an index, a rewrite, a
//! setting —, matched by [`database::profile::compare::diff`].
//!
//! - **The head:** which two (each one's time of day and duration), the statement's time
//!   before → after with its change (`845 ms → 12.4 ms`, `−98.5 %`), the planning, rows,
//!   buffers and temporary files the same way, and "plan shape changed" when a step changed,
//!   came or went. Better is blue with ▼ (an arrow down), worse orange with ▲ — never by the
//!   color alone (Okabe–Ito: told apart with any color vision).
//! - **The steps:** one tree of both plans aligned row by row: each step's time before and
//!   after side by side, its change, its rows and buffers before → after; a step changed
//!   says what it was (`Seq Scan → Index Scan`), one added or removed is marked in the gutter
//!   (its whole subtree with it).
//! - **The findings** (in the panel): resolved (before only), new (after only), still there.

use std::sync::Arc;

use database::plan::Measure;
use database::profile::compare::{Change, Delta, DiffRow, ProfileDiff, Trend, diff};
use database::profile::derive::ProfileReport;
use database::profile::node::NodeId;
use database::profile::rules::Finding;
use kernel::{Point, Rect, Rgba};
use render_api::Quad;
use text_layout::Font;
use ui_components::ds::{self, Ctx, Tone, radius};
use ui_components::{
    Ink, Level, Palette, Span, SpanStyle, TableCell, TableColumn, TablePaint, TableRow, Tip,
    TreeTable, ViewerKey,
};
use ui_kit::{draw_icon, draw_text};

use crate::profile::analysis;
use crate::profile::figures::{self, level};
use crate::profile::tree::{op_icon, operation_spans};

/// The head's height (logical pixels): its two lines.
pub const HEAD_H: f32 = 74.0;
/// From this width (logical pixels) the findings compared are beside the steps, this wide.
pub const WIDE: f32 = 1100.0;
/// The head's line of the findings compared, when they aren't beside (logical pixels).
pub const FINDINGS_LINE: f32 = 32.0;
pub const PANEL_W: f32 = 340.0;

/// A trend's color: better blue, worse orange (the severities' Info and Warning), even muted.
pub fn trend_ink(t: Trend) -> Option<Ink> {
    match t {
        Trend::Better => Some(Ink::Severity(Level::Info)),
        Trend::Worse => Some(Ink::Severity(Level::Warning)),
        Trend::Even => None,
    }
}

/// A trend's arrow (▼ better, ▲ worse: lower is better for what's compared here).
pub fn trend_icon(t: Trend) -> Option<&'static str> {
    match t {
        Trend::Better => Some("arrow-down"),
        Trend::Worse => Some("arrow-up"),
        Trend::Even => None,
    }
}

/// A change as written: `−98.5 %`, `+12 %`, `≈`.
pub fn change_text(d: &Delta) -> String {
    match d.ratio() {
        Some(r) if r.abs() >= database::profile::compare::NOISE => {
            let sign = if r < 0.0 { "−" } else { "+" };
            format!("{sign}{}", figures::share_exact(r.abs()))
        }
        Some(_) => "≈".to_owned(),
        None => match d.change() {
            Some(c) if c.abs() > f64::EPSILON => i18n::t("profile.compare.new_cost"),
            _ => "≈".to_owned(),
        },
    }
}

/// A side's label: `14:02:11 · 845 ms`.
pub fn side_label(at: &str, time_ms: Option<f64>) -> String {
    let clock = at.get(11..19).unwrap_or(at);
    match time_ms {
        Some(ms) if !clock.is_empty() => format!("{clock} · {}", figures::ms(ms)),
        Some(ms) => figures::ms(ms),
        None => clock.to_owned(),
    }
}

/// A column of the comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CCol {
    Step,
    Before,
    After,
    Change,
    Rows,
    Buffers,
}

impl CCol {
    fn key(self) -> &'static str {
        match self {
            CCol::Step => "step",
            CCol::Before => "before",
            CCol::After => "after",
            CCol::Change => "change",
            CCol::Rows => "rows",
            CCol::Buffers => "buffers",
        }
    }

    fn width(self) -> f32 {
        match self {
            CCol::Step => 0.0,
            CCol::Before | CCol::After => 92.0,
            CCol::Change => 96.0,
            CCol::Rows => 132.0,
            CCol::Buffers => 132.0,
        }
    }
}

/// The comparison's table: its columns, a row per row of the diff.
#[derive(Debug, Clone, Default)]
pub struct CompareTable {
    pub cols: Vec<CCol>,
    pub columns: Vec<TableColumn>,
    pub rows: Vec<TableRow>,
}

/// The comparison of `before` and `after` as a table (`diff` theirs).
pub fn table(before: &ProfileReport, after: &ProfileReport, d: &ProfileDiff) -> CompareTable {
    let time = d.measure == Some(Measure::Time);
    let mut cols = vec![CCol::Step, CCol::Before, CCol::After, CCol::Change];
    if d.rows.iter().any(|r| r.rows.is_known()) {
        cols.push(CCol::Rows);
    }
    if d.rows.iter().any(|r| r.buffers.is_known()) {
        cols.push(CCol::Buffers);
    }
    let columns = cols
        .iter()
        .map(|c| {
            let key = c.key();
            let title = if !time && matches!(c, CCol::Before | CCol::After) {
                i18n::t(&format!("profile.compare.col.{key}_cost"))
            } else {
                i18n::t(&format!("profile.compare.col.{key}"))
            };
            let column = TableColumn::new(title, c.width())
                .with_tip(i18n::t(&format!("profile.compare.col.{key}_tip")));
            if *c == CCol::Step { column.with_min(220.0) } else { column.right().with_min(56.0) }
        })
        .collect();
    let rows = d.rows.iter().map(|r| row(before, after, r, &cols, time)).collect();
    CompareTable { cols, columns, rows }
}

/// A row's weight: its time in all its loops, else its cost.
fn weight(r: &DiffRow, time: bool) -> Delta {
    if time { r.time } else { r.cost }
}

fn figure(v: f64, time: bool) -> String {
    if time { figures::ms(v) } else { figures::number(v.round()) }
}

fn row(
    before: &ProfileReport,
    after: &ProfileReport,
    r: &DiffRow,
    cols: &[CCol],
    time: bool,
) -> TableRow {
    let a = r.before.map(|id| before.profile.node(id));
    let b = r.after.map(|id| after.profile.node(id));
    let w = weight(r, time);
    let trend = if matches!(r.change, Change::Same | Change::Changed) {
        w.trend(true)
    } else {
        Trend::Even
    };
    let cells = cols
        .iter()
        .map(|c| match c {
            CCol::Step => {
                let mut spans: Vec<Span> = Vec::new();
                match (r.change, a, b) {
                    (Change::Changed, Some(a), Some(b)) => {
                        spans.push(Span::new(a.name.clone(), SpanStyle::Muted));
                        if let Some(i) = a.index.as_ref().filter(|i| Some(*i) != b.index.as_ref()) {
                            spans.push(Span::new(i.clone(), SpanStyle::Muted));
                        }
                        spans.push(Span::new("→", SpanStyle::Muted));
                        spans.extend(operation_spans(b));
                    }
                    (Change::Removed, Some(a), _) => {
                        spans.extend(operation_spans(a).into_iter().map(|s| match s.style {
                            SpanStyle::Pill(_) => s,
                            _ => Span::new(s.text, SpanStyle::Muted),
                        }));
                    }
                    (_, _, Some(b)) => spans.extend(operation_spans(b)),
                    (_, Some(a), None) => spans.extend(operation_spans(a)),
                    _ => {}
                }
                if r.change != Change::Same {
                    let ink = match r.change {
                        Change::Changed => Ink::Severity(Level::Info),
                        Change::Added => Ink::Tone(Tone::Accent),
                        _ => Ink::Tone(Tone::Neutral),
                    };
                    spans.push(Span::new(
                        i18n::t(&format!("profile.compare.change.{}", r.change.id())),
                        SpanStyle::Pill(ink),
                    ));
                }
                let tip = match (a, b) {
                    (Some(a), Some(b)) if r.change == Change::Changed => format!(
                        "{}\n→ {}",
                        database::profile::export::head(a),
                        database::profile::export::head(b)
                    ),
                    (_, Some(n)) | (Some(n), None) => database::profile::export::head(n),
                    _ => String::new(),
                };
                TableCell::spans(spans).with_tip(tip)
            }
            CCol::Before => match w.before {
                Some(v) => TableCell::spans(vec![Span::new(figure(v, time), SpanStyle::Muted)]),
                None => TableCell::empty(),
            },
            CCol::After => match w.after {
                Some(v) => TableCell::text(figure(v, time)),
                None => TableCell::empty(),
            },
            CCol::Change => {
                if !(w.before.is_some() && w.after.is_some()) {
                    return TableCell::empty();
                }
                let text = change_text(&w);
                let tip = delta_tip(&w, time);
                let cell = match trend_ink(trend) {
                    Some(ink) => TableCell::spans(vec![Span::new(text, SpanStyle::Ink(ink))]),
                    None => TableCell::spans(vec![Span::new(text, SpanStyle::Muted)]),
                };
                let cell = match (trend_icon(trend), trend_ink(trend)) {
                    (Some(icon), Some(ink)) => cell.with_lead(icon, ink),
                    _ => cell,
                };
                cell.with_tip(tip)
            }
            CCol::Rows => pair_cell(&r.rows, false, figures::count),
            CCol::Buffers => pair_cell(&r.buffers, true, figures::count),
        })
        .collect();
    let node = b.or(a);
    let mut out = TableRow::new(r.parent, cells).with_icon(node.map_or("plan", |n| op_icon(n.op)));
    match r.change {
        Change::Changed => {
            out = out.with_sign(
                "compare",
                Ink::Severity(Level::Info),
                i18n::t("profile.compare.change.changed_tip"),
            );
        }
        Change::Added => {
            out = out.with_sign(
                "plus",
                Ink::Tone(Tone::Accent),
                i18n::t("profile.compare.change.added_tip"),
            );
        }
        Change::Removed => {
            out = out.with_sign(
                "minus",
                Ink::Tone(Tone::Neutral),
                i18n::t("profile.compare.change.removed_tip"),
            );
        }
        Change::Same => {}
    }
    // A step changed much better or worse stands out.
    let big = r.change == Change::Changed && w.ratio().is_some_and(|x| x.abs() >= 0.5);
    if big {
        out = out.tinted(trend_ink(trend));
    }
    out
}

/// `before → after` (`trend`: lower is better, colored).
fn pair_cell(d: &Delta, trend: bool, write: impl Fn(f64) -> String) -> TableCell {
    match (d.before, d.after) {
        (Some(a), Some(b)) => {
            let t = if trend { d.trend(true) } else { Trend::Even };
            let style = match trend_ink(t) {
                Some(ink) => SpanStyle::Ink(ink),
                None => SpanStyle::Normal,
            };
            TableCell::spans(vec![
                Span::new(write(a), SpanStyle::Muted),
                Span::new("→", SpanStyle::Muted),
                Span::new(write(b), style),
            ])
            .with_tip(format!("{} → {} ({})", write(a), write(b), change_text(d)))
        }
        (Some(a), None) => TableCell::spans(vec![Span::new(write(a), SpanStyle::Muted)]),
        (None, Some(b)) => TableCell::text(write(b)),
        _ => TableCell::empty(),
    }
}

fn delta_tip(d: &Delta, time: bool) -> String {
    let (Some(a), Some(b)) = (d.before, d.after) else { return String::new() };
    let exact = |v: f64| if time { figures::ms_exact(v) } else { figures::number(v) };
    let change = b - a;
    let sign = if change < 0.0 { "−" } else { "+" };
    i18n::tr!(
        "profile.compare.delta_tip",
        before = exact(a).as_str(),
        after = exact(b).as_str(),
        change = format!("{sign}{}", exact(change.abs())).as_str(),
        share = change_text(d).as_str()
    )
}

/// A figure of the head: its label, before and after, lower is better, how it's written.
type Figure = (String, Delta, bool, fn(f64) -> String);

/// A section of the findings compared: its title's key, icon, findings (each with its index
/// in the profile shown, when it's one of its), color.
type Section<'a> = (&'static str, &'static str, Vec<(&'a Finding, Option<usize>)>, Option<Ink>);

/// What a press on the comparison asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareEvent {
    /// Leave the comparison.
    Close,
    /// Swap which is shown as before and after.
    Swap,
    /// A finding of the profile shown chosen (an index into its findings).
    Finding(usize),
    /// A step of the profile shown chosen.
    Step(NodeId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CHit {
    Close,
    Swap,
    Finding(usize),
    Info,
}

/// One side of the comparison: its report and its label.
#[derive(Debug, Clone)]
pub struct Side {
    pub report: Arc<ProfileReport>,
    pub label: String,
}

/// The comparison's view (see the module's doc).
#[derive(Debug)]
pub struct CompareView {
    pub before: Side,
    pub after: Side,
    /// The profile shown is the one after (its steps and findings are the view's).
    pub shown_after: bool,
    pub diff: ProfileDiff,
    pub table: TreeTable,
    pub built: CompareTable,
    hits: Vec<(Rect, CHit, Option<String>)>,
    hover: Option<CHit>,
    head: Rect,
    body: Rect,
    panel: Rect,
}

impl CompareView {
    /// `before` against `after` (`shown_after`: the view's own profile is `after`).
    pub fn new(before: Side, after: Side, shown_after: bool) -> Self {
        let d = diff(&before.report, &after.report);
        let built = table(&before.report, &after.report, &d);
        let mut t = TreeTable::new(built.columns.clone());
        t.set_rows(built.rows.clone(), |_| true);
        t.set_empty(i18n::t("profile.compare.empty"));
        Self {
            before,
            after,
            shown_after,
            diff: d,
            table: t,
            built,
            hits: Vec::new(),
            hover: None,
            head: Rect::default(),
            body: Rect::default(),
            panel: Rect::default(),
        }
    }

    /// The same two the other way round.
    pub fn swapped(self) -> Self {
        Self::new(self.after, self.before, !self.shown_after)
    }

    /// The statement's change in words: `845 ms → 12.4 ms · −98.5 %`.
    pub fn headline(&self) -> String {
        let time = self.diff.measure == Some(Measure::Time);
        let d = if time { self.diff.statement.time } else { self.diff.statement.cost };
        match (d.before, d.after) {
            (Some(a), Some(b)) => {
                format!("{} → {} · {}", figure(a, time), figure(b, time), change_text(&d))
            }
            _ => String::new(),
        }
    }

    /// The step of the view's own profile a row is.
    pub fn own_step(&self, row: usize) -> Option<NodeId> {
        let r = self.diff.rows.get(row)?;
        if self.shown_after { r.after } else { r.before }
    }

    /// The view's own findings of a kind: `new` those only after, else those resolved.
    fn own(&self) -> &ProfileReport {
        if self.shown_after { &self.after.report } else { &self.before.report }
    }

    pub fn paint(
        &mut self,
        whole: Rect,
        scale: f32,
        text: &mut text_layout::TextSystem,
        scene: &mut render_api::Scene,
        p: &Palette,
        icons: &icons::Icons,
    ) {
        let ds = Ctx::new(p, scale);
        let px = |v: f32| ds.px(v);
        self.hits.clear();
        // The findings beside when there's room for both.
        let panel_w = if whole.size.width >= px(WIDE) { px(PANEL_W).round() } else { 0.0 };
        let area =
            Rect::new(whole.min_x(), whole.min_y(), whole.size.width - panel_w, whole.size.height);
        let panel = Rect::new(
            area.max_x(),
            whole.min_y(),
            panel_w,
            if panel_w > 0.0 { whole.size.height } else { 0.0 },
        );
        let head = Rect::new(
            area.min_x(),
            area.min_y(),
            area.size.width,
            px(if panel_w > 0.0 { HEAD_H } else { HEAD_H + FINDINGS_LINE }).min(area.size.height),
        );
        let body = Rect::new(
            area.min_x(),
            head.max_y(),
            area.size.width,
            (area.max_y() - head.max_y()).max(0.0),
        );
        self.head = head;
        self.body = body;
        self.panel = panel;
        self.paint_head(head, &ds, text, scene, icons);
        self.table.paint(TablePaint { bounds: body, scale, text, scene, palette: p, icons });
        if panel.size.width > 0.0 {
            self.paint_findings(panel, &ds, text, scene, icons);
            let layer = scene.push_layer(Some(panel));
            ds::divider_vertical(layer, &ds, panel.min_x(), panel.min_y(), panel.max_y());
        }
    }

    fn paint_head(
        &mut self,
        r: Rect,
        ds: &Ctx<'_>,
        text: &mut text_layout::TextSystem,
        scene: &mut render_api::Scene,
        icons: &icons::Icons,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let layer = scene.push_layer(Some(r));
        layer.quad(Quad::filled(r, p.editor_background));
        layer.quad(Quad::filled(r, p.text.with_alpha(0.018)));
        ds::divider(layer, ds, r.min_x(), r.max_x(), r.max_y() - ds.line());
        let font = ds.ui_font(-1.0);
        let small = ds.ui_font(-1.5);
        let strong = ds.ui_font(-1.0).with_weight(Font::SEMIBOLD);
        // Line one: the two sides, the shape's badge, swap and close.
        let mid1 = (r.min_y() + px(19.0)).round();
        let mut x = r.min_x() + px(12.0);
        if let Some(id) = icons.id("compare") {
            let s = px(14.0);
            draw_icon(
                layer,
                icons,
                id,
                Rect::new(x.round(), (mid1 - s / 2.0).round(), s, s),
                p.text_muted,
            );
            x += s + px(8.0);
        }
        let title = i18n::t("profile.compare.title");
        let shaped = text.shape_line(&title, &strong);
        draw_text(
            layer,
            &shaped,
            x.round(),
            ds::optical_baseline(mid1 - px(11.0), px(22.0), &strong),
            p.text,
        );
        x += shaped.width + px(12.0);
        for (i, side) in [&self.before, &self.after].into_iter().enumerate() {
            let word =
                i18n::t(if i == 0 { "profile.compare.before" } else { "profile.compare.after" });
            let ws = text.shape_line(&word, &small);
            let ls = text.shape_line(&side.label, &font);
            let w = ws.width + ls.width + px(22.0);
            let chip = Rect::new(x.round(), (mid1 - px(11.0)).round(), w.round(), px(22.0));
            layer.quad(
                Quad::filled(chip, p.text.with_alpha(if i == 0 { 0.04 } else { 0.07 }))
                    .rounded(px(radius::SM))
                    .bordered(ds.line(), p.border),
            );
            let base = ds::optical_baseline(chip.min_y(), chip.size.height, &font);
            draw_text(layer, &ws, (chip.min_x() + px(8.0)).round(), base, p.text_muted);
            draw_text(layer, &ls, (chip.min_x() + px(14.0) + ws.width).round(), base, p.text);
            let own = (i == 1) == self.shown_after;
            let tip = if own {
                i18n::t("profile.compare.this_tip")
            } else {
                i18n::t("profile.compare.other_tip")
            };
            self.hits.push((chip, CHit::Info, Some(tip)));
            x = chip.max_x() + px(6.0);
            if i == 0 {
                if let Some(id) = icons.id("arrow-right") {
                    let s = px(12.0);
                    draw_icon(
                        layer,
                        icons,
                        id,
                        Rect::new(x.round(), (mid1 - s / 2.0).round(), s, s),
                        p.text_muted,
                    );
                }
                x += px(18.0);
            }
        }
        // The right: close, swap, the shape.
        let mut right = r.max_x() - px(8.0);
        for (hit, icon, tip) in [
            (CHit::Close, "x", i18n::t("profile.compare.close")),
            (CHit::Swap, "rotate-ccw", i18n::t("profile.compare.swap")),
        ] {
            let b = Rect::new(right - px(26.0), mid1 - px(12.0), px(26.0), px(24.0));
            crate::bar::icon_button(
                layer,
                icons,
                ds,
                b,
                icon,
                None,
                ui_components::ds::Press::new(true, self.hover == Some(hit)),
            );
            self.hits.push((b, hit, Some(tip)));
            right = b.min_x() - px(4.0);
        }
        let (shape, tone, tip) = if self.diff.shape_changed {
            let (_, changed, added, removed) = self.diff.counts();
            (
                i18n::t("profile.compare.shape_changed"),
                Tone::Warning,
                i18n::tr!(
                    "profile.compare.shape_tip",
                    changed = changed as i64,
                    added = added as i64,
                    removed = removed as i64
                ),
            )
        } else {
            (
                i18n::t("profile.compare.shape_same"),
                Tone::Neutral,
                i18n::t("profile.compare.shape_same_tip"),
            )
        };
        let bw = text.shape_line(&shape, &small).width + px(14.0);
        if right - bw > x + px(8.0) {
            let b = ds::badge(
                layer,
                text,
                ds,
                ds::BadgeSpec {
                    label: &shape,
                    x: right - px(4.0),
                    mid: mid1,
                    align_right: true,
                    tone,
                },
            );
            self.hits.push((b, CHit::Info, Some(tip)));
        }
        // Line two: the statement's time, then its other figures.
        let mid2 = (r.min_y() + px(52.0)).round();
        let mut x = r.min_x() + px(12.0);
        let time = self.diff.measure == Some(Measure::Time);
        let s = &self.diff.statement;
        let main = if time { s.time } else { s.cost };
        if let (Some(a), Some(b)) = (main.before, main.after) {
            let big = ds.ui_font(2.0).with_weight(Font::SEMIBOLD);
            let before = text.shape_line(&figure(a, time), &big);
            let base = ds::optical_baseline(mid2 - px(14.0), px(28.0), &big);
            draw_text(layer, &before, x.round(), base, p.text_muted);
            x += before.width + px(8.0);
            if let Some(id) = icons.id("arrow-right") {
                let sz = px(14.0);
                draw_icon(
                    layer,
                    icons,
                    id,
                    Rect::new(x.round(), (mid2 - sz / 2.0).round(), sz, sz),
                    p.text_muted,
                );
            }
            x += px(22.0);
            let after = text.shape_line(&figure(b, time), &big);
            draw_text(layer, &after, x.round(), base, p.text);
            x += after.width + px(10.0);
            let trend = main.trend(true);
            let change = change_text(&main);
            let label = match trend {
                Trend::Better => i18n::tr!("profile.compare.faster", change = change.as_str()),
                Trend::Worse => i18n::tr!("profile.compare.slower", change = change.as_str()),
                Trend::Even => i18n::t("profile.compare.even"),
            };
            let label = if time {
                label
            } else {
                format!("{label} · {}", i18n::t("profile.compare.cost_word"))
            };
            let ls = text.shape_line(&label, &strong);
            let w = ls.width + px(if trend == Trend::Even { 16.0 } else { 34.0 });
            let pill = Rect::new(x.round(), (mid2 - px(12.0)).round(), w.round(), px(24.0));
            let c = trend_ink(trend).map_or(p.text_muted, |i| i.color(p));
            layer.quad(
                Quad::filled(pill, c.with_alpha(0.13))
                    .rounded(px(12.0))
                    .bordered(ds.line(), c.with_alpha(0.5)),
            );
            let mut tx = pill.min_x() + px(8.0);
            if let Some(id) = trend_icon(trend).and_then(|i| icons.id(i)) {
                let sz = px(13.0);
                draw_icon(
                    layer,
                    icons,
                    id,
                    Rect::new(tx.round(), (mid2 - sz / 2.0).round(), sz, sz),
                    c,
                );
                tx += sz + px(5.0);
            }
            draw_text(
                layer,
                &ls,
                tx.round(),
                ds::optical_baseline(pill.min_y(), pill.size.height, &strong),
                c,
            );
            self.hits.push((pill, CHit::Info, Some(delta_tip(&main, time))));
            x = pill.max_x() + px(14.0);
        }
        // The other figures, as many as fit.
        let mut figs: Vec<Figure> = Vec::new();
        if time {
            figs.push((i18n::t("profile.view.pill.planning"), s.planning, true, figures::ms));
        }
        figs.push((i18n::t("profile.view.pill.rows"), s.rows, false, figures::count));
        let blocks = |v: f64| figures::count(v);
        figs.push((i18n::t("profile.compare.read"), s.buffers_read, true, blocks));
        figs.push((i18n::t("profile.compare.hit"), s.buffers_hit, true, blocks));
        figs.push((i18n::t("profile.view.pill.temp"), s.temp, true, blocks));
        for (label, d, lower, write) in figs {
            let (Some(a), Some(b)) = (d.before, d.after) else { continue };
            if a == 0.0 && b == 0.0 {
                continue;
            }
            let trend = if lower { d.trend(true) } else { Trend::Even };
            let ls = text.shape_line(&label, &small);
            let value = format!("{} → {}", write(a), write(b));
            let vs = text.shape_line(&value, &font);
            let icon_w = if trend == Trend::Even { 0.0 } else { px(16.0) };
            let w = ls.width + vs.width + icon_w + px(22.0);
            if x + w > r.max_x() - px(10.0) {
                break;
            }
            let chip = Rect::new(x.round(), (mid2 - px(11.0)).round(), w.round(), px(22.0));
            layer.quad(Quad::filled(chip, p.text.with_alpha(0.05)).rounded(px(radius::SM)));
            let base = ds::optical_baseline(chip.min_y(), chip.size.height, &font);
            draw_text(layer, &ls, (chip.min_x() + px(8.0)).round(), base, p.text_muted);
            let mut vx = chip.min_x() + px(14.0) + ls.width;
            let c = trend_ink(trend).map_or(p.text, |i| i.color(p));
            if let Some(id) = trend_icon(trend).and_then(|i| icons.id(i)) {
                let sz = px(11.0);
                draw_icon(
                    layer,
                    icons,
                    id,
                    Rect::new(vx.round(), (mid2 - sz / 2.0).round(), sz, sz),
                    c,
                );
                vx += px(16.0);
            }
            draw_text(layer, &vs, vx.round(), base, c);
            self.hits.push((
                chip,
                CHit::Info,
                Some(format!("{label}: {value} ({})", change_text(&d))),
            ));
            x = chip.max_x() + px(6.0);
        }
        // Line three, when the findings aren't beside: each kind's count, its titles in the
        // tooltip.
        if r.size.height >= px(HEAD_H + FINDINGS_LINE) - 1.0 {
            self.paint_findings_line(r, ds, text, layer, icons);
        }
    }

    /// The findings compared on one line of the head: resolved, new, still there.
    fn paint_findings_line(
        &mut self,
        r: Rect,
        ds: &Ctx<'_>,
        text: &mut text_layout::TextSystem,
        layer: &mut render_api::Layer,
        icons: &icons::Icons,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let mid = (r.min_y() + px(HEAD_H + FINDINGS_LINE / 2.0 - 6.0)).round();
        let font = ds.ui_font(-1.0);
        let small = ds.ui_font(-1.5);
        let label = i18n::t("profile.compare.findings");
        let ls = text.shape_line(&label, &small);
        let mut x = r.min_x() + px(12.0);
        draw_text(
            layer,
            &ls,
            x.round(),
            ds::optical_baseline(mid - px(11.0), px(22.0), &small),
            p.text_muted,
        );
        x += ls.width + px(10.0);
        let f = &self.diff.findings;
        let titles = |report: &ProfileReport, list: &[usize]| -> String {
            list.iter()
                .filter_map(|i| report.findings.get(*i))
                .map(|f| {
                    format!("{} · {}", analysis::severity(f.severity), analysis::texts(f).title)
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let kept: Vec<usize> = f.kept.iter().map(|(_, b)| *b).collect();
        let groups = [
            (
                "profile.compare.resolved",
                "check",
                f.resolved.len(),
                Some(Ink::Severity(Level::Info)),
                titles(&self.before.report, &f.resolved),
            ),
            (
                "profile.compare.new",
                "alert",
                f.new.len(),
                Some(Ink::Severity(Level::Warning)),
                titles(&self.after.report, &f.new),
            ),
            ("profile.compare.kept", "minus", kept.len(), None, titles(&self.after.report, &kept)),
        ];
        for (key, icon, count, ink, tip) in groups {
            let c = ink.map_or(p.text_muted, |i| i.color(p));
            let label = i18n::tr!(key, count = count as i64);
            let shaped = text.shape_line(&label, &font);
            let w = shaped.width + px(32.0);
            if x + w > r.max_x() - px(10.0) {
                break;
            }
            let chip = Rect::new(x.round(), (mid - px(11.0)).round(), w.round(), px(22.0));
            let alpha = if count > 0 { 0.12 } else { 0.04 };
            layer.quad(Quad::filled(chip, c.with_alpha(alpha)).rounded(px(11.0)));
            if let Some(id) = icons.id(icon) {
                let sz = px(12.0);
                draw_icon(
                    layer,
                    icons,
                    id,
                    Rect::new((chip.min_x() + px(8.0)).round(), (mid - sz / 2.0).round(), sz, sz),
                    c,
                );
            }
            draw_text(
                layer,
                &shaped,
                (chip.min_x() + px(24.0)).round(),
                ds::optical_baseline(chip.min_y(), chip.size.height, &font),
                if count > 0 { c } else { p.text_muted },
            );
            let tip = if tip.is_empty() { i18n::t("profile.compare.none") } else { tip };
            self.hits.push((chip, CHit::Info, Some(tip)));
            x = chip.max_x() + px(6.0);
        }
    }

    fn paint_findings(
        &mut self,
        area: Rect,
        ds: &Ctx<'_>,
        text: &mut text_layout::TextSystem,
        scene: &mut render_api::Scene,
        icons: &icons::Icons,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let layer = scene.push_layer(Some(area));
        layer.quad(Quad::filled(area, p.editor_background));
        let head = Rect::new(
            area.min_x(),
            area.min_y(),
            area.size.width,
            px(ui_components::tree_table::HEADER_H),
        );
        layer.quad(Quad::filled(head, p.text.with_alpha(0.025)));
        ds::divider(layer, ds, head.min_x(), head.max_x(), head.max_y() - ds.line());
        let head_font = ds.ui_font(-0.5).with_weight(Font::SEMIBOLD);
        let font = ds.ui_font(-1.0);
        let small = ds.ui_font(-1.5);
        if let Some(id) = icons.id("lightbulb") {
            let s = px(13.0);
            let mid = head.min_y() + head.size.height / 2.0;
            draw_icon(
                layer,
                icons,
                id,
                Rect::new((head.min_x() + px(12.0)).round(), (mid - s / 2.0).round(), s, s),
                p.text_muted,
            );
        }
        let title = i18n::t("profile.compare.findings");
        let shaped = text.shape_line(&title, &head_font);
        draw_text(
            layer,
            &shaped,
            (head.min_x() + px(32.0)).round(),
            ds::optical_baseline(head.min_y(), head.size.height, &head_font),
            p.text,
        );
        let f = &self.diff.findings;
        let before = Arc::clone(&self.before.report);
        let after = Arc::clone(&self.after.report);
        let sections: [Section<'_>; 3] = [
            (
                "profile.compare.resolved",
                "check",
                f.resolved
                    .iter()
                    .filter_map(|i| {
                        before.findings.get(*i).map(|x| (x, (!self.shown_after).then_some(*i)))
                    })
                    .collect(),
                Some(Ink::Severity(Level::Info)),
            ),
            (
                "profile.compare.new",
                "alert",
                f.new
                    .iter()
                    .filter_map(|i| {
                        after.findings.get(*i).map(|x| (x, self.shown_after.then_some(*i)))
                    })
                    .collect(),
                Some(Ink::Severity(Level::Warning)),
            ),
            (
                "profile.compare.kept",
                "minus",
                f.kept
                    .iter()
                    .filter_map(|(a, b)| {
                        let own = if self.shown_after { *b } else { *a };
                        after.findings.get(*b).map(|x| (x, Some(own)))
                    })
                    .collect(),
                None,
            ),
        ];
        let mut y = head.max_y() + px(6.0);
        let row_h = px(26.0);
        for (key, icon, list, ink) in sections {
            if y + row_h > area.max_y() {
                break;
            }
            let color = ink.map_or(p.text_muted, |i| i.color(p));
            let label = i18n::tr!(key, count = list.len() as i64);
            let ls = text.shape_line(&label, &small.clone().with_weight(Font::SEMIBOLD));
            if let Some(id) = icons.id(icon) {
                let s = px(12.0);
                draw_icon(
                    layer,
                    icons,
                    id,
                    Rect::new(
                        (area.min_x() + px(12.0)).round(),
                        (y + (row_h - s) / 2.0).round(),
                        s,
                        s,
                    ),
                    color,
                );
            }
            draw_text(
                layer,
                &ls,
                (area.min_x() + px(30.0)).round(),
                ds::optical_baseline(y, row_h, &small),
                color,
            );
            y += row_h;
            if list.is_empty() {
                let none = text.shape_line(&i18n::t("profile.compare.none"), &small);
                draw_text(
                    layer,
                    &none,
                    (area.min_x() + px(30.0)).round(),
                    ds::optical_baseline(y - px(4.0), row_h, &small),
                    p.text_muted,
                );
                y += row_h - px(4.0);
            }
            for (finding, own) in list {
                if y + row_h > area.max_y() {
                    break;
                }
                let row = Rect::new(area.min_x() + px(6.0), y, area.size.width - px(12.0), row_h);
                if let Some(i) = own
                    && self.hover == Some(CHit::Finding(i))
                {
                    layer.quad(Quad::filled(row, p.tree_hover).rounded(px(radius::SM)));
                }
                let l = level(finding.severity);
                if let Some(id) = icons.id(l.icon()) {
                    let s = px(12.0);
                    draw_icon(
                        layer,
                        icons,
                        id,
                        Rect::new(
                            (row.min_x() + px(24.0)).round(),
                            (y + (row_h - s) / 2.0).round(),
                            s,
                            s,
                        ),
                        l.color(p),
                    );
                }
                let t = analysis::texts(finding);
                let room = row.size.width - px(52.0);
                let shown = ds::ellipsize(text, &t.title, &font, room);
                let ts = text.shape_line(&shown, &font);
                let tc: Rgba = if ink.is_none() { p.text_muted } else { p.text };
                draw_text(
                    layer,
                    &ts,
                    (row.min_x() + px(44.0)).round(),
                    ds::optical_baseline(y, row_h, &font),
                    tc,
                );
                let tip =
                    format!("{} · {}\n{}", analysis::severity(finding.severity), t.title, t.why);
                let hit = own.map_or(CHit::Info, CHit::Finding);
                self.hits.push((row, hit, Some(tip)));
                y += row_h;
            }
            y += px(6.0);
        }
    }

    fn hit_at(&self, p: Point) -> Option<CHit> {
        self.hits.iter().rev().find(|(r, ..)| r.contains(p)).map(|(_, h, _)| *h)
    }

    /// A press: what it asked for (a row chosen gives its step of the view's profile).
    pub fn press(&mut self, p: Point, clicks: u32) -> Option<CompareEvent> {
        match self.hit_at(p) {
            Some(CHit::Close) => return Some(CompareEvent::Close),
            Some(CHit::Swap) => return Some(CompareEvent::Swap),
            Some(CHit::Finding(i)) => return Some(CompareEvent::Finding(i)),
            Some(CHit::Info) => return None,
            None => {}
        }
        if self.body.contains(p) {
            self.table.press(p, clicks);
            return self.table.selected().and_then(|r| self.own_step(r)).map(CompareEvent::Step);
        }
        None
    }

    pub fn hover(&mut self, p: Point) -> bool {
        let hit = self.hit_at(p);
        let changed = hit != self.hover;
        self.hover = hit;
        let body = if self.body.contains(p) { self.table.hover(p) } else { self.table.leave() };
        changed || body
    }

    pub fn leave(&mut self) -> bool {
        let changed = self.hover.is_some();
        self.hover = None;
        self.table.leave() || changed
    }

    pub fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        self.table.scroll(dx, dy)
    }

    pub fn key(&mut self, key: ViewerKey) -> bool {
        self.table.key(key)
    }

    pub fn tooltip_at(&self, p: Point) -> Option<Tip> {
        if let Some((r, _, tip)) = self.hits.iter().rev().find(|(r, ..)| r.contains(p)) {
            return Some(Tip::new(*r, tip.clone()?));
        }
        if self.body.contains(p) {
            return self.table.tooltip_at(p);
        }
        None
    }

    pub fn is_over_button(&self, p: Point) -> bool {
        matches!(self.hit_at(p), Some(CHit::Close | CHit::Swap | CHit::Finding(_)))
    }

    /// The comparison as text (copied): its headline, then each row.
    pub fn text(&mut self) -> String {
        format!("{}\n{}", self.headline(), self.table.text())
    }

    /// The findings of the view's own profile the comparison knows (for its tests).
    pub fn own_findings(&self) -> usize {
        self.own().findings.len()
    }
}

#[cfg(test)]
#[path = "../../tests/unit/profile/compare.rs"]
mod tests;
