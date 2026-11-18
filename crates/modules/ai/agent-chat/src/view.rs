//! A session as a conversation, in a tab of the editor: what was said (the user's messages on
//! a bubble, the agent's plain, its thinking quieter), the tools it uses (their status, the
//! files they change — a click compares the change), its plan; at the foot, the permission it
//! waits for (its choices as buttons), the field to write in (↵ sends, Esc stops the turn) and
//! a line saying who's there and how it stands, with "Terminal" to take the same session to
//! a terminal instead.
//!
//! The agent is started on a worker (`acp-start`: found, then started) and read on its own
//! threads; what it says arrives by a poll. The lines are laid out per item, again only for
//! what changed or another width; only those in view are drawn.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use acp::{Conversation, Item};
use acp_client::{AgentProcess, Event};
use agents::{AgentSession, Launch};
use contracts::terminal::{RunInTerminal, TerminalPlace};
use i18n::t;
use kernel::{Point, Rect, Rgba};
use render_api::Quad;
use ui_components::ds::{self, Ctx, radius};
use ui_components::{
    Asked, EdgeScrollbar, KeyReach, Viewer, ViewerAction, ViewerCursor, ViewerKey, ViewerPaint,
};
use ui_kit::{TextInput, draw_text};

use crate::layout::{self, Ink, Laid, Link};

/// What the tab is opened with.
#[derive(Debug, Clone)]
pub struct ChatStart {
    pub agent: &'static str,
    pub session: Option<String>,
    pub folder: PathBuf,
    pub title: String,
}

/// How the agent stands.
enum Phase {
    /// Not asked yet (the first paint starts it).
    Idle,
    Starting(Receiver<Started>),
    /// Its adapter isn't installed: how it would be.
    Missing(Option<Launch>),
    Running(AgentProcess),
    Failed(String),
    /// Taken to a terminal: the tab closes.
    Moved,
}

/// What starting it came to.
enum Started {
    Agent(AgentProcess),
    Missing(Option<Launch>),
    Failed(String),
}

/// What a click is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Send,
    Stop,
    Choice(usize),
    Link(Link),
    Install,
    Terminal,
}

/// The conversation's column at most this wide, the field's height, a line's height as a
/// share of the font's size (logical pixels where not said).
const COLUMN: f32 = 820.0;
const FIELD_H: f32 = 44.0;
const LINE: f32 = 1.55;

pub struct ChatView {
    start: Option<ChatStart>,
    phase: Phase,
    /// The session's id, once it's open.
    session: Option<String>,
    conversation: Conversation,
    input: TextInput,
    waker: Option<Arc<dyn Fn() + Send + Sync>>,
    /// The items laid out (each for its revision), at a width and for a version.
    laid: Vec<Laid>,
    laid_for: Option<(u64, u32)>,
    scroll: f32,
    max_scroll: f32,
    /// Following the end while it grows (scrolled up: left where it is).
    following: bool,
    bar: EdgeScrollbar,
    area: Rect,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
    field: Rect,
    action: Option<ViewerAction>,
    /// What went wrong last (said on the status line).
    problem: Option<String>,
    /// Its agent's icon (its tab's), found once.
    icon: Option<&'static str>,
    /// A made-up conversation, no agent (snapshots).
    #[cfg(feature = "snapshot")]
    sample: bool,
}

impl Default for ChatView {
    fn default() -> Self {
        Self {
            start: None,
            phase: Phase::Idle,
            session: None,
            conversation: Conversation::default(),
            input: TextInput::default(),
            waker: None,
            laid: Vec::new(),
            laid_for: None,
            scroll: 0.0,
            max_scroll: 0.0,
            following: true,
            bar: EdgeScrollbar::default(),
            area: Rect::default(),
            hits: Vec::new(),
            hover: None,
            field: Rect::default(),
            action: None,
            problem: None,
            icon: None,
            #[cfg(feature = "snapshot")]
            sample: false,
        }
    }
}

impl std::fmt::Debug for ChatView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatView")
            .field("items", &self.conversation.items.len())
            .finish_non_exhaustive()
    }
}

impl ChatView {
    fn agent(&self) -> Option<Arc<dyn agents::AgentProvider>> {
        agents::provider(self.start.as_ref()?.agent)
    }

