//! The bar over a remote project's file that waits to go up (uploads off, the file in the
//! "To upload" list): why — changed here, only here, moved, changed on the server too, or
//! why it couldn't go — with "Compare" (with the server's copy) and "Upload" (a spinner on it
//! while the file goes, the button widening for it and narrowing back, eased; pressed again
//! meanwhile, nothing). It follows the
//! session's list (read in the background; the bar only looks at its generation), and has no
//! height otherwise.

use std::path::{Path, PathBuf};

use i18n::{t, tr};
use kernel::{Point, ProjectId, Rect};
use render_api::Quad;
use ui_components::ViewerCursor;
use ui_components::anim::Tween;
use ui_components::ds::{self, Ctx};
use ui_components::editor_toolbar::{
    EditorToolbar, EditorToolbarProvider, HEIGHT, ToolbarAction, ToolbarContext, ToolbarPaint,
};
use ui_kit::{draw_icon, draw_text};

use crate::outgoing::Why;

/// The bars' provider: one for each file of a remote project (shown only when it waits).
#[derive(Debug, Default)]
pub(crate) struct UploadBarProvider;

impl EditorToolbarProvider for UploadBarProvider {
    fn id(&self) -> &'static str {
        "remote-upload"
    }

    fn open(&self, context: &ToolbarContext<'_>) -> Option<Box<dyn EditorToolbar>> {
        let project = context.project?;
        let root = context.root?;
        // A local project: no bar.
        crate::session::session(project)?;
        let relative = relative(root, context.path)?;
        Some(Box::new(UploadBar::new(project, root.to_path_buf(), relative)))
    }
}

