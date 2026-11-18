//! The Git module's screens.

use super::*;

impl Steps<'_> {
    /// The comparisons with git's versions, the log, the commit window, the blame.
    pub(super) fn git(&mut self, kind: &str) {
        match kind {
            "menu" => self.git_menu(),
            "head" => {
                self.wb.run_command(command_model::CommandId("git.compare_head"));
            }
            "history" => {
                self.wb.run_command(command_model::CommandId("git.compare_revision"));
                self.wait(Duration::from_secs(10), Workbench::poll_jobs);
            }
            "branch" => {
                self.wb.run_command(command_model::CommandId("git.compare_branch"));
            }
            "log" => {
                self.wb.run_command(command_model::CommandId("git.log"));
                self.wait_for_viewers(Duration::from_secs(20));
                // The first commit selected: its details.
                let down = EditKey::Move { motion: document::Motion::Down, extend: false };
                self.wb.edit_key(self.text, down);
                self.wait_for_viewers(Duration::from_secs(20));
            }
            "diff" => {
                // The log, the first commit's first file compared beside it.
                self.wb.run_command(command_model::CommandId("git.log"));
                self.wait_for_viewers(Duration::from_secs(20));
                let down = EditKey::Move { motion: document::Motion::Down, extend: false };
                self.wb.edit_key(self.text, down);
                self.wait_for_viewers(Duration::from_secs(20));
                self.wb.edit_key(self.text, EditKey::Enter { shift: false });
                self.wait_for_viewers(Duration::from_secs(20));
            }
            kind if kind.starts_with("file-history") => self.git_file_history(kind),
            // The log, its view's commands run (`log-filter:filter-text:fix,filter:user`: the
            // text filter set, a filter's list open).
            kind if kind.starts_with("log-filter:") => {
                self.wb.run_command(command_model::CommandId("git.log"));
                self.wait_for_viewers(Duration::from_secs(20));
                for command in kind["log-filter:".len()..].split(',') {
                    self.wb.run_tool_view_menu(self.text, command);
                    self.frame();
                    self.wait_for_viewers(Duration::from_secs(20));
                }
                self.frame();
            }
            "changes" => {
                self.wb.run_command(command_model::CommandId("git.changes"));
                self.wait(Duration::from_secs(10), Workbench::services_settled);
                self.wait_for_viewers(Duration::from_secs(10));
            }
            "push" => {
                self.wb.run_command(command_model::CommandId("git.push_to"));
                self.wait(Duration::from_secs(10), Workbench::poll_jobs);
                self.frame();
            }
            "commit" => {
                self.wb.run_command(command_model::CommandId("git.commit"));
                self.wait_for_viewers(Duration::from_secs(10));
            }
            // The Git dialogs: New Branch… (a name typed), New Tag….
            "new-branch" | "new-tag" => {
                let id = if kind == "new-branch" { "git.new_branch" } else { "git.new_tag" };
                self.wb.run_command(command_model::CommandId(id));
                self.wait_for_viewers(Duration::from_secs(10));
                self.wb.type_text(self.text, "feature/login..");
            }
            // A message of several lines written (wrapped, its subject past its hint).
            "commit-message" => {
                self.wb.run_command(command_model::CommandId("git.commit"));
                self.wait_for_viewers(Duration::from_secs(10));
                let message = "Fix the cart's totals: rounded once, at the end, for every item\n\nThe totals were rounded twice; now once, so a cart of many cheap items adds up the same as its receipt does, whatever the order they were added in.";
                self.wb.type_text(self.text, message);
            }
            "blame" | "annotated" => {
                self.wb.run_command(command_model::CommandId("git.annotate"));
                self.wait(Duration::from_secs(20), Workbench::decorations_settled);
                self.wait(Duration::from_secs(20), |wb| {
                    wb.decorations_settled();
                    wb.annotations_ready()
                });
                if kind == "annotated" {
                    // The cursor's line's commit, in the log (as a click on its annotation).
                    self.wb.run_command(command_model::CommandId("git.annotated_commit"));
                    self.wait_for_viewers(Duration::from_secs(20));
                    self.frame();
                    self.wait_for_viewers(Duration::from_secs(20));
                }
            }
            _ => {}
        }
    }

    /// The editor's right-click menu with its Git submenu open (from the keyboard).
    pub(super) fn git_menu(&mut self) {
        let entries = self.wb.context_menu(workbench_model::MenuPlace::Editor);
        self.wb.open_context_menu(entries, kernel::Point::new(620.0, 160.0));
        self.frame();
        let up = EditKey::Move { motion: document::Motion::Up, extend: false };
        self.wb.edit_key(self.text, up);
        let right = EditKey::Move { motion: document::Motion::Right, extend: false };
        self.wb.edit_key(self.text, right);
        self.frame();
    }

    /// The active file's history in the Git window, its newest commit chosen; scrolled to
    /// its files, or a commit's / a file's menu open (`GIT=file-history…`).
    pub(super) fn git_file_history(&mut self, kind: &str) {
        self.wb.run_command(command_model::CommandId("git.file_history"));
        self.wait_for_viewers(Duration::from_secs(20));
        let down = EditKey::Move { motion: document::Motion::Down, extend: false };
        self.wb.edit_key(self.text, down);
        self.wait_for_viewers(Duration::from_secs(20));
        self.frame();
        let Some(view) = self.wb.tool_view_area() else {
            tracing::error!("no history in the Git window");
            return;
        };
        if kind == "file-history-commit-menu" {
            // The first commit's row (under the view's toolbar).
            let at = Point::new(view.min_x() + view.size.width * 0.25, view.min_y() + 140.0);
            if let Some(entries) = self.wb.tool_view_menu_at(at) {
                self.wb.open_context_menu(entries, at);
            }
            self.wait_for_viewers(Duration::from_secs(20));
            self.frame();
            settle(220);
            return;
        }
        // Every file of the commit, the wheel over the details (on the right) to their end.
        self.wb.run_tool_view_menu(self.text, "commit.all-files");
        self.frame();
        let at = Point::new(view.max_x() - 240.0, view.max_y() - 120.0);
        self.wb.hover(at);
        self.wb.scroll(at, 0.0, 100_000.0);
        self.frame();
        if kind == "file-history-file-menu" {
            let at = Point::new(view.max_x() - 420.0, view.max_y() - 80.0);
            self.wb.hover(at);
            if let Some(entries) = self.wb.tool_view_menu_at(at) {
                self.wb.open_context_menu(entries, at);
            }
            self.frame();
            settle(220);
        }
    }
}
