//! What the Diagrams window does to its entries: a diagram or a folder made (on a worker, in
//! the folder last clicked; a diagram opened once it's there), an entry renamed in its row
//! (the app renames it: its tab follows), moved to another folder (the app moves it), or put
//! in the system's trash (on a worker; its deletion told on the project's events).

use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError};

use i18n::t;
use kernel::Point;
use ui_components::{ToolPanelAction, ViewMenuEntry, ViewerKey};
use ui_kit::TextInput;

use crate::window::DiagramsPanel;

/// What the worker did.
#[derive(Debug)]
pub enum Done {
    Made { path: PathBuf, folder: bool },
    Trashed(PathBuf),
    Failed(String),
}

/// The field of a row being renamed.
pub fn field_style(
    p: &ui_components::Palette,
    ds: &ui_components::ds::Ctx<'_>,
    font: text_layout::Font,
) -> ui_kit::TextInputStyle {
    ui_kit::TextInputStyle {
        font,
        background: p.input_background,
        border: p.input_border,
        border_focused: p.search_field_border_focused,
        text: p.text,
        placeholder: p.text_muted,
        selection: p.selection,
        caret: p.caret,
        radius: ds.px(4.0),
        border_width: ds.line(),
        padding: ds.px(4.0),
        caret_width: ds.line().max(1.0),
        lead: 0.0,
    }
}

/// A diagram's name without its `.drawio` (any case).
pub fn stem(name: &str) -> &str {
    let cut = name.len().saturating_sub(EXTENSION.len());
    match (name.get(..cut), name.get(cut..)) {
        (Some(stem), Some(ext)) if ext.eq_ignore_ascii_case(EXTENSION) && !stem.is_empty() => stem,
        _ => name,
    }
}

const EXTENSION: &str = ".drawio";

impl DiagramsPanel {
    /// Where something new goes: the folder last clicked, else the diagrams' own.
    fn target_dir(&self) -> Option<PathBuf> {
        self.current
            .clone()
            .filter(|c| self.dir.as_ref().is_some_and(|d| c.starts_with(d)))
            .or_else(|| self.dir.clone())
    }

    /// A diagram (or a folder) made on the worker; without a waker (tests) here and now.
    pub(crate) fn create(&mut self, folder: bool) {
        let (Some(root), Some(dir)) = (self.root.clone(), self.target_dir()) else { return };
        let job = move || match if folder {
            crate::create::new_folder(&root, &dir)
        } else {
            crate::create::new_diagram(&root, &dir)
        } {
            Ok(path) => Done::Made { path, folder },
            Err(e) => Done::Failed(e.to_string()),
        };
        self.run(job);
    }

    /// `job` on the worker, its outcome taken at the next poll.
    fn run(&mut self, job: impl FnOnce() -> Done + Send + 'static) {
        let (into, waker) = (Arc::clone(&self.done), Arc::clone(&self.waker));
        let work = move || {
            let done = job();
            into.lock().unwrap_or_else(PoisonError::into_inner).push(done);
            if let Some(wake) = waker.lock().unwrap_or_else(PoisonError::into_inner).clone() {
                wake();
            }
        };
        if self.waker.lock().unwrap_or_else(PoisonError::into_inner).is_none() {
            work();
        } else {
            kernel::work::run("diagram-list", work);
        }
    }

    /// What the worker did, taken: a diagram made opened, a folder made chosen (where the
    /// next goes), a deletion told, a failure said. Listed again either way.
    pub(crate) fn take_done(&mut self) {
        let done: Vec<Done> =
            std::mem::take(&mut *self.done.lock().unwrap_or_else(PoisonError::into_inner));
        if done.is_empty() {
            return;
        }
        for d in done {
            match d {
                Done::Made { path, folder: true } => self.current = Some(path),
                Done::Made { path, folder: false } => {
                    self.action = Some(ToolPanelAction::Open { path, line: 0 })
                }
                Done::Trashed(path) => {
                    if let Some(events) = &self.events {
                        events.publish(&workspace::events::EntriesDeleted { paths: vec![path] });
                    }
                }
                Done::Failed(why) => tracing::warn!("a diagram's entry wasn't changed: {why}"),
            }
        }
        self.list();
    }

