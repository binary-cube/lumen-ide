//! What's New: a modal of the module's own (`host.open_modal`: closed only by its buttons,
//! × or Esc) with the version found, the one running, when it was released and how much
//! it downloads, its notes as written for the app ([`app_updates::notes::for_app`]: titles,
//! points, paragraphs — a [`ReadText`]: selected, copied, scrolled by the wheel and its
//! bar), and the choice: update (or restart, once it's ready), skip this version, or close.

use std::path::Path;

use app_updates::notes::{Kind, Line};
use i18n::t;
use kernel::{Point, Rect};
use ui_components::ds::{self, Ctx};
use ui_components::{
    Block, BlockKind, PressMods, ReadText, Viewer, ViewerAction, ViewerCursor, ViewerKey,
    ViewerKind, ViewerPaint, ViewerProvider, WithHost,
};

use crate::state::{self, Phase};

/// The modal's virtual extension.
pub const EXTENSION: &str = "view-update-notes";

/// What the modal is opened with: the version it's about (its notes read then).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notes {
    pub version: String,
    pub date: String,
    pub size: u64,
    pub lines: Vec<Line>,
}

impl Notes {
    /// The version found now, if any.
    pub fn current() -> Option<Self> {
        let phase = state::phase();
        let available = phase.available()?;
        let release = &available.release;
        let notes = app_updates::notes::for_app(&release.notes);
        Some(Self {
            version: release.version.to_string(),
            date: release.published.get(..10).unwrap_or(&available.manifest.date).to_owned(),
            size: available.file.size,
            lines: app_updates::notes::lines(&notes),
        })
    }

    /// Its title.
    pub fn title(&self) -> String {
        i18n::tr!("updates.notes_title", app = kernel::brand::NAME, version = self.version.as_str())
    }
}

#[derive(Debug, Default)]
pub struct NotesProvider;

impl ViewerProvider for NotesProvider {
    fn id(&self) -> &'static str {
        EXTENSION
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        (path.extension().and_then(|e| e.to_str()) == Some(EXTENSION))
            .then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(NotesView::default()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Go,
    Skip,
    Close,
}

/// The modal's view.
#[derive(Debug, Default)]
pub struct NotesView {
    notes: Option<Notes>,
    read: ReadText,
    action: Option<ViewerAction>,
    done: bool,
}

/// The notes as blocks to read (none written: a line saying so).
fn blocks(lines: &[Line]) -> Vec<Block> {
    if lines.is_empty() {
        return vec![Block::new(BlockKind::Paragraph, t("updates.notes_none"))];
    }
    lines
        .iter()
        .map(|line| {
            let kind = match line.kind {
                Kind::Title => BlockKind::Heading,
                Kind::Point { depth } => BlockKind::Point { depth },
                Kind::Text => BlockKind::Paragraph,
                Kind::Gap => BlockKind::Gap,
            };
            Block::new(kind, line.text.clone())
        })
        .collect()
}

/// Logical sizes: a block's row as the notes draw it, the window's least and most.
const HEADING_ROW: f32 = 28.0;
const BODY_ROW: f32 = 22.0;
const GAP_ROW: f32 = 10.0;
const MIN_HEIGHT: f32 = 240.0;
const MAX_HEIGHT: f32 = 520.0;
/// As wide as the projects list (a medium modal): notes are short lines, read better
/// narrow; a long title is cut with `…`, never widening it.
const WIDTH: f32 = 680.0;
/// Room under the notes for the download's progress (the buttons are the window's footer).
const FOOTER: f32 = 48.0;
const HEADER: f32 = 40.0;

impl NotesView {
    fn press(&mut self, hit: Hit) {
        match hit {
            // Downloaded here (the window stays, its progress shown); once ready, the
            // restart. Downloading: nothing to press.
            Hit::Go => match state::phase() {
                Phase::Available(_) => crate::checker::download(),
                Phase::Ready { .. } => self.finish(|host| crate::install(host)),
                Phase::Downloading { .. } | Phase::Idle => {}
            },
            Hit::Skip => self.finish(|host| crate::skip(host)),
            Hit::Close => {
                // Downloaded (Later): in place for the next start.
                crate::later();
                self.done = true;
                self.action = Some(ViewerAction::Close);
            }
        }
    }

