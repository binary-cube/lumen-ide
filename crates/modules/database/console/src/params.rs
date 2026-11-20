//! A run's parameters (`?`, `?1`, `$1`, `:name`, `@name`, `$name`), asked before anything is
//! sent: a modal of the module's own (`view-console-params`, `host.open_modal`: closed only
//! by its buttons, × or Esc — never by a click outside) listing each parameter with its value,
//! its kind (text, a number, `NULL`, or an expression written as typed) and "remember"; the
//! statements above them, the places of the parameter being edited lit. ↵ runs, Esc gives up.
//!
//! The values are written into the statement by the source's dialect
//! ([`database::console::bind_statements`]: a text quoted safely, a number as it is, an
//! expression raw), so what runs is exactly what the Output shows, on every engine; the map
//! kept with each statement takes an engine's error back to the user's own text. What's
//! remembered stays with the console's runner while the project is open — never on disk,
//! never in the history (the console keeps only the parameters' names).
//!
//! The command that asks leaves what to do with the values here ([`ask`]); the modal's main
//! button closes it and runs [`GIVEN`], whose handler does it with the host. Closed any other
//! way, nothing is done. The bar's `(p)` opens the same modal to edit the values kept
//! ([`Purpose::Edit`]: saved, nothing runs).

use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;

use database::SourceColor;
use database::Value;
use database::sql::{ParamName, ParamValue};

use i18n::t;
use kernel::{Point, Rect};
use render_api::Quad;
use ui_components::code_tokens::{self, CodeLine};
use ui_components::control::{Control, Controls, Press, Thumb};
use ui_components::ds::{self, Ctx, radius};
use ui_components::{
    EdgeScrollbar, KeyReach, Tip, Viewer, ViewerAction, ViewerCursor, ViewerKey, ViewerKind,
    ViewerPaint, ViewerProvider,
};
use ui_kit::{TextInput, TextInputStyle, draw_icon, draw_text};
use workbench_model::WorkbenchHost;

use crate::answer_slot::Once;
use crate::field::KeyField;

/// The modal's virtual extension (its view's kind).
pub const EXTENSION: &str = "view-console-params";

/// How a value is written into the statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValueKind {
    /// A string literal, quoted by the dialect.
    Text,
    /// A number, as typed (checked to be one).
    Number,
    /// `NULL` (no value typed).
    Null,
    /// Written as typed: an expression (`now()`, `ARRAY[1, 2]`).
    Raw,
}

impl ValueKind {
    pub const ALL: [ValueKind; 4] = [Self::Text, Self::Number, Self::Null, Self::Raw];

    pub fn label(self) -> String {
        t(match self {
            Self::Text => "console.param_text",
            Self::Number => "console.param_number",
            Self::Null => "console.param_null",
            Self::Raw => "console.param_raw",
        })
    }
}

/// A parameter's value kept for its console (while the project is open, in memory only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remembered {
    pub name: ParamName,
    pub kind: ValueKind,
    pub text: String,
}

/// Why a value can't be written in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// A number that isn't one.
    NotANumber,
    /// An expression left empty.
    EmptyExpression,
}

/// A parameter as the modal edits it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamEntry {
    pub name: ParamName,
    /// As written in the statement (`:status`, `$1`, `?`).
    pub label: String,
    pub uses: usize,
    pub kind: ValueKind,
    pub text: String,
    /// Its kind was chosen (else it follows the value: a number typed is a number).
    pub kind_chosen: bool,
    /// Kept for the console's next runs.
    pub remember: bool,
}

impl ParamEntry {
    /// A parameter asked for, with what its console kept (else empty text, remembered).
    pub fn new(asked: &database::console::AskedParam, kept: Option<&Remembered>) -> Self {
        Self {
            name: asked.name.clone(),
            label: asked.label.clone(),
            uses: asked.uses,
            kind: kept.map_or(ValueKind::Text, |k| k.kind),
            text: kept.map(|k| k.text.clone()).unwrap_or_default(),
            kind_chosen: kept.is_some(),
            remember: true,
        }
    }

    /// The kind the value is written as: the one chosen, else a number when it reads as one.
    pub fn effective_kind(&self) -> ValueKind {
        if self.kind_chosen || self.kind != ValueKind::Text {
            return self.kind;
        }
        if number(&self.text).is_some() { ValueKind::Number } else { ValueKind::Text }
    }

    /// Why its value can't be written in, if it can't.
    pub fn problem(&self) -> Option<Problem> {
        match self.effective_kind() {
            ValueKind::Number if number(&self.text).is_none() => Some(Problem::NotANumber),
            ValueKind::Raw if self.text.trim().is_empty() => Some(Problem::EmptyExpression),
            _ => None,
        }
    }