    /// The menu of the row at `p`: rename, move to a folder, delete.
    pub(crate) fn row_menu(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        let entry = self.entries.get(self.entry_at(p)?)?;
        // Kept by its path: a listing made again meanwhile doesn't change what it acts on.
        self.menu_target = Some(entry.path.clone());
        let mut folders = vec![ViewMenuEntry::item("move:", format!("{}/", crate::window::FOLDER))];
        folders.extend(
            self.entries
                .iter()
                .filter(|e| e.is_dir && e.path != entry.path && !e.path.starts_with(&entry.path))
                .map(|e| {
                    ViewMenuEntry::item(
                        format!("move:{}", e.path.display()),
                        "  ".repeat(e.depth + 1) + &e.name,
                    )
                }),
        );
        Some(vec![
            ViewMenuEntry::item("rename", t("diagram_files.rename")),
            ViewMenuEntry::submenu(t("diagram_files.move_to"), folders),
            ViewMenuEntry::Separator,
            ViewMenuEntry::item("delete", t("diagram_files.delete")),
        ])
    }

    /// A row of that menu chosen.
    pub(crate) fn row_command(&mut self, id: &str) -> bool {
        let Some(target) = self.menu_target.take() else { return false };
        let Some(e) = self.entries.iter().find(|e| e.path == target).cloned() else { return false };
        if id == "rename" {
            let name = if e.is_dir { e.name.as_str() } else { stem(&e.name) };
            let mut input = TextInput::new(name);
            input.select_all();
            self.renaming = Some((e.path, input));
            return true;
        }
        if id == "delete" {
            let path = e.path;
            self.run(move || match fs_local::move_to_trash(std::slice::from_ref(&path)) {
                Ok(()) => Done::Trashed(path),
                Err(err) => Done::Failed(err.to_string()),
            });
            return true;
        }
        if let Some(to) = id.strip_prefix("move:") {
            let into = if to.is_empty() { self.dir.clone() } else { Some(PathBuf::from(to)) };
            if let Some(into) = into.filter(|into| e.path.parent() != Some(into.as_path())) {
                self.action = Some(ToolPanelAction::Move { from: e.path, into });
            }
            return true;
        }
        false
    }

    /// A key while renaming: the field's; ↩ renames (a name that's a name), Esc doesn't.
    pub(crate) fn rename_key(&mut self, key: ViewerKey) -> bool {
        let Some((path, input)) = self.renaming.as_mut() else { return false };
        match key {
            ViewerKey::Escape => self.renaming = None,
            ViewerKey::Enter => {
                let (path, name) = (path.clone(), input.text().trim().to_owned());
                self.renaming = None;
                let Some(e) = self.entries.iter().find(|e| e.path == path) else { return true };
                let bad = name.is_empty() || name.starts_with('.') || name.contains(['/', '\\']);
                if !bad {
                    let file = if e.is_dir || stem(&name) != name {
                        name
                    } else {
                        format!("{name}{EXTENSION}")
                    };
                    let to = e.path.parent().unwrap_or(Path::new("")).join(file);
                    if to != e.path {
                        self.action = Some(ToolPanelAction::Rename { from: e.path.clone(), to });
                    }
                }
            }
            ViewerKey::Backspace => input.backspace(false),
            ViewerKey::Delete => input.delete(),
            ViewerKey::Left => input.move_left(false, false),
            ViewerKey::Right => input.move_right(false, false),
            ViewerKey::Home => input.home(false),
            ViewerKey::End => input.end(false),
            _ => return false,
        }
        true
    }
}