    fn finish(
        &mut self,
        job: impl FnOnce(&mut dyn workbench_model::WorkbenchHost) + Send + 'static,
    ) {
        self.done = true;
        self.action = Some(ViewerAction::WithHost(WithHost::once(job)));
    }

    /// The main button's label as things are: update, downloading (how far), restart.
    fn go_label(&self) -> (String, bool) {
        match state::phase() {
            Phase::Ready { .. } => (t("updates.restart_now"), true),
            Phase::Downloading { received, total, .. } => {
                let percent = received.saturating_mul(100).checked_div(total).unwrap_or(0);
                (i18n::tr!("updates.notes_downloading", percent = percent), false)
            }
            _ => (t("updates.notes_update"), true),
        }
    }
}

/// Closed: the download's notification can show again.
impl Drop for NotesView {
    fn drop(&mut self) {
        if self.notes.is_some() {
            state::notes_open(false);
        }
    }
}

impl Viewer for NotesView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        if let Some(notes) = data.downcast_ref::<Notes>() {
            self.read.set_blocks(blocks(&notes.lines));
            self.notes = Some(notes.clone());
            state::notes_open(true);
        }
    }

    fn subtitle(&self) -> Option<String> {
        let notes = self.notes.as_ref()?;
        Some(i18n::tr!(
            "updates.notes_subtitle",
            current = kernel::brand::version(),
            date = notes.date.as_str(),
            size = i18n::bytes(notes.size)
        ))
    }

    fn modal_width(&self) -> Option<f32> {
        Some(WIDTH)
    }

    fn modal_title_widens(&self) -> bool {
        false
    }

    /// As tall as its notes (unwrapped: a long line taking two rows scrolls a little),
    /// between a short window and [`MAX_HEIGHT`]; past it, they scroll.
    fn modal_height(&self) -> Option<f32> {
        let notes: f32 = self
            .read
            .blocks()
            .iter()
            .map(|b| match b.kind {
                BlockKind::Heading => HEADING_ROW,
                BlockKind::Gap => GAP_ROW,
                _ => BODY_ROW,
            })
            .sum();
        Some((notes + FOOTER + HEADER).clamp(MIN_HEIGHT, MAX_HEIGHT))
    }

    fn modal_icon(&self) -> Option<&'static str> {
        Some(crate::ICON)
    }

    /// In the window's footer, the first the rightmost: the main one (↵), Skip, Close.
    /// While it downloads, Skip can't be pressed (it would stop it); Close only hides the
    /// window (the download goes on, its notification follows it).
    fn modal_buttons(&self) -> Vec<ui_components::ModalButton> {
        use ui_components::ModalButton;
        let (go, can_go) = self.go_label();
        // Downloaded: only when to restart is left (now, or later — at the next start);
        // skipping it means nothing any more.
        if matches!(state::phase(), Phase::Ready { .. }) {
            return vec![
                ModalButton::main("go", go, can_go),
                ModalButton::other("close", t("updates.notes_later")),
            ];
        }
        let mut skip = ModalButton::other("skip", t("updates.notes_skip"));
        skip.enabled = !matches!(state::phase(), Phase::Downloading { .. });
        vec![
            ModalButton::main("go", go, can_go),
            skip,
            ModalButton::other("close", t("updates.notes_close")),
        ]
    }

    /// Why the download failed (its progress is under the notes).
    fn modal_status(&self) -> Option<ui_components::ModalStatus> {
        use ui_components::ModalStatus;
        match state::phase() {
            // How far it went is under the notes: only a failure here.
            Phase::Downloading { .. } | Phase::Ready { .. } => None,
            _ => state::download_failed().map(ModalStatus::Failed),
        }
    }

    /// Frames while it downloads (the progress moving).
    fn is_loading(&self) -> bool {
        matches!(state::phase(), Phase::Downloading { .. })
    }

    fn closed(&self) -> bool {
        self.done
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        let ViewerPaint { bounds: b, scale, text, scene, palette: p, .. } = ctx;
        let ds = Ctx::new(p, scale);
        let px = |v: f32| ds.px(v);
        let pad = px(24.0);
        let notes_box = Rect::new(
            b.min_x() + pad,
            b.min_y() + px(HEADER) - px(30.0),
            b.size.width - 2.0 * pad,
            (b.size.height - px(FOOTER) - px(HEADER) + px(22.0)).max(px(40.0)),
        );
        self.read.paint(scene, text, &ds, notes_box);
        // Under the notes, while it downloads or once it's ready: how far.
        let progress = match state::phase() {
            Phase::Downloading { received, total, .. } => Some((
                ds::ProgressBar::counted(received, total),
                i18n::tr!(
                    "updates.downloading_detail",
                    received = i18n::bytes(received),
                    total = i18n::bytes(total)
                ),
            )),
            Phase::Ready { .. } => {
                Some((ds::ProgressBar::of(1.0).tone(ds::Tone::Success), t("updates.notes_ready")))
            }
            _ => None,
        };
        if let Some((bar, label)) = &progress {
            let bar = bar.label(label).percent();
            let h = bar.height(&ds);
            let at =
                Rect::new(notes_box.min_x(), b.max_y() - px(12.0) - h, notes_box.size.width, h);
            bar.paint(scene.push_layer(Some(b)), text, &ds, at);
        }
    }

    /// Over the notes the I-beam (they're selected); elsewhere, the bar too, the arrow (the
    /// buttons are the footer's: a hand).
    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        Some(if self.read.over_text(p) { ViewerCursor::Text } else { ViewerCursor::Arrow })
    }

    fn hover(&mut self, p: Point) -> bool {
        self.read.hover(p)
    }

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        self.read.scroll_by(dy)
    }

    /// A press: the notes' selection (⇧ extends it), or their bar.
    fn press_with(&mut self, p: Point, clicks: u32, mods: PressMods) -> Option<ViewerAction> {
        self.read.press(p, clicks, mods.shift);
        self.action.take()
    }

    fn press(&mut self, p: Point, clicks: u32) -> Option<ViewerAction> {
        self.press_with(p, clicks, PressMods::default())
    }

    fn drag(&mut self, p: Point) -> bool {
        self.read.drag(p)
    }

    fn release(&mut self) -> bool {
        self.read.release()
    }

    fn select_all(&mut self) -> bool {
        self.read.select_all()
    }

    fn selected_text(&self) -> Option<String> {
        self.read.selected_text()
    }

    fn clear_selection(&mut self) -> bool {
        self.read.clear_selection()
    }

    /// Its scrollbar fading in or out.
    fn frame_deadline(&self) -> Option<std::time::Instant> {
        self.read.next_frame()
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        match key {
            ViewerKey::Enter => self.press(Hit::Go),
            ViewerKey::Escape => self.press(Hit::Close),
            _ => return false,
        }
        true
    }

    /// A right-click on the notes: the text's own menu (Copy, Copy All).
    fn context_menu(&mut self, p: kernel::Point) -> Option<Vec<ui_components::ViewMenuEntry>> {
        self.read.contains(p).then(|| self.read.menu())
    }

    fn command(&mut self, id: &str) -> bool {
        if let Some(text) = self.read.copied(id) {
            self.action = Some(ViewerAction::Request(ui_components::AppRequest::CopyText(text)));
            return true;
        }
        match id {
            "go" => self.press(Hit::Go),
            "skip" => self.press(Hit::Skip),
            "close" => self.press(Hit::Close),
            "select_all" => return self.read.select_all(),
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
