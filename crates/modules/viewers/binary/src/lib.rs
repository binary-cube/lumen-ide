//! Binary files module: a file that isn't text (a video, an archive, an executable…) or is
//! too large to edit opens as a tab saying so — its size, why, and a button to reveal it in
//! the file manager — instead of being read whole into the editor (gigabytes of memory for
//! a video). Only the file's first bytes are read to decide.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use document::{MAX_TEXT_LEN, TextCheck, probe_text};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use fs_local::LocalFileSystem;
use i18n::{t, tr};
use kernel::{Point, Rect};
use render_api::Quad;
use text_layout::Font;
use ui_components::ds::{self, Ctx, radius};
use ui_components::{
    AppRequest, Viewer, ViewerAction, ViewerKind, ViewerPaint, ViewerPriority, ViewerProvider,
};
use ui_kit::draw_text;

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("viewer.binary"),
    name: "Binary Files",
    description: "Files that aren't text (videos, archives…) or are too large open as a notice, not in the editor",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "hash", ..extensions::ModuleInfo::BUILT_IN },
};

#[derive(Debug, Default)]
pub struct BinaryFilesModule;

impl Module for BinaryFilesModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(Provider));
        Ok(())
    }
}

#[derive(Debug)]
struct Provider;

impl ViewerProvider for Provider {
    fn id(&self) -> &'static str {
        "binary"
    }

    /// Asked after every other viewer (the images, Markdown, data files): what no one else
    /// shows.
    fn priority(&self) -> ViewerPriority {
        ViewerPriority::Fallback
    }

    /// Reads only the file's start.
    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        match probe_text(path, &LocalFileSystem) {
            Ok(TextCheck::Binary | TextCheck::TooLarge) => Some(ViewerKind::Replace),
            _ => None,
        }
    }

    fn open(&self, path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        let check = probe_text(path, &LocalFileSystem)?;
        let size = std::fs::metadata(path)?.len();
        Ok(Box::new(BinaryView::new(path.to_path_buf(), check, size)))
    }
}

/// The notice shown instead of the file.
#[derive(Debug)]
pub struct BinaryView {
    path: PathBuf,
    check: TextCheck,
    size: u64,
    reveal: Rect,
    reveal_hovered: bool,
}

impl BinaryView {
    pub fn new(path: PathBuf, check: TextCheck, size: u64) -> Self {
        Self { path, check, size, reveal: Rect::default(), reveal_hovered: false }
    }

    /// Why it isn't shown (for the user).
    fn reason(&self) -> String {
        match self.check {
            TextCheck::TooLarge => tr!(
                "binary.too_large",
                size = i18n::bytes(self.size),
                limit = i18n::bytes(MAX_TEXT_LEN)
            ),
            _ => t("binary.not_text"),
        }
    }
}

/// The file manager's name on this system (the button's label).
fn reveal_label() -> String {
    if cfg!(target_os = "macos") {
        t("command.reveal_finder")
    } else if cfg!(target_os = "windows") {
        t("command.reveal_explorer")
    } else {
        t("command.reveal_folder")
    }
}

impl Viewer for BinaryView {
    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        self.reveal
            .contains(p)
            .then(|| ViewerAction::Request(AppRequest::Reveal(self.path.clone())))
    }

    /// A hand over "show anyway".
    fn cursor_at(&self, p: Point) -> Option<ui_components::ViewerCursor> {
        self.reveal.contains(p).then_some(ui_components::ViewerCursor::Pointer)
    }

    /// Its hover says when what it shows changed.
    fn hover_tells_changes(&self) -> bool {
        true
    }

    fn hover(&mut self, p: Point) -> bool {
        let hovered = self.reveal.contains(p);
        let changed = hovered != self.reveal_hovered;
        self.reveal_hovered = hovered;
        changed
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let p = ctx.palette;
        let px = |v: f32| ds.px(v);
        let b = ctx.bounds;
        let layer = ctx.scene.push_layer(Some(b));
        layer.quad(Quad::filled(b, p.editor_background).rounded(px(radius::CARD)));

        let name = self.path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into());
        let title_font = ds.ui_font(3.0).with_weight(Font::SEMIBOLD);
        let small = ds.ui_font(-0.5);
        let max = b.size.width - px(64.0);
        let lines = [
            (ds::ellipsize_middle(ctx.text, &name, &title_font, max), &title_font, p.text),
            (t("binary.title"), &small, p.text_muted),
            (self.reason(), &small, p.text_muted),
        ];
        let button_h = px(ds::BUTTON_H);
        let gap = px(10.0);
        let height: f32 =
            lines.iter().map(|(_, f, _)| f.size * 1.4).sum::<f32>() + gap * 2.0 + button_h;
        let mut y = (b.min_y() + (b.size.height - height) / 2.0).round();
        for (i, (line, font, color)) in lines.iter().enumerate() {
            let shaped = ctx.text.shape_line(line, font);
            let x = (b.min_x() + (b.size.width - shaped.width) / 2.0).round();
            draw_text(layer, &shaped, x, (y + font.size).round(), *color);
            y += font.size * 1.4 + if i == 0 { gap } else { 0.0 };
        }
        y += gap;
        let label = reveal_label();
        let w = ds::primary_width(ctx.text, &ds, &[&label]);
        self.reveal = Rect::new((b.min_x() + (b.size.width - w) / 2.0).round(), y, w, button_h);
        ds::primary_button(layer, ctx.text, &ds, self.reveal, &label, true, self.reveal_hovered);
    }

    fn status(&self) -> Option<String> {
        Some(i18n::bytes(self.size))
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
