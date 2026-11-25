//! The code actions at the cursor: the bulb beside its line when there are some, and "Show
//! Context Actions" (⌥⏎, ⌘., a click on the bulb) listing them — the built-in engine's
//! quick fixes ("Import class", "Remove unused import"), the refactorings that apply there
//! (the modules' rows of the editor's Refactor section), and the file's language server's
//! code actions, each title once (ours first). Ours are cheap and worked out after the
//! cursor rests ([`BULB_DELAY`]); the server is asked then, its answer looked at when it
//! comes. A server's action is applied as a refactoring (one undo), its command run by it.

use std::time::{Duration, Instant};

use code_intelligence::servers::{CodeAction, Polled, Reply, WorkspaceChange};
use workbench_model::Notification;

use super::*;

/// How long the cursor rests before the bulb is worked out.
pub(super) const BULB_DELAY: Duration = Duration::from_millis(300);

/// A row of the actions' list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ContextAction {
    /// Ours: replacements in the active document (one undo).
    Edits(Vec<(std::ops::Range<usize>, String)>),
    /// A refactoring command (its own dialog).
    Command(command_model::CommandId),
    /// The server's action `id` (of its last list).
    Served(u64),
}

/// An action as listed: its title, what kind it is (shown dim), whether it's a fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Listed {
    pub title: String,
    pub kind: String,
    pub fix: bool,
    pub action: ContextAction,
}

/// Where the cursor rested: the document, its version, the cursor.
type BulbKey = (DocumentId, u64, usize);

/// The bulb's state and what the actions' list waits for (the project's).
#[derive(Debug, Default)]
pub(super) struct ActionsState {
    key: Option<BulbKey>,
    /// When the cursor's rest is over (the bulb worked out then).
    due: Option<Instant>,
    /// Ours at the key.
    ours: Vec<Listed>,
    /// The server's question, and its answer (`None` inside: none came).
    asked: Option<Reply<Vec<CodeAction>>>,
    served: Option<Vec<CodeAction>>,
    /// The list was asked for: shown when the server's answer comes, or at this time.
    menu: Option<(Instant, Option<Point>)>,
    /// A server's action being worked out (its edit).
    running: Option<Reply<Result<WorkspaceChange, String>>>,
    /// A row chosen: done at the next frame (it needs the text system).
    pub chosen: Option<ContextAction>,
}

impl Workbench {
    /// Keeps the bulb up to date with the cursor (cheap while it moves: worked out once it
    /// rests). `true` if what's shown changed.
    pub(super) fn sync_bulb(&mut self) -> bool {
        let Some(tab) = self.tabs.get(self.active).filter(|t| t.editor_visible()) else {
            // No editor in front (another kind of tab): nothing waits to be worked out —
            // a wait left due would ask for a frame at every frame. Worked out anew back.
            self.data.actions.due = None;
            self.data.actions.key = None;
            return false;
        };
        let key = (tab.doc.id(), tab.doc.version(), tab.doc.selections().primary().head);
        let state = &mut self.data.actions;
        if state.key != Some(key) {
            let shown = state.key.is_some() && (!state.ours.is_empty() || state.served.is_some());
            state.key = Some(key);
            state.due = Some(Instant::now() + BULB_DELAY);
            state.ours.clear();
            state.asked = None;
            state.served = None;
            state.menu = None;
            return shown;
        }
        if state.due.is_some_and(|at| at <= Instant::now()) {
            state.due = None;
            self.work_out_actions(key.2);
            return true;
        }
        false
    }

    /// When the bulb is next worked out.
    pub(super) fn actions_deadline(&self) -> Option<Instant> {
        let s = &self.data.actions;
        [s.due, s.menu.map(|(at, _)| at)].into_iter().flatten().min()
    }

    /// The bulb, if there are actions at the cursor.
    pub(super) fn bulb(&self) -> Option<ui_workbench::Bulb> {
        // Turned off in the settings: no bulb (⌥⏎ still lists the actions).
        if !crate::insight::bulb() {
            return None;
        }
        let s = &self.data.actions;
        let tab = self.tabs.get(self.active)?;
        let key = s.key?;
        if key.0 != tab.doc.id() || key.1 != tab.doc.version() || s.due.is_some() {
            return None;
        }
        let served = s.served.as_deref().unwrap_or_default();
        let fix = s.ours.iter().any(|a| a.fix)
            || served.iter().any(|a| {
                a.preferred || a.kind.as_deref().is_some_and(|k| k.starts_with("quickfix"))
            });
        // The server's own source actions (organize imports) are everywhere: no bulb for them.
        let other =
            served.iter().any(|a| !a.kind.as_deref().is_some_and(|k| k.starts_with("source")));
        let line = tab.doc.buffer().line_of(key.2) as u32;
        (fix || other).then_some(ui_workbench::Bulb { line, fix })
    }

