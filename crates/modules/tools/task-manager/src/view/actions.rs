//! The parts' actions: what a part offers (stop a program, close a session, stop an
//! operation, release its data, close its idle sessions) asked of whoever reported it —
//! through the window's resources ([`workbench_model::ResourceSource::act`]); the Task
//! Manager never stops anything by its id itself. Those that lose work (`confirm`) are asked
//! first in the module's modal ([`crate::confirm`]: closed only by ×, Esc or its buttons,
//! nothing behind it reached); how each went is said as a notification (a toast).

use std::sync::Weak;

use i18n::{t, tr};
use workbench_model::{ActOutcome, ActionKind, ActionTarget, Notification, PartAction};

use super::TaskManager;
use crate::collector::Line;

/// An action's place among the labels ([`super::texts::Labels::actions`]).
pub(crate) fn kind_index(kind: ActionKind) -> usize {
    match kind {
        ActionKind::Stop => 0,
        ActionKind::Close => 1,
        ActionKind::Restart => 2,
        ActionKind::Release => 3,
        ActionKind::Disconnect => 4,
        ActionKind::ShowLog => 5,
        ActionKind::Settings => 6,
    }
}

/// The icon of an action's small button (a program's, an operation's).
pub(super) fn kind_icon(kind: ActionKind) -> &'static str {
    match kind {
        ActionKind::Stop => "stop",
        ActionKind::Close => "x",
        ActionKind::Restart => "rotate-ccw",
        ActionKind::Release => "trash",
        ActionKind::Disconnect => "x-square",
        ActionKind::ShowLog => "file-text",
        ActionKind::Settings => "settings",
    }
}

/// What an action is done to, as said to the user: a program (`node · 4243`), an operation
/// (its subject), the part (its name).
pub(super) fn subject(line: &Line, target: ActionTarget) -> String {
    match target {
        ActionTarget::Part => line.name.clone(),
        ActionTarget::Process(pid) => line
            .processes
            .iter()
            .find(|p| p.pid == pid)
            .and_then(|p| p.name.as_deref())
            .map_or_else(|| pid.to_string(), |name| format!("{name} · {pid}")),
        ActionTarget::Activity(id) => line
            .activities
            .iter()
            .find(|a| a.id == id)
            .map_or_else(|| line.name.clone(), |a| a.subject.clone()),
    }
}

/// An action written in a menu row's id (`act:<id>:<target>`) and read back.
pub(super) fn encode(action: &PartAction) -> String {
    let target = match action.target {
        ActionTarget::Part => "part".to_owned(),
        ActionTarget::Process(pid) => format!("p{pid}"),
        ActionTarget::Activity(id) => format!("a{id:x}"),
    };
    format!("act:{}:{target}", action.id)
}

/// The action id and target an `act:` command names.
pub(super) fn decode(command: &str) -> Option<(&str, ActionTarget)> {
    let rest = command.strip_prefix("act:")?;
    let (id, target) = rest.rsplit_once(':')?;
    let target = match target {
        "part" => ActionTarget::Part,
        t if t.starts_with('p') => ActionTarget::Process(t[1..].parse().ok()?),
        t if t.starts_with('a') => ActionTarget::Activity(u128::from_str_radix(&t[1..], 16).ok()?),
        _ => return None,
    };
    Some((id, target))
}

impl TaskManager {
    /// Action `action` of part `key` asked for: done now, or — losing work — asked first.
    pub(crate) fn request(&mut self, key: &str, action: PartAction) {
        let Some(line) = self.frame.line(key) else {
            self.notifications.push(Notification::warning(t("tasks.action_gone")));
            return;
        };
        if !action.confirm {
            return self.perform(key, action);
        }
        let what = subject(line, action.target);
        let (title, body, icon) = match action.kind {
            ActionKind::Close => (
                tr!("tasks.confirm_close", name = what.as_str()),
                t("tasks.confirm_close_body"),
                "x",
            ),
            _ => (
                tr!("tasks.confirm_stop", name = what.as_str()),
                t("tasks.confirm_stop_body"),
                "stop",
            ),
        };
        self.popover = None;
        let question = crate::confirm::Question {
            title,
            body,
            button: self.texts.labels.actions[kind_index(action.kind)].clone(),
            icon,
            yes: crate::confirm::Yes::default(),
        };
        let asked = question.clone();
        self.asking = Some((question, key.to_owned(), action));
        self.action = Some(ui_components::ViewerAction::WithHost(ui_components::WithHost::once(
            move |host| {
                host.open_modal(&asked.title, crate::confirm::EXTENSION, &asked);
            },
        )));
    }

