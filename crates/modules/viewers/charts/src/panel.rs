//! The series settings beside a chart and the thin bar over it, drawn from the shared
//! components (buttons, dropdown fields, checkboxes), with what each part does when pressed.
//! The kinds a chart can be, its fields (X, groups, values, how they're summed) and the
//! options that mean something for the kind chosen.

use charts_model::{Aggregate, ChartKind, ChartSpec, SeriesSpec};
use kernel::Rect;
use render_api::{Layer, Quad};
use text_layout::{Font, TextSystem};
use ui_components::ds::{self, ButtonKind, ButtonSpec, Check, Ctx, DropdownSpec};
use ui_kit::draw_text;

/// The panel's width (logical pixels).
pub(crate) const PANEL_W: f32 = 290.0;
/// The bar over the chart (logical pixels).
pub(crate) const BAR_H: f32 = 30.0;
/// The room around the panel's parts (logical pixels).
const PAD: f32 = 14.0;
/// A field's row (logical pixels).
const ROW_H: f32 = 34.0;
/// An option's row (logical pixels).
const OPTION_H: f32 = 28.0;
/// The labels' column before the fields (logical pixels).
const LABEL_W: f32 = 84.0;
/// A kind's button, the panel's and the bar's (logical pixels).
const KIND: f32 = ds::button_size::MEDIUM;

/// A field of the series.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Field {
    X,
    Group,
    Y,
    Aggregate,
}

/// An option of the chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Opt {
    Stepped,
    Points,
    Percent,
    Log,
}

/// What a press on the panel or the bar does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PanelHit {
    Kind(ChartKind),
    Field(Field),
    Opt(Opt),
    /// The panel closed.
    Close,
    /// The panel shown or hidden (the bar's ⚙).
    Settings,
    /// Every row of the result fetched.
    FetchAll,
    /// Drawn from all the rows again (not the grid's selection).
    WholeRows,
    /// The chart saved as an image (PNG or SVG).
    Export,
    /// Series `i` chosen to set.
    Series(usize),
    /// A series added (and chosen).
    AddSeries,
    /// The series chosen removed.
    RemoveSeries,
}

/// The most series a chart holds (past it, groups split them).
pub(crate) const MAX_SERIES: usize = 8;

/// A kind draws several series of its own (a pie and a histogram draw the first).
pub(crate) fn many_series(kind: ChartKind) -> bool {
    !matches!(kind, ChartKind::Pie | ChartKind::Histogram)
}

/// A kind's icon (the module's own).
pub(crate) fn kind_icon(kind: ChartKind) -> &'static str {
    match kind {
        ChartKind::Bar => "chart-bar",
        ChartKind::Pie => "chart-pie",
        ChartKind::Area => "chart-area",
        ChartKind::Line => "chart-line",
        ChartKind::Scatter => "chart-scatter",
        ChartKind::Bubble => "chart-bubble",
        ChartKind::StackedBar => "chart-stacked-bar",
        ChartKind::StackedArea => "chart-stacked-area",
        ChartKind::Histogram => "chart-histogram",
    }
}

/// A kind's name (its button's tooltip).
pub(crate) fn kind_name(kind: ChartKind) -> String {
    i18n::t(match kind {
        ChartKind::Bar => "charts.kind_bar",
        ChartKind::Pie => "charts.kind_pie",
        ChartKind::Area => "charts.kind_area",
        ChartKind::Line => "charts.kind_line",
        ChartKind::Scatter => "charts.kind_scatter",
        ChartKind::Bubble => "charts.kind_bubble",
        ChartKind::StackedBar => "charts.kind_stacked_bar",
        ChartKind::StackedArea => "charts.kind_stacked_area",
        ChartKind::Histogram => "charts.kind_histogram",
    })
}

/// An aggregate's name.
pub(crate) fn aggregate_name(how: Aggregate) -> String {
    i18n::t(match how {
        Aggregate::Sum => "charts.sum",
        Aggregate::Count => "charts.count",
        Aggregate::Average => "charts.average",
        Aggregate::Min => "charts.min",
        Aggregate::Max => "charts.max",
    })
}

/// The aggregates offered, in order.
pub(crate) const AGGREGATES: [Aggregate; 5] =
    [Aggregate::Sum, Aggregate::Count, Aggregate::Average, Aggregate::Min, Aggregate::Max];