    /// The agent found and started on a worker (once).
    fn begin(&mut self) {
        let (Some(start), Some(agent)) = (self.start.clone(), self.agent()) else { return };
        let wake = self.waker.clone().unwrap_or_else(|| Arc::new(|| {}));
        let (tx, rx) = std::sync::mpsc::channel();
        kernel::work::run("acp-start", move || {
            let started = match agent.acp(&start.folder) {
                None => Started::Missing(agent.acp_install()),
                Some(launch) => {
                    let cwd = start.folder.to_string_lossy();
                    match AgentProcess::start(
                        &launch,
                        &cwd,
                        start.session.as_deref(),
                        Arc::clone(&wake),
                    ) {
                        Ok(process) => Started::Agent(process),
                        Err(why) => Started::Failed(why),
                    }
                }
            };
            let _ = tx.send(started);
            wake();
        });
        self.phase = Phase::Starting(rx);
    }

    /// What the agent said since, taken in: `true` if anything changed.
    fn take(&mut self) -> bool {
        let mut changed = false;
        if let Phase::Starting(rx) = &self.phase
            && let Ok(started) = rx.try_recv()
        {
            self.phase = match started {
                Started::Agent(process) => Phase::Running(process),
                Started::Missing(install) => Phase::Missing(install),
                Started::Failed(why) => Phase::Failed(why),
            };
            changed = true;
        }
        let Phase::Running(process) = &self.phase else { return changed };
        let events = process.events();
        let errors = events.iter().any(|e| matches!(e, Event::Exited)).then(|| process.errors());
        for event in events {
            changed = true;
            match event {
                Event::Ready { session } => self.session = Some(session),
                Event::Update(update) => {
                    self.conversation.update(&update);
                }
                Event::Ask { id, params } => self.conversation.ask(id, &params),
                Event::TurnEnded(_) => self.conversation.ended(),
                Event::Failed(why) => self.problem = Some(why),
                Event::NotResumed => self.problem = Some(t("chat.not_resumed")),
                Event::Exited => {
                    let last = errors.as_ref().and_then(|e| e.last().cloned());
                    self.phase = Phase::Failed(last.unwrap_or_else(|| t("chat.ended")));
                    self.conversation.ended();
                    break;
                }
            }
        }
        changed
    }

    /// What's written, sent (while the session is open and no turn runs).
    fn send(&mut self) -> bool {
        let text = self.input.text().trim().to_owned();
        let Phase::Running(process) = &self.phase else { return false };
        if text.is_empty() || self.conversation.running || !process.prompt(&text) {
            return false;
        }
        self.conversation.typed(&text);
        self.input = TextInput::default();
        self.following = true;
        true
    }

    fn stop(&mut self) -> bool {
        let Phase::Running(process) = &self.phase else { return false };
        if !self.conversation.running {
            return false;
        }
        process.cancel();
        true
    }

    /// Permission choice `i`, answered.
    fn choose(&mut self, i: usize) {
        let Phase::Running(process) = &self.phase else { return };
        let Some(permission) = &self.conversation.permission else { return };
        let Some(choice) = permission.choices.get(i).map(|c| c.id.clone()) else { return };
        if let Some(id) = self.conversation.answered() {
            process.answer(&id, Some(&choice));
        }
    }

    /// Change `diff` of tool `item`, compared: as it was, as the agent makes it.
    fn compare(&mut self, item: usize, diff: usize) {
        let Some(Item::Tool(tool)) = self.conversation.items.get(item) else { return };
        let Some(change) = tool.diffs.get(diff) else { return };
        let path = PathBuf::from(&change.path);
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let input = diff::CompareInput {
            left: (path.clone(), change.old.clone().unwrap_or_default()),
            right: (path, change.new.clone()),
            labels: [Some(t("chat.before")), Some(t("chat.proposed"))],
            read_only: [true, true],
        };
        self.action = Some(ViewerAction::Compare(
            Box::new(input),
            format!("{name} · {}", t("chat.proposed")),
        ));
    }

