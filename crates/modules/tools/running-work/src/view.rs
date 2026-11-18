//! The question's view: a line saying what closing does, the work by project and module
//! (each with its icon, what it is, how far and since when, what stopping it costs), the
//! buttons at the bottom. "Cancel" has the keys at first (a stray ↵ loses nothing); Tab /
//! ← / → move them. While waiting the list follows the work (given again by the app) and a
//! loader turns.

use std::time::{Duration, Instant};

use i18n::{t, tr};
use kernel::Rect;
use text_layout::Font;
use ui_components::ds::{self, Ctx};
use ui_components::{AppRequest, Viewer, ViewerAction, ViewerKey, ViewerPaint};
use ui_kit::draw_text;
use workbench_model::running::{Closing, RunningPrompt, RunningWork, WorkGroup, WorkKind};
use workbench_model::{CloseAnswer, StopImpact};

const PAD: f32 = 24.0;
/// Above the first line.
const TOP: f32 = 8.0;
const LINE_H: f32 = 20.0;
/// The intro's lines at most.
const INTRO_LINES: usize = 2;
const GROUP_H: f32 = 30.0;
const ROW_H: f32 = 46.0;
/// The rows listed at most (the rest counted in a line).
const MAX_ROWS: usize = 7;
const MORE_H: f32 = 24.0;
const WIDTH: f32 = 540.0;

/// A button of the question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Button {
    Cancel,
    Wait,
    Stop,
}

impl Button {
    /// Its command (the window draws the buttons; a click comes as it).
    fn id(self) -> &'static str {
        match self {
            Self::Cancel => "running.footer.cancel",
            Self::Wait => "running.footer.wait",
            Self::Stop => "running.footer.stop",
        }
    }

    fn answer(self) -> CloseAnswer {
        match self {
            Self::Cancel => CloseAnswer::Cancel,
            Self::Wait => CloseAnswer::Wait,
            Self::Stop => CloseAnswer::StopAndClose,
        }
    }
}

/// The question's view.
#[derive(Debug, Default)]
pub struct RunningView {
    prompt: Option<RunningPrompt>,
    /// The button with the keys (an index in [`Self::buttons`]).
    focus: usize,
    action: Option<ViewerAction>,
    /// Answered with Stop or Cancel: the modal goes.
    done: bool,
    /// The intro's lines, as last wrapped (the most before the first paint).
    intro_lines: Option<usize>,
}

impl RunningView {
    /// What it shows.
    pub fn prompt(&self) -> Option<&RunningPrompt> {
        self.prompt.as_ref()
    }

    /// The user chose to wait.
    pub fn waiting(&self) -> bool {
        self.prompt.as_ref().is_some_and(|p| p.waiting)
    }

    /// The buttons, left to right (while waiting, no "Wait").
    fn buttons(&self) -> &'static [Button] {
        if self.waiting() {
            &[Button::Cancel, Button::Stop]
        } else {
            &[Button::Cancel, Button::Wait, Button::Stop]
        }
    }

    /// The "Stop" button's label: stopping, then closing — or going on (a restart).
    fn stop_label(&self) -> &'static str {
        match self.prompt.as_ref().map(|p| p.closing) {
            Some(Closing::Restart) => "running.stop_and_continue",
            _ => "running.stop_and_close",
        }
    }

    /// The answer `answer` given: told the app; Wait keeps the question open, following
    /// the work.
    pub fn answer(&mut self, answer: CloseAnswer) {
        match answer {
            CloseAnswer::Wait => {
                if let Some(prompt) = &mut self.prompt {
                    prompt.waiting = true;
                }
                self.focus = 0;
            }
            CloseAnswer::StopAndClose | CloseAnswer::Cancel => self.done = true,
        }
        self.action = Some(ViewerAction::Request(AppRequest::RunningAnswer(answer)));
    }

    /// The rows shown (at most [`MAX_ROWS`]), and how many are left out.
    fn shown(&self) -> (usize, usize) {
        let count = self.prompt.as_ref().map_or(0, RunningPrompt::count);
        (count.min(MAX_ROWS), count.saturating_sub(MAX_ROWS))
    }

    /// The groups with at least one row shown.
    fn shown_groups(&self) -> usize {
        let Some(prompt) = &self.prompt else { return 0 };
        let mut left = MAX_ROWS;
        prompt
            .groups
            .iter()
            .take_while(|g| {
                let shown = left > 0;
                left = left.saturating_sub(g.items.len());
                shown
            })
            .count()
    }
}

