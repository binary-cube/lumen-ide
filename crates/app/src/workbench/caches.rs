//! "Clear and Rebuild Caches": everything the workbench keeps only to avoid recomputing
//! (syntax colors, in-code colors, breadcrumbs, the files' icons, usages, viewer images,
//! the tool windows' views and the side panels' data, what frameworks gathered) is
//! released; frameworks are detected again; the project's indexes (paths, declarations,
//! the on-disk cache in `<project dir>/`) are built again from scratch — asked with
//! `ReindexRequested`; what the modules keep on disk for the project (the language servers'
//! folders) goes — `CachesCleared`. Everything is recreated as needed, just like on first
//! use.

use super::*;

impl Purge for Workbench {
    fn purge(&mut self) {
        for tab in all_tabs_mut(&mut self.tabs, &mut self.editors.groups) {
            tab.highlight.purge();
            if let Some(v) = &mut tab.viewer {
                v.viewer.purge();
            }
            if let Some(bar) = &mut tab.toolbar {
                bar.bar.purge();
            }
        }
        self.swatches.purge();
        self.crumbs.purge();
        self.data.disk_reads.purge();
        self.chrome.decorations.purge();
        for group in &mut self.editors.groups {
            group.swatches.purge();
            group.crumbs.purge();
        }
        self.file_icons.purge();
        // The settings window's wrapped descriptions.
        self.settings.purge();
        for window in &mut self.tools.windows {
            window.purge();
        }
        for side in self.tools.sides.iter_mut().chain(&mut self.tools.parked) {
            side.panel.purge();
        }
        self.scope_colors = HashMap::new();
        self.spans = Vec::new();
        self.colors = Vec::new();
        if let Some(root) = self.project.as_ref().map(|p| p.root_path().to_path_buf()) {
            self.load_frameworks(&root);
        }
        // What the modules keep for the project (the language servers' folders), then its
        // indexes from scratch (the indexing module).
        if let Some(root) = self.project.as_ref().map(|p| p.root_path().to_path_buf()) {
            self.data.events.publish(&workspace::events::CachesCleared { root: root.clone() });
            self.data.events.publish(&workspace::events::ReindexRequested { root });
        }
        tracing::info!("the workbench's caches were cleared");
    }
}
