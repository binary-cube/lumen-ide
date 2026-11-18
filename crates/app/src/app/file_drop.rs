//! Files and folders dropped from the system's file manager: opened as the workbench's
//! plan says (`workbench::file_drop`) — the folders as projects through "Open Project…"'s
//! path (here, or windows of their own), the files as tabs in the group under the drop.

use super::*;
use crate::workbench::DropAction;

impl EditorApp {
    /// `paths` dropped on the window at `at` (physical pixels).
    pub(super) fn drop_files(&mut self, paths: &[PathBuf], at: Point) {
        self.workbench.end_file_drag();
        let plan = self.workbench.plan_drop(paths);
        let (missing, skipped) = (plan.missing, plan.skipped);
        let action = self.workbench.drop_action(plan);
        tracing::info!(items = paths.len(), missing, skipped, ?action, "files dropped");
        let DropAction::Open(open) = action else {
            self.redraw();
            return;
        };
        let project_here = open.here.is_some();
        if let Some(folder) = &open.here {
            self.open_project(folder);
        }
        for folder in &open.elsewhere {
            self.open_recent(folder);
        }
        if !open.files.is_empty() {
            // The groups are as they were under the drop unless a project just opened here.
            if !project_here {
                self.workbench.focus_drop_group(at);
            }
            for file in &open.files {
                let opened = self.workbench.open_file(file);
                self.succeeded(opened, Doing::Opening(file));
            }
        }
        // After the project opened here (its data, the notifications with it, is new).
        if open.skipped > 0 {
            workbench_model::WorkbenchHost::notify(
                &mut self.workbench,
                workbench_model::Notification::warning(crate::workbench::skipped_message(
                    open.skipped,
                )),
            );
        }
        self.relayout();
    }
}