/// What closes, said in the intro.
fn intro(prompt: &RunningPrompt) -> String {
    if prompt.waiting {
        return t(match prompt.closing {
            Closing::Window => "running.waiting_window",
            Closing::Project => "running.waiting_project",
            Closing::App => "running.waiting_app",
            Closing::Restart => "running.waiting_restart",
        });
    }
    tr!(
        match prompt.closing {
            Closing::Window => "running.intro_window",
            Closing::Project => "running.intro_project",
            Closing::App => "running.intro_app",
            Closing::Restart => "running.intro_restart",
        },
        count = prompt.count()
    )
}

/// A group's header: its project (or "the app") and its module.
fn group_title(prompt: &RunningPrompt, group: &WorkGroup) -> String {
    let project = match group.project {
        Some(id) => prompt
            .projects
            .iter()
            .find(|(p, _)| *p == id)
            .map_or_else(|| t("running.project"), |(_, name)| name.clone()),
        None => t("running.app"),
    };
    match prompt.module_name(&group.owner) {
        Some(module) => format!("{project} · {module}"),
        None => project,
    }
}

/// The icon of a piece of work.
pub(crate) fn icon(kind: WorkKind) -> &'static str {
    match kind {
        WorkKind::Operation(kind) => kind.icon(),
        WorkKind::Program => "terminal",
        WorkKind::Query => "database",
        WorkKind::Network => "git-branch",
    }
}

/// What a piece of work is, as its row's first line: an operation's kind and subject
/// ("Upload · index.php"), else its title.
pub(crate) fn title(work: &RunningWork) -> String {
    match work.kind {
        WorkKind::Operation(kind) => {
            format!("{} · {}", t(&format!("tasks.op_{}", kind.key())), work.title)
        }
        _ => work.title.clone(),
    }
}

/// `12 s`, `3 min`, `2 h`.
fn running_for(d: Duration) -> String {
    match d.as_secs() {
        s if s < 60 => tr!("running.for_seconds", n = s.max(1)),
        s if s < 3600 => tr!("running.for_minutes", n = s / 60),
        s => tr!("running.for_hours", n = s / 3600),
    }
}

/// A row's second line: its detail, since when, how far, what stopping it costs.
pub(crate) fn detail(work: &RunningWork, now: Instant) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !work.detail.is_empty() {
        parts.push(work.detail.clone());
    }
    if let Some(since) = work.since {
        parts.push(running_for(now.saturating_duration_since(since)));
    }
    if let Some(fraction) = work.progress.and_then(|p| p.fraction()) {
        parts.push(format!("{:.0}%", fraction * 100.0));
    }
    parts.push(match (&work.stop, work.impact) {
        (None, _) => t("running.not_stoppable"),
        (Some(_), StopImpact::LosesWork) => t("running.loses_work"),
        (Some(_), _) => t("running.restarts"),
    });
    parts.join(" · ")
}

