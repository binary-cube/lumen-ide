//! The log's right-click menus: a commit's (every file of it or only the history's file,
//! its hash or message copied, the history's file compared with its previous revision or
//! with the working copy) and a file's of the details (opened, its change in this commit,
//! its path copied, shown in the project view or the file manager). The rows come back as
//! the view's commands.

use super::*;
use crate::ref_dialog::{self, Target};
use ui_components::{AppRequest, ViewMenuEntry};

/// How a dialog is opened on what the menu chose.
type Opens = fn(&mut dyn workbench_model::WorkbenchHost, Option<Target>) -> bool;

/// The commands of the menus' rows (a file's carry its place in the commit's list).
const ALL_FILES: &str = "commit.all-files";
const COPY_HASH: &str = "commit.copy-hash";
const COPY_MESSAGE: &str = "commit.copy-message";
const COMPARE_PREVIOUS: &str = "commit.compare-previous";
const COMPARE_WORKING: &str = "commit.compare-working";
const OPEN_FILE: &str = "file.open:";
const FILE_DIFF: &str = "file.diff:";
const COPY_PATH: &str = "file.copy-path:";
const SHOW_IN_PROJECT: &str = "file.show-in-project:";
const REVEAL_FILE: &str = "file.reveal:";
const NEW_BRANCH_HERE: &str = "commit.new-branch";
const NEW_TAG_HERE: &str = "commit.new-tag";
const CHECKOUT_HERE: &str = "commit.checkout";

impl LogView {
    /// The menu of what's under `p`: a commit's row (it's selected) or a file of the details.
    pub(super) fn menu_at(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        match self.hit(p)? {
            Hit::Row(i) => {
                if !self.chosen.contains(&i) {
                    self.select(i);
                }
                Some(self.commit_menu())
            }
            Hit::Files => {
                let i = self.tree.file_at(p)?;
                self.files_focus = true;
                Some(self.file_menu(i))
            }
            Hit::Branch(i) => {
                self.branch_selected = Some(i);
                self.branch_menu(i)
            }
            _ => None,
        }
    }

    /// A commit's rows (a file's history adds the ones about its file).
    pub(super) fn commit_menu(&self) -> Vec<ViewMenuEntry> {
        let mut rows = Vec::new();
        if self.path.is_some() {
            let label = if self.all_files { t("git.only_this_file") } else { t("git.all_files") };
            rows.push(ViewMenuEntry::item(ALL_FILES, label).with_icon("layers"));
            rows.push(ViewMenuEntry::Separator);
        }
        rows.push(ViewMenuEntry::item(COPY_HASH, t("git.copy_hash")).with_icon("hash"));
        rows.push(ViewMenuEntry::item(COPY_MESSAGE, t("git.copy_message")).with_icon("copy"));
        if self.path.is_some() && workbench_model::presence::compare() {
            rows.push(ViewMenuEntry::Separator);
            rows.push(
                ViewMenuEntry::item(COMPARE_PREVIOUS, t("git.compare_previous"))
                    .with_icon("compare"),
            );
            rows.push(
                ViewMenuEntry::item(COMPARE_WORKING, t("git.compare_working")).with_icon("compare"),
            );
        }
        // A branch or a tag from it.
        if self.chosen.len() <= 1 {
            rows.push(ViewMenuEntry::Separator);
            rows.push(
                ViewMenuEntry::item(NEW_BRANCH_HERE, t("git.new_branch_here"))
                    .with_icon("git-branch"),
            );
            rows.push(ViewMenuEntry::item(NEW_TAG_HERE, t("git.new_tag_here")).with_icon("tag"));
            rows.push(
                ViewMenuEntry::item(CHECKOUT_HERE, t("git.checkout_commit"))
                    .with_icon("arrow-left-right"),
            );
        }
        rows
    }