    /// The same session taken to a terminal: this tab's agent stopped first (one process a
    /// session), the terminal asked to take it up again.
    fn move_to_terminal(&mut self) {
        let (Some(start), Some(agent)) = (self.start.clone(), self.agent()) else { return };
        let launch = match self.session.clone().or(start.session.clone()) {
            Some(id) => agent.resume(&AgentSession {
                agent: start.agent,
                id,
                title: start.title.clone(),
                folder: start.folder.clone(),
                started: None,
                active: 0,
                file: None,
            }),
            None => agent.new_session(&start.folder),
        };
        let key = match self.session.clone().or(start.session.clone()) {
            Some(id) => format!("{}-{id}", start.agent),
            None => format!("{}-new-{:x}", start.agent, kernel::id::new_uuid()),
        };
        let run = RunInTerminal {
            launch,
            key,
            title: start.title,
            icon: Some(agent.icon()),
            place: TerminalPlace::Editor,
        };
        self.phase = Phase::Moved;
        self.action = Some(ViewerAction::Ask(Asked::new(run)));
    }

    /// Its adapter installed, in a terminal at the bottom (the user sees what runs).
    fn install(&mut self) {
        let Phase::Missing(Some(launch)) = &self.phase else { return };
        let run = RunInTerminal {
            launch: launch.clone(),
            key: "acp-install".into(),
            title: t("chat.install_tab"),
            icon: None,
            place: TerminalPlace::Bottom,
        };
        self.action = Some(ViewerAction::Ask(Asked::new(run)));
        // Looked for again when shown next.
        self.phase = Phase::Idle;
    }

    fn hit_at(&self, p: Point) -> Option<Hit> {
        self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)
    }

    /// The items laid out again where they changed (by their revision: what they hold is
    /// never read for it), or all at another width.
    fn lay_out(
        &mut self,
        text: &mut text_layout::TextSystem,
        font: &text_layout::Font,
        width: f32,
    ) {
        let stamp = (self.conversation.version(), width.to_bits());
        if self.laid_for == Some(stamp) {
            return;
        }
        if self.laid_for.is_some_and(|(_, w)| w != stamp.1) {
            self.laid.clear();
        }
        self.laid.truncate(self.conversation.items.len());
        for (i, item) in self.conversation.items.iter().enumerate() {
            let revision = self.conversation.revision(i);
            if self.laid.get(i).is_some_and(|l| l.revision == revision) {
                continue;
            }
            let at = (i, revision);
            if let Some(kept) = self.laid.get_mut(i) {
                let old = std::mem::take(kept);
                *kept = layout::lay_on(old, item, at, text, font, width);
            } else {
                self.laid.push(layout::lay(item, at, text, font, width));
            }
        }
        self.laid_for = Some(stamp);
    }
}

/// A round icon button at `r` (lit while hovered; `on`: in the accent).
fn icon_button(
    layer: &mut render_api::Layer,
    icons: &icons::Icons,
    ds: &Ctx<'_>,
    r: Rect,
    icon: &str,
    (hovered, on): (bool, bool),
) {
    let p = ds.palette;
    let fill = if on { p.accent } else { p.text.with_alpha(if hovered { 0.12 } else { 0.06 }) };
    layer.quad(Quad::filled(r, fill).rounded(r.size.height / 2.0));
    if let Some(id) = icons.id(icon) {
        let s = ds.px(14.0);
        let at = Rect::new(
            r.min_x() + (r.size.width - s) / 2.0,
            r.min_y() + (r.size.height - s) / 2.0,
            s,
            s,
        );
        ui_kit::draw_icon(layer, icons, id, at, if on { p.editor_background } else { p.text });
    }
}

/// A text button (a permission's choice), returning its area.
fn text_button(
    layer: &mut render_api::Layer,
    text: &mut text_layout::TextSystem,
    ds: &Ctx<'_>,
    (x, mid): (f32, f32),
    label: &str,
    (hovered, primary): (bool, bool),
) -> Rect {
    let p = ds.palette;
    let font = ds.ui_font(-0.5);
    let shaped = text.shape_line(label, &font);
    let r = Rect::new(x, (mid - ds.px(14.0)).round(), shaped.width + ds.px(24.0), ds.px(28.0));
    let fill = if primary {
        p.accent.with_alpha(if hovered { 0.28 } else { 0.18 })
    } else {
        p.text.with_alpha(if hovered { 0.12 } else { 0.06 })
    };
    layer.quad(Quad::filled(r, fill).rounded(ds.px(radius::MD)));
    let color = if primary { p.accent } else { p.text };
    draw_text(
        layer,
        &shaped,
        r.min_x() + ds.px(12.0),
        ds::optical_baseline(r.min_y(), r.size.height, &font),
        color,
    );
    r
}

