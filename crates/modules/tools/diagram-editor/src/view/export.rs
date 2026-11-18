//! The page exported (its right-click menu on the sheet: "Export as SVG… / PNG…"): where to
//! is asked with the system's "Save As" dialog, the page drawn in draw.io's colors, written
//! on a worker (with a text system of its own, let go when it's done); the user told how
//! it went.

use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError};

use i18n::{t, tr};
use ui_components::{AppRequest, ViewMenuEntry, ViewerAction};
use workbench_model::Notification;

use super::DiagramView;
use crate::export::{self, Format};

/// How an export went: where it was written, or why not.
pub(super) type Exported = Result<PathBuf, String>;

/// The page drawn and written to `to` as `format`.
fn write(page: &diagram_model::Page, format: Format, to: &Path) -> Exported {
    let drawing = crate::drawing::draw(page, &export::light());
    let background = page.background().and_then(crate::look::color);
    let mut text = text_layout::TextSystem::new();
    let family = text_layout::FontFamily::SansSerif;
    let bytes = match format {
        Format::Svg => export::svg(&drawing, background, &mut text, &family).into_bytes(),
        Format::Png => export::png(&drawing, background, &mut text, &family, export::PNG_SCALE)?,
    };
    files::write_atomic(to, &bytes).map_err(|e| e.to_string())?;
    Ok(to.to_path_buf())
}

impl DiagramView {
    /// The sheet's menu with nothing chosen: the exports.
    pub(super) fn export_menu(&self) -> Option<Vec<ViewMenuEntry>> {
        self.diagram.as_ref()?;
        Some(vec![
            ViewMenuEntry::item("export:svg", t("diagram_editor.export_svg")),
            ViewMenuEntry::item("export:png", t("diagram_editor.export_png")),
        ])
    }

    /// The name offered: the file's, with the page's when it has several.
    fn export_name(&self, format: Format) -> String {
        let stem =
            self.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let pages = self.diagram.as_ref().map(|d| &d.doc.pages);
        let page = pages
            .filter(|p| p.len() > 1)
            .and_then(|p| p.get(self.page))
            .map(|p| p.name().to_owned());
        let name = match page {
            Some(page) => format!("{stem}-{}", page.replace(['/', '\\', ':'], "-")),
            None => stem,
        };
        format!("{name}.{}", format.extension())
    }

    /// An export's commands: asking where, then the path chosen. `None`: not one of them.
    pub(super) fn export_command(&mut self, id: &str) -> Option<bool> {
        let format = match id {
            "export:svg" => Some(Format::Svg),
            "export:png" => Some(Format::Png),
            _ => None,
        };
        if let Some(format) = format {
            if self.diagram.is_none() {
                return Some(false);
            }
            self.export_as = Some(format);
            let start = self.path.parent().map(Path::to_path_buf);
            let name = self.export_name(format);
            self.action = Some(ViewerAction::Request(AppRequest::PickSavePath { start, name }));
            return Some(true);
        }
        let to = id.strip_prefix("picked-save:")?;
        let format = self.export_as.take()?;
        let page = self.diagram.as_ref()?.doc.pages.get(self.page)?.clone();
        let (to, into, waker) =
            (PathBuf::from(to), Arc::clone(&self.exported), Arc::clone(&self.waker));
        let work = move || {
            let done = write(&page, format, &to);
            *into.lock().unwrap_or_else(PoisonError::into_inner) = Some(done);
            if let Some(wake) = waker.lock().unwrap_or_else(PoisonError::into_inner).clone() {
                wake();
            }
        };
        // Without a waker (tests): here and now.
        if self.waker.lock().unwrap_or_else(PoisonError::into_inner).is_none() {
            work();
        } else {
            kernel::work::run("diagram-export", work);
        }
        Some(true)
    }

    /// An export done: the user told. `true` if one was.
    pub(super) fn take_exported(&mut self) -> bool {
        let Some(done) = self.exported.lock().unwrap_or_else(PoisonError::into_inner).take() else {
            return false;
        };
        self.notices.push(match done {
            Ok(path) => {
                let name =
                    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                Notification::success(tr!("diagram_editor.exported", name = name))
            }
            Err(reason) => {
                Notification::error(tr!("diagram_editor.export_failed", reason = reason))
            }
        });
        true
    }
}
