//! The question about what an earlier run left running (`crate::left_behind`): one program
//! at a time in the app's dialog — what it is, its project, how long it has run, whether it
//! works now — with Stop / Leave it, and, while more wait, Stop all / Leave all.

use super::*;

impl EditorApp {
    /// What an earlier run left for this window (its project's; `first`: also those of no
    /// project) asked about, unless a question is already open here.
    pub fn offer_left_behind(&mut self, first: bool) {
        if self.pending.asking.is_some()
            || !self.pending.left_behind.is_empty()
            || self.workbench.dialog_open()
            || self.workbench.modal_open()
        {
            return;
        }
        let root = self.project_root();
        let found = crate::left_behind::take(root.as_deref(), first);
        if found.is_empty() {
            return;
        }
        self.pending.left_behind_total = found.len();
        self.pending.left_behind = found.into();
        self.ask_left_behind();
    }

    /// The question about the first program still to ask about.
    fn ask_left_behind(&mut self) {
        use ui_workbench::{ButtonKind, DialogButton, DialogSpec, ds::Tone};
        let Some(found) = self.pending.left_behind.front() else { return };
        let row = process_registry::row(found, process_registry::row::now());
        let total = self.pending.left_behind_total;
        let left = self.pending.left_behind.len();
        let title = if total > 1 {
            let index = total.saturating_sub(left).saturating_add(1);
            i18n::tr!("left_behind.title_count", index = index, count = total)
        } else {
            i18n::t("left_behind.title")
        };
        let mut buttons = Vec::new();
        if left > 1 {
            buttons.push(DialogButton::new(
                "leave_all",
                i18n::t("left_behind.leave_all"),
                ButtonKind::Secondary,
            ));
            buttons.push(DialogButton::new(
                "stop_all",
                i18n::t("left_behind.stop_all"),
                ButtonKind::Destructive,
            ));
        }
        buttons.push(DialogButton::new(
            "leave",
            i18n::t("left_behind.leave"),
            ButtonKind::Secondary,
        ));
        buttons.push(DialogButton::new("stop", i18n::t("left_behind.stop"), ButtonKind::Primary));
        self.workbench.open_dialog(DialogSpec {
            tone: Tone::Accent,
            icon: "clock",
            title,
            message: crate::left_behind::message(&row),
            buttons,
            // Esc, ×: left running (nothing is stopped without being asked to).
            cancel: "leave",
            input: None,
            check: None,
            list: Vec::new(),
        });
        self.pending.asking = Some(Ask::LeftBehind);
        self.redraw();
    }

    /// The answer about the first program (or, for all, every one still waiting); the next
    /// one asked about.
    pub(super) fn on_left_behind(&mut self, answer: &'static str) {
        let (answered, stop): (Vec<_>, bool) = match answer {
            "stop_all" => (self.pending.left_behind.drain(..).collect(), true),
            "leave_all" => (self.pending.left_behind.drain(..).collect(), false),
            _ => (self.pending.left_behind.pop_front().into_iter().collect(), answer == "stop"),
        };
        crate::left_behind::answer(answered, stop);
        if self.pending.left_behind.is_empty() {
            self.pending.left_behind_total = 0;
        } else {
            self.ask_left_behind();
        }
        self.relayout();
    }
}