impl ChatView {
    /// The foot: the permission waited for, the field, the status line. Returns where the
    /// conversation ends (its bottom).
    fn paint_foot(&mut self, ctx: &mut ViewerPaint<'_>, ds: &Ctx<'_>, column: Rect) -> f32 {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let b = ctx.bounds;
        let layer = ctx.scene.push_layer(Some(b));
        let status_h = px(30.0);
        let status_top = b.max_y() - px(8.0) - status_h;
        // The status line: who, how it stands; "Terminal" on its right.
        let small = ds.ui_font(-1.0);
        let name = self.agent().map(|a| a.name()).unwrap_or_default();
        let state = match &self.phase {
            Phase::Idle | Phase::Starting(_) => t("chat.starting"),
            Phase::Missing(_) => t("chat.missing"),
            Phase::Failed(why) => why.clone(),
            Phase::Moved => String::new(),
            Phase::Running(_) if self.session.is_none() => t("chat.opening"),
            Phase::Running(_) if self.conversation.running => t("chat.working"),
            Phase::Running(_) => self.problem.clone().unwrap_or_else(|| t("chat.ready")),
        };
        let line = if name.is_empty() { state } else { format!("{name} · {state}") };
        let room = column.size.width - px(120.0);
        let line = ds::ellipsize(ctx.text, &line, &small, room.max(0.0));
        let shaped = ctx.text.shape_line(&line, &small);
        draw_text(
            layer,
            &shaped,
            column.min_x(),
            ds::optical_baseline(status_top, status_h, &small),
            p.text_muted,
        );
        if self.start.is_some() && !matches!(self.phase, Phase::Moved) {
            let label = t("chat.terminal");
            let x =
                column.max_x() - ctx.text.shape_line(&label, &ds.ui_font(-0.5)).width - px(24.0);
            let hovered = self.hover == Some(Hit::Terminal);
            let r = text_button(
                layer,
                ctx.text,
                ds,
                (x, status_top + status_h / 2.0),
                &label,
                (hovered, false),
            );
            self.hits.push((r, Hit::Terminal));
        }
        // The field, with send (or stop while a turn runs).
        let field = Rect::new(
            column.min_x(),
            status_top - px(6.0) - px(FIELD_H),
            column.size.width,
            px(FIELD_H),
        );
        self.field = field;
        let focused = ctx.focused;
        layer.quad(
            Quad::filled(field, p.input_background)
                .rounded(px(radius::LG))
                .bordered(ds.line(), if focused { p.accent } else { p.input_border }),
        );
        let button =
            Rect::new(field.max_x() - px(36.0), field.min_y() + px(6.0), px(32.0), px(32.0));
        let running = self.conversation.running;
        let (hit, icon) = if running { (Hit::Stop, "stop") } else { (Hit::Send, "arrow-up") };
        let ready = matches!(self.phase, Phase::Running(_)) && self.session.is_some();
        icon_button(
            layer,
            ctx.icons,
            ds,
            button,
            icon,
            (self.hover == Some(hit), ready && (running || !self.input.text().trim().is_empty())),
        );
        self.hits.push((button, hit));
        let style = ui_kit::TextInputStyle {
            background: Rgba::TRANSPARENT,
            border: Rgba::TRANSPARENT,
            border_focused: Rgba::TRANSPARENT,
            border_width: 0.0,
            padding: 0.0,
            lead: 0.0,
            ..ds::field_style(ds, ds.ui_font(0.0), false)
        };
        let area = Rect::new(
            field.min_x() + px(14.0),
            field.min_y(),
            button.min_x() - px(8.0) - field.min_x() - px(14.0),
            field.size.height,
        );
        self.input.paint(layer, ctx.text, area, focused, &t("chat.placeholder"), &style);
        let mut bottom = field.min_y() - px(10.0);
        // A permission waited for: what for, its choices.
        if let Some(permission) = self.conversation.permission.clone() {
            let card_h = px(76.0);
            let card = Rect::new(column.min_x(), bottom - card_h, column.size.width, card_h);
            layer.quad(
                Quad::filled(card, p.accent.with_alpha(0.06))
                    .rounded(px(radius::LG))
                    .bordered(ds.line(), p.accent.with_alpha(0.3)),
            );
            let font = ds.ui_font(-0.5);
            let what = if permission.title.is_empty() {
                t("chat.permission")
            } else {
                permission.title.clone()
            };
            let what = ds::ellipsize(ctx.text, &what, &font, card.size.width - px(28.0));
            let shaped = ctx.text.shape_line(&what, &font);
            draw_text(layer, &shaped, card.min_x() + px(14.0), card.min_y() + px(24.0), p.text);
            let mut x = card.min_x() + px(14.0);
            let mid = card.max_y() - px(22.0);
            for (i, choice) in permission.choices.iter().enumerate() {
                let primary = choice.kind.starts_with("allow");
                let hovered = self.hover == Some(Hit::Choice(i));
                let r =
                    text_button(layer, ctx.text, ds, (x, mid), &choice.name, (hovered, primary));
                self.hits.push((r, Hit::Choice(i)));
                x = r.max_x() + px(8.0);
            }
            bottom = card.min_y() - px(10.0);
        }
        // Its adapter missing: how it's installed.
        if let Phase::Missing(install) = &self.phase {
            let font = ds.ui_font(-0.5);
            let says = install.as_ref().map_or_else(|| t("chat.missing_detail"), |l| l.display());
            let says = ds::ellipsize(ctx.text, &says, &font, column.size.width);
            let shaped = ctx.text.shape_line(&says, &font);
            draw_text(layer, &shaped, column.min_x(), bottom - px(48.0), p.text_muted);
            if install.is_some() {
                let hovered = self.hover == Some(Hit::Install);
                let r = text_button(
                    layer,
                    ctx.text,
                    ds,
                    (column.min_x(), bottom - px(20.0)),
                    &t("chat.install"),
                    (hovered, true),
                );
                self.hits.push((r, Hit::Install));
            }
            bottom -= px(64.0);
        }
        bottom
    }

