//! The open dialog and its answer, the question a module asked, what a name dialog renames.

use super::*;

/// The dialog shown and what was answered.
#[derive(Debug, Default)]
pub(super) struct Dialogs {
    /// The open dialog (save, open project…) and the answer received.
    pub dialog: ui_workbench::Dialog,
    pub answer: Option<&'static str>,
    /// The text in the dialog's field, at answer time (e.g. the new name).
    pub value: Option<String>,
    /// The answered dialog's checkbox was checked.
    pub checked: bool,
    /// A question asked (`WorkbenchHost::confirm`): what runs on "yes".
    pub confirming: Option<Confirming>,
    /// The bottom window's tab being renamed (the dialog's answer).
    pub tool_rename: Option<usize>,
    /// The editor's tab whose view is being renamed (its document: the tabs may move).
    pub view_rename: Option<kernel::DocumentId>,
}

impl Dialogs {
    /// The dialog was answered: its field's text and its checkbox are kept, and it closes.
    pub fn answered(&mut self) {
        self.value = self.dialog.input.as_ref().map(|i| i.text().to_owned());
        self.checked = self.dialog.checked();
        self.dialog.close();
    }
}
