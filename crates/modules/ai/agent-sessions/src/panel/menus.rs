//! The panel's menus: the agents' ("+"), a session's (open, marks, labels, folders, copy),
//! moving into a folder; and what their commands do.

use i18n::t;
use ui_components::{ToolPanelAction, ViewMenuEntry, ViewerAction};

use crate::favorites::Label;

use super::{
    COPY, COPY_WHAT, DONE, FAVORITE, FOLDER_DELETE, FOLDER_RENAME, LABEL, MOVE, MOVE_NEW,
    NEW_FOLDER, NEW_WITH, OPEN_CHAT, OPEN_TERMINAL, PIN, RENAME, Renamed, SLEEP, SessionsPanel,
    UNREAD, WAKE, session_file,
};

impl SessionsPanel {
    /// The agents to choose from: those offered (greyed out while not installed), then those
    /// not hosted yet (greyed out); the one "+" starts checked.
    pub(super) fn agents_menu(&self) -> Vec<ViewMenuEntry> {
        let chosen = self.chosen().map(|a| a.id());
        let mut rows: Vec<ViewMenuEntry> = self
            .agents()
            .iter()
            .map(|a| {
                ViewMenuEntry::item(format!("{NEW_WITH}{}", a.id()), a.name())
                    .with_icon(a.icon())
                    .disabled_if(!a.installed())
                    .checked_if(chosen == Some(a.id()))
            })
            .collect();
        // Tests list their own agents only.
        if self.agents.is_none() {
            rows.extend(agents::planned().into_iter().map(|a| {
                ViewMenuEntry::item(format!("{NEW_WITH}{}", a.id), a.name)
                    .with_icon(a.icon)
                    .disabled_if(true)
            }));
        }
        rows
    }

    /// Session `i`'s menu (its right click's, its "⋮"'s), the session kept for the row chosen.
    pub(super) fn session_menu(&mut self, i: usize) -> Option<Vec<ViewMenuEntry>> {
        let session = self.sessions.get(i)?;
        self.menu_on = Some((session.agent, session.id.clone()));
        self.menu_favorite = Some(crate::favorites::Favorite {
            agent: session.agent.to_owned(),
            id: session.id.clone(),
            title: session.title.clone(),
        });
        let mark = self.marked.get(i).cloned().flatten().unwrap_or_default();
        let mut rows = vec![
            ViewMenuEntry::item(OPEN_TERMINAL, t("agents.open_terminal")).with_icon("terminal"),
        ];
        if self.chats() {
            rows.push(ViewMenuEntry::item(OPEN_CHAT, t("agents.open_chat")).with_icon(crate::ICON));
        }
        if let Some(row) = sleep_row(session.agent, &session.id) {
            rows.push(row);
        }
        rows.push(ViewMenuEntry::Separator);
        let pin = if mark.pinned { "agents.unpin" } else { "agents.pin" };
        rows.push(ViewMenuEntry::item(PIN, t(pin)).with_icon("pin"));
        let (done, done_icon) =
            if mark.done { ("agents.not_done", "rotate-ccw") } else { ("agents.done", "check") };
        rows.push(ViewMenuEntry::item(DONE, t(done)).with_icon(done_icon));
        let unread = if mark.unread { "agents.mark_read" } else { "agents.mark_unread" };
        rows.push(ViewMenuEntry::item(UNREAD, t(unread)).with_icon(crate::UNREAD_ICON));
        let mut labels: Vec<ViewMenuEntry> = Label::ALL
            .into_iter()
            .map(|l| {
                let name = t(&format!("agents.color_{}", l.key()));
                ViewMenuEntry::item(format!("{LABEL}{}", l.key()), name)
                    .with_swatch(l.rgb())
                    .checked_if(mark.label == Some(l))
            })
            .collect();
        labels.push(ViewMenuEntry::Separator);
        labels.push(
            ViewMenuEntry::item(LABEL, t("agents.color_default")).checked_if(mark.label.is_none()),
        );
        rows.push(ViewMenuEntry::submenu(t("agents.color_label"), labels).with_icon("tag"));
        rows.push(ViewMenuEntry::Separator);
        let key = if self.favorite.get(i).copied().unwrap_or(false) {
            "agents.unfavorite"
        } else {
            "agents.favorite"
        };
        rows.push(ViewMenuEntry::item(FAVORITE, t(key)).with_icon(crate::FAVORITE_ICON));
        let renamable = self.renamable(i);
        rows.push(
            ViewMenuEntry::item(RENAME, t("agents.rename"))
                .with_icon("text-cursor")
                .disabled_if(!renamable),
        );
        rows.push(self.move_menu(mark.folder.as_deref()));
        rows.push(ViewMenuEntry::Separator);
        let copies = COPY_WHAT
            .iter()
            .filter(|(what, _)| *what != "file" || session_file(self.sessions.get(i)).is_some())
            .map(|(what, key)| ViewMenuEntry::item(format!("{COPY}{what}"), t(key)))
            .collect();
        rows.push(ViewMenuEntry::submenu(t("agents.copy_info"), copies).with_icon("copy"));
        Some(rows)
    }

    /// Session `agent` / `id`'s terminal put to sleep by hand, asked of the terminal (its tab
    /// known by its key): nobody answers, or not now — nothing happens.
    pub(super) fn sleep(&mut self, agent: &str, id: &str) {
        let Some(key) = crate::follow::key_of(agent, id) else { return };
        crate::follow::expect(&key, contracts::terminal::CanSleep::Sleeping);
        let asked = contracts::terminal::SleepTerminal { key };
        self.action =
            Some(ToolPanelAction::View(ViewerAction::Ask(ui_components::Asked::new(asked))));
    }

