//! "What can be optimized": a card per finding, the worst first (as the analysis sorts them:
//! the hot spot, then by severity and impact) — its severity's icon and color, its title,
//! the share of the time it touches, why (its figures written in), the step it's about (a
//! link), what to do, and the SQL it suggests colored, with Copy and Insert into console
//! (appended, selected, never run), and "Ignore in this console". A click on a card chooses
//! its step and lights its place in the console. Without findings, "Nothing to report" and
//! what was checked; on a plan only estimated, "Run Explain Analyze for more advice".
//!
//! The cards are laid out once per width and set of findings ([`kernel::Memo`]); only those
//! in view are drawn.

use database::profile::derive::ProfileReport;
use database::profile::rules::Finding;
use kernel::{Memo, Point, Rect, Rgba};
use render_api::{Quad, Scene};
use text_layout::{Font, TextSystem};
use ui_components::ds::{self, Ctx, radius};
use ui_components::{EdgeScrollbar, Palette};
use ui_kit::{draw_icon, draw_text};

use crate::profile::analysis;
use crate::profile::figures::{self, level};

/// Where a press on the list falls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindingHit {
    Card(usize),
    Step(usize),
    Copy(usize),
    Insert(usize),
    Ignore(usize),
    /// "Show ignored (N)".
    ShowIgnored,
}

/// A card's lines, laid out.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Card {
    pub finding: usize,
    pub top: f32,
    pub height: f32,
    pub title: String,
    pub why: Vec<String>,
    pub step: Option<String>,
    pub fix: Vec<String>,
    pub sql: Vec<String>,
}

/// The cards for a width.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Cards {
    pub cards: Vec<Card>,
    pub height: f32,
}

/// Sizes (logical pixels).
const PAD: f32 = 12.0;
const GAP: f32 = 8.0;
const TITLE_H: f32 = 22.0;
const LINE_H: f32 = 17.0;
const CODE_H: f32 = 16.0;
const BUTTONS_H: f32 = 26.0;

/// The findings' list (see the module's doc).
#[derive(Debug, Default)]
pub struct FindingsList {
    layout: Memo<(u32, u32, u64), Cards>,
    pub scroll: f32,
    bar: EdgeScrollbar,
    hits: Vec<(Rect, FindingHit)>,
    tips: Vec<(Rect, String)>,
    pub hover: Option<FindingHit>,
    area: Rect,
    content: f32,
}

/// What the list draws.
pub struct ListPaint<'a> {
    pub area: Rect,
    pub scale: f32,
    pub text: &'a mut TextSystem,
    pub scene: &'a mut Scene,
    pub palette: &'a Palette,
    pub icons: &'a icons::Icons,
    pub report: &'a ProfileReport,
    pub findings: &'a [&'a Finding],
    /// Bumped when the findings shown change (ignored, filtered).
    pub generation: u64,
    pub selected: Option<usize>,
    pub ignored: usize,
    /// The console is there to insert into.
    pub can_insert: bool,
}

impl std::fmt::Debug for ListPaint<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListPaint").field("area", &self.area).finish_non_exhaustive()
    }
}

impl FindingsList {
    /// The cards of `findings` at `width` (physical pixels).
    pub fn cards(
        &mut self,
        text: &mut TextSystem,
        ds: &Ctx<'_>,
        width: f32,
        findings: &[&Finding],
        report: &ProfileReport,
        generation: u64,
    ) -> &Cards {
        let key = (width.to_bits(), ds.scale.to_bits(), generation);
        self.layout.get_or_update(key, || lay_out(text, ds, width, findings, report))
    }

    /// How many times the cards were laid out (tests: once per width).
    pub fn laid_out(&self) -> bool {
        self.layout.value().is_some()
    }

