//! The syntax thread: the parse results and syncing them with the tabs.

use super::*;

impl Workbench {
    /// Background parsing (created once the window exists, for the waker).
    pub fn set_worker(&mut self, worker: SyntaxWorker) {
        self.worker = Some(worker);
    }

    /// How this window's carets look: the user's own, else its theme's (one without
    /// `[caret]`: as always).
    pub fn caret_look(&self) -> theming::caret::CaretLook {
        crate::caret::own()
            .unwrap_or_else(|| self.theme.as_ref().map(ResolvedTheme::caret).unwrap_or_default())
    }

    pub fn set_theme(&mut self, theme: ResolvedTheme) {
        self.theme = Some(theme);
        self.scope_colors.clear();
        self.theme_generation += 1;
    }

    /// Takes the background parse results. `true` if a redraw is needed.
    pub fn poll_syntax(&mut self) -> bool {
        let Some(worker) = &self.worker else { return false };
        let mut changed = false;
        while let Some(result) = worker.try_recv() {
            // The tab can be in any editor group.
            let tab = all_tabs_mut(&mut self.tabs, &mut self.editors.groups)
                .find(|t| t.doc.id() == result.doc);
            if let Some(tab) = tab {
                tab.syntax.accept(result.snapshot, &tab.doc);
                if let Some(snapshot) = tab.syntax.snapshot() {
                    tab.folds.set_available(snapshot.folds());
                    tab.folds_version = snapshot.version();
                }
                changed = true;
            }
        }
        changed
    }

    /// Sends the active tab to parsing if it changed (or parses it synchronously).
    pub fn sync_syntax(&mut self) {
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        tab.sync_folds();
        if !self.support.syntax.grammars().contains(tab.syntax.language()) {
            return;
        }
        match &self.worker {
            Some(worker) => tab.syntax.sync(&tab.doc, worker),
            None if tab.syntax.snapshot().is_none_or(|s| s.version() != tab.doc.version()) => {
                let text = tab.doc.buffer().snapshot();
                if let Some(snapshot) = parse_document(
                    &self.support.syntax,
                    tab.syntax.language(),
                    &text,
                    None,
                    &[],
                    tab.doc.version(),
                ) {
                    tab.folds.set_available(snapshot.folds());
                    tab.syntax.accept(snapshot, &tab.doc);
                }
            }
            None => {}
        }
    }

    /// What the window's first frame shows is complete: the active tab's text has its
    /// colors (parsed; or a language without a grammar) and its view isn't loading. What
    /// comes later (indexes, a language server, Git) never counts: the start doesn't wait.
    pub fn first_paint_ready(&self) -> bool {
        let Some(tab) = self.tabs.get(self.active) else { return true };
        let parsed = tab.syntax.snapshot().is_some()
            || !self.support.syntax.grammars().contains(tab.syntax.language());
        let shown = tab.viewer.as_ref().is_none_or(|v| !v.viewer.is_loading());
        parsed && shown
    }

    // ─── Language level ───

    /// Frees the syntax trees and text copies of tabs not on screen for a few minutes (they come back,
    /// parsed again, when the tab is shown). Checked at most every 30 s.
    pub(super) fn release_unseen_syntax(&mut self) {
        const UNSEEN: std::time::Duration = std::time::Duration::from_secs(300);
        if self.syntax_swept.elapsed() < std::time::Duration::from_secs(30) {
            return;
        }
        self.syntax_swept = std::time::Instant::now();
        let worker = self.worker.as_ref();
        for tab in all_tabs_mut(&mut self.tabs, &mut self.editors.groups) {
            if tab.seen.elapsed() <= UNSEEN {
                continue;
            }
            // Its text's copy (made for hover, occurrences, the modules) goes too, and what
            // its viewer holds (a decoded image, a laid out preview), made again when shown.
            tab.text.get_mut().take();
            if let Some(viewer) = &mut tab.viewer {
                viewer.viewer.purge();
            }
            if tab.syntax.snapshot().is_some() {
                tab.syntax.release();
                if let Some(worker) = worker {
                    worker.forget(tab.doc.id());
                }
            }
        }
    }
}