/// The fields a kind uses (a histogram: only its values; a pie: no groups).
pub(crate) fn fields_for(kind: ChartKind) -> &'static [Field] {
    match kind {
        ChartKind::Histogram => &[Field::Y],
        ChartKind::Pie => &[Field::X, Field::Y, Field::Aggregate],
        ChartKind::Scatter | ChartKind::Bubble => &[Field::X, Field::Group, Field::Y],
        _ => &[Field::X, Field::Group, Field::Y, Field::Aggregate],
    }
}

/// The options that mean something for a kind.
pub(crate) fn options_for(kind: ChartKind) -> &'static [Opt] {
    match kind {
        ChartKind::Line | ChartKind::Area => &[Opt::Stepped, Opt::Points, Opt::Log],
        ChartKind::StackedArea => &[Opt::Stepped, Opt::Points, Opt::Percent],
        ChartKind::StackedBar => &[Opt::Percent],
        ChartKind::Bar | ChartKind::Scatter | ChartKind::Bubble => &[Opt::Log],
        ChartKind::Pie | ChartKind::Histogram => &[],
    }
}

fn field_label(field: Field) -> String {
    i18n::t(match field {
        Field::X => "charts.x_axis",
        Field::Group => "charts.groups",
        Field::Y => "charts.y_axis",
        Field::Aggregate => "charts.aggregate",
    })
}

fn option_label(opt: Opt) -> String {
    i18n::t(match opt {
        Opt::Stepped => "charts.stepped",
        Opt::Points => "charts.points",
        Opt::Percent => "charts.percent",
        Opt::Log => "charts.log",
    })
}

/// An option's state in `spec`.
pub(crate) fn option_on(spec: &ChartSpec, opt: Opt) -> bool {
    let o = spec.options;
    match opt {
        Opt::Stepped => o.stepped,
        Opt::Points => o.points,
        Opt::Percent => o.percent,
        Opt::Log => o.log,
    }
}

/// What a field shows (empty: nothing chosen).
fn field_value(series: &SeriesSpec, field: Field) -> String {
    match field {
        Field::X => series.x.clone().unwrap_or_default(),
        Field::Group => series.group.clone().unwrap_or_default(),
        Field::Y => series.y.join(", "),
        Field::Aggregate => aggregate_name(series.aggregate),
    }
}

/// What the panel and the bar draw with.
pub(crate) struct Pen<'a> {
    pub layer: &'a mut Layer,
    pub text: &'a mut TextSystem,
    pub icons: &'a icons::Icons,
    pub ctx: &'a Ctx<'a>,
    pub hover: Option<PanelHit>,
    pub hits: &'a mut Vec<(Rect, PanelHit)>,
}