    /// Session `agent` / `id`'s sleeping terminal woken, asked of the terminal (its tab
    /// brought to the front): nobody answers, or it isn't asleep — nothing happens.
    pub(super) fn wake(&mut self, agent: &str, id: &str) {
        let Some(key) = crate::follow::key_of(agent, id) else { return };
        crate::follow::expect(&key, contracts::terminal::CanSleep::Waking);
        let asked = contracts::terminal::WakeTerminal { key };
        self.action =
            Some(ToolPanelAction::View(ViewerAction::Ask(ui_components::Asked::new(asked))));
    }

    /// "Move to Folder ›": the project's folders (the one it's in checked), a new one, out
    /// of the one it's in.
    fn move_menu(&self, now: Option<&str>) -> ViewMenuEntry {
        let mut entries: Vec<ViewMenuEntry> = self
            .folders
            .iter()
            .map(|f| {
                ViewMenuEntry::item(format!("{MOVE}{}", f.id), f.name.clone())
                    .with_icon("folder")
                    .checked_if(now == Some(f.id.as_str()))
            })
            .collect();
        entries.push(ViewMenuEntry::Separator);
        entries.push(
            ViewMenuEntry::item(MOVE_NEW, t("agents.new_folder_with")).with_icon("folder-plus"),
        );
        if now.is_some() {
            entries.push(
                ViewMenuEntry::item(MOVE, t("agents.remove_from_folder")).with_icon("folder-x"),
            );
        }
        ViewMenuEntry::submenu(t("agents.move_to_folder"), entries).with_icon("folder-input")
    }

    /// A row of the folders' menus chosen: `true` if it was one.
    pub(super) fn folder_command(&mut self, id: &str) -> bool {
        let Some(project) = self.project else { return false };
        match id {
            NEW_FOLDER => self.new_folder(|_, _| {}),
            FOLDER_RENAME => {
                let Some(folder) = self.menu_folder.take() else { return true };
                let name = self.folders.iter().find(|f| f.id == folder).map(|f| f.name.clone());
                self.rename_in_row(Renamed::Folder(folder), &name.unwrap_or_default());
            }
            FOLDER_DELETE => {
                if let Some(folder) = self.menu_folder.take() {
                    crate::folders::remove(project, &folder);
                }
            }
            // A new folder, the session the menu was on moved into it.
            MOVE_NEW => {
                let on = self.menu_on.take();
                self.new_folder(|_, folder| {
                    if let Some((agent, session)) = on {
                        crate::folders::move_to(project, agent, &session, Some(folder));
                    }
                });
            }
            _ => {
                let Some(folder) = id.strip_prefix(MOVE) else { return false };
                if let Some((agent, session)) = self.menu_on.take() {
                    let folder = (!folder.is_empty()).then_some(folder);
                    crate::folders::move_to(project, agent, &session, folder);
                }
            }
        }
        true
    }

    /// What's copied of the session the menu was on (`what`: a [`COPY_WHAT`]'s).
    pub(super) fn copy(&mut self, what: &str) {
        let Some(i) = self.menu_session() else { return };
        let session = &self.sessions[i];
        let text = match what {
            "title" => session.title.clone(),
            "id" => session.id.clone(),
            "command" => {
                let Some(agent) = self.agents().into_iter().find(|a| a.id() == session.agent)
                else {
                    return;
                };
                agent.resume(session).display()
            }
            "file" => match session_file(Some(session)) {
                Some(file) => file.display().to_string(),
                None => return,
            },
            _ => return,
        };
        let job = ui_components::WithHost::once(move |host| host.set_clipboard_text(&text));
        self.action = Some(ToolPanelAction::View(ViewerAction::WithHost(job)));
    }
}

/// "Put to Sleep" for session `agent` / `id`: only while its terminal is open, someone puts
/// terminals to sleep, and it could sleep (greyed out, saying why, while something typed
/// isn't sent or it's on its way to sleep); "Wake Up" in its place while it sleeps (greyed
/// out while it wakes). Read from what the terminal said last, or what was just asked of it.
pub(super) fn sleep_row(agent: &str, id: &str) -> Option<ViewMenuEntry> {
    use contracts::terminal::CanSleep;
    let state = crate::follow::state_of(agent, id)?;
    let waking = state.can_sleep == CanSleep::Waking;
    if state.asleep || waking {
        let wakes = workbench_model::presence::request::<contracts::terminal::WakeTerminal>();
        let label = t(if waking { "agents.waking" } else { "agents.wake" });
        return wakes
            .then(|| ViewMenuEntry::item(WAKE, label).with_icon("sun").disabled_if(waking));
    }
    if !workbench_model::presence::request::<contracts::terminal::SleepTerminal>() {
        return None;
    }
    let can = state.can_sleep;
    let label = match can {
        CanSleep::No | CanSleep::Waking => return None,
        CanSleep::Yes => t("agents.sleep"),
        CanSleep::Typed => t("agents.sleep_typed"),
        CanSleep::Sleeping => t("agents.going_to_sleep"),
    };
    Some(
        ViewMenuEntry::item(SLEEP, label)
            .with_icon(super::paint::ASLEEP_ICON)
            .disabled_if(can != CanSleep::Yes),
    )
}