    /// The value written in (`None`: it has a problem).
    pub fn value(&self) -> Option<ParamValue> {
        if self.problem().is_some() {
            return None;
        }
        Some(match self.effective_kind() {
            ValueKind::Text => ParamValue::Literal(Value::Text(self.text.clone())),
            ValueKind::Number => match number(&self.text)? {
                Number::Int(i) => ParamValue::Literal(Value::Int(i)),
                Number::Float(f) => ParamValue::Literal(Value::Float(f)),
            },
            ValueKind::Null => ParamValue::Literal(Value::Null),
            ValueKind::Raw => ParamValue::Raw(self.text.trim().to_owned()),
        })
    }
}

/// The values of `entries`, by name (`None`: one has a problem).
pub fn values(entries: &[ParamEntry]) -> Option<HashMap<ParamName, ParamValue>> {
    entries.iter().map(|e| Some((e.name.clone(), e.value()?))).collect()
}

enum Number {
    Int(i64),
    Float(f64),
}

/// `text` as a number (an integer, else a decimal: `12`, `-3`, `2.5`, `1e3`).
fn number(text: &str) -> Option<Number> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    if let Ok(i) = t.parse::<i64>() {
        return Some(Number::Int(i));
    }
    t.parse::<f64>().ok().filter(|f| f.is_finite()).map(Number::Float)
}

/// What the parameters are asked for (the main button's word).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Run,
    Explain,
    /// The bar's `(p)`: the values kept edited, nothing run.
    Edit,
}

/// What the modal shows.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamsRequest {
    /// Set by [`ask`]: what its values do.
    pub then: Once<OnValues>,
    pub purpose: Purpose,
    pub console: String,
    pub source: String,
    pub color: Option<SourceColor>,
    pub production: bool,
    /// The statements, as written.
    pub statements: Vec<String>,
    /// Where each parameter is written: the statement (its index), the bytes in it.
    pub places: Vec<(usize, Range<usize>, ParamName)>,
    pub entries: Vec<ParamEntry>,
}

impl ParamsRequest {
    pub fn new(purpose: Purpose, console: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            then: Once::default(),
            purpose,
            console: console.into(),
            source: source.into(),
            color: None,
            production: false,
            statements: Vec::new(),
            places: Vec::new(),
            entries: Vec::new(),
        }
    }

    /// The main button's label.
    pub fn go_label(&self) -> String {
        t(match self.purpose {
            Purpose::Run => "console.params_run",
            Purpose::Explain => "console.params_explain",
            Purpose::Edit => "console.params_save",
        })
    }
}

/// What to do with the values given, with the host.
pub type OnValues = Box<dyn FnOnce(&mut dyn WorkbenchHost, Vec<ParamEntry>) + Send>;

/// Asks `request`'s values in the modal, `then` done with them. `false`: the host has no
/// modals (nothing is done).
pub fn ask(host: &mut dyn WorkbenchHost, mut request: ParamsRequest, then: OnValues) -> bool {
    request.then = Once::new(then);
    host.open_modal(&t("console.params_title"), EXTENSION, &request)
}

/// The modal's provider (its virtual extension).
#[derive(Debug, Default)]
pub struct ParamsProvider;

impl ViewerProvider for ParamsProvider {
    fn id(&self) -> &'static str {
        EXTENSION
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        (path.extension().and_then(|e| e.to_str()) == Some(EXTENSION))
            .then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(ParamsView::default()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Kind(usize, ValueKind),
    Remember(usize),
    Go,
    Cancel,
}

/// The modal's view.
#[derive(Default)]
pub struct ParamsView {
    request: Option<ParamsRequest>,
    inputs: Vec<TextInput>,
    /// Where each value's field was drawn last (empty: not drawn, scrolled out of view).
    fields: Vec<Rect>,
    /// Given: the window closes (and the values are used).
    done: bool,
    /// The main button was pressed with a problem left: it's shown.
    tried: bool,
    scroll: f32,
    max_scroll: f32,
    rows_area: Rect,
    bar: EdgeScrollbar,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
    action: Option<ViewerAction>,
    /// Its parts' state of use (see [`Self::with_controls`]).
    controls: Controls,
    /// The row whose value has the keys (while the values have the focus).
    row: usize,
    /// Set when a field's text changed (the fields say it; nothing here follows it).
    edited: bool,
}

impl std::fmt::Debug for ParamsView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParamsView").field("request", &self.request).finish_non_exhaustive()
    }
}

/// The rows shown at most before the list scrolls.
pub const ROWS: usize = 6;
/// A parameter's row (logical pixels).
const ROW_H: f32 = 46.0;
/// The source's row, the header of the list, the footer (logical pixels).
const SOURCE_H: f32 = 42.0;
const HEAD_H: f32 = 26.0;
const FOOTER_H: f32 = 64.0;
/// The statements' box: its lines, their height (logical pixels).
pub const CODE_LINES: std::ops::RangeInclusive<usize> = 1..=8;
const CODE_ROW: f32 = 21.0;