    /// Ours at `head` worked out, the server asked.
    fn work_out_actions(&mut self, head: usize) {
        self.data.actions.ours = self.built_in_fixes(head);
        let range = self.tabs.get(self.active).map_or(head..head, |t| {
            let p = t.doc.selections().primary();
            if p.is_empty() { head..head } else { p.range() }
        });
        let asked = self.ask_server(head, |s, at| s.code_actions(at, range));
        self.data.actions.asked = asked;
    }

    /// The server's answers (its actions, an action's edit). `true` if one came.
    pub(super) fn poll_actions(&mut self) -> bool {
        let mut changed = false;
        let state = &mut self.data.actions;
        if let Some(reply) = state.asked.as_mut() {
            match reply.poll() {
                Polled::Waiting => {}
                Polled::Ready(actions) => {
                    state.asked = None;
                    state.served = Some(actions);
                    changed = true;
                }
                Polled::Gone => {
                    state.asked = None;
                    state.served = Some(Vec::new());
                    changed = true;
                }
            }
        }
        if let Some(reply) = state.running.as_mut() {
            match reply.poll() {
                Polled::Waiting => {}
                Polled::Ready(result) => {
                    state.running = None;
                    changed = true;
                    match result {
                        Ok(change) if change.is_empty() => {}
                        Ok(change) => {
                            if let Err(why) = self.apply_refactoring(&refactor::plan_of(&change)) {
                                self.show_notification(Notification::error(i18n::tr!(
                                    "actions.not_applied",
                                    reason = why
                                )));
                            }
                        }
                        Err(why) => {
                            self.show_notification(Notification::error(i18n::tr!(
                                "actions.not_applied",
                                reason = why
                            )));
                        }
                    }
                }
                Polled::Gone => {
                    self.data.actions.running = None;
                    self.show_notification(Notification::warning(i18n::t("actions.no_answer")));
                }
            }
        }
        changed
    }

    /// "Show Context Actions" at the cursor: listed now when nothing is waited for, else
    /// when the server's answer comes (within [`super::servers::SERVER_GRACE`]).
    pub fn show_context_actions(&mut self, text: &mut TextSystem) -> bool {
        let Some(tab) = self.tabs.get(self.active).filter(|t| t.editor_visible()) else {
            return false;
        };
        let head = tab.doc.selections().primary().head;
        let key = (tab.doc.id(), tab.doc.version(), head);
        // Not worked out yet for here (the cursor just moved): now.
        if self.data.actions.key != Some(key) || self.data.actions.due.is_some() {
            self.data.actions.key = Some(key);
            self.data.actions.due = None;
            self.data.actions.served = None;
            self.work_out_actions(head);
        }
        let anchor = self.caret_rect(text).map(|r| Point::new(r.min_x(), r.max_y()));
        if self.data.actions.asked.is_some() {
            let at = Instant::now() + super::servers::SERVER_GRACE;
            self.data.actions.menu = Some((at, anchor));
            return true;
        }
        self.open_actions_list(anchor)
    }

    /// The list waited for: shown once the server answered or its grace is over.
    pub(super) fn show_waited_actions(&mut self) {
        let Some((at, anchor)) = self.data.actions.menu else { return };
        if self.data.actions.asked.is_some() && at > Instant::now() {
            return;
        }
        self.data.actions.menu = None;
        self.open_actions_list(anchor);
    }

    /// The actions' list: ours, the refactorings there, the server's (each title once).
    fn open_actions_list(&mut self, anchor: Option<Point>) -> bool {
        let mut listed = self.data.actions.ours.clone();
        listed.extend(self.refactorings_here());
        for action in self.data.actions.served.clone().unwrap_or_default() {
            let title = action.title.trim().to_owned();
            if listed.iter().any(|l| l.title.eq_ignore_ascii_case(&title)) {
                continue;
            }
            let kind = action.kind.clone().unwrap_or_default();
            let fix = action.preferred || kind.starts_with("quickfix");
            listed.push(Listed { title, kind, fix, action: ContextAction::Served(action.id) });
        }
        if listed.is_empty() {
            self.show_notification(Notification::info(i18n::t("actions.none")));
            return false;
        }
        // Fixes first, then the refactorings, then the rest (the server's source actions).
        listed.sort_by_key(|l| match &l.action {
            _ if l.fix => 0,
            ContextAction::Command(_) => 1,
            _ if l.kind.starts_with("source") => 3,
            _ => 2,
        });
        let choices: Vec<ui_workbench::Choice> = listed
            .iter()
            .map(|l| {
                let (icon, group) = match &l.action {
                    _ if l.fix => ("lightbulb", 0),
                    ContextAction::Command(id) => {
                        (crate::commands::menu_icon(id.0).unwrap_or("file-pen"), 1)
                    }
                    ContextAction::Served(_) => ("zap", 2),
                    ContextAction::Edits(_) => ("sparkles", 2),
                };
                let shortcut = match &l.action {
                    ContextAction::Command(id) => crate::commands::shortcut_label(*id),
                    _ => None,
                };
                ui_workbench::Choice {
                    icon,
                    label: l.title.clone(),
                    value: l.kind.clone(),
                    shortcut,
                    group,
                }
            })
            .collect();
        let subject = self.tabs.get(self.active).map_or_else(String::new, |t| t.title());
        self.code_hover.hide();
        self.completion.close();
        let (title, action) = (i18n::t("actions.title"), i18n::t("actions.apply"));
        self.choice_menu.open((&title, &subject, "lightbulb", &action), choices, anchor);
        self.choice_purpose =
            ChoicePurpose::Actions(listed.into_iter().map(|l| l.action).collect());
        self.choice_menu.is_open()
    }