/// `path` relative to `root`, with `/`.
fn relative(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let parts: Vec<String> =
        rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Button {
    Compare,
    Upload,
}

/// A file's bar (see the module's doc).
#[derive(Debug)]
pub struct UploadBar {
    project: ProjectId,
    root: PathBuf,
    relative: String,
    /// The list's generation and the switch last looked at.
    seen: Option<(u64, bool)>,
    /// Its file sent and still going (Upload's spinner).
    sending: bool,
    /// How far Upload has made room for its spinner (0..=1), easing toward `sending`.
    busy: Tween,
    /// Its row in the list, while it shows.
    row: Option<(String, Why)>,
    buttons: Vec<(Rect, Button)>,
    hovered: Option<Button>,
}

impl UploadBar {
    pub(crate) fn new(project: ProjectId, root: PathBuf, relative: String) -> Self {
        let mut bar = Self {
            project,
            root,
            relative,
            seen: None,
            sending: false,
            busy: Tween::new(0.0, ds::BUSY_TRANSITION),
            row: None,
            buttons: Vec::new(),
            hovered: None,
        };
        bar.resolve();
        // Already going when opened: shown so, not growing.
        bar.busy.set(if bar.sending { 1.0 } else { 0.0 });
        bar
    }

    /// Whether its file is going now: Upload's spinner eases in or out. `true` if it changed.
    fn follow_sending(&mut self, sending: bool) -> bool {
        self.busy.retarget(if sending { 1.0 } else { 0.0 });
        std::mem::replace(&mut self.sending, sending) != sending
    }

    /// Its row again when the list or the switch changed, and whether its file is going:
    /// `true` if either did.
    fn resolve(&mut self) -> bool {
        let Some(state) = crate::session::session(self.project) else {
            let changed = self.row.is_some();
            self.row = None;
            return self.follow_sending(false) | changed;
        };
        self.follow_sending(state.is_sending(&self.relative)) | self.resolve_row(&state)
    }

    /// Its row again when the list or the switch changed: `true` if it did.
    fn resolve_row(&mut self, state: &crate::session::State) -> bool {
        let (generation, rows) = state.outgoing.rows();
        let now = (generation, state.auto_upload());
        if self.seen == Some(now) {
            return false;
        }
        self.seen = Some(now);
        let row =
            (!now.1).then(|| rows.iter().find(|(p, _)| *p == self.relative).cloned()).flatten();
        let changed = row != self.row;
        self.row = row;
        changed
    }

    /// What it says of its file.
    fn message(why: &Why) -> String {
        match why {
            Why::Changed => t("remote.bar_changed"),
            Why::New => t("remote.bar_new"),
            Why::Renamed { from } => tr!("remote.bar_renamed", from = from.as_str()),
            Why::Conflict => t("remote.bar_conflict"),
            Why::Waiting(reason) => tr!("remote.bar_waiting", reason = reason.as_str()),
            Why::Deleted => t("remote.pending_deleted"),
        }
    }

    fn press(&mut self, button: Button) {
        let (Some(state), Some(row)) = (crate::session::session(self.project), self.row.clone())
        else {
            return;
        };
        match button {
            Button::Upload if self.sending => {}
            Button::Upload => {
                state.send(std::slice::from_ref(&row));
                self.follow_sending(true);
            }
            Button::Compare => {
                let there = match &row.1 {
                    Why::Renamed { from } => from.as_str(),
                    _ => row.0.as_str(),
                };
                state.compare_with_server(&row.0, there);
            }
        }
    }

    fn button_at(&self, p: Point) -> Option<Button> {
        self.buttons.iter().find(|(r, _)| r.contains(p)).map(|(_, b)| *b)
    }
}

impl EditorToolbar for UploadBar {
    fn height(&self) -> f32 {
        if self.row.is_some() { HEIGHT } else { 0.0 }
    }

    fn paint(&mut self, ctx: ToolbarPaint<'_>) {
        let Some((_, why)) = self.row.clone() else { return };
        let ToolbarPaint { bounds: b, scale, text, scene, palette: p, icons } = ctx;
        let ds = Ctx::new(p, scale);
        let px = |v: f32| ds.px(v);
        let layer = scene.push_layer(Some(b));
        let warn = matches!(why, Why::Conflict | Why::Waiting(_));
        let tint = if warn { p.warning } else { p.accent };
        layer.quad(Quad::filled(b, tint.with_alpha(0.07)));
        ds::divider(layer, &ds, b.min_x(), b.max_x(), b.max_y() - ds.line());
        let mid = (b.min_y() + b.size.height / 2.0).round();
        // The buttons, from the right: Upload (the main one), Compare.
        let button_font = ds::button_font(&ds);
        let h = px(26.0);
        let mut right = b.max_x() - px(12.0);
        self.buttons.clear();
        for (button, key) in
            [(Button::Upload, "remote.upload"), (Button::Compare, "remote.compare")]
        {
            let label = t(key);
            let busy = if button == Button::Upload { self.busy.value() } else { 0.0 };
            let extra = px(ds::BUSY_EXTRA) * busy;
            let w = (text.shape_line(&label, &button_font).width + px(24.0) + extra).round();
            let r = Rect::new((right - w).round(), (mid - h / 2.0).round(), w, h);
            let hovered = self.hovered == Some(button);
            if button == Button::Upload {
                ds::busy_primary_button(layer, text, &ds, r, &label, busy, hovered);
            } else {
                ds::secondary_button(layer, text, &ds, r, &label, true, hovered);
            }
            self.buttons.push((r, button));
            right = r.min_x() - px(8.0);
        }
        // The icon and why, cut to what's left.
        let mut x = b.min_x() + px(12.0);
        let s = px(14.0);
        let icon = if warn { "alert" } else { "upload" };
        if let Some(id) = icons.id(icon) {
            draw_icon(layer, icons, id, Rect::new(x.round(), (mid - s / 2.0).round(), s, s), tint);
        }
        x += s + px(8.0);
        let font = ds.ui_font(-1.0);
        let room = (right - px(8.0) - x).max(0.0);
        let shown = ds::ellipsize(text, &Self::message(&why), &font, room);
        let line = text.shape_line(&shown, &font);
        draw_text(
            layer,
            &line,
            x.round(),
            ds::optical_baseline(b.min_y(), b.size.height, &font),
            p.text,
        );
    }

    fn pointer_down(&mut self, p: Point) -> Option<ToolbarAction> {
        let button = self.button_at(p)?;
        self.press(button);
        None
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        let on = p.and_then(|p| self.button_at(p));
        std::mem::replace(&mut self.hovered, on) != on
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        self.button_at(p)
            .filter(|b| !(*b == Button::Upload && self.sending))
            .map(|_| ViewerCursor::Pointer)
    }

    /// Its file going: Upload's spinner turns.
    fn is_loading(&self) -> bool {
        self.sending
    }

    /// Upload widening for its spinner, or narrowing back.
    fn is_animating(&self) -> bool {
        self.busy.animating()
    }

    fn poll(&mut self) -> bool {
        self.resolve()
    }

    fn moved(&mut self, path: &Path) -> bool {
        // Still the project's: it follows the file.
        match relative(&self.root, path) {
            Some(relative) => {
                self.relative = relative;
                self.seen = None;
                self.resolve();
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/upload_bar.rs"]
mod tests;