impl ParamsView {
    /// The entries as edited (their texts from the fields).
    pub fn entries(&self) -> Vec<ParamEntry> {
        let Some(request) = &self.request else { return Vec::new() };
        request
            .entries
            .iter()
            .zip(&self.inputs)
            .map(|(e, input)| ParamEntry { text: input.text().to_owned(), ..e.clone() })
            .collect()
    }

    /// The value's row having the keys.
    pub fn focus(&self) -> Option<usize> {
        (self.controls.focused() == Some(VALUES)).then_some(self.row)
    }

    /// Row `i`'s value given the keys.
    fn focus_row(&mut self, i: usize) {
        self.row = i;
        self.with_controls(|c, parts| c.focus(parts, Some(VALUES)));
    }

    /// Tab (⇧Tab `back`): the next row's value given the keys, around the ends; the first
    /// (the last going back) when none has them.
    fn step_focus(&mut self, back: bool) {
        let n = self.inputs.len();
        if n == 0 {
            return;
        }
        let next = match (self.focus(), back) {
            (None, false) => 0,
            (None, true) => n - 1,
            (Some(at), false) => (at + 1) % n,
            (Some(at), true) => (at + n - 1) % n,
        };
        self.focus_row(next);
    }

    /// Entry `i`'s value typed (as if by the keyboard: the field's text replaced).
    pub fn set_text(&mut self, i: usize, text: &str) {
        if let Some(input) = self.inputs.get_mut(i) {
            input.set_text(text);
        }
    }

    /// Entry `i`'s kind chosen.
    pub fn set_kind(&mut self, i: usize, kind: ValueKind) {
        if let Some(e) = self.request.as_mut().and_then(|r| r.entries.get_mut(i)) {
            e.kind = kind;
            e.kind_chosen = true;
        }
        if kind != ValueKind::Null {
            self.focus_row(i);
        }
    }

    pub fn toggle_remember(&mut self, i: usize) {
        if let Some(e) = self.request.as_mut().and_then(|r| r.entries.get_mut(i)) {
            e.remember = !e.remember;
        }
    }

    /// The first entry whose value can't be written in, and why.
    pub fn problem(&self) -> Option<(usize, Problem)> {
        self.entries().iter().enumerate().find_map(|(i, e)| e.problem().map(|p| (i, p)))
    }

    /// The values given (when they can all be written in): the window closes, they're used.
    pub fn give(&mut self) -> bool {
        if let Some((i, _)) = self.problem() {
            self.tried = true;
            self.focus_row(i);
            return false;
        }
        let Some(then) = self.request.as_ref().map(|r| r.then.clone()) else { return false };
        let entries = self.entries();
        self.done = true;
        self.action = Some(ViewerAction::WithHost(ui_components::WithHost::once(move |host| {
            if let Some(then) = then.take() {
                then(host, entries);
            }
        })));
        true
    }

    /// Closed without values (Cancel, Esc).
    pub fn cancel(&mut self) {
        // What the answer would have done goes now, not with the last copy of the question.
        if let Some(r) = &self.request {
            drop(r.then.take());
        }
        self.action = Some(ViewerAction::Close);
    }

    /// Its parts (the rows' scrollbar, then the values: one part, each row's field in it),
    /// with their state of use; a fixed list, nothing made for it at each event. The focus
    /// moves among the rows with Tab and ↑ ↓ (a `NULL` row's too: it takes no text) and
    /// stays where it was on a press beside them.
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let entries = self.request.as_ref().map_or(&[][..], |r| &r.entries[..]);
        let mut values = Values {
            inputs: &mut self.inputs,
            fields: &self.fields,
            entries,
            row: &mut self.row,
            edited: &mut self.edited,
        };
        let mut rows = Thumb { bar: &mut self.bar, offset: &mut self.scroll };
        let mut parts: [&mut dyn Control; 2] = [&mut rows, &mut values];
        f(&mut self.controls, &mut parts)
    }

    fn hit(&self, p: Point) -> Option<Hit> {
        self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)
    }

    /// The focused row takes a value (it isn't `NULL`).
    fn focused_value(&self) -> bool {
        let Some(i) = self.focus() else { return false };
        self.request
            .as_ref()
            .and_then(|r| r.entries.get(i))
            .is_some_and(|e| e.kind != ValueKind::Null)
    }

    fn press(&mut self, hit: Hit) {
        match hit {
            Hit::Kind(i, kind) => self.set_kind(i, kind),
            Hit::Remember(i) => self.toggle_remember(i),
            Hit::Go => {
                self.give();
            }
            Hit::Cancel => self.cancel(),
        }
    }

    fn style(ds: &Ctx<'_>, enabled: bool, wrong: bool) -> TextInputStyle {
        let p = ds.palette;
        TextInputStyle {
            font: ds.code_font(-0.5),
            background: if enabled { p.input_background } else { p.text.with_alpha(0.03) },
            border: if wrong { p.danger } else { p.input_border },
            border_focused: if wrong { p.danger } else { p.accent },
            text: if enabled { p.text } else { p.text_muted },
            placeholder: p.text_muted,
            selection: p.selection,
            caret: p.caret,
            radius: ds.px(radius::MD),
            border_width: ds.line(),
            padding: ds.px(10.0),
            caret_width: ds.px(1.5).max(1.0),
            lead: 0.0,
        }
    }
}