    /// A chosen row done (at the next frame: it may edit the text).
    pub(super) fn run_chosen_action(&mut self, text: &mut TextSystem) {
        let Some(action) = self.data.actions.chosen.take() else { return };
        match action {
            ContextAction::Edits(edits) => {
                let edits: Vec<document::Edit> =
                    edits.into_iter().map(|(r, t)| document::Edit::new(r, t)).collect();
                self.move_focus(Focus::Editor);
                self.edit(text, |doc| {
                    doc.apply(document::Transaction::new(edits), None, document::EditKind::Command);
                });
            }
            ContextAction::Command(id) => {
                let _ = self.run_command(id);
            }
            ContextAction::Served(id) => {
                let head =
                    self.tabs.get(self.active).map_or(0, |t| t.doc.selections().primary().head);
                self.data.actions.running = self.ask_server(head, |s, at| s.run_action(at, id));
            }
        }
    }

    /// The modules' refactorings that can run here (the editor menu's Refactor section).
    fn refactorings_here(&self) -> Vec<Listed> {
        let items = self.section_items(
            workbench_model::MenuPlace::Editor,
            workbench_model::MenuSection::Refactor,
        );
        items
            .into_iter()
            .filter_map(|item| match item {
                platform_api::MenuItem::Command { id, label, .. } => {
                    let command = self.support.commands.iter().find(|c| c.spec.id.0 == id)?.spec.id;
                    Some(Listed {
                        title: label.trim_end_matches('…').to_owned(),
                        kind: String::new(),
                        fix: false,
                        action: ContextAction::Command(command),
                    })
                }
                _ => None,
            })
            .collect()
    }

    /// The built-in engine's quick fixes at `head`: the import of the class named there
    /// (each class of that name the index knows), the removal of an unused import there.
    pub(super) fn built_in_fixes(&mut self, head: usize) -> Vec<Listed> {
        let mut out = Vec::new();
        let Some(intel) = self.intelligence_at(head) else { return out };
        let Some(tab) = self.tabs.get(self.active) else { return out };
        let source = tab.text();
        // Remove an unused import (its whole line, when the line is only that).
        let unused = tab
            .diagnostics
            .unused
            .iter()
            .find(|u| u.kind == "import" && u.range.start <= head && head <= u.range.end);
        if let Some(unused) = unused {
            let buffer = tab.doc.buffer();
            let line = buffer.line_of(unused.range.start);
            let (start, end) = (buffer.line_start(line), buffer.line_end(line));
            let content = buffer.slice(start..end);
            let trimmed = content.trim();
            if trimmed.starts_with("use ") && trimmed.ends_with(';') && !trimmed.contains('{') {
                let end = (end + 1).min(buffer.len());
                out.push(Listed {
                    title: i18n::tr!("actions.remove_import", name = unused.name.as_str()),
                    kind: String::new(),
                    fix: true,
                    action: ContextAction::Edits(vec![(start..end, String::new())]),
                });
            }
        }
        // Import the class named at the cursor, when nothing it's written as resolves.
        let naming = intel.naming();
        if let Some(code_intelligence::Target::Class(name)) = intel.target(&source, head)
            && !name.is_empty()
            && naming.namespace_separator.is_none_or(|s| !name.contains(s))
        {
            self.refresh_symbols_now();
            let path = Self::index_path(&self.tabs[self.active]);
            if let Ok(index) = self.data.symbols.read() {
                let resolved = index.file(&path).map(|f| f.resolve_class(&name, head, &naming));
                let known = resolved.as_deref().is_some_and(|fqn| index.type_by_fqn(fqn).is_some());
                if !known {
                    let mut fqns: Vec<String> = index
                        .by_name(&name)
                        .into_iter()
                        .filter(|(_, s)| s.kind.is_type() && *s.name == *name)
                        .map(|(_, s)| s.fqn.to_string())
                        .collect();
                    fqns.sort();
                    fqns.dedup();
                    for fqn in fqns.into_iter().take(8) {
                        let Some((at, statement)) = intel.import_edit(&source, &fqn) else {
                            continue;
                        };
                        out.push(Listed {
                            title: i18n::tr!("actions.import_class", name = fqn.as_str()),
                            kind: String::new(),
                            fix: true,
                            action: ContextAction::Edits(vec![(at..at, statement)]),
                        });
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/context_actions.rs"]
mod tests;