    fn paint_items(&mut self, ctx: &mut ViewerPaint<'_>, ds: &Ctx<'_>, column: Rect, bottom: f32) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let font = ds.ui_font(0.0);
        let line_h = (font.size * LINE).round();
        let gap = px(14.0);
        let bubble_pad = px(10.0);
        self.lay_out(ctx.text, &font, column.size.width);
        self.area = Rect::new(
            ctx.bounds.min_x(),
            ctx.bounds.min_y(),
            ctx.bounds.size.width,
            (bottom - ctx.bounds.min_y()).max(0.0),
        );
        let heights: Vec<f32> = self
            .laid
            .iter()
            .map(|l| l.lines.len() as f32 * line_h + if l.bubble { bubble_pad * 2.0 } else { 0.0 })
            .collect();
        let content = heights.iter().sum::<f32>() + gap * heights.len() as f32 + px(16.0);
        self.max_scroll = (content - self.area.size.height).max(0.0);
        if self.following {
            self.scroll = self.max_scroll;
        }
        self.scroll = self.scroll.min(self.max_scroll);
        let scroll = self.scroll.round();
        let layer = ctx.scene.push_layer(Some(self.area));
        let mut y = self.area.min_y() + px(16.0) - scroll;
        for (laid, h) in self.laid.iter().zip(&heights) {
            if y > self.area.max_y() {
                break;
            }
            if y + h < self.area.min_y() {
                y += h + gap;
                continue;
            }
            let mut x = column.min_x();
            let mut top = y;
            if laid.bubble {
                let widest = laid
                    .lines
                    .iter()
                    .map(|l| ctx.text.shape_line(&l.text, &font).width)
                    .fold(0.0, f32::max);
                let w = widest + bubble_pad * 2.0;
                let r = Rect::new(column.max_x() - w, y, w, *h);
                layer.quad(Quad::filled(r, p.accent.with_alpha(0.1)).rounded(px(radius::LG)));
                x = r.min_x() + bubble_pad;
                top += bubble_pad;
            }
            for line in &laid.lines {
                let color = match line.ink {
                    Ink::Text => p.text,
                    Ink::Muted => p.text_muted,
                    Ink::Accent => p.accent,
                    Ink::Danger => p.danger,
                };
                let mut lx = x + px(line.indent);
                if let Some(id) = line.icon.and_then(|i| ctx.icons.id(i)) {
                    let s = px(13.0);
                    ui_kit::draw_icon(
                        layer,
                        ctx.icons,
                        id,
                        Rect::new(lx, top + (line_h - s) / 2.0, s, s),
                        color,
                    );
                    lx += px(22.0);
                }
                let shaped = ctx.text.shape_line(&line.text, &font);
                draw_text(layer, &shaped, lx, ds::optical_baseline(top, line_h, &font), color);
                if let Some(link) = line.link {
                    let r = Rect::new(lx, top, shaped.width, line_h);
                    if self.hover == Some(Hit::Link(link)) {
                        let under = Rect::new(lx, top + line_h - px(5.0), shaped.width, ds.line());
                        layer.quad(Quad::filled(under, color));
                    }
                    if let Some(visible) = r.intersect(&self.area) {
                        self.hits.push((visible, Hit::Link(link)));
                    }
                }
                top += line_h;
            }
            y += h + gap;
        }
        let total = self.max_scroll + self.area.size.height;
        self.bar.paint(ctx.scene, p, ctx.scale, self.area, total, scroll);
    }
}

