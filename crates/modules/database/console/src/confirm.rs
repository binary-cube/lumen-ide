//! The console's confirmation, before anything is sent: a modal of the module's own
//! (`view-console-confirm`, `host.open_modal`: closed only by its buttons, × or Esc — never
//! by a click outside) showing the exact statements (colored as SQL), the source in its color
//! with its "production" label, and what's at stake (an `UPDATE` / `DELETE` without `WHERE`,
//! a `DROP`, a `TRUNCATE`, a write on production made permanent at once). On production the
//! button is enabled only once "I understand" is ticked.
//!
//! One modal for every question a console asks ([`ConfirmKind`]): a run's dangers, Explain
//! Analyze over a write, a console closed with its transaction open (Commit, Roll Back or
//! keep it), and — for the profiling steps — a profile run on production, asked once per
//! console and session ([`ConfirmRequest::once`]: the answer kept by the console's runner).
//!
//! What to do with the answer travels with the question ([`ask`]); the modal's button closes
//! it and does that with the host. Closed any other way (×, Esc), nothing is done.

use std::path::Path;

use crate::answer_slot::Once;
use database::SourceColor;
use database::console::ConsoleId;
use i18n::t;
use kernel::{Point, Rect};
use render_api::Quad;
use ui_components::code_tokens::{self, CodeLine};
use ui_components::control::{Control, Controls, Thumb};
use ui_components::ds::{self, Ctx, radius};
use ui_components::{
    EdgeScrollbar, Viewer, ViewerAction, ViewerKey, ViewerKind, ViewerPaint, ViewerProvider,
};
use ui_kit::{draw_icon, draw_text};
use workbench_model::WorkbenchHost;

/// The modal's virtual extension (its view's kind).
pub const EXTENSION: &str = "view-console-confirm";

/// What a confirmation is about (its title, its buttons).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmKind {
    /// Statements with dangers, or writes on production made permanent at once.
    Run,
    /// Explain Analyze runs the statement (a write, undone at the end).
    Analyze,
    /// A profile run on production (it loads the server): asked once per console.
    Profile,
    /// The console closes with its transaction open: Commit, Roll Back, or keep it open.
    CloseTransaction,
    /// A result's changes submitted (plan 39l): the statements that will run, shown first.
    Submit,
}

/// What the user answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// Go on (run it).
    Go,
    Commit,
    Rollback,
}

/// What the modal shows.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmRequest {
    /// Set by [`ask`]: what its answer does.
    pub then: Once<OnAnswer>,
    pub kind: ConfirmKind,
    /// The console's name and its source's.
    pub console: String,
    pub source: String,
    pub color: Option<SourceColor>,
    pub production: bool,
    /// The statements, exactly as they'd be sent.
    pub statements: Vec<String>,
    /// What's at stake, in words (each a line, in the danger's color).
    pub reasons: Vec<String>,
    /// A line under them (what isn't undone, what happens next).
    pub note: Option<String>,
    /// Asked once per console's session under this key: a "yes" isn't asked again.
    pub once: Option<(ConsoleId, &'static str)>,
    /// What's confirmed waits for the console's Commit (a submit joining its transaction):
    /// nothing is permanent yet, "I understand" isn't asked on production.
    pub pending: bool,
}

impl ConfirmRequest {
    pub fn new(kind: ConfirmKind, console: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            then: Once::default(),
            kind,
            console: console.into(),
            source: source.into(),
            color: None,
            production: false,
            statements: Vec::new(),
            reasons: Vec::new(),
            note: None,
            once: None,
            pending: false,
        }
    }

    /// Its title (the modal's).
    pub fn title(&self) -> String {
        t(match self.kind {
            ConfirmKind::Run => "console.confirm_title",
            ConfirmKind::Analyze => "console.confirm_analyze_title",
            ConfirmKind::Profile => "console.confirm_profile_title",
            ConfirmKind::CloseTransaction => "console.close_tx_title",
            ConfirmKind::Submit => "console.submit_title",
        })
    }

    /// Its main button's label.
    pub fn go_label(&self) -> String {
        t(match self.kind {
            ConfirmKind::Run => "console.confirm_run",
            ConfirmKind::Analyze => "console.analyze_yes",
            ConfirmKind::Profile => "console.confirm_profile",
            ConfirmKind::CloseTransaction => "console.close_tx_commit",
            ConfirmKind::Submit => "console.submit_yes",
        })
    }

    /// "I understand" must be ticked first (production, except for a transaction's end —
    /// committing it is what's asked — and for what waits for the console's Commit).
    pub fn needs_tick(&self) -> bool {
        self.production && self.kind != ConfirmKind::CloseTransaction && !self.pending
    }
}

