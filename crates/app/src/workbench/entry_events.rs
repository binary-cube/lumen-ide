//! The tabs follow the project's entries whoever moves or removes them (the tree's paste or
//! deletion, a refactoring's move, a module): the app listens to the domain's events
//! (`FileRenamed`, `EntriesDeleted`) instead of being called, and applies them on its next
//! look — a moved file's tab takes its new path (its view and its module's bar follow it,
//! or the bars' providers are asked again), a deleted file's saved tab closes (an unsaved
//! one stays: its text isn't lost).

use std::sync::Mutex;

use workspace::clipboard::moved_path;
use workspace::events::{EntriesDeleted, FileRenamed};

use super::*;

/// A change heard, waiting for the next look.
#[derive(Debug, Clone, PartialEq, Eq)]
enum EntryChange {
    Renamed { from: PathBuf, to: PathBuf },
    Deleted(Vec<PathBuf>),
}

/// The changes heard (the subscriptions live as long as it does).
pub(super) struct EntryChanges {
    heard: Arc<Mutex<Vec<EntryChange>>>,
    _subscriptions: Vec<kernel::Subscription>,
}

impl std::fmt::Debug for EntryChanges {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EntryChanges").finish_non_exhaustive()
    }
}

impl EntryChanges {
    pub(super) fn listen(events: &kernel::EventBus) -> Self {
        let heard: Arc<Mutex<Vec<EntryChange>>> = Arc::default();
        let renamed = Arc::clone(&heard);
        let deleted = Arc::clone(&heard);
        Self {
            _subscriptions: vec![
                events.subscribe(move |e: &FileRenamed| {
                    if let Ok(mut heard) = renamed.lock() {
                        heard.push(EntryChange::Renamed { from: e.from.clone(), to: e.to.clone() });
                    }
                }),
                events.subscribe(move |e: &EntriesDeleted| {
                    if let Ok(mut heard) = deleted.lock() {
                        heard.push(EntryChange::Deleted(e.paths.clone()));
                    }
                }),
            ],
            heard,
        }
    }

    fn take(&self) -> Vec<EntryChange> {
        self.heard.lock().map(|mut h| std::mem::take(&mut *h)).unwrap_or_default()
    }
}

impl Workbench {
    /// The tabs of `from` (or of what's inside it, a folder) follow it to `to`: their path,
    /// their view, their module's bar (it follows, else the providers are asked again: a
    /// console moved out of its folder is a plain file, a file moved into it a console).
    /// What showed the old path (the side panels, the breadcrumb) is told again. `true` if
    /// a tab moved.
    pub(super) fn follow_move(&mut self, from: &Path, to: &Path) -> bool {
        let (mut ask, mut moved) = (Vec::new(), false);
        for tab in all_tabs_mut(&mut self.tabs, &mut self.editors.groups) {
            let Some(new) = tab.doc.path().and_then(|p| moved_path(p, from, to)) else {
                continue;
            };
            tab.doc.set_path(new.clone());
            moved = true;
            if let Some(v) = &mut tab.viewer {
                v.viewer.moved(&new);
            }
            let kept = tab.toolbar.as_mut().is_some_and(|t| t.bar.moved(&new));
            // A view in the text's place (a SQLite file) never has a bar.
            let text =
                tab.viewer.as_ref().is_none_or(|v| v.kind != ui_workbench::ViewerKind::Replace);
            if !kept && text {
                ask.push((tab.doc.id(), new, tab.syntax.language().to_string()));
            }
        }
        for (id, path, language) in ask {
            // The new bar made before the old one goes (a console's connection stays).
            let toolbar = self.toolbar_for(&path, Some(language.as_str()));
            if let Some(tab) =
                all_tabs_mut(&mut self.tabs, &mut self.editors.groups).find(|t| t.doc.id() == id)
            {
                tab.toolbar = toolbar;
            }
        }
        if moved {
            for side in &mut self.tools.sides {
                side.key = None;
            }
            self.crumbs = Memo::default();
        }
        moved
    }

    /// The diagnostics the project's modules found since the last look (the marks follow).
    pub fn poll_diagnostics(&mut self) -> bool {
        self.data.diagnostics.sync()
    }

    /// Applies the entries' changes heard meanwhile to the tabs. `true` if one changed.
    pub fn poll_entry_changes(&mut self) -> bool {
        let changes = self.entry_changes.take();
        let mut changed = false;
        for change in changes {
            match change {
                EntryChange::Renamed { from, to } => changed |= self.follow_move(&from, &to),
                EntryChange::Deleted(paths) => {
                    let deleted = |tab: &Tab| {
                        tab.doc.path().is_some_and(|p| workspace::delete::was_deleted(p, &paths))
                            && !tab.is_dirty()
                    };
                    // In every group (each brought in, in turn; one left empty goes), then
                    // back to the group whose tab was current.
                    let current = self.tabs.get(self.active).map(|t| t.doc.id());
                    for g in (0..self.editors.groups.len()).rev() {
                        if g >= self.editors.groups.len() {
                            continue;
                        }
                        self.focus_group(g);
                        for i in (0..self.tabs.len()).rev() {
                            if deleted(&self.tabs[i]) {
                                self.close_tab(i);
                                changed = true;
                            }
                        }
                    }
                    let back = (0..self.editors.groups.len()).find(|&g| {
                        let tabs = if g == self.editors.current {
                            &self.tabs
                        } else {
                            &self.editors.groups[g].tabs
                        };
                        tabs.iter().any(|t| Some(t.doc.id()) == current)
                    });
                    if let Some(g) = back {
                        self.focus_group(g);
                    }
                }
            }
        }
        changed
    }
}