    /// The question it asked answered "yes" in the modal: the action done. `true` if it was.
    pub(crate) fn confirmed(&mut self) -> bool {
        let Some((question, ..)) = &self.asking else { return false };
        if !question.yes.take() {
            return false;
        }
        let Some((_, key, action)) = self.asking.take() else { return false };
        self.perform(&key, action);
        true
    }

    /// What it asked last (the tests'): the question's identity, the part's key, the action.
    #[cfg(test)]
    pub(crate) fn asking(&self) -> Option<(&crate::confirm::Question, &str, PartAction)> {
        self.asking.as_ref().map(|(question, key, action)| (question, key.as_str(), *action))
    }

    /// Action `action` of part `key` done by whoever reported the part; how it went said.
    fn perform(&mut self, key: &str, action: PartAction) {
        let Some(line) = self.frame.line(key) else {
            self.notifications.push(Notification::warning(t("tasks.action_gone")));
            return;
        };
        let what = subject(line, action.target);
        let part = line.part_ref();
        let outcome = match self.source.as_ref().and_then(Weak::upgrade) {
            Some(source) => source.act(&part, action.id, action.target),
            None => ActOutcome::Unsupported,
        };
        let name = what.as_str();
        // What opens a view (a log, the settings) runs as from its menu: nothing to say.
        if let ActOutcome::Command(command) = outcome {
            self.action = Some(ui_components::ViewerAction::RunCommand(command));
            return;
        }
        self.notifications.push(match outcome {
            ActOutcome::Done => Notification::success(match action.kind {
                ActionKind::Stop => tr!("tasks.done_stop", name = name),
                ActionKind::Close => tr!("tasks.done_close", name = name),
                ActionKind::Restart => tr!("tasks.done_restart", name = name),
                ActionKind::Release => tr!("tasks.done_release", name = name),
                ActionKind::Disconnect => tr!("tasks.done_disconnect", name = name),
                ActionKind::ShowLog | ActionKind::Settings => tr!("tasks.done_open", name = name),
            }),
            ActOutcome::Failed(why) => {
                Notification::error(tr!("tasks.action_failed", name = name, reason = why.as_str()))
            }
            ActOutcome::Unsupported | ActOutcome::Command(_) => {
                Notification::warning(tr!("tasks.action_unsupported", name = name))
            }
        });
        // Its effect shows at the next look: asked now.
        if let Some(subscription) = &self.subscription {
            subscription.look_now();
        }
    }

    /// The action the palette's "Stop" means for the keys' row: its program's, its
    /// operation's, else the part's own (stopping or closing it).
    pub(super) fn stop_target(&mut self) -> Option<(String, PartAction)> {
        let model = self.model();
        let i = self.cursor_index(&model)?;
        let row = super::keys::row_ref(&model, i)?;
        let stops = |a: &&PartAction| matches!(a.kind, ActionKind::Stop | ActionKind::Close);
        let (key, target) = match row {
            super::RowRef::Process(key, pid) => (key, Some(ActionTarget::Process(pid))),
            super::RowRef::Operation(id) => {
                let owner = model.operations().find(|o| o.activity.id == id)?.owner.clone();
                (owner, Some(ActionTarget::Activity(id)))
            }
            super::RowRef::Part(key) => (key, None),
            super::RowRef::Group(_) => return None,
        };
        let line = model.frame.line(&key)?;
        let action = match target {
            Some(target) => line.actions_on(target).find(stops),
            None => line
                .actions_on(ActionTarget::Part)
                .find(stops)
                .or_else(|| line.actions.iter().find(stops)),
        };
        Some((key, *action?))
    }
}

#[cfg(test)]
#[path = "../../tests/unit/view/actions.rs"]
mod tests;
