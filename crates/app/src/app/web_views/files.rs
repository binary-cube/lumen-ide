//! Files dragged from the system over the window: the page under them that takes them is
//! told they came, moved, left or were dropped; elsewhere they're the window's.

use super::WebViews;

/// Files dragged from the system over the window.
#[derive(Debug)]
pub(super) struct DraggedFiles {
    files: Vec<std::path::PathBuf>,
    /// The page they're over (that takes them).
    over: Option<u128>,
}

/// Where files dragged over the window are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum FilesOver {
    /// A page that takes them: its.
    Page,
    /// The window's (the editor opens them).
    Window,
}

impl WebViews {
    /// Files dragged over a page let go (or made again: it's told they came, anew).
    pub(super) fn forget_drag(&mut self, live: &[u128]) {
        if let Some(dragged) = self.files.as_mut()
            && dragged.over.is_some_and(|id| !live.contains(&id))
        {
            dragged.over = None;
        }
    }

    /// Files dragged from the system came over the window.
    pub(in crate::app) fn files_entered(&mut self, files: &[std::path::PathBuf]) {
        self.files = Some(DraggedFiles { files: files.to_vec(), over: None });
    }

    /// Files are being dragged over the window.
    pub(in crate::app) fn files_dragged(&self) -> bool {
        self.files.is_some()
    }

    /// The files dragged, as they came.
    pub(in crate::app) fn dragged_files(&self) -> Vec<std::path::PathBuf> {
        self.files.as_ref().map(|d| d.files.clone()).unwrap_or_default()
    }

    /// The files dragged at `p` (`open`: nothing of the app's over the pages): over a page
    /// that takes them (told it came, moved or left), or the window's.
    pub(in crate::app) fn files_at(&mut self, p: kernel::Point, open: bool) -> FilesOver {
        let under = self.page_at(p, open).filter(|l| l.view.takes_files()).map(|l| l.id);
        let Some(dragged) = self.files.as_mut() else { return FilesOver::Window };
        let before = std::mem::replace(&mut dragged.over, under);
        // The files told only as they come over a page (not at each move).
        let entering = under.filter(|id| before != Some(*id)).map(|_| dragged.files.clone());
        if let Some(left) = before.filter(|b| Some(*b) != under) {
            self.drag(left, platform_api::PageDrag::Leave);
        }
        let Some(id) = under else { return FilesOver::Window };
        let at = self.in_page(id, p);
        let drag = match entering {
            Some(files) => platform_api::PageDrag::Enter { files, at },
            None => platform_api::PageDrag::Over { at },
        };
        self.drag(id, drag);
        FilesOver::Page
    }

    /// The drag ended: dropped at `p` (`Some`: `true` if a page took them), or given up.
    pub(in crate::app) fn files_ended(&mut self, dropped: Option<kernel::Point>) -> bool {
        let Some(DraggedFiles { over, .. }) = self.files.take() else { return false };
        // A page gone meanwhile takes nothing: the window's then.
        let Some(id) = over.filter(|id| self.live(*id).is_some()) else { return false };
        match dropped {
            Some(p) => {
                let at = self.in_page(id, p);
                self.drag(id, platform_api::PageDrag::Drop { at });
                true
            }
            None => {
                self.drag(id, platform_api::PageDrag::Leave);
                false
            }
        }
    }

    /// `p` (window) in page `id`'s own pixels.
    fn in_page(&self, id: u128, p: kernel::Point) -> kernel::Point {
        self.live(id).map_or(p, |l| l.local(p))
    }

    fn drag(&self, id: u128, drag: platform_api::PageDrag) {
        if let Some(live) = self.live(id) {
            live.view.input(platform_api::PageInput::Drag(drag));
        }
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/app/web_views/files.rs"]
mod tests;