/// What to do with the answer, with the host.
pub type OnAnswer = Box<dyn FnOnce(&mut dyn WorkbenchHost, Answer) + Send>;

/// Asks `request` in the modal, `then` done with the answer; one asked once already (its
/// console's runner kept the "yes") is answered at once. `false`: the host has no modals
/// (nothing is done).
pub fn ask(host: &mut dyn WorkbenchHost, mut request: ConfirmRequest, then: OnAnswer) -> bool {
    if let Some((console, key)) = request.once
        && runner_of(host, console).is_some_and(|r| r.acknowledged(key))
    {
        then(host, Answer::Go);
        return true;
    }
    let once = request.once;
    // A "yes" to a question asked once is kept by its console.
    let then: OnAnswer = match once {
        // Kept once what was asked is done (that may be what makes the console's runner).
        Some((console, key)) => Box::new(move |host, answer| {
            then(host, answer);
            if answer == Answer::Go
                && let Some(runner) = runner_of(host, console)
            {
                runner.acknowledge(key);
            }
        }),
        None => then,
    };
    request.then = Once::new(then);
    host.open_modal(&request.title(), EXTENSION, &request)
}

fn runner_of(host: &dyn WorkbenchHost, console: ConsoleId) -> Option<crate::runner::Runner> {
    crate::consoles::consoles(host.project_id()?)?.runner_of(console)
}

/// The modal's provider (its virtual extension).
#[derive(Debug, Default)]
pub struct ConfirmProvider;

impl ViewerProvider for ConfirmProvider {
    fn id(&self) -> &'static str {
        EXTENSION
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        (path.extension().and_then(|e| e.to_str()) == Some(EXTENSION))
            .then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(ConfirmView::default()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Go,
    Rollback,
    Cancel,
    Understood,
}

/// The modal's view.
#[derive(Default)]
pub struct ConfirmView {
    request: Option<ConfirmRequest>,
    /// "I understand" ticked (production).
    understood: bool,
    /// Answered: the window closes (and the answer is done).
    done: bool,
    scroll: f32,
    bar: EdgeScrollbar,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
    action: Option<ViewerAction>,
    /// Its parts' state of use (see [`Self::with_controls`]).
    controls: Controls,
}

impl std::fmt::Debug for ConfirmView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConfirmView").field("request", &self.request).finish_non_exhaustive()
    }
}

impl ConfirmView {
    /// The main button may be pressed ("I understand" ticked when asked).
    pub fn can_go(&self) -> bool {
        self.request.as_ref().is_some_and(|r| !r.needs_tick() || self.understood)
    }

    pub fn understood(&self) -> bool {
        self.understood
    }

    /// "I understand" ticked or not.
    pub fn toggle_understood(&mut self) {
        self.understood = !self.understood;
    }

    /// Answers `answer` (when it may): the window closes, the answer is done.
    pub fn answer(&mut self, answer: Answer) -> bool {
        if answer == Answer::Go && !self.can_go() {
            return false;
        }
        let Some(request) = &self.request else { return false };
        let then = request.then.clone();
        self.done = true;
        self.action = Some(ViewerAction::WithHost(ui_components::WithHost::once(move |host| {
            if let Some(then) = then.take() {
                then(host, answer);
            }
        })));
        true
    }

    /// Closed without an answer (Cancel, Esc).
    pub fn cancel(&mut self) {
        // What the answer would have done goes now, not with the last copy of the question.
        if let Some(r) = &self.request {
            drop(r.then.take());
        }
        self.action = Some(ViewerAction::Close);
    }

