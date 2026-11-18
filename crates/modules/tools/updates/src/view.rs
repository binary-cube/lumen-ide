//! What's New: a modal of the module's own (`host.open_modal`: closed only by its buttons,
//! × or Esc) with the version found, the one running, when it was released and how much
//! it downloads, its notes as written for the app ([`app_updates::notes::for_app`]: titles,
//! points, paragraphs, wrapped to its width — laid out again only when the width does), and
//! the choice: update (or restart, once it's ready), skip this version, or close.

use std::path::Path;

use app_updates::notes::{Kind, Line};
use i18n::t;
use kernel::{Point, Rect};
use ui_components::ds::{self, Ctx};
use ui_components::{
    EdgeScrollbar, Viewer, ViewerAction, ViewerKey, ViewerKind, ViewerPaint, ViewerProvider,
    WithHost,
};
use ui_kit::draw_text;

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

/// The notes laid out for a width: each line's kind and its wrapped rows.
type Laid = Vec<(Kind, Vec<String>)>;

/// The modal's view.
#[derive(Debug, Default)]
pub struct NotesView {
    notes: Option<Notes>,
    laid: Option<(f32, f32, Laid)>,
    scroll: f32,
    bar: EdgeScrollbar,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
    action: Option<ViewerAction>,
    done: bool,
}

/// Logical sizes.
const ROW: f32 = 22.0;
const TITLE_ROW: f32 = 28.0;
const GAP: f32 = 10.0;
const FOOTER: f32 = 64.0;
const HEADER: f32 = 40.0;

impl NotesView {
    fn press(&mut self, hit: Hit) {
        match hit {
            Hit::Go => {
                // Downloading: nothing more to do here (its notification follows it).
                if matches!(state::phase(), Phase::Downloading { .. }) {
                    return;
                }
                self.finish(|host| crate::install(host));
            }
            Hit::Skip => self.finish(|host| crate::skip(host)),
            Hit::Close => {
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

    fn hit(&self, p: Point) -> Option<Hit> {
        self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)
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

    /// The notes wrapped to `width`, made again only when the width or the scale changed.
    fn lay(&mut self, text: &mut text_layout::TextSystem, ds: &Ctx<'_>, width: f32) {
        let stale = self.laid.as_ref().is_none_or(|(w, s, _)| *w != width || *s != ds.scale);
        if stale {
            let body = ds.ui_font(0.0);
            let title = ds.ui_font(1.0).with_weight(text_layout::Font::SEMIBOLD);
            let laid = self
                .notes
                .iter()
                .flat_map(|n| &n.lines)
                .map(|line| {
                    let rows = match line.kind {
                        Kind::Gap => Vec::new(),
                        Kind::Title => ds::wrap(text, &line.text, &title, width, 4),
                        Kind::Point { depth } => {
                            let indent = ds.px(18.0) * (f32::from(depth) + 1.0);
                            ds::wrap(text, &line.text, &body, width - indent, 12)
                        }
                        Kind::Text => ds::wrap(text, &line.text, &body, width, 40),
                    };
                    (line.kind, rows)
                })
                .collect();
            self.laid = Some((width, ds.scale, laid));
        }
    }
}

impl Viewer for NotesView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        if let Some(notes) = data.downcast_ref::<Notes>() {
            self.notes = Some(notes.clone());
            self.laid = None;
        }
    }

    fn subtitle(&self) -> Option<String> {
        let notes = self.notes.as_ref()?;
        Some(i18n::tr!(
            "updates.notes_subtitle",
            current = kernel::brand::VERSION,
            date = notes.date.as_str(),
            size = i18n::bytes(notes.size)
        ))
    }

    fn modal_height(&self) -> Option<f32> {
        Some(520.0)
    }

    fn modal_icon(&self) -> Option<&'static str> {
        Some(crate::ICON)
    }

    fn key_hints(&self) -> Vec<(Vec<&'static str>, String)> {
        vec![(vec!["↵"], self.go_label().0)]
    }