impl Viewer for ChatView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        #[cfg(feature = "snapshot")]
        if data.downcast_ref::<Sample>().is_some() {
            self.show_sample();
            return;
        }
        if let Some(start) = data.downcast_ref::<ChatStart>()
            && self.start.is_none()
        {
            self.start = Some(start.clone());
            self.icon = self.agent().map(|a| a.icon());
        }
    }

    fn poll(&mut self) -> bool {
        self.take()
    }

    fn is_loading(&self) -> bool {
        matches!(self.phase, Phase::Starting(_)) || self.conversation.running
    }

    fn closed(&self) -> bool {
        matches!(self.phase, Phase::Moved) && self.action.is_none()
    }

    fn paint(&mut self, mut ctx: ViewerPaint<'_>) {
        if ctx.waker.is_some() {
            self.waker.clone_from(&ctx.waker);
        }
        #[cfg(feature = "snapshot")]
        let sample = self.sample;
        #[cfg(not(feature = "snapshot"))]
        let sample = false;
        if matches!(self.phase, Phase::Idle) && !sample {
            self.begin();
        }
        self.take();
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let b = ctx.bounds;
        ctx.scene.push_layer(Some(b)).quad(Quad::filled(b, ctx.palette.editor_background));
        let width = ds.px(COLUMN).min(b.size.width - ds.px(48.0)).max(ds.px(200.0));
        let column = Rect::new(
            (b.min_x() + (b.size.width - width) / 2.0).round(),
            b.min_y(),
            width,
            b.size.height,
        );
        self.hits.clear();
        let bottom = self.paint_foot(&mut ctx, &ds, column);
        self.paint_items(&mut ctx, &ds, column, bottom);
    }

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        let before = self.scroll;
        self.scroll = (self.scroll + dy).clamp(0.0, self.max_scroll);
        self.following = self.scroll >= self.max_scroll - 1.0;
        self.scroll != before
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        if let Some(offset) = self.bar.press(p) {
            self.scroll = offset;
            self.following = false;
            return None;
        }
        match self.hit_at(p) {
            Some(Hit::Send) => {
                self.send();
            }
            Some(Hit::Stop) => {
                self.stop();
            }
            Some(Hit::Choice(i)) => self.choose(i),
            Some(Hit::Link(Link::Diff { item, diff })) => self.compare(item, diff),
            Some(Hit::Install) => self.install(),
            Some(Hit::Terminal) => self.move_to_terminal(),
            None if self.field.contains(p) => self.input.press(p.x, 1),
            None => {}
        }
        self.action.take()
    }

    fn drag(&mut self, p: Point) -> bool {
        let Some(offset) = self.bar.drag(p) else { return false };
        self.scroll = offset;
        self.following = self.scroll >= self.max_scroll - 1.0;
        true
    }

    fn release(&mut self) -> bool {
        self.bar.release()
    }

    fn hover(&mut self, p: Point) -> bool {
        let mut changed = self.bar.hover(p);
        let hit = self.hit_at(p);
        changed |= hit != self.hover;
        self.hover = hit;
        changed
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        if self.hit_at(p).is_some() {
            return Some(ViewerCursor::Pointer);
        }
        self.field.contains(p).then_some(ViewerCursor::Text)
    }

    fn wants_text(&self) -> bool {
        true
    }

    fn text_input(&mut self, typed: &str) -> bool {
        let typed: String = typed.chars().filter(|c| !c.is_control()).collect();
        self.input.insert(&typed);
        true
    }

    fn paste(&mut self, text: &str) -> bool {
        self.input.insert(&text.replace(['\r', '\n'], " "));
        true
    }

    fn selected_text(&self) -> Option<String> {
        let selected = self.input.selected_text();
        (!selected.is_empty()).then(|| selected.to_owned())
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        match key {
            ViewerKey::Enter => self.send(),
            ViewerKey::Escape => self.stop(),
            _ => reach.edit(&mut self.input, key),
        }
    }

    fn command(&mut self, id: &str) -> bool {
        match id {
            "select_all" => {
                self.input.select_all();
                true
            }
            "undo" => self.input.undo(),
            "redo" => self.input.redo(),
            "cut" if !self.input.selected_text().is_empty() => {
                self.input.insert("");
                true
            }
            _ => false,
        }
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }

    fn tab_icon(&self) -> Option<&'static str> {
        self.icon
    }

    fn restorable(&self) -> bool {
        false
    }
}