/// The series settings in `rect`: its title and ×, the kinds, the fields, the options.
/// Returns each field's box (a list opens under it).
pub(crate) fn paint_panel(
    pen: &mut Pen<'_>,
    rect: Rect,
    (spec, current): (&ChartSpec, usize),
    choosing: Option<Field>,
) -> Vec<(Field, Rect)> {
    let ctx = pen.ctx;
    let p = ctx.palette;
    let px = |v: f32| ctx.px(v);
    pen.layer.quad(Quad::filled(rect, ctx.fade(p.sidebar_background)));
    ds::divider_vertical(pen.layer, ctx, rect.min_x(), rect.min_y(), rect.max_y());
    let (x0, w) = (rect.min_x() + px(PAD), rect.size.width - 2.0 * px(PAD));
    let mut y = rect.min_y() + px(PAD) / 2.0;
    // The title and its ×.
    let title_font = ctx.ui_font(0.0).with_weight(Font::SEMIBOLD);
    let title = pen.text.shape_line(&i18n::t("charts.series"), &title_font);
    let row = px(ROW_H);
    draw_text(pen.layer, &title, x0, ds::optical_baseline(y, row, &title_font), p.text);
    let side = px(KIND);
    let close = Rect::new(rect.max_x() - px(PAD) / 2.0 - side, y + (row - side) / 2.0, side, side);
    let spec_close = ButtonSpec::new(ButtonKind::Ghost)
        .icon("close")
        .hovered(pen.hover == Some(PanelHit::Close));
    ds::button(pen.layer, pen.text, pen.icons, ctx, close, spec_close);
    pen.hits.push((close, PanelHit::Close));
    y += row;
    // The kinds, a button each.
    let gap =
        ((w - side * ChartKind::ALL.len() as f32) / (ChartKind::ALL.len() - 1) as f32).max(px(2.0));
    for (i, kind) in ChartKind::ALL.iter().copied().enumerate() {
        let r = Rect::new(x0 + i as f32 * (side + gap), y, side, side);
        let spec_kind = ButtonSpec::new(ButtonKind::Ghost)
            .icon(kind_icon(kind))
            .on(spec.kind == kind)
            .hovered(pen.hover == Some(PanelHit::Kind(kind)));
        ds::button(pen.layer, pen.text, pen.icons, ctx, r, spec_kind);
        pen.hits.push((r, PanelHit::Kind(kind)));
    }
    y += side + px(10.0);
    ds::divider(pen.layer, ctx, x0, x0 + w, y.round());
    y += px(8.0);
    let label_font = ctx.ui_font(-0.5);
    // The series: a button each, one more to add, the chosen one's removal.
    if many_series(spec.kind) {
        let label = pen.text.shape_line(&i18n::t("charts.series_one"), &label_font);
        draw_text(pen.layer, &label, x0, ds::optical_baseline(y, row, &label_font), p.text_muted);
        let mut x = x0 + px(LABEL_W);
        let top = y + (row - side) / 2.0;
        for i in 0..spec.series.len() {
            let r = Rect::new(x, top, side, side);
            let number = (i + 1).to_string();
            let chip = ButtonSpec::new(ButtonKind::Ghost)
                .label(&number)
                .on(i == current)
                .hovered(pen.hover == Some(PanelHit::Series(i)));
            ds::button(pen.layer, pen.text, pen.icons, ctx, r, chip);
            pen.hits.push((r, PanelHit::Series(i)));
            x += side + px(2.0);
        }
        if spec.series.len() < MAX_SERIES {
            let r = Rect::new(x, top, side, side);
            let add = ButtonSpec::new(ButtonKind::Ghost)
                .icon("plus")
                .hovered(pen.hover == Some(PanelHit::AddSeries));
            ds::button(pen.layer, pen.text, pen.icons, ctx, r, add);
            pen.hits.push((r, PanelHit::AddSeries));
        }
        if spec.series.len() > 1 {
            let r = Rect::new(x0 + w - side, top, side, side);
            let remove = ButtonSpec::new(ButtonKind::Ghost)
                .icon("trash")
                .tone(ds::Tone::Danger)
                .hovered(pen.hover == Some(PanelHit::RemoveSeries));
            ds::button(pen.layer, pen.text, pen.icons, ctx, r, remove);
            pen.hits.push((r, PanelHit::RemoveSeries));
        }
        y += row;
    }
    // The fields: a label, then the shared dropdown field.
    let series = spec.series.get(current).cloned().unwrap_or_default();
    let mut boxes = Vec::new();
    let placeholder = i18n::t("charts.add_column");
    for &field in fields_for(spec.kind) {
        let label = pen.text.shape_line(&field_label(field), &label_font);
        draw_text(pen.layer, &label, x0, ds::optical_baseline(y, row, &label_font), p.text_muted);
        let h = px(26.0);
        let r = Rect::new(x0 + px(LABEL_W), y + (row - h) / 2.0, w - px(LABEL_W), h);
        let value = field_value(&series, field);
        let none = matches!(field, Field::X | Field::Group) && value.is_empty();
        let shown = if none && field == Field::X { i18n::t("charts.rows_in_order") } else { value };
        let field_spec = DropdownSpec {
            value: &shown,
            open: choosing == Some(field),
            hovered: pen.hover == Some(PanelHit::Field(field)),
            enabled: true,
            note: None,
            placeholder: Some(&placeholder),
        };
        ds::dropdown(pen.layer, pen.text, pen.icons, ctx, r, field_spec);
        pen.hits.push((r, PanelHit::Field(field)));
        boxes.push((field, r));
        y += row;
    }
    // The options for its kind.
    let options = options_for(spec.kind);
    if !options.is_empty() {
        y += px(4.0);
        ds::divider(pen.layer, ctx, x0, x0 + w, y.round());
        y += px(6.0);
    }
    let option_h = px(OPTION_H);
    for &opt in options {
        let mid = y + option_h / 2.0;
        let state = if option_on(spec, opt) { Check::On } else { Check::Off };
        let boxed = ds::checkbox(pen.layer, pen.text, ctx, x0, mid, state, true);
        let label = pen.text.shape_line(&option_label(opt), &label_font);
        let lx = boxed.max_x() + px(8.0);
        draw_text(pen.layer, &label, lx, ds::optical_baseline(y, option_h, &label_font), p.text);
        pen.hits.push((Rect::new(x0, y, lx + label.width - x0, option_h), PanelHit::Opt(opt)));
        y += option_h;
    }
    // At its foot, as the shared link-like button: one more series.
    if many_series(spec.kind) && spec.series.len() < MAX_SERIES {
        let label = i18n::t("charts.add_new_series");
        let h = px(ds::button_size::LARGE);
        let add = ButtonSpec::new(ButtonKind::Ghost)
            .label(&label)
            .tone(ds::Tone::Accent)
            .hovered(pen.hover == Some(PanelHit::AddSeries));
        let w = ds::button_width(pen.text, ctx, &add, h);
        let r =
            Rect::new(rect.min_x() + (rect.size.width - w) / 2.0, rect.max_y() - px(PAD) - h, w, h);
        if r.min_y() > y + px(4.0) {
            ds::button(pen.layer, pen.text, pen.icons, ctx, r, add);
            pen.hits.push((r, PanelHit::AddSeries));
        }
    }
    boxes
}