/// The statements' lines as the box shows them (an empty line between two statements),
/// each with its statement and where its text starts in it.
fn code_lines(request: &ParamsRequest) -> Vec<(&str, Option<(usize, usize)>)> {
    let mut out = Vec::new();
    for (i, s) in request.statements.iter().enumerate() {
        if i > 0 {
            out.push(("", None));
        }
        let mut at = 0;
        for line in s.split('\n') {
            out.push((line.trim_end_matches('\r'), Some((i, at))));
            at += line.len() + 1;
        }
    }
    out
}

/// The statements' box height (logical pixels): its lines within [`CODE_LINES`].
pub fn code_box_height(request: &ParamsRequest) -> f32 {
    let lines = code_lines(request).len().clamp(*CODE_LINES.start(), *CODE_LINES.end());
    lines as f32 * CODE_ROW + 20.0
}

/// The modal's content height (logical pixels), as [`ParamsView`]'s paint lays it out.
pub fn content_height(request: &ParamsRequest) -> f32 {
    let rows = request.entries.len().clamp(1, ROWS) as f32 * ROW_H;
    // The problem's line under the list, when there is one (room kept for it).
    SOURCE_H + code_box_height(request) + 14.0 + HEAD_H + rows + 30.0 + FOOTER_H
}

impl Viewer for ParamsView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        if let Some(request) = data.downcast_ref::<ParamsRequest>() {
            self.inputs = request.entries.iter().map(|e| TextInput::new(&e.text)).collect();
            self.fields = vec![Rect::default(); request.entries.len()];
            // The first value to type: the first left empty, else the first.
            let first = request
                .entries
                .iter()
                .position(|e| e.text.is_empty() && e.kind != ValueKind::Null)
                .or((!request.entries.is_empty()).then_some(0));
            if let Some(input) = first.and_then(|i| self.inputs.get_mut(i)) {
                input.select_all();
            }
            self.request = Some(request.clone());
            if let Some(i) = first {
                self.focus_row(i);
            }
        }
    }

    fn subtitle(&self) -> Option<String> {
        let r = self.request.as_ref()?;
        Some(i18n::tr!(
            "console.confirm_subtitle",
            console = r.console.as_str(),
            source = r.source.as_str()
        ))
    }

    fn modal_height(&self) -> Option<f32> {
        self.request.as_ref().map(content_height)
    }

    fn modal_icon(&self) -> Option<&'static str> {
        Some("parameter")
    }

    fn key_hints(&self) -> Vec<(Vec<&'static str>, String)> {
        let label = self.request.as_ref().map(ParamsRequest::go_label).unwrap_or_default();
        vec![(vec!["tab"], t("console.params_next")), (vec!["↵"], label)]
    }

    fn closed(&self) -> bool {
        self.done
    }

    fn wants_text(&self) -> bool {
        self.focus().is_some()
    }

    fn text_input(&mut self, typed: &str) -> bool {
        self.with_controls(|c, parts| c.text(parts, typed)).is_some()
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        let ViewerPaint { bounds: b, scale, text, scene, palette: p, icons, .. } = ctx;
        let ds = Ctx::new(p, scale);
        let px = |v: f32| ds.px(v);
        self.hits.clear();
        let Some(request) = self.request.clone() else { return };
        let entries = self.entries();
        let pad = px(20.0);
        let layer = scene.push_layer(Some(b));
        // The source: its color, its name, its production label.
        let mut y = b.min_y() + px(6.0);
        let font = ds.ui_font(0.0).with_weight(text_layout::Font::SEMIBOLD);
        let mid = y + px(14.0);
        let mut x = b.min_x() + pad;
        let color = request.color.and_then(|c| ui_components::tag_colors::named(&c.name(), p));
        if let Some(icon) = icons.id("database") {
            let s = px(16.0);
            let r = Rect::new(x.round(), (mid - s / 2.0).round(), s, s);
            draw_icon(layer, icons, icon, r, color.unwrap_or(p.text_muted));
            x += s + px(8.0);
        }
        let shaped = text.shape_line(&request.source, &font);
        draw_text(
            layer,
            &shaped,
            x.round(),
            ds::optical_baseline(mid - px(10.0), px(20.0), &font),
            p.text,
        );
        x += shaped.width + px(10.0);
        if request.production {
            let label = t("console.production");
            ds::badge(
                layer,
                text,
                &ds,
                ds::BadgeSpec { label: &label, x, mid, align_right: false, tone: ds::Tone::Danger },
            );
        }
        // How many, at the right.
        let count = i18n::tr!("console.params_count", count = request.entries.len());
        let small = ds.ui_font(-1.0);
        let shaped = text.shape_line(&count, &small);
        draw_text(
            layer,
            &shaped,
            (b.max_x() - pad - shaped.width).round(),
            ds::optical_baseline(mid - px(10.0), px(20.0), &small),
            p.text_muted,
        );
        y += px(SOURCE_H - 6.0);
        // The statements, the focused parameter's places lit.
        let card =
            Rect::new(b.min_x() + pad, y, b.size.width - 2.0 * pad, px(code_box_height(&request)));
        layer.quad(
            Quad::filled(card, p.editor_background)
                .rounded(px(radius::MD))
                .bordered(ds.line(), p.border),
        );
        let code_font = ds.code_font(-1.0);
        let row_h = px(CODE_ROW);
        let inner = Rect::new(
            card.min_x() + px(14.0),
            card.min_y() + px(10.0),
            card.size.width - px(28.0),
            card.size.height - px(20.0),
        );
        let lines = code_lines(&request);
        let focus = self.focus();
        let focused_name = focus.and_then(|i| request.entries.get(i)).map(|e| e.name.clone());
        let metrics = text.metrics(&code_font);
        {
            let code = scene.push_layer(Some(inner));
            // The first line with the focused parameter in view (the box shows a few).
            let first_lit = focused_name.as_ref().and_then(|name| {
                lines.iter().position(|(line, at)| {
                    at.is_some_and(|(s, start)| {
                        request.places.iter().any(|(ps, r, n)| {
                            *ps == s
                                && n == name
                                && r.start >= start
                                && r.start < start + line.len().max(1)
                        })
                    })
                })
            });
            let visible = *CODE_LINES.end();
            let from = first_lit.map_or(0, |l| l.saturating_sub(visible.saturating_sub(1)));
            for (k, (line, at)) in lines.iter().enumerate().skip(from).take(visible) {
                let top = inner.min_y() + (k - from) as f32 * row_h;
                let baseline = ui_kit::centered_baseline(top, row_h, &metrics);
                // The places of the parameter being edited, under the text.
                if let (Some(name), Some((s, start))) = (&focused_name, at) {
                    for (ps, r, n) in &request.places {
                        let end = start + line.len();
                        if ps != s || n != name || r.start < *start || r.start > end {
                            continue;
                        }
                        let (a, z) = (r.start - start, (r.end - start).min(line.len()));
                        let x0 = text.shape_line(&line[..a], &code_font).width;
                        let x1 = text.shape_line(&line[..z], &code_font).width;
                        let lit = Rect::new(
                            (inner.min_x() + x0 - px(2.0)).round(),
                            top + px(2.0),
                            (x1 - x0 + px(4.0)).round(),
                            row_h - px(4.0),
                        );
                        code.quad(
                            Quad::filled(lit, p.accent.with_alpha(0.18))
                                .rounded(px(4.0))
                                .bordered(ds.line(), p.accent.with_alpha(0.55)),
                        );
                    }
                }
                code_tokens::draw_code(
                    code,
                    text,
                    CodeLine {
                        source: line,
                        extension: "sql",
                        font: &code_font,
                        x: inner.min_x(),
                        baseline,
                        palette: p,
                        max_x: inner.max_x(),
                    },
                );
            }
        }
        let layer = scene.push_layer(Some(b));
        y = card.max_y() + px(14.0);
        // The list's header.
        let name_w = px(150.0);
        let kinds: Vec<String> = ValueKind::ALL.iter().map(|k| k.label()).collect();
        let kind_labels: Vec<&str> = kinds.iter().map(String::as_str).collect();
        let kinds_w = ds::segmented_width(text, &ds, &kind_labels);
        let remember_w = px(96.0);
        let left = b.min_x() + pad;
        let right = b.max_x() - pad;
        let value_x = left + name_w;
        let kinds_x = right - remember_w - px(12.0) - kinds_w;
        let value_w = (kinds_x - px(12.0) - value_x).max(px(80.0));
        let head = ds.ui_font(-1.5).with_weight(text_layout::Font::SEMIBOLD);
        for (label, hx) in [
            (t("console.params_name"), left),
            (t("console.params_value"), value_x),
            (t("console.params_kind"), kinds_x),
            (t("console.params_remember"), right - remember_w + px(4.0)),
        ] {
            let shaped = text.shape_line(&label, &head);
            draw_text(
                layer,
                &shaped,
                hx.round(),
                ds::optical_baseline(y, px(HEAD_H), &head),
                p.text_muted,
            );
        }
        y += px(HEAD_H);
        ds::divider(layer, &ds, left, right, y - ds.line());
        // The rows (a few, then they scroll).
        let shown = request.entries.len().clamp(1, ROWS);
        let area = Rect::new(b.min_x(), y, b.size.width, shown as f32 * px(ROW_H));
        self.rows_area = area;
        // Only the rows drawn now can be pressed.
        self.fields.iter_mut().for_each(|r| *r = Rect::default());
        let content = request.entries.len() as f32 * px(ROW_H);
        self.max_scroll = (content - area.size.height).max(0.0);
        // The focused row kept in view.
        if let Some(i) = focus {
            let top = i as f32 * px(ROW_H);
            if top < self.scroll {
                self.scroll = top;
            } else if top + px(ROW_H) > self.scroll + area.size.height {
                self.scroll = top + px(ROW_H) - area.size.height;
            }
        }
        self.scroll = self.scroll.clamp(0.0, self.max_scroll);
        let problem = self.problem();
        {
            let rows = scene.push_layer(Some(area));
            let name_font = ds.code_font(-0.5).with_weight(text_layout::Font::SEMIBOLD);
            for (i, entry) in entries.iter().enumerate() {
                let top = area.min_y() + i as f32 * px(ROW_H) - self.scroll;
                if top + px(ROW_H) < area.min_y() || top > area.max_y() {
                    continue;
                }
                let mid = (top + px(ROW_H) / 2.0).round();
                if focus == Some(i) {
                    rows.quad(
                        Quad::filled(
                            Rect::new(
                                left - px(8.0),
                                top + px(3.0),
                                right - left + px(16.0),
                                px(ROW_H - 6.0),
                            ),
                            p.accent.with_alpha(0.06),
                        )
                        .rounded(px(radius::MD)),
                    );
                }
                // Its name as written, and how many places use it.
                let shown_name = ds::ellipsize(text, &entry.label, &name_font, name_w - px(44.0));
                let shaped = text.shape_line(&shown_name, &name_font);
                let base = ds::optical_baseline(top, px(ROW_H), &name_font);
                draw_text(rows, &shaped, left.round(), base, p.accent);
                if entry.uses > 1 {
                    let uses = format!("×{}", entry.uses);
                    let small = ds.ui_font(-1.5);
                    let us = text.shape_line(&uses, &small);
                    draw_text(
                        rows,
                        &us,
                        (left + shaped.width + px(6.0)).round(),
                        ds::optical_baseline(top, px(ROW_H), &small),
                        p.text_muted,
                    );
                }
                // Its value.
                let field = Rect::new(value_x, mid - px(17.0), value_w, px(34.0));
                let null = entry.kind == ValueKind::Null;
                let wrong = (self.tried || !entry.text.is_empty())
                    && problem.as_ref().is_some_and(|(at, _)| *at == i);
                let style = Self::style(&ds, !null, wrong);
                let focused = focus == Some(i) && !null;
                let placeholder =
                    if null { "NULL".to_owned() } else { t("console.params_placeholder") };
                if let Some(input) = self.inputs.get(i) {
                    if null {
                        TextInput::new("").paint(rows, text, field, false, &placeholder, &style);
                    } else {
                        input.paint(rows, text, field, focused, &placeholder, &style);
                    }
                }
                if let Some(at) = self.fields.get_mut(i) {
                    *at = field;
                }
                // Its kind.
                let selected =
                    ValueKind::ALL.iter().position(|k| *k == entry.effective_kind()).unwrap_or(0);
                let hovered = match self.hover {
                    Some(Hit::Kind(h, k)) if h == i => ValueKind::ALL.iter().position(|x| *x == k),
                    _ => None,
                };
                let segs = ds::segmented(
                    rows,
                    text,
                    &ds,
                    ds::SegmentedSpec {
                        right: kinds_x + kinds_w,
                        mid,
                        labels: &kind_labels,
                        selected,
                        hovered,
                    },
                );
                for (seg, kind) in segs.into_iter().zip(ValueKind::ALL) {
                    self.hits.push((seg, Hit::Kind(i, kind)));
                }
                // Remembered for the next runs.
                let cx = right - remember_w + px(30.0);
                let state = if entry.remember { ds::Check::On } else { ds::Check::Off };
                let check = ds::checkbox(rows, text, &ds, cx, mid, state, true);
                self.hits.push((
                    Rect::new(check.min_x() - px(8.0), top, check.size.width + px(16.0), px(ROW_H)),
                    Hit::Remember(i),
                ));
            }
        }
        self.bar.paint(scene, p, scale, area, content, self.scroll);
        let layer = scene.push_layer(Some(b));
        // What keeps the values back, under the list (once tried, or as typed).
        let note_y = area.max_y() + px(6.0);
        let entries_now = self.entries();
        let said = problem
            .as_ref()
            .filter(|(i, _)| self.tried || entries_now.get(*i).is_some_and(|e| !e.text.is_empty()));
        let small = ds.ui_font(-1.0);
        match said {
            Some((i, why)) => {
                let label = entries_now.get(*i).map(|e| e.label.clone()).unwrap_or_default();
                let why = match why {
                    Problem::NotANumber => {
                        i18n::tr!("console.params_not_a_number", name = label.as_str())
                    }
                    Problem::EmptyExpression => {
                        i18n::tr!("console.params_empty_expression", name = label.as_str())
                    }
                };
                if let Some(icon) = icons.id("circle-alert") {
                    let s = px(13.0);
                    let r = Rect::new(left.round(), (note_y + px(10.0) - s / 2.0).round(), s, s);
                    draw_icon(layer, icons, icon, r, p.danger);
                }
                let shaped = text.shape_line(&why, &small);
                draw_text(
                    layer,
                    &shaped,
                    (left + px(20.0)).round(),
                    ds::optical_baseline(note_y, px(20.0), &small),
                    p.danger,
                );
            }
            None => {
                let note = t("console.params_note");
                let shown = ds::ellipsize(text, &note, &small, right - left);
                let shaped = text.shape_line(&shown, &small);
                draw_text(
                    layer,
                    &shaped,
                    left.round(),
                    ds::optical_baseline(note_y, px(20.0), &small),
                    p.text_muted,
                );
            }
        }
        // The buttons, right to left: the main one, Cancel.
        let bh = px(ds::BUTTON_H);
        let by = b.max_y() - px(FOOTER_H) + (px(FOOTER_H) - bh) / 2.0;
        let go = request.go_label();
        let w = ds::primary_width(text, &ds, &[&go]);
        let g = Rect::new(b.max_x() - pad - w, by, w, bh);
        ds::primary_button(
            layer,
            text,
            &ds,
            g,
            &go,
            problem.is_none(),
            self.hover == Some(Hit::Go),
        );
        self.hits.push((g, Hit::Go));
        let cancel = t("console.confirm_cancel");
        let cw = ds::primary_width(text, &ds, &[&cancel]);
        let c = Rect::new(g.min_x() - px(10.0) - cw, by, cw, bh);
        ds::secondary_button(layer, text, &ds, c, &cancel, true, self.hover == Some(Hit::Cancel));
        self.hits.push((c, Hit::Cancel));
    }

    /// The scrollbar's thumb or a value's field (its caret put, a drag selecting), else a
    /// button; beside them the focus stays where it was.
    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        let pressed = self.with_controls(|c, parts| {
            let kept = c.focused();
            let pressed = c.press(parts, p, 1);
            if pressed.is_none() {
                c.focus(parts, kept);
            }
            pressed
        });
        if pressed.is_some() {
            return None;
        }
        if let Some(hit) = self.hit(p) {
            self.press(hit);
        }
        self.action.take()
    }

    fn drag(&mut self, p: Point) -> bool {
        self.with_controls(|c, parts| c.drag(parts, p)).is_some()
    }

    fn release(&mut self) -> bool {
        self.with_controls(|c, parts| c.release(parts)).is_some_and(|(_, changed)| changed)
    }

    fn hover(&mut self, p: Point) -> bool {
        let hover = self.hit(p);
        let changed = hover != self.hover;
        self.hover = hover;
        changed | self.with_controls(|c, parts| c.hover(parts, Some(p)))
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        if let Some(cursor) = self.controls.cursor_at(p) {
            return Some(cursor);
        }
        self.hit(p).map(|_| ViewerCursor::Pointer)
    }

    fn tooltip_at(&self, p: Point) -> Option<Tip> {
        let (rect, hit) = self.hits.iter().rev().find(|(r, _)| r.contains(p))?;
        let label = match hit {
            Hit::Kind(_, ValueKind::Text) => t("console.param_text_tip"),
            Hit::Kind(_, ValueKind::Number) => t("console.param_number_tip"),
            Hit::Kind(_, ValueKind::Null) => t("console.param_null_tip"),
            Hit::Kind(_, ValueKind::Raw) => t("console.param_raw_tip"),
            Hit::Remember(_) => t("console.params_remember_tip"),
            _ => return None,
        };
        Some(Tip::new(*rect, label))
    }

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        let before = self.scroll;
        self.scroll = (self.scroll + dy).clamp(0.0, self.max_scroll);
        before != self.scroll
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        self.edit_key_with(key, KeyReach::default())
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        match key {
            ViewerKey::Enter => {
                self.give();
            }
            ViewerKey::Escape => self.cancel(),
            // ↓ ↑ move the focus as Tab and ⇧Tab do.
            ViewerKey::Tab | ViewerKey::Down | ViewerKey::Up => {
                let back = key == ViewerKey::Up || key == ViewerKey::Tab && reach.extend;
                self.step_focus(back);
            }
            // The focused value's editing keys (⌦ at its end too: nothing to delete).
            key => {
                let used = self.with_controls(|c, parts| c.key(parts, key, reach)).is_some();
                return used || key == ViewerKey::Delete && self.focused_value();
            }
        }
        true
    }

    fn selected_text(&self) -> Option<String> {
        self.controls.selected_text_with(|i| {
            (i == VALUES).then(|| self.inputs.get(self.row).map(|f| f as &dyn Control)).flatten()
        })
    }

    fn command(&mut self, id: &str) -> bool {
        if let Some(rest) = id.strip_prefix("kind:") {
            // `kind:<row>:<text|number|null|raw>` (the tests', the snapshots').
            let Some((row, kind)) = rest.split_once(':') else { return false };
            let Ok(row) = row.parse::<usize>() else { return false };
            let kind = match kind {
                "text" => ValueKind::Text,
                "number" => ValueKind::Number,
                "null" => ValueKind::Null,
                "raw" => ValueKind::Raw,
                _ => return false,
            };
            self.set_kind(row, kind);
            return true;
        }
        if let Some(rest) = id.strip_prefix("value:") {
            let Some((row, value)) = rest.split_once(':') else { return false };
            let Ok(row) = row.parse::<usize>() else { return false };
            self.set_text(row, value);
            self.focus_row(row);
            return true;
        }
        if let Some(row) = id.strip_prefix("remember:").and_then(|r| r.parse::<usize>().ok()) {
            self.toggle_remember(row);
            return true;
        }
        match id {
            "go" => {
                self.give();
                true
            }
            // The focused value's edit commands (⌘A, ⌘Z, ⇧⌘Z, ⌘X).
            id => self.with_controls(|c, parts| c.command(parts, id)).is_some(),
        }
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }
}

