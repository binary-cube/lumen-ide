//! Renaming in place: a session's or a folder's row turned into a field, a new folder
//! named, the name given written.

use kernel::Rect;
use ui_kit::TextInput;

use super::{RENAME_FIELD, Renamed, Renaming, SessionsPanel};

impl SessionsPanel {
    /// Whether session `i`'s agent can rename it in its files.
    pub(super) fn renamable(&self, i: usize) -> bool {
        let Some(session) = self.sessions.get(i) else { return false };
        self.agents().iter().any(|a| a.id() == session.agent && a.can_rename())
    }

    /// The session the menu was opened on, by its index now.
    pub(super) fn menu_session(&self) -> Option<usize> {
        let (agent, id) = self.menu_on.as_ref()?;
        self.sessions.iter().position(|s| s.agent == *agent && s.id == *id)
    }

    /// Session `i` renamed in its row: a field with its title, all of it selected.
    pub(super) fn start_rename(&mut self, i: usize) {
        let Some(session) = self.sessions.get(i) else { return };
        let target = Renamed::Session(session.agent, session.id.clone());
        let title = session.title.clone();
        self.rename_in_row(target, &title);
    }

    /// `target` renamed in its row, from `now`, all of it selected.
    pub(super) fn rename_in_row(&mut self, target: Renamed, now: &str) {
        let mut input = TextInput::new(now);
        input.select_all();
        self.renaming = Some(Renaming { target, input, field: Rect::default() });
        self.with_controls(|c, parts| c.focus(parts, Some(RENAME_FIELD)));
    }

    /// A folder made (named "New Folder"), `then` done with its id, and named in its row.
    pub(super) fn new_folder(&mut self, then: impl FnOnce(&mut Self, &str)) {
        let Some(project) = self.project else { return };
        let Some(id) = crate::folders::create(project, "") else { return };
        then(self, &id);
        self.shown_for = None;
        self.filter();
        let name = self.folders.iter().find(|f| f.id == id).map(|f| f.name.clone());
        self.rename_in_row(Renamed::Folder(id), &name.unwrap_or_default());
    }

    /// The name typed, given: in the agent's files — at once if its tab isn't open, else
    /// once its agent ends (and its tab named so now) — and shown here meanwhile.
    pub(super) fn finish_rename(&mut self) {
        let Some(renaming) = self.renaming.take() else { return };
        self.rename_ended();
        let title = crate::one_line(renaming.input.text());
        let (agent, id) = match renaming.target {
            Renamed::Folder(id) => {
                if let Some(project) = self.project {
                    crate::folders::rename(project, &id, &title);
                }
                return;
            }
            Renamed::Session(agent, id) => (agent, id),
        };
        let Some(i) = self.sessions.iter().position(|s| s.agent == agent && s.id == id) else {
            return;
        };
        if title.is_empty() || title == self.sessions[i].title {
            return;
        }
        title.clone_into(&mut self.sessions[i].title);
        let folder = self.sessions[i].folder.clone();
        self.retitled.retain(|(a, r, _)| !(*a == agent && *r == id));
        self.retitled.push((agent, id.clone(), title.clone()));
        self.shown_for = None;
        if let Some(project) = self.project
            && self.favorite.get(i).copied().unwrap_or(false)
        {
            // A favorite keeps the name it's known by when gone.
            crate::favorites::retitle(project, agent, &id, &title);
        }
        // Its tab open: its name there now, in the agent's files once it ends.
        if let Some(key) = crate::follow::key_of(agent, &id) {
            crate::follow::renamed(&key, &title);
            crate::follow::rename_tab(key, title);
            return;
        }
        kernel::work::run("agent-sessions", move || {
            let renamed = agents::provider(agent)
                .map_or(Err("no such agent".into()), |a| a.rename_session(&folder, &id, &title));
            if let Err(e) = renamed {
                tracing::warn!(error = %e, "a session wasn't renamed in its agent");
            }
        });
    }
}
