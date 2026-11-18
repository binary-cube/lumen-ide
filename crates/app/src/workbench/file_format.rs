//! The active file's format chosen from the status bar's file window (or the palette): its
//! language, the encoding it's written in or read as, its line terminator, its indentation.
//! Each opens the choice list with the current one checked. A terminator or encoding chosen
//! is a change to save (the document says so); "read as" reads the file again on the
//! reload worker, its result taken at the next poll. A language chosen is the tab's for
//! as long as it's open.

use document::{Decoded, Encoding, LineEnding};
use i18n::{t, tr};

use super::*;

/// The worker files are read again on (the one following the disk's changes).
const READ_THREAD: &str = "file-reload";

/// What a row of a format list does.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum FormatPick {
    Language(LanguageId),
    /// Written in it from the next save.
    Convert(Encoding),
    /// The file read again as it.
    ReadAs(Encoding),
    LineEnding(LineEnding),
    Indent(Indent),
}

/// A file read again in the encoding the user chose: its path, its stamp, the text.
pub(super) type FormatRead = (PathBuf, Option<DiskStamp>, Decoded);

/// The indentations offered.
const INDENTS: [Indent; 4] =
    [Indent::Spaces(2), Indent::Spaces(4), Indent::Spaces(8), Indent::Tabs];

impl Workbench {
    /// The active tab's text file (none for a viewer's tab without text).
    fn format_tab(&self) -> Option<&super::Tab> {
        self.tabs.get(self.active).filter(|t| t.holds_text())
    }