    pub fn hit(&self, p: Point) -> Option<FindingHit> {
        if !self.area.contains(p) {
            return None;
        }
        // The buttons before the cards they're in.
        self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)
    }

    /// The tooltip of a button under `p`.
    pub fn tooltip_at(&self, p: Point) -> Option<ui_components::Tip> {
        if !self.area.contains(p) {
            return None;
        }
        let (r, tip) = self.tips.iter().rev().find(|(r, _)| r.contains(p))?;
        Some(ui_components::Tip::new(*r, tip.clone()))
    }

    /// The bar's press (`true`: on it).
    pub fn press_bar(&mut self, p: Point) -> bool {
        if let Some(offset) = self.bar.press(p) {
            self.scroll = offset;
            return true;
        }
        false
    }

    pub fn drag(&mut self, p: Point) -> bool {
        match self.bar.drag(p) {
            Some(offset) => {
                self.scroll = offset;
                true
            }
            None => false,
        }
    }

    pub fn release(&mut self) -> bool {
        self.bar.release()
    }

    pub fn hover(&mut self, p: Point) -> bool {
        let bar = self.bar.hover(p);
        let hit = self.hit(p);
        let changed = hit != self.hover;
        self.hover = hit;
        changed || bar
    }

    pub fn scroll_by(&mut self, dy: f32) -> bool {
        let before = self.scroll;
        let max = (self.content - self.area.size.height).max(0.0);
        self.scroll = (self.scroll + dy).clamp(0.0, max);
        self.scroll != before
    }

    /// Card `i` brought into view (once laid out).
    pub fn reveal(&mut self, i: usize) {
        let Some(cards) = self.layout.value() else { return };
        let Some(card) = cards.cards.iter().find(|c| c.finding == i) else { return };
        let (top, bottom) = (card.top, card.top + card.height);
        if top < self.scroll || card.height > self.area.size.height {
            self.scroll = top;
        } else if bottom > self.scroll + self.area.size.height {
            self.scroll = bottom - self.area.size.height;
        }
    }

    /// The list in `area`: the cards in view, or what's said when there are none.
    pub fn paint(&mut self, ctx: ListPaint<'_>) {
        let ListPaint {
            area,
            scale,
            text,
            scene,
            palette: p,
            icons,
            report,
            findings,
            generation,
            selected,
            ignored,
            can_insert,
        } = ctx;
        let ds = Ctx::new(p, scale);
        let px = |v: f32| ds.px(v);
        self.area = area;
        self.hits.clear();
        self.tips.clear();
        let width = (area.size.width - px(PAD) * 2.0).max(px(80.0));
        let cards = self.cards(text, &ds, width, findings, report, generation).clone();
        let footer = if ignored > 0 { px(30.0) } else { 0.0 };
        self.content = cards.height + footer + px(PAD);
        self.scroll = self.scroll.clamp(0.0, (self.content - area.size.height).max(0.0));
        let layer = scene.push_layer(Some(area));
        if cards.cards.is_empty() {
            let estimated = !report.profile.depth.runs();
            let (title, detail) = if estimated {
                (i18n::t("profile.view.nothing"), i18n::t("profile.view.run_analyze"))
            } else {
                (
                    i18n::t("profile.view.nothing"),
                    i18n::tr!(
                        "profile.view.checked",
                        count = database::profile::rules::rules().len() as i64
                    ),
                )
            };
            let room = Rect::new(
                area.min_x(),
                area.min_y(),
                area.size.width,
                (area.size.height - footer).max(px(60.0)),
            );
            let detail =
                ds::wrap(text, &detail, &ds.ui_font(-1.0), area.size.width - px(32.0), 3).join(" ");
            ds::empty_state(layer, text, &ds, room, &title, Some(&detail));
        }
        let title_font = ds.ui_font(-0.5).with_weight(Font::SEMIBOLD);
        let body = ds.ui_font(-1.0);
        let small = ds.ui_font(-1.5);
        let code = ds.code_font(-1.5);
        let left = area.min_x() + px(PAD);
        for card in &cards.cards {
            let top = area.min_y() + px(PAD) + card.top - self.scroll;
            if top > area.max_y() || top + card.height < area.min_y() {
                continue;
            }
            let Some(f) = findings.get(card.finding) else { continue };
            let lvl = level(f.severity);
            let color = if f.is_problem() { lvl.color(p) } else { p.heat(f.impact as f32) };
            let r = Rect::new(left, top.round(), width, card.height);
            let on = selected == Some(card.finding);
            let hovered = matches!(self.hover, Some(FindingHit::Card(i)) if i == card.finding);
            let fill = if on {
                color.with_alpha(0.10)
            } else if hovered {
                p.text.with_alpha(0.05)
            } else {
                p.text.with_alpha(0.025)
            };
            let border = if on { color.with_alpha(0.9) } else { p.border };
            layer.quad(Quad::filled(r, fill).rounded(px(radius::MD)).bordered(ds.line(), border));
            // The severity's strip down its left.
            let strip = Rect::new(
                r.min_x() + px(1.0),
                r.min_y() + px(8.0),
                px(3.0),
                r.size.height - px(16.0),
            );
            layer.quad(Quad::filled(strip, color).rounded(px(1.5)));
            self.hits.push((r, FindingHit::Card(card.finding)));
            let x = r.min_x() + px(14.0);
            let right = r.max_x() - px(12.0);
            let mut y = r.min_y() + px(8.0);
            // The icon and the title; the share it touches at the right.
            let icon = if f.is_problem() { lvl.icon() } else { "flame" };
            if let Some(id) = icons.id(icon) {
                let s = px(15.0);
                draw_icon(
                    layer,
                    icons,
                    id,
                    Rect::new(x.round(), (y + (px(TITLE_H) - s) / 2.0).round(), s, s),
                    color,
                );
            }
            let share = figures::share(f.impact);
            let share_shaped = text.shape_line(&share, &small);
            let pill_w = share_shaped.width + px(12.0);
            let pill = Rect::new(
                (right - pill_w).round(),
                (y + px(3.0)).round(),
                pill_w.round(),
                px(16.0),
            );
            layer.quad(
                Quad::filled(pill, p.heat(f.impact as f32).with_alpha(0.22)).rounded(px(8.0)),
            );
            draw_text(
                layer,
                &share_shaped,
                (pill.min_x() + px(6.0)).round(),
                ds::optical_baseline(pill.min_y(), pill.size.height, &small),
                p.text,
            );
            let tx = x + px(22.0);
            let title = ds::ellipsize(
                text,
                &card.title,
                &title_font,
                (pill.min_x() - px(8.0) - tx).max(0.0),
            );
            let shaped = text.shape_line(&title, &title_font);
            draw_text(
                layer,
                &shaped,
                tx.round(),
                ds::optical_baseline(y, px(TITLE_H), &title_font),
                p.text,
            );
            y += px(TITLE_H) + px(2.0);
            for line in &card.why {
                let shaped = text.shape_line(line, &body);
                draw_text(
                    layer,
                    &shaped,
                    tx.round(),
                    ds::optical_baseline(y, px(LINE_H), &body),
                    p.text.with_alpha(0.86),
                );
                y += px(LINE_H);
            }
            if let Some(step) = &card.step {
                y += px(3.0);
                let hit = matches!(self.hover, Some(FindingHit::Step(i)) if i == card.finding);
                let label = ds::ellipsize(text, &format!("→ {step}"), &body, right - tx);
                let shaped = text.shape_line(&label, &body);
                let c = if hit { p.accent } else { p.accent.with_alpha(0.85) };
                draw_text(
                    layer,
                    &shaped,
                    tx.round(),
                    ds::optical_baseline(y, px(LINE_H), &body),
                    c,
                );
                if hit {
                    let base = ds::optical_baseline(y, px(LINE_H), &body) + px(2.0);
                    layer.quad(Quad::filled(
                        Rect::new(tx.round(), base, shaped.width, ds.line()),
                        c,
                    ));
                }
                self.hits.push((
                    Rect::new(tx, y, shaped.width, px(LINE_H)),
                    FindingHit::Step(card.finding),
                ));
                y += px(LINE_H);
            }
            if !card.fix.is_empty() {
                y += px(4.0);
                if let Some(id) = icons.id("lightbulb") {
                    let s = px(12.0);
                    draw_icon(
                        layer,
                        icons,
                        id,
                        Rect::new(
                            (tx - px(17.0)).round(),
                            (y + (px(LINE_H) - s) / 2.0).round(),
                            s,
                            s,
                        ),
                        p.text_muted,
                    );
                }
                for line in &card.fix {
                    let shaped = text.shape_line(line, &body);
                    draw_text(
                        layer,
                        &shaped,
                        tx.round(),
                        ds::optical_baseline(y, px(LINE_H), &body),
                        p.text_muted,
                    );
                    y += px(LINE_H);
                }
            }
            if !card.sql.is_empty() {
                y += px(8.0);
                let h = px(CODE_H) * card.sql.len() as f32 + px(12.0);
                let block = Rect::new(tx.round(), y.round(), (right - tx).round(), h.round());
                layer.quad(
                    Quad::filled(block, p.text.with_alpha(0.05))
                        .rounded(px(radius::SM))
                        .bordered(ds.line(), p.border.with_alpha(0.6)),
                );
                let metrics = text.metrics(&code);
                let mut ly = block.min_y() + px(6.0);
                for line in &card.sql {
                    ui_components::code_tokens::draw_code(
                        layer,
                        text,
                        ui_components::CodeLine {
                            source: line,
                            extension: "sql",
                            font: &code,
                            x: block.min_x() + px(10.0),
                            baseline: ui_kit::centered_baseline(ly, px(CODE_H), &metrics),
                            palette: p,
                            max_x: block.max_x() - px(8.0),
                        },
                    );
                    ly += px(CODE_H);
                }
                y += h;
            }
            // The buttons: Copy, Insert into console (a suggestion's SQL); Ignore.
            y += px(6.0);
            let mut bx = tx;
            let row = Rect::new(tx, y, right - tx, px(BUTTONS_H));
            if !card.sql.is_empty() {
                for (hit, icon, label, enabled) in [
                    (
                        FindingHit::Copy(card.finding),
                        "copy",
                        i18n::t("profile.view.copy_sql"),
                        true,
                    ),
                    (
                        FindingHit::Insert(card.finding),
                        "arrow-down-to-line",
                        i18n::t("profile.view.insert"),
                        can_insert,
                    ),
                ] {
                    let r = text_button(
                        &mut ui_components::LayerPainter::new(layer, text, icons, ds),
                        bx,
                        row,
                        (icon, &label),
                        ui_components::ds::Press::new(enabled, self.hover == Some(hit)),
                    );
                    if enabled {
                        self.hits.push((r, hit));
                    }
                    bx = r.max_x() + px(6.0);
                }
            }
            if !card.sql.is_empty() {
                self.tips.push((
                    Rect::new(tx, row.min_y(), bx - tx, row.size.height),
                    i18n::t("profile.view.insert_tip"),
                ));
            }
            // Ignore (a problem's rule, for this console): an icon at the right.
            if f.is_problem() {
                let ignore = FindingHit::Ignore(card.finding);
                let ir =
                    Rect::new((right - px(26.0)).round(), row.min_y(), px(26.0), row.size.height);
                crate::bar::icon_button(
                    layer,
                    icons,
                    &ds,
                    ir,
                    "eye-off",
                    None,
                    ui_components::ds::Press::new(true, self.hover == Some(ignore)),
                );
                self.hits.push((ir, ignore));
                self.tips.push((ir, i18n::t("profile.view.ignore")));
            }
        }
        if ignored > 0 {
            let y = area.min_y() + px(PAD) + cards.height - self.scroll + px(4.0);
            let label = i18n::tr!("profile.view.show_ignored", count = ignored as i64);
            let shaped = text.shape_line(&label, &small);
            let r = Rect::new(left, y, shaped.width + px(8.0), px(22.0));
            let c =
                if self.hover == Some(FindingHit::ShowIgnored) { p.accent } else { p.text_muted };
            draw_text(
                layer,
                &shaped,
                (left + px(4.0)).round(),
                ds::optical_baseline(r.min_y(), r.size.height, &small),
                c,
            );
            self.hits.push((r, FindingHit::ShowIgnored));
        }
        self.bar.paint(scene, p, scale, area, self.content, self.scroll);
    }
}