/// The bar over the chart: what it's drawn from (how many rows; all of them fetched on a
/// press), the settings' button.
/// The bar's spinner while a chart is prepared, and the room after it (logical pixels).
const BAR_SPINNER: f32 = 14.0;
const BAR_SPINNER_GAP: f32 = 8.0;

pub(crate) fn paint_bar(
    pen: &mut Pen<'_>,
    rect: Rect,
    (note, working): (&str, bool),
    fetch: Option<(&str, PanelHit)>,
    (settings_on, can_export): (bool, bool),
) {
    let ctx = pen.ctx;
    let px = |v: f32| ctx.px(v);
    let font = ctx.ui_font(-1.0);
    let shaped = pen.text.shape_line(note, &font);
    let mut x = rect.min_x() + px(10.0);
    // Preparing (on a worker): the shared spinner before what it says.
    if working {
        let mid = rect.min_y() + rect.size.height / 2.0;
        let loader = ds::Loader { size: BAR_SPINNER, ..ds::Loader::default() };
        ds::loader_spinner(pen.layer, ctx, (x, mid), &loader);
        x += px(BAR_SPINNER + BAR_SPINNER_GAP);
    }
    draw_text(
        pen.layer,
        &shaped,
        x,
        ds::optical_baseline(rect.min_y(), rect.size.height, &font),
        ctx.palette.text_muted,
    );
    let side = px(KIND);
    let mid = rect.min_y() + rect.size.height / 2.0;
    let gear = Rect::new(rect.max_x() - px(6.0) - side, (mid - side / 2.0).round(), side, side);
    let spec = ButtonSpec::new(ButtonKind::Ghost)
        .icon("settings")
        .on(settings_on)
        .hovered(pen.hover == Some(PanelHit::Settings));
    ds::button(pen.layer, pen.text, pen.icons, ctx, gear, spec);
    pen.hits.push((gear, PanelHit::Settings));
    let mut right = gear.min_x() - px(2.0);
    if can_export {
        let save = Rect::new(right - side, gear.min_y(), side, side);
        let spec = ButtonSpec::new(ButtonKind::Ghost)
            .icon("download")
            .hovered(pen.hover == Some(PanelHit::Export));
        ds::button(pen.layer, pen.text, pen.icons, ctx, save, spec);
        pen.hits.push((save, PanelHit::Export));
        right = save.min_x() - px(2.0);
    }
    if let Some((label, hit)) = fetch {
        let h = px(ds::button_size::SMALL);
        let spec =
            ButtonSpec::new(ButtonKind::Soft).round().label(label).hovered(pen.hover == Some(hit));
        let w = ds::button_width(pen.text, ctx, &spec, h);
        let left = (x + shaped.width + px(10.0)).max(rect.min_x());
        if left + w < right - px(6.0) {
            let r = Rect::new(left, (mid - h / 2.0).round(), w, h);
            ds::button(pen.layer, pen.text, pen.icons, ctx, r, spec);
            pen.hits.push((r, hit));
        }
    }
    ds::divider(pen.layer, ctx, rect.min_x(), rect.max_x(), rect.max_y() - ctx.line());
}