/// The values' place among the modal's parts (after the rows' scrollbar).
const VALUES: usize = 1;

/// The parameters' values as one form's control: each row's field (a press there gives
/// its row the keys), the keys, the text and the edit commands to the row having them; a
/// `NULL` row takes the focus and no text.
struct Values<'a> {
    inputs: &'a mut [TextInput],
    /// Where each row's field was drawn last (empty: scrolled out of view).
    fields: &'a [Rect],
    entries: &'a [ParamEntry],
    /// The row having the keys.
    row: &'a mut usize,
    edited: &'a mut bool,
}

impl Values<'_> {
    /// The row whose field is at `p`.
    fn at(&self, p: Point) -> Option<usize> {
        self.fields.iter().position(|r| r.contains(p))
    }

    fn null(&self, i: usize) -> bool {
        self.entries.get(i).is_some_and(|e| e.kind == ValueKind::Null)
    }

    /// Row `i`'s field (`None`: a `NULL` row, or no such row).
    fn field(&mut self, i: usize) -> Option<KeyField<'_>> {
        if self.null(i) {
            return None;
        }
        Some(KeyField {
            input: self.inputs.get_mut(i)?,
            area: self.fields.get(i).copied().unwrap_or_default(),
            selects: true,
            edited: self.edited,
        })
    }
}

impl Control for Values<'_> {
    fn hit(&self, p: Point) -> bool {
        self.at(p).is_some()
    }

    fn press(&mut self, p: Point, clicks: u32) -> Press {
        let Some(i) = self.at(p) else { return Press::Ignored };
        *self.row = i;
        self.field(i).map_or(Press::Took, |mut f| f.press(p, clicks))
    }

    fn drag(&mut self, p: Point) -> bool {
        let row = *self.row;
        self.field(row).is_some_and(|mut f| f.drag(p))
    }

    fn cursor(&self, _p: Point) -> Option<ViewerCursor> {
        Some(ViewerCursor::Text)
    }

    fn focusable(&self) -> bool {
        !self.inputs.is_empty()
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        let row = *self.row;
        self.field(row).is_some_and(|mut f| f.key(key, reach))
    }

    fn text(&mut self, typed: &str) -> bool {
        let row = *self.row;
        self.field(row).is_some_and(|mut f| f.text(typed))
    }

    fn command(&mut self, id: &str) -> bool {
        let row = *self.row;
        self.field(row).is_some_and(|mut f| f.command(id))
    }
}

#[cfg(test)]
#[path = "../tests/unit/params.rs"]
mod tests;