/// A small button with an icon and a label, from `x` in `row`; its rectangle.
pub fn text_button(
    pen: &mut ui_components::LayerPainter<'_>,
    x: f32,
    row: Rect,
    (icon, label): (&str, &str),
    press: ui_components::ds::Press,
) -> Rect {
    let (layer, text, icons, ds) = (&mut *pen.layer, &mut *pen.text, pen.icons, &pen.ds);
    let ui_components::ds::Press { enabled, hovered } = press;
    let p = ds.palette;
    let px = |v: f32| ds.px(v);
    let font = ds.ui_font(-1.5);
    let shaped = text.shape_line(label, &font);
    let w = (shaped.width + px(32.0)).round();
    let r = Rect::new(x.round(), row.min_y().round(), w, row.size.height.round());
    let fill = if hovered && enabled { p.text.with_alpha(0.09) } else { p.text.with_alpha(0.045) };
    layer.quad(
        Quad::filled(r, fill).rounded(px(radius::SM)).bordered(ds.line(), p.border.with_alpha(0.7)),
    );
    let color: Rgba = if enabled { p.text } else { p.text_muted.with_alpha(0.5) };
    if let Some(id) = icons.id(icon) {
        let s = px(12.0);
        draw_icon(
            layer,
            icons,
            id,
            Rect::new(
                (r.min_x() + px(8.0)).round(),
                (r.min_y() + (r.size.height - s) / 2.0).round(),
                s,
                s,
            ),
            color,
        );
    }
    draw_text(
        layer,
        &shaped,
        (r.min_x() + px(24.0)).round(),
        ds::optical_baseline(r.min_y(), r.size.height, &font),
        color,
    );
    r
}