    /// The tab's file name, as the list's subject.
    fn format_subject(&self) -> String {
        self.format_tab()
            .and_then(|t| t.doc.path())
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// Opens a format list: `rows` (icon, label, detail, group, what it does).
    fn open_format_list(
        &mut self,
        title: &str,
        icon: &'static str,
        rows: Vec<(&'static str, String, String, u8, FormatPick)>,
    ) -> bool {
        if rows.is_empty() {
            return false;
        }
        let Some(doc) = self.format_tab().map(|t| t.doc.id()) else { return false };
        let subject = self.format_subject();
        let action = t("format.use");
        let (choices, picks): (Vec<_>, Vec<_>) = rows
            .into_iter()
            .map(|(icon, label, value, group, pick)| {
                (ui_workbench::Choice { icon, label, value, shortcut: None, group }, pick)
            })
            .unzip();
        self.code_hover.hide();
        self.completion.close();
        self.choice_menu.open((title, subject.as_str(), icon, action.as_str()), choices, None);
        self.choice_purpose = ChoicePurpose::Format(doc, picks);
        self.choice_menu.is_open()
    }

    /// The languages, the file's checked first.
    pub fn choose_language(&mut self) -> bool {
        let Some(tab) = self.format_tab() else { return false };
        let current = tab.syntax.language().clone();
        let mut languages: Vec<_> = self.support.languages.languages().iter().collect();
        languages.sort_by_key(|l| (l.id != current, l.name.to_lowercase()));
        let rows = languages
            .into_iter()
            .map(|l| {
                let on = l.id == current;
                let detail = if on { t("format.current") } else { String::new() };
                (
                    if on { "check" } else { l.icon },
                    l.name.to_owned(),
                    detail,
                    0,
                    FormatPick::Language(l.id.clone()),
                )
            })
            .collect();
        self.open_format_list(&t("format.language_title"), "file-code", rows)
    }

    /// The encodings: written in one (from the next save), or the file read again as one
    /// (only while it has no unsaved changes: they'd be lost).
    pub fn choose_encoding(&mut self) -> bool {
        let Some(tab) = self.format_tab() else { return false };
        let current = tab.doc.encoding();
        let can_read = tab.doc.path().is_some() && tab.untitled.is_none() && !tab.doc.is_dirty();
        let mut rows = Vec::new();
        for e in Encoding::ALL {
            let on = e == current;
            let detail = if on { t("format.current") } else { t("format.convert") };
            rows.push((
                if on { "check" } else { "file-text" },
                e.label().to_owned(),
                detail,
                0,
                FormatPick::Convert(e),
            ));
        }
        if can_read {
            for e in Encoding::ALL.into_iter().filter(|&e| e != current) {
                let label = tr!("format.read_as", encoding = e.label());
                rows.push((
                    "rotate-ccw",
                    label,
                    t("format.read_as_detail"),
                    1,
                    FormatPick::ReadAs(e),
                ));
            }
        }
        self.open_format_list(&t("format.encoding_title"), "file-text", rows)
    }

    /// LF or CRLF, from the next save.
    pub fn choose_line_ending(&mut self) -> bool {
        let Some(tab) = self.format_tab() else { return false };
        let current = tab.doc.line_ending();
        let rows = [(LineEnding::Lf, "format.lf"), (LineEnding::CrLf, "format.crlf")]
            .into_iter()
            .map(|(le, detail)| {
                let on = le == current;
                (
                    if on { "check" } else { "text-wrap" },
                    le.label().to_owned(),
                    t(detail),
                    0,
                    FormatPick::LineEnding(le),
                )
            })
            .collect();
        self.open_format_list(&t("format.line_ending_title"), "text-wrap", rows)
    }

    /// 2, 4 or 8 spaces, or a tab: what a new level of indentation is.
    pub fn choose_indent(&mut self) -> bool {
        let Some(tab) = self.format_tab() else { return false };
        let current = tab.doc.indent();
        let rows = INDENTS
            .into_iter()
            .map(|indent| {
                let on = indent == current;
                let label = match indent {
                    Indent::Spaces(n) => tr!("status.spaces", count = n),
                    Indent::Tabs => t("status.tab"),
                };
                let detail = if on { t("format.current") } else { String::new() };
                (if on { "check" } else { "indent" }, label, detail, 0, FormatPick::Indent(indent))
            })
            .collect();
        self.open_format_list(&t("format.indent_title"), "indent", rows)
    }

    /// A format row chosen for document `doc` (the list's: another tab may be active by
    /// now): applied to its tab and the same file's other tabs. Its tab gone: nothing.
    pub(super) fn format_picked(&mut self, doc: kernel::DocumentId, pick: FormatPick) {
        let Some(i) = self.tabs.iter().position(|t| t.doc.id() == doc && t.holds_text()) else {
            return;
        };
        if let FormatPick::Convert(encoding) = pick
            && !document::represents_chunks(self.tabs[i].doc.buffer().chunks(), encoding)
        {
            self.show_notification(workbench_model::Notification::warning(tr!(
                "format.cant_convert",
                encoding = encoding.label()
            )));
            return;
        }
        let Some(path) = self.tabs[i].doc.path().map(PathBuf::from) else {
            // An untitled document: only its own tab.
            apply(&mut self.tabs[i], &pick);
            return;
        };
        if let FormatPick::ReadAs(encoding) = pick {
            self.read_as(path, encoding);
            return;
        }
        for tab in all_tabs_mut(&mut self.tabs, &mut self.editors.groups) {
            if tab.doc.path() == Some(path.as_path()) {
                apply(tab, &pick);
            }
        }
    }

    /// `path` read again as `encoding`, on the worker.
    fn read_as(&mut self, path: PathBuf, encoding: Encoding) {
        let disk = std::sync::Arc::clone(&self.ports.disk);
        let read = move || -> Option<FormatRead> {
            let stamp = disk.stamp(&path);
            // Grown past what's opened as text since: not read whole.
            if stamp.is_some_and(|(len, _)| len > document::MAX_TEXT_LEN) {
                return None;
            }
            let bytes = document::DocumentStorage::read(&*disk, &path).ok()?;
            Some((path, stamp, document::decode_as(&bytes, encoding)))
        };
        let Some(waker) = self.waker.as_ref().map(|w| std::sync::Arc::clone(&w.0)) else {
            // No window to wake (tests): here.
            if let Some(read) = read() {
                self.apply_read_as(read);
            }
            return;
        };
        let tx = self.format_reads.0.clone();
        kernel::work::run(READ_THREAD, move || {
            if let Some(read) = read()
                && tx.send(read).is_ok()
            {
                waker();
            }
        });
    }

    /// The files read again as an encoding, applied to their tabs. `true`: a redraw.
    pub(crate) fn poll_format_reads(&mut self) -> bool {
        let done: Vec<FormatRead> = self.format_reads.1.try_iter().collect();
        let any = !done.is_empty();
        for read in done {
            self.apply_read_as(read);
        }
        any
    }

    fn apply_read_as(&mut self, (path, stamp, decoded): FormatRead) {
        for tab in all_tabs_mut(&mut self.tabs, &mut self.editors.groups) {
            // Edited meanwhile: what was typed is kept.
            if tab.doc.path() != Some(path.as_path()) || tab.doc.is_dirty() {
                continue;
            }
            tab.doc.reload(decoded.clone());
            tab.disk_stamp = stamp;
        }
    }
}

/// `pick` applied to one tab.
fn apply(tab: &mut super::Tab, pick: &FormatPick) {
    match pick {
        FormatPick::Language(language) => {
            if tab.syntax.language() != language {
                tab.syntax = DocumentSyntax::new(language.clone());
                tab.diagnostics.errors_version = None;
                tab.symbols_version = None;
            }
        }
        FormatPick::Convert(encoding) => tab.doc.change_encoding(*encoding),
        FormatPick::LineEnding(le) => tab.doc.change_line_ending(*le),
        FormatPick::Indent(indent) => tab.doc.set_indent(*indent),
        FormatPick::ReadAs(_) => {}
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/file_format.rs"]
mod tests;
