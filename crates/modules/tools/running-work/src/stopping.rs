//! The same window once the app is ending: what it started being stopped. A line saying so,
//! a progress bar (how many have ended, of how many), then the programs in a field-like box
//! — each with where it is (a turning loader while asked, a check once ended, a mark when
//! it had to be made to), its name and its id — past [`VISIBLE_ROWS`] scrolled by the
//! wheel or the app's scrollbar (its thumb dragged, its track pressed). No buttons, no `esc`: nothing to leave midway; the app goes on by
//! itself once all have ended.

use i18n::{t, tr};
use kernel::{Rect, Rgba};
use render_api::Quad;
use ui_components::ds::{self, Ctx};
use ui_components::{EdgeScrollbar, ViewerPaint};
use ui_kit::draw_text;
use workbench_model::running::{ProgramState, Stopping};

const PAD: f32 = 24.0;
const TOP: f32 = 8.0;
const LINE_H: f32 = 20.0;
/// Between the intro, the bar and the box.
const GAP: f32 = 14.0;
const ROW_H: f32 = 28.0;
/// The box's rows shown at most (more: scrolled).
pub const VISIBLE_ROWS: usize = 8;
/// Inside the box, above the first row and under the last.
const INSET: f32 = 4.0;

/// The progress bar's thickness (logical pixels).
const BAR: f32 = 6.0;

/// The content's height for `stopping` (logical pixels).
pub fn height(stopping: &Stopping) -> f32 {
    let rows = stopping.programs.len().clamp(1, VISIBLE_ROWS) as f32;
    TOP + LINE_H + GAP + bar_height() + GAP + ROW_H * rows + INSET * 2.0 + GAP
}

/// The bar with its text row (its height in logical pixels, as `ProgressBar` draws it).
fn bar_height() -> f32 {
    // A text row (18) and its gap (6) above the bar.
    18.0 + 6.0 + BAR
}

/// The furthest the list can be scrolled (logical pixels).
pub fn max_scroll(stopping: &Stopping) -> f32 {
    stopping.programs.len().saturating_sub(VISIBLE_ROWS) as f32 * ROW_H
}