/// The cards of `findings` for `width` (physical pixels): their lines wrapped.
pub fn lay_out(
    text: &mut TextSystem,
    ds: &Ctx<'_>,
    width: f32,
    findings: &[&Finding],
    report: &ProfileReport,
) -> Cards {
    let px = |v: f32| ds.px(v);
    let body = ds.ui_font(-1.0);
    let inner = (width - px(14.0) - px(22.0) - px(12.0)).max(px(40.0));
    let mut out = Cards::default();
    let mut y = 0.0;
    for (i, f) in findings.iter().enumerate() {
        let t = analysis::texts(f);
        let why = ds::wrap(text, &t.why, &body, inner, 6);
        let fix =
            if t.fix.is_empty() { Vec::new() } else { ds::wrap(text, &t.fix, &body, inner, 4) };
        let step = f
            .nodes
            .first()
            .and_then(|id| report.profile.get(*id))
            .map(database::profile::export::head);
        let sql: Vec<String> = f
            .suggestion
            .as_ref()
            .and_then(|s| s.sql())
            .map(|s| {
                let code = ds.code_font(-1.5);
                s.lines()
                    .flat_map(|line| {
                        let wrapped = ds::wrap(text, line, &code, inner - px(20.0), 12);
                        if wrapped.is_empty() { vec![String::new()] } else { wrapped }
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut h = px(8.0) + px(TITLE_H) + px(2.0) + px(LINE_H) * why.len() as f32;
        if step.is_some() {
            h += px(3.0) + px(LINE_H);
        }
        if !fix.is_empty() {
            h += px(4.0) + px(LINE_H) * fix.len() as f32;
        }
        if !sql.is_empty() {
            h += px(8.0) + px(CODE_H) * sql.len() as f32 + px(12.0);
        }
        h += if sql.is_empty() && !f.is_problem() {
            px(4.0)
        } else {
            px(6.0) + px(BUTTONS_H) + px(10.0)
        };
        out.cards.push(Card {
            finding: i,
            top: y,
            height: h.round(),
            title: t.title,
            why,
            step,
            fix,
            sql,
        });
        y += h.round() + px(GAP);
    }
    out.height = (y - px(GAP)).max(0.0);
    out
}

#[cfg(test)]
#[path = "../../tests/unit/profile/findings.rs"]
mod tests;