    /// Its parts (the statements' scrollbar), the topmost first, with their state of use.
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let mut statements = Thumb { bar: &mut self.bar, offset: &mut self.scroll };
        let mut parts: [&mut dyn Control; 1] = [&mut statements];
        f(&mut self.controls, &mut parts)
    }

    fn hit(&self, p: Point) -> Option<Hit> {
        self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)
    }

    fn press(&mut self, hit: Hit) {
        match hit {
            Hit::Go => {
                let go = if self
                    .request
                    .as_ref()
                    .is_some_and(|r| r.kind == ConfirmKind::CloseTransaction)
                {
                    Answer::Commit
                } else {
                    Answer::Go
                };
                self.answer(go);
            }
            Hit::Rollback => {
                self.answer(Answer::Rollback);
            }
            Hit::Cancel => self.cancel(),
            Hit::Understood => self.toggle_understood(),
        }
    }
}

impl Viewer for ConfirmView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        if let Some(request) = data.downcast_ref::<ConfirmRequest>() {
            self.request = Some(request.clone());
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

    /// As tall as what it shows: the source, the reasons, the statements' box (fitted to
    /// them), the note, the tick and the buttons.
    fn modal_height(&self) -> Option<f32> {
        self.request.as_ref().map(content_height)
    }

    fn modal_icon(&self) -> Option<&'static str> {
        Some("alert")
    }

    fn key_hints(&self) -> Vec<(Vec<&'static str>, String)> {
        let label = self.request.as_ref().map(ConfirmRequest::go_label).unwrap_or_default();
        vec![(vec!["↵"], label)]
    }

    fn closed(&self) -> bool {
        self.done
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        let ViewerPaint { bounds: b, scale, text, scene, palette: p, icons, .. } = ctx;
        let ds = Ctx::new(p, scale);
        let px = |v: f32| ds.px(v);
        self.hits.clear();
        let Some(request) = self.request.clone() else { return };
        let pad = px(20.0);
        let footer_h = px(64.0);
        let layer = scene.push_layer(Some(b));
        let mut y = b.min_y() + px(6.0);
        // The source: its color, its name, its production label.
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
        y += px(36.0);
        // What's at stake.
        let small = ds.ui_font(-0.5);
        let line_h = px(24.0);
        for reason in &request.reasons {
            let mid = y + line_h / 2.0;
            if let Some(icon) = icons.id("circle-alert") {
                let s = px(14.0);
                let r = Rect::new((b.min_x() + pad).round(), (mid - s / 2.0).round(), s, s);
                draw_icon(layer, icons, icon, r, p.danger);
            }
            let room = b.size.width - 2.0 * pad - px(22.0);
            let shown = ds::ellipsize(text, reason, &small, room);
            let shaped = text.shape_line(&shown, &small);
            draw_text(
                layer,
                &shaped,
                (b.min_x() + pad + px(22.0)).round(),
                ds::optical_baseline(y, line_h, &small),
                p.text,
            );
            y += line_h;
        }
        if !request.reasons.is_empty() {
            y += px(8.0);
        }
        // Below the statements: the note, then "I understand".
        let note_h = if request.note.is_some() { px(28.0) } else { 0.0 };
        let tick_h = if request.needs_tick() { px(44.0) } else { 0.0 };
        // As tall as its statements (2 to 12 lines, then they scroll), within the window.
        let room = (b.max_y() - footer_h - note_h - tick_h - px(8.0) - y).max(px(40.0));
        let card = Rect::new(
            b.min_x() + pad,
            y,
            b.size.width - 2.0 * pad,
            px(code_box_height(&request)).min(room),
        );
        layer.quad(
            Quad::filled(card, p.editor_background)
                .rounded(px(radius::MD))
                .bordered(ds.line(), p.border),
        );
        let code_font = ds.code_font(-1.0);
        let row_h = px(21.0);
        let inner = Rect::new(
            card.min_x() + px(14.0),
            card.min_y() + px(10.0),
            card.size.width - px(28.0),
            card.size.height - px(20.0),
        );
        let lines = code_lines(&request);
        let content = lines.len() as f32 * row_h;
        self.scroll = self.scroll.clamp(0.0, (content - inner.size.height).max(0.0));
        let code = scene.push_layer(Some(inner));
        let metrics = text.metrics(&code_font);
        for (i, line) in lines.iter().enumerate() {
            let top = inner.min_y() + i as f32 * row_h - self.scroll;
            if top + row_h < inner.min_y() || top > inner.max_y() {
                continue;
            }
            code_tokens::draw_code(
                code,
                text,
                CodeLine {
                    source: line,
                    extension: "sql",
                    font: &code_font,
                    x: inner.min_x(),
                    baseline: ui_kit::centered_baseline(top, row_h, &metrics),
                    palette: p,
                    max_x: inner.max_x(),
                },
            );
        }
        self.bar.paint(scene, p, scale, card, content + px(20.0), self.scroll);
        let layer = scene.push_layer(Some(b));
        let mut y = card.max_y() + px(8.0);
        if let Some(note) = &request.note {
            let small = ds.ui_font(-1.0);
            let shown = ds::ellipsize(text, note, &small, card.size.width);
            let shaped = text.shape_line(&shown, &small);
            draw_text(
                layer,
                &shaped,
                card.min_x(),
                ds::optical_baseline(y, px(20.0), &small),
                p.text_muted,
            );
            y += note_h;
        }
        if request.needs_tick() {
            let r = Rect::new(card.min_x(), y, card.size.width, px(36.0));
            layer.quad(Quad::filled(r, p.danger.with_alpha(0.10)).rounded(px(radius::MD)));
            let mid = r.min_y() + r.size.height / 2.0;
            let state = if self.understood { ds::Check::On } else { ds::Check::Off };
            let check = ds::checkbox(layer, text, &ds, r.min_x() + px(12.0), mid, state, true);
            let label = t("console.confirm_understood");
            let shown = ds::ellipsize(text, &label, &small, r.max_x() - check.max_x() - px(20.0));
            let shaped = text.shape_line(&shown, &small);
            draw_text(
                layer,
                &shaped,
                check.max_x() + px(10.0),
                ds::optical_baseline(r.min_y(), r.size.height, &small),
                p.danger,
            );
            self.hits.push((r, Hit::Understood));
        }
        // The buttons, right to left: the main one, Roll Back (a transaction's end), Cancel.
        let bh = px(ds::BUTTON_H);
        let by = b.max_y() - footer_h + (footer_h - bh) / 2.0;
        let go = request.go_label();
        let w = ds::primary_width(text, &ds, &[&go]);
        let g = Rect::new(b.max_x() - pad - w, by, w, bh);
        let hovered = self.hover == Some(Hit::Go);
        if request.kind == ConfirmKind::CloseTransaction {
            ds::primary_button(layer, text, &ds, g, &go, true, hovered);
        } else {
            ds::danger_button(layer, text, &ds, g, &go, self.can_go(), hovered);
        }
        self.hits.push((g, Hit::Go));
        let mut left = g.min_x();
        if request.kind == ConfirmKind::CloseTransaction {
            let label = t("console.close_tx_rollback");
            let rw = ds::primary_width(text, &ds, &[&label]);
            let r = Rect::new(left - px(10.0) - rw, by, rw, bh);
            ds::danger_button(layer, text, &ds, r, &label, true, self.hover == Some(Hit::Rollback));
            self.hits.push((r, Hit::Rollback));
            left = r.min_x();
        }
        let cancel = t(if request.kind == ConfirmKind::CloseTransaction {
            "console.close_tx_keep"
        } else {
            "console.confirm_cancel"
        });
        let cw = ds::primary_width(text, &ds, &[&cancel]);
        let c = Rect::new(left - px(10.0) - cw, by, cw, bh);
        ds::secondary_button(layer, text, &ds, c, &cancel, true, self.hover == Some(Hit::Cancel));
        self.hits.push((c, Hit::Cancel));
    }

    /// A press on the statements' scrollbar takes its thumb (a drag follows); elsewhere a
    /// button.
    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        if self.with_controls(|c, parts| c.press(parts, p, 1)).is_some() {
            return None;
        }
        if let Some(hit) = self.hit(p) {
            self.press(hit);
        }
        self.action.take()
    }

    fn drag(&mut self, p: Point) -> bool {
        self.with_controls(|c, parts| c.drag(parts, p)).is_some_and(|(_, moved)| moved)
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

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        self.scroll = (self.scroll + dy).max(0.0);
        true
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        match key {
            ViewerKey::Enter => {
                self.press(Hit::Go);
                true
            }
            ViewerKey::Escape => {
                self.cancel();
                true
            }
            _ => false,
        }
    }

    fn command(&mut self, id: &str) -> bool {
        match id {
            "understood" => self.toggle_understood(),
            "go" => self.press(Hit::Go),
            "rollback" => self.press(Hit::Rollback),
            _ => return false,
        }
        true
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }
}