/// Draws `stopping` in the view's bounds, the list scrolled by `scroll` (logical pixels),
/// with `bar` along the box's right edge when it doesn't all fit.
pub fn paint(ctx: ViewerPaint<'_>, stopping: &Stopping, scroll: f32, bar: &mut EdgeScrollbar) {
    let ViewerPaint { bounds: b, scale, text, scene, palette: p, icons, .. } = ctx;
    let ds = Ctx::new(p, scale);
    let px = |v: f32| ds.px(v);
    let layer = scene.push_layer(Some(b));
    let left = b.min_x() + px(PAD);
    let right = b.max_x() - px(PAD);
    let width = (right - left).max(0.0);
    let done = stopping.done();
    // What's going on (or that it's over).
    let font = ds.ui_font(0.0);
    let mut y = b.min_y() + px(TOP);
    let intro = t(if done { "running.stopping_done" } else { "running.stopping_intro" });
    let base = ds::optical_baseline(y, px(LINE_H), &font);
    ds::draw_ellipsized(layer, text, &intro, &font, (left, base), width, p.text_muted);
    y += px(LINE_H + GAP);
    // How far: ended of all.
    let (ended, count) = (stopping.ended(), stopping.programs.len());
    let label = tr!("running.stopping_count", done = ended, count = count);
    let tone = if done { ds::Tone::Success } else { ds::Tone::Accent };
    let progress = ds::ProgressBar::counted(ended as u64, count as u64)
        .tone(tone)
        .thickness(BAR)
        .label(&label)
        .percent();
    let bar_h = progress.height(&ds);
    progress.paint(layer, text, &ds, Rect::new(left, y, width, bar_h));
    y += bar_h + px(GAP);
    // The programs, in a box like a form's field.
    let rows = count.clamp(1, VISIBLE_ROWS);
    let frame = Rect::new(left, y, width, px(ROW_H * rows as f32 + INSET * 2.0));
    let line = ds.line();
    layer.quad(Quad::filled(frame, p.input_border).rounded(px(ds::radius::MD)));
    let inner = Rect::new(
        frame.min_x() + line,
        frame.min_y() + line,
        frame.size.width - line * 2.0,
        frame.size.height - line * 2.0,
    );
    layer.quad(Quad::filled(inner, p.input_background).rounded(px(ds::radius::MD) - line));
    let list = scene.push_layer(Some(inner));
    let name_font = ds.ui_font(0.0);
    let small = ds.ui_font(-1.0);
    let code = ds.code_font(-1.0);
    let icon_size = px(14.0);
    let state_w = ["running.state_asked", "running.state_ended", "running.state_forced"]
        .iter()
        .map(|key| text.shape_line(&t(key), &small).width)
        .fold(0.0, f32::max);
    let top = inner.min_y() + px(INSET) - px(scroll);
    // A list that scrolls keeps its scrollbar's lane free on the right.
    let lane = if max_scroll(stopping) > 0.0 {
        ui_components::Scrollbar::track_thickness(scale)
    } else {
        0.0
    };
    let rows_w = inner.size.width - lane;
    for (i, program) in stopping.programs.iter().enumerate() {
        let row = Rect::new(inner.min_x(), top + px(ROW_H) * i as f32, rows_w, px(ROW_H));
        if row.max_y() < inner.min_y() || row.min_y() > inner.max_y() {
            continue;
        }
        let mid = row.min_y() + row.size.height / 2.0;
        let x = row.min_x() + px(12.0);
        let (state, color): (String, Rgba) = match program.state {
            ProgramState::Asked => (t("running.state_asked"), p.text_muted),
            ProgramState::Ended => (t("running.state_ended"), ds::Tone::Success.color(p)),
            ProgramState::Forced => (t("running.state_forced"), p.warning),
        };
        match program.state {
            ProgramState::Asked => {
                let spinner = ds::Loader { text: None, size: 12.0, ..ds::Loader::default() };
                ds::loader(list, text, &ds, x + px(1.0), mid, &spinner);
            }
            ProgramState::Ended | ProgramState::Forced => {
                let name = if program.state == ProgramState::Ended { "check" } else { "zap" };
                if let Some(id) = icons.id(name) {
                    let at = Rect::new(x, (mid - icon_size / 2.0).round(), icon_size, icon_size);
                    ui_kit::draw_icon(list, icons, id, at, color);
                }
            }
        }
        // The state and the id on the right, each in a column of its own (the widest
        // state's: the ids line up whatever the state); the name in what's left.
        let state_line = text.shape_line(&state, &small);
        let state_x = (row.max_x() - px(12.0) - state_line.width).round();
        let base = ds::optical_baseline(row.min_y(), row.size.height, &small);
        draw_text(list, &state_line, state_x, base, color);
        let pid = text.shape_line(&program.pid.to_string(), &code);
        let pid_x = (row.max_x() - px(12.0) - state_w - px(18.0) - pid.width).round();
        let base = ds::optical_baseline(row.min_y(), row.size.height, &code);
        draw_text(list, &pid, pid_x, base, p.text_muted);
        let name_x = x + icon_size + px(10.0);
        let room = (pid_x - px(12.0) - name_x).max(0.0);
        let base = ds::optical_baseline(row.min_y(), row.size.height, &name_font);
        let ink = if program.state == ProgramState::Asked { p.text } else { p.text_muted };
        ds::draw_ellipsized(list, text, &program.name, &name_font, (name_x, base), room, ink);
    }
    // More than fits: the app's scrollbar along the box's right edge (nothing when it fits).
    let content = px(ROW_H) * count as f32 + px(INSET) * 2.0;
    let area = Rect::new(
        inner.min_x(),
        inner.min_y() + px(2.0),
        inner.size.width - px(2.0),
        inner.size.height - px(4.0),
    );
    bar.paint(scene, p, scale, area, content, px(scroll));
}

#[cfg(test)]
#[path = "../tests/unit/stopping.rs"]
mod tests;