/// A made-up conversation (snapshots).
#[cfg(feature = "snapshot")]
pub struct Sample;

#[cfg(feature = "snapshot")]
impl ChatView {
    fn show_sample(&mut self) {
        use serde_json::json;
        self.sample = true;
        self.session = Some("sample".into());
        let c = &mut self.conversation;
        c.typed("Move the validation into AuthService and write the tests");
        c.update(&json!({"sessionUpdate": "agent_thought_chunk", "content": {"type": "text",
            "text": "The validation is in the controller now; I'll move it and keep the error messages."}}));
        c.update(&json!({"sessionUpdate": "plan", "entries": [
            {"content": "Read AuthController and AuthService", "priority": "high", "status": "completed"},
            {"content": "Move the validation", "priority": "high", "status": "in_progress"},
            {"content": "Write the tests", "priority": "medium", "status": "pending"}]}));
        c.update(&json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Read src/Http/AuthController.php",
            "kind": "read", "status": "completed"}));
        c.update(&json!({"sessionUpdate": "tool_call", "toolCallId": "t2", "title": "Edit src/Auth/AuthService.php",
            "kind": "edit", "status": "completed", "content": [
                {"type": "diff", "path": "/p/src/Auth/AuthService.php", "oldText": "a\nb", "newText": "a\nc\nd"}]}));
        c.update(&json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text",
            "text": "I moved the validation into AuthService::validate(). The controller calls it now before signing in.\n\nNext, the tests: I'm running phpunit to see what passes."}}));
        c.ask(json!(1), &json!({"toolCall": {"toolCallId": "t3", "title": "Run vendor/bin/phpunit", "kind": "execute"},
            "options": [
                {"optionId": "a", "name": "Allow", "kind": "allow_once"},
                {"optionId": "aa", "name": "Always here", "kind": "allow_always"},
                {"optionId": "r", "name": "No", "kind": "reject_once"}]}));
    }
}

/// Opens a made-up conversation (snapshots).
#[cfg(feature = "snapshot")]
pub fn snapshot(host: &mut dyn workbench_model::WorkbenchHost) -> bool {
    host.open_view("refactor auth", crate::EXTENSION, &Sample)
}

#[cfg(test)]
#[path = "../tests/unit/view.rs"]
mod tests;