    /// File `i` of the selected commit's rows.
    pub(super) fn file_menu(&self, i: usize) -> Vec<ViewMenuEntry> {
        let mut rows = vec![
            ViewMenuEntry::item(format!("{OPEN_FILE}{i}"), t("git.open_file")).with_icon("file"),
            ViewMenuEntry::item(format!("{FILE_DIFF}{i}"), t("git.show_diff")).with_icon("compare"),
            ViewMenuEntry::Separator,
            ViewMenuEntry::item(format!("{COPY_PATH}{i}"), t("git.copy_path")).with_icon("link"),
            ViewMenuEntry::Separator,
            ViewMenuEntry::item(format!("{SHOW_IN_PROJECT}{i}"), t("git.show_in_project"))
                .with_icon("crosshair"),
            ViewMenuEntry::item(format!("{REVEAL_FILE}{i}"), t("git.reveal_file"))
                .with_icon("folder-open"),
        ];
        if !workbench_model::presence::compare() {
            rows.remove(1);
        }
        rows
    }

    /// A branch's rows of the branches panel (the same as the commit screen's places:
    /// [`crate::ref_menu`]).
    pub(super) fn branch_menu(&self, i: usize) -> Option<Vec<ViewMenuEntry>> {
        let on = self.menu_ref(i)?;
        Some(crate::ref_menu::rows(i, &on, &self.remotes))
    }

    /// The reference branches panel row `i` is (with what a local branch tracks).
    pub(super) fn menu_ref(&self, i: usize) -> Option<crate::ref_menu::MenuRef> {
        match &self.branch_rows.get(i)?.item {
            BranchItem::Ref { name, kind, .. } => {
                let local = *kind == RefKind::Branch;
                let upstream = local
                    .then(|| self.branch_states.iter().find(|s| s.branch == *name))
                    .flatten()
                    .and_then(|s| s.upstream.clone());
                let current = local && self.sync.as_ref().is_some_and(|s| s.branch == *name);
                let kind = Some(*kind);
                Some(crate::ref_menu::MenuRef {
                    name: name.clone(),
                    local,
                    upstream,
                    kind,
                    current,
                })
            }
            BranchItem::Head => Some(crate::ref_menu::MenuRef {
                name: "HEAD".into(),
                local: false,
                upstream: None,
                kind: None,
                current: true,
            }),
            _ => None,
        }
    }

    /// A dialog asked from a menu: it starts from `target`.
    fn ask(target: Target, open: Opens) -> ViewerAction {
        ViewerAction::WithHost(ui_components::WithHost::once(move |host| {
            open(host, Some(target));
        }))
    }

    /// A row of the menus chosen. `true` if it was one of theirs.
    pub(super) fn menu_command(&mut self, id: &str) -> bool {
        if let Some(action) = crate::ref_menu::chosen(id, |i| self.menu_ref(i), &self.remotes) {
            self.pending = action;
            return true;
        }
        if id == CHECKOUT_HERE {
            self.pending = self.selected_commit().map(|c| {
                let wanted =
                    crate::switch::Wanted::Commit { id: c.id.clone(), short: c.short().to_owned() };
                crate::ref_menu::switch_action(wanted)
            });
            return true;
        }
        let asked = if id == NEW_BRANCH_HERE || id == NEW_TAG_HERE {
            let commit = self
                .selected_commit()
                .map(|c| Target::Commit { id: c.id.clone(), short: c.short().to_owned() });
            let open =
                if id == NEW_BRANCH_HERE { ref_dialog::new_branch } else { ref_dialog::new_tag };
            Some((commit, open as Opens))
        } else {
            None
        };
        if let Some((target, open)) = asked {
            self.pending = target.map(|t| Self::ask(t, open));
            return true;
        }
        let file = |prefix: &str| id.strip_prefix(prefix).and_then(|i| i.parse::<usize>().ok());
        let action = if let Some(i) = file(OPEN_FILE) {
            self.file_path(i).map(|path| ViewerAction::OpenFile { path, line: None, column: None })
        } else if let Some(i) = file(FILE_DIFF) {
            self.compare_file(i)
        } else if let Some(i) = file(COPY_PATH) {
            self.file_relative(i).map(|path| ViewerAction::Request(AppRequest::CopyText(path)))
        } else if let Some(i) = file(SHOW_IN_PROJECT) {
            self.file_path(i).map(|path| ViewerAction::Request(AppRequest::ShowFolder(path)))
        } else if let Some(i) = file(REVEAL_FILE) {
            self.file_path(i).map(|path| ViewerAction::Request(AppRequest::Reveal(path)))
        } else {
            match id {
                ALL_FILES => {
                    self.all_files = !self.all_files;
                    self.refresh_files();
                    return true;
                }
                COPY_HASH => self
                    .selected_commit()
                    .map(|c| ViewerAction::Request(AppRequest::CopyText(c.id.clone()))),
                COPY_MESSAGE => {
                    self.selected_message().map(|m| ViewerAction::Request(AppRequest::CopyText(m)))
                }
                COMPARE_PREVIOUS => self.compare_previous(),
                COMPARE_WORKING => self.compare_working(),
                _ => return false,
            }
        };
        self.pending = action;
        true
    }

