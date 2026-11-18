//! The footer's buttons: testing the connection and its outcome, applying, closing (asking before
//! what changed is lost).

use super::*;

impl SourcesView {
    /// The selected source's connection tried in the background (the one running
    /// forgotten).
    pub fn test(&mut self) {
        let (Some(request), Some(editor)) = (&self.request, &self.editor) else { return };
        let Some(entry) = editor.current() else { return };
        let Some(driver) = request.sessions.driver(&entry.source.driver) else {
            self.tester.set(Probe::Failed(t("datasources.no_driver")));
            return;
        };
        let typed = entry.password_typed.then(|| Secret::new(entry.password.text()));
        self.tested = Some(entry.source.id);
        let mut source = entry.source.clone();
        source.file = source.file_in(request.sessions.root());
        self.tester.start(
            request.sessions.runtime(),
            driver,
            source,
            typed,
            request.sessions.credentials(),
            self.wake(),
        );
    }

    pub fn cancel_test(&mut self) {
        self.tester.cancel();
    }

    /// The probe shown is the selected source's.
    pub fn probe_shown(&self) -> Probe {
        let current = self.editor.as_ref().and_then(|e| e.current()).map(|e| e.source.id);
        if self.tested.is_some() && self.tested == current {
            self.tester.probe()
        } else {
            Probe::Idle
        }
    }

    /// The test's schemas kept for the Schemas tab.
    pub(super) fn take_probe(&mut self) {
        if let (Probe::Connected { schemas, databases, .. }, Some(id), Some(editor)) =
            (self.tester.probe(), self.tested, &mut self.editor)
        {
            editor.schemas_found.retain(|(i, _)| *i != id);
            editor.schemas_found.push((id, schemas));
            editor.databases_found.retain(|(i, _)| *i != id);
            editor.databases_found.push((id, databases));
        }
    }

    /// What a footer's button does (a click on it comes as its command).
    pub(super) fn footer(&mut self, button: Footer) {
        match button {
            Footer::Test => {
                if self.tester.probe() == Probe::Running {
                    self.cancel_test();
                } else {
                    self.test();
                }
            }
            Footer::Cancel => self.close(),
            Footer::Apply => {
                self.apply();
            }
            Footer::Ok => self.ok(),
            Footer::KeepEditing => self.confirm_close = false,
            Footer::Discard => self.discard(),
        }
    }

    /// The changes applied: the catalog now, the passwords in the background. `false`
    /// (and why, shown) when a field must be fixed first or the catalog refused.
    pub fn apply(&mut self) -> bool {
        let wake = self.wake();
        let (Some(request), Some(editor)) = (self.request.clone(), &mut self.editor) else {
            return false;
        };
        // What's typed in the selected source's fields is in it.
        if let Some(problem) = editor.problems().into_iter().next() {
            let at = problem.entry();
            if editor.selected != Some(at) {
                editor.select(Some(at));
            }
            self.error = Some(problem_text(editor, &problem));
            return false;
        }
        let plan = editor.plan();
        if plan.is_empty() {
            return true;
        }
        self.secrets_pending = !plan.secrets.is_empty();
        let notes = super::super::move_notes(&plan);
        let saved = Arc::clone(&self.saved);
        *saved.lock().unwrap_or_else(PoisonError::into_inner) = None;
        self.failure_told = false;
        let done: Arc<dyn Fn(Result<(), String>) + Send + Sync> = Arc::new(move |outcome| {
            *saved.lock().unwrap_or_else(PoisonError::into_inner) = Some(outcome);
            wake();
        });
        match super::super::apply(&request.sessions, &plan, done) {
            Ok(()) => {
                editor.applied();
                self.error = None;
                // Told as notifications (over the modal): saved, and where passwords went.
                request.sessions.notify(Notification::success(t("datasources.applied")));
                for note in notes {
                    request.sessions.notify(Notification::info(note));
                }
                (request.wake)();
                true
            }
            Err(why) => {
                self.error = Some(why);
                false
            }
        }
    }

    /// Applied, then closed (once the passwords are kept).
    pub fn ok(&mut self) {
        if !self.apply() {
            return;
        }
        let pending = self.saved.lock().unwrap_or_else(PoisonError::into_inner).is_none();
        if self.secrets_pending && pending {
            self.close_when_saved = true;
            return;
        }
        self.close_now();
    }

    /// Closing asked (×, Esc, Cancel): with changes, the user is asked first.
    pub fn close(&mut self) {
        if self.popup.is_some() {
            self.close_popup();
            return;
        }
        if self.editor.as_ref().is_some_and(Editor::dirty) && !self.confirm_close {
            self.confirm_close = true;
            return;
        }
        self.close_now();
    }

    pub(super) fn close_now(&mut self) {
        self.tester.cancel();
        self.action = Some(ViewerAction::Close);
    }

    pub fn discard(&mut self) {
        self.confirm_close = false;
        self.close_now();
    }
}