impl Viewer for RunningView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        if let Some(prompt) = data.downcast_ref::<RunningPrompt>() {
            let buttons_before = self.buttons().len();
            self.prompt = Some(prompt.clone());
            if self.buttons().len() != buttons_before {
                self.focus = 0;
            }
        }
    }

    fn modal_height(&self) -> Option<f32> {
        let (rows, more) = self.shown();
        let more = if more > 0 { MORE_H } else { 0.0 };
        Some(
            TOP + LINE_H * self.intro_lines.unwrap_or(INTRO_LINES) as f32
                + ds::space::MD
                + GROUP_H * self.shown_groups() as f32
                + ROW_H * rows as f32
                + more,
        )
    }

    fn modal_width(&self) -> Option<f32> {
        Some(WIDTH)
    }

    fn modal_icon(&self) -> Option<&'static str> {
        Some("clock")
    }

    fn closed(&self) -> bool {
        self.done
    }

    /// While waiting: a loader's pace, the list following the work.
    fn is_loading(&self) -> bool {
        self.waiting()
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        let ViewerPaint { bounds: b, scale, text, scene, palette: p, icons, .. } = ctx;
        let Some(prompt) = &self.prompt else { return };
        let ds = Ctx::new(p, scale);
        let px = |v: f32| ds.px(v);
        let layer = scene.push_layer(Some(b));
        let left = b.min_x() + px(PAD);
        let right = b.max_x() - px(PAD);
        let width = (right - left).max(0.0);
        let now = Instant::now();
        // What closing does (or that it waits).
        let font = ds.ui_font(0.0);
        let mut y = b.min_y() + px(TOP);
        let mut x = left;
        if prompt.waiting {
            let mid = y + px(LINE_H) / 2.0;
            x += ds::loader(layer, text, &ds, left, mid, &ds::Loader::default()).size.width
                + px(8.0);
        }
        let lines = ds::wrap(text, &intro(prompt), &font, right - x, INTRO_LINES);
        let intro_lines = lines.len().max(1);
        for line in lines {
            let base = ds::optical_baseline(y, px(LINE_H), &font);
            draw_text(layer, &text.shape_line(&line, &font), x.round(), base, p.text_muted);
            y += px(LINE_H);
        }
        y = b.min_y() + px(TOP + LINE_H * intro_lines as f32 + ds::space::MD);
        // The work, by project and module.
        let header_font = ds.ui_font(-1.0).with_weight(Font::SEMIBOLD);
        let title_font = ds.ui_font(0.0);
        let detail_font = ds.ui_font(-1.0);
        let icon_size = px(16.0);
        let mut left_rows = MAX_ROWS;
        for group in &prompt.groups {
            if left_rows == 0 {
                break;
            }
            let base = ds::optical_baseline(y + px(8.0), px(GROUP_H - 8.0), &header_font);
            ds::draw_ellipsized(
                layer,
                text,
                &group_title(prompt, group),
                &header_font,
                (left, base),
                width,
                p.text_muted,
            );
            y += px(GROUP_H);
            for work in group.items.iter().take(left_rows) {
                left_rows -= 1;
                let row = Rect::new(left, y, width, px(ROW_H));
                let tint = match work.impact {
                    StopImpact::LosesWork => p.warning,
                    _ => p.text_muted,
                };
                if let Some(id) = icons.id(icon(work.kind)) {
                    ui_kit::draw_icon(
                        layer,
                        icons,
                        id,
                        Rect::new(left, y + px(7.0), icon_size, icon_size),
                        tint,
                    );
                }
                let tx = left + icon_size + px(10.0);
                let room = (right - tx).max(0.0);
                let base = ds::optical_baseline(y + px(4.0), px(18.0), &title_font);
                ds::draw_ellipsized(
                    layer,
                    text,
                    &title(work),
                    &title_font,
                    (tx, base),
                    room,
                    p.text,
                );
                let base = ds::optical_baseline(y + px(22.0), px(16.0), &detail_font);
                ds::draw_ellipsized(
                    layer,
                    text,
                    &detail(work, now),
                    &detail_font,
                    (tx, base),
                    room,
                    p.text_muted,
                );
                if let Some(fraction) = work.progress.and_then(|w| w.fraction()) {
                    let bar = Rect::new(tx, row.max_y() - px(5.0), room, px(3.0));
                    ds::progress_bar(layer, &ds, bar, Some(fraction));
                }
                y += px(ROW_H);
            }
        }
        let (_, more) = self.shown();
        if more > 0 {
            let base = ds::optical_baseline(y, px(MORE_H), &detail_font);
            let line = text.shape_line(&tr!("running.more", count = more), &detail_font);
            draw_text(layer, &line, left.round(), base, p.text_muted);
        }
        self.intro_lines = Some(intro_lines);
    }

    /// Right to left: Stop and close (in the danger color), Wait, Cancel — the one with the
    /// keys ringed (Cancel at first: ↵ never stops anything by itself).
    fn modal_buttons(&self) -> Vec<ui_components::ModalButton> {
        use ui_components::ModalButton;
        self.buttons()
            .iter()
            .enumerate()
            .rev()
            .map(|(i, b)| {
                let button = match b {
                    Button::Cancel => ModalButton::other(b.id(), t("running.cancel")),
                    Button::Wait => ModalButton::other(b.id(), t("running.wait")),
                    Button::Stop => ModalButton::danger(b.id(), t(self.stop_label())),
                };
                button.focused(i == self.focus)
            })
            .collect()
    }

    fn command(&mut self, id: &str) -> bool {
        let Some(button) = self.buttons().iter().find(|b| b.id() == id).copied() else {
            return false;
        };
        self.answer(button.answer());
        true
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        let count = self.buttons().len();
        match key {
            ViewerKey::Escape => self.answer(CloseAnswer::Cancel),
            ViewerKey::Enter => {
                let button = self.buttons().get(self.focus).copied().unwrap_or(Button::Cancel);
                self.answer(button.answer());
            }
            ViewerKey::Tab | ViewerKey::Right => self.focus = (self.focus + 1) % count,
            ViewerKey::Left => self.focus = (self.focus + count - 1) % count,
            _ => return false,
        }
        true
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }
}

#[cfg(test)]
#[path = "../tests/unit/view.rs"]
mod tests;