#[cfg(test)]
#[path = "../tests/unit/confirm.rs"]
mod tests;

/// The statements' lines as the box shows them (an empty line between two statements).
fn code_lines(request: &ConfirmRequest) -> Vec<&str> {
    request
        .statements
        .iter()
        .enumerate()
        .flat_map(|(i, s)| {
            let gap = (i > 0).then_some("");
            gap.into_iter().chain(s.lines())
        })
        .collect()
}

/// The fewest and the most lines the statements' box shows (more scroll).
pub const CODE_LINES: std::ops::RangeInclusive<usize> = 2..=12;

/// The statements' box height (logical pixels): its lines (within [`CODE_LINES`]) and its
/// padding.
pub fn code_box_height(request: &ConfirmRequest) -> f32 {
    let lines = code_lines(request).len().clamp(*CODE_LINES.start(), *CODE_LINES.end());
    lines as f32 * 21.0 + 20.0
}

/// The modal's content height (logical pixels), as [`ConfirmView`]'s paint lays it out.
pub fn content_height(request: &ConfirmRequest) -> f32 {
    let reasons = request.reasons.len() as f32 * 24.0;
    let after_reasons = if request.reasons.is_empty() { 0.0 } else { 8.0 };
    let note = if request.note.is_some() { 28.0 } else { 0.0 };
    let tick = if request.needs_tick() { 44.0 } else { 0.0 };
    // The source's row (6 + 36), the box, the gap under it, the buttons' footer (64).
    42.0 + reasons + after_reasons + code_box_height(request) + 8.0 + note + tick + 64.0
}