    fn closed(&self) -> bool {
        self.done
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        let ViewerPaint { bounds: b, scale, text, scene, palette: p, .. } = ctx;
        let ds = Ctx::new(p, scale);
        let px = |v: f32| ds.px(v);
        self.hits.clear();
        let pad = px(24.0);
        let notes_box = Rect::new(
            b.min_x() + pad,
            b.min_y() + px(HEADER) - px(30.0),
            b.size.width - 2.0 * pad,
            (b.size.height - px(FOOTER) - px(HEADER) + px(22.0)).max(px(40.0)),
        );
        let empty = self.notes.as_ref().is_none_or(|n| n.lines.is_empty());
        self.lay(text, &ds, notes_box.size.width - px(12.0));
        // Taken out while it's drawn (no copy), put back after.
        let laid = self.laid.take().map(|(_, _, l)| l).unwrap_or_default();
        let content: f32 = laid
            .iter()
            .map(|(kind, rows)| match kind {
                Kind::Gap => px(GAP),
                Kind::Title => rows.len() as f32 * px(TITLE_ROW),
                _ => rows.len() as f32 * px(ROW),
            })
            .sum();
        self.scroll = self.scroll.clamp(0.0, (content - notes_box.size.height).max(0.0));
        let layer = scene.push_layer(Some(notes_box));
        let body = ds.ui_font(0.0);
        let title = ds.ui_font(1.0).with_weight(text_layout::Font::SEMIBOLD);
        let mut y = notes_box.min_y() - self.scroll;
        if empty {
            let shaped = text.shape_line(&t("updates.notes_none"), &body);
            draw_text(
                layer,
                &shaped,
                notes_box.min_x(),
                ds::optical_baseline(y, px(ROW), &body),
                p.text_muted,
            );
        }
        for (kind, rows) in &laid {
            match kind {
                Kind::Gap => y += px(GAP),
                Kind::Title => {
                    for row in rows {
                        let shaped = text.shape_line(row, &title);
                        let base = ds::optical_baseline(y, px(TITLE_ROW), &title);
                        draw_text(layer, &shaped, notes_box.min_x(), base, p.text);
                        y += px(TITLE_ROW);
                    }
                }
                Kind::Point { depth } => {
                    let indent = px(18.0) * (f32::from(*depth) + 1.0);
                    for (i, row) in rows.iter().enumerate() {
                        let base = ds::optical_baseline(y, px(ROW), &body);
                        if i == 0 {
                            let dot = text.shape_line("•", &body);
                            draw_text(
                                layer,
                                &dot,
                                notes_box.min_x() + indent - px(12.0),
                                base,
                                p.accent,
                            );
                        }
                        let shaped = text.shape_line(row, &body);
                        draw_text(layer, &shaped, notes_box.min_x() + indent, base, p.text);
                        y += px(ROW);
                    }
                }
                Kind::Text => {
                    for row in rows {
                        let shaped = text.shape_line(row, &body);
                        let base = ds::optical_baseline(y, px(ROW), &body);
                        draw_text(layer, &shaped, notes_box.min_x(), base, p.text);
                        y += px(ROW);
                    }
                }
            }
        }
        let width = notes_box.size.width - px(12.0);
        self.laid = Some((width, ds.scale, laid));
        self.bar.paint(scene, p, scale, notes_box, content, self.scroll);
        // The buttons, right to left: the main one, Skip, Close.
        let layer = scene.push_layer(Some(b));
        let bh = px(ds::BUTTON_H);
        let by = b.max_y() - px(FOOTER) + (px(FOOTER) - bh) / 2.0;
        let (go, can_go) = self.go_label();
        let w = ds::primary_width(text, &ds, &[&go]);
        let g = Rect::new(b.max_x() - pad - w, by, w, bh);
        ds::primary_button(layer, text, &ds, g, &go, can_go, self.hover == Some(Hit::Go));
        self.hits.push((g, Hit::Go));
        let skip = t("updates.notes_skip");
        let sw = ds::primary_width(text, &ds, &[&skip]);
        let s = Rect::new(g.min_x() - px(10.0) - sw, by, sw, bh);
        ds::secondary_button(layer, text, &ds, s, &skip, true, self.hover == Some(Hit::Skip));
        self.hits.push((s, Hit::Skip));
        let close = t("updates.notes_close");
        let cw = ds::primary_width(text, &ds, &[&close]);
        let c = Rect::new(b.min_x() + pad, by, cw, bh);
        ds::secondary_button(layer, text, &ds, c, &close, true, self.hover == Some(Hit::Close));
        self.hits.push((c, Hit::Close));
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        if let Some(hit) = self.hit(p) {
            self.press(hit);
        }
        self.action.take()
    }

    fn hover(&mut self, p: Point) -> bool {
        let hover = self.hit(p);
        let changed = hover != self.hover;
        self.hover = hover;
        changed | self.bar.hover(p)
    }

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        self.scroll = (self.scroll + dy).max(0.0);
        true
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        match key {
            ViewerKey::Enter => self.press(Hit::Go),
            ViewerKey::Escape => self.press(Hit::Close),
            _ => return false,
        }
        true
    }

    fn command(&mut self, id: &str) -> bool {
        match id {
            "go" => self.press(Hit::Go),
            "skip" => self.press(Hit::Skip),
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