    fn selected_commit(&self) -> Option<&version_control::Commit> {
        Some(&self.rows.get(self.selected?)?.entry.commit)
    }

    /// The selected commit's whole message (its summary until its details are read).
    fn selected_message(&self) -> Option<String> {
        let commit = self.selected_commit()?;
        Some(match &self.details {
            Some(details) if details.commit.id == commit.id => details.message.clone(),
            _ => commit.summary.clone(),
        })
    }

    /// File `i` of the selected commit: where it is (relative, with `/`), and on disk.
    fn file_relative(&self, i: usize) -> Option<String> {
        Some(self.files.get(i)?.1.path.clone())
    }

    fn file_path(&self, i: usize) -> Option<std::path::PathBuf> {
        Some(self.repo.as_ref()?.root().join(self.file_relative(i)?))
    }

    /// The history's file in the selected commit, compared with its revision before (read in
    /// the background).
    fn compare_previous(&mut self) -> Option<ViewerAction> {
        let (repo, path) = (self.repo.clone()?, self.path.clone()?);
        let commit = self.selected_commit()?.id.clone();
        let known = self.details.clone().filter(|d| d.commit.id == commit);
        self.opening.start("git-log", move || {
            let details = known.or_else(|| repo.commit_details(&commit).map(Arc::new))?;
            let file = details
                .files
                .iter()
                .find(|f| f.path == path || f.old_path.as_deref() == Some(path.as_str()))?;
            let input = compare_commit_file(repo.as_ref(), &details, file);
            let name = file.path.rsplit('/').next().unwrap_or(&file.path);
            let title = format!("{name} · {}", details.commit.short());
            Some(ViewerAction::CompareBeside(Box::new(input), title))
        });
        None
    }

    /// The history's file at the selected commit, compared with the working copy (as it's
    /// on disk; nothing there: empty), read in the background.
    fn compare_working(&mut self) -> Option<ViewerAction> {
        let (repo, path) = (self.repo.clone()?, self.path.clone()?);
        let commit = self.selected_commit()?.clone();
        let label = t("git.working_copy");
        self.opening.start("git-log", move || {
            let file = repo.root().join(&path);
            let current = std::fs::read_to_string(&file).unwrap_or_default();
            crate::DISK.read(current.len() as u64);
            let revision = version_control::Revision::Named(commit.id.clone());
            let short = commit.short().to_owned();
            let input = version_control::application::compare_with(
                repo.as_ref(),
                &file,
                &current,
                &revision,
                &short,
            )?;
            let name = path.rsplit('/').next().unwrap_or(&path);
            let title = format!("{name} · {short} · {label}");
            Some(ViewerAction::CompareBeside(Box::new(input), title))
        });
        None
    }
}

#[cfg(test)]
#[path = "../../tests/unit/log_view/menus.rs"]
mod tests;