/// A statement's danger in words (the locales' `schema.*`, as the structure's modals say
/// them).
pub fn danger_text(danger: &database::Danger) -> String {
    use database::Danger;
    use database::sql::WriteKind;
    let kind = |k: database::DropKind| {
        use database::DropKind as K;
        t(match k {
            K::Table => "schema.object_table",
            K::View => "schema.object_view",
            K::Index => "schema.object_index",
            K::Schema => "schema.object_schema",
            K::Database => "schema.object_database",
            K::Function => "schema.object_function",
            K::Procedure => "schema.object_procedure",
            K::Trigger => "schema.object_trigger",
            K::Sequence => "schema.object_sequence",
            K::Type => "schema.object_type",
            K::Other => "schema.object_other",
        })
    };
    match danger {
        Danger::UnfilteredWrite { kind: WriteKind::Delete, table } => {
            i18n::tr!("schema.lose_unfiltered_delete", table = table.to_string())
        }
        Danger::UnfilteredWrite { table, .. } => {
            i18n::tr!("schema.lose_unfiltered_update", table = table.to_string())
        }
        Danger::DropObject { kind: k, name, cascade } => i18n::tr!(
            if *cascade { "schema.lose_drop_cascade" } else { "schema.lose_drop" },
            kind = kind(*k),
            name = name.as_str()
        ),
        Danger::Truncate { table } => i18n::tr!("schema.lose_truncate", table = table.to_string()),
        Danger::AlterDrop { table, column } => i18n::tr!(
            "schema.lose_drop_column",
            table = table.to_string(),
            column = column.as_str()
        ),
        // The structure's own (never found in a statement's text).
        other => format!("{other:?}"),
    }
}
