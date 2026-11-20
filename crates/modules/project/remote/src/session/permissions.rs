//! The server's permissions in the project's tree (a setting, off by default): each row of
//! a remote project says its mode and owner (`rw-r--r-- www-data`), read once per folder
//! (one listing) when the tree shows it, in the background; and a warning when a file just
//! uploaded can't be read by the web server — neither its group nor the others may read it
//! (a setting, on by default).

use std::collections::HashMap;
use std::sync::Arc;

use remote::properties::{Access, Bit, Class, Properties};
use workspace::Note;

use super::{Notice, State, parent};

/// What a row says of a server entry: its mode as `ls -l` writes it and its owner; to look
/// at when the web server (the group, the others) can't read it — a folder: can't enter it.
pub(crate) fn note_of(props: &Properties) -> Option<Note> {
    let mode = props.mode?;
    let owner = props.owner.as_ref().map(|o| match &o.name {
        Some(name) => name.clone(),
        None => o.to_string(),
    });
    let text = match owner {
        Some(owner) => format!("{} {owner}", mode.symbolic()),
        None => mode.symbolic(),
    };
    Some(Note { text: text.into(), warn: !web_readable(props) })
}

/// Whether the group or the others may read it (a folder: also enter it).
pub(crate) fn web_readable(props: &Properties) -> bool {
    let Some(mode) = props.mode else { return true };
    [Class::Group, Class::Others].into_iter().any(|class| {
        let read = mode.has(Bit::Access(class, Access::Read));
        let enter = mode.has(Bit::Access(class, Access::Execute));
        read && (!props.is_dir || enter)
    })
}

impl State {
    /// The notes of folder `dir`'s entries (by name), when the setting shows them: as read
    /// before, else asked for (the tree hears once they're here).
    pub(crate) fn permission_notes(&self, dir: &str) -> Option<HashMap<String, Note>> {
        if !crate::settings::show_permissions() {
            return None;
        }
        if let Some(known) = self.permissions.lock().ok().and_then(|p| p.get(dir).cloned()) {
            return Some(known);
        }
        let first =
            self.permissions_asked.lock().is_ok_and(|mut asked| asked.insert(dir.to_owned()));
        if first && let Some(state) = super::session(self.project) {
            let dir = dir.to_owned();
            kernel::work::run("remote-permissions", move || state.read_permissions(&dir));
        }
        None
    }

    /// Folder `dir`'s permissions read from the server (one listing), kept, the tree told.
    fn read_permissions(self: &Arc<Self>, dir: &str) {
        let listed = self.connector.connect().and_then(|mut fs| fs.list_properties(dir));
        let notes: HashMap<String, Note> = match listed {
            Ok(listed) => listed
                .iter()
                .filter_map(|p| Some((p.path.rsplit('/').next()?.to_owned(), note_of(p)?)))
                .collect(),
            // Not readable now: asked again the next time the tree shows it.
            Err(e) => {
                tracing::debug!(dir = %format_args!("/{dir}"), error = %e, "the permissions weren't read");
                if let Ok(mut asked) = self.permissions_asked.lock() {
                    asked.remove(dir);
                }
                return;
            }
        };
        if let Ok(mut known) = self.permissions.lock() {
            known.insert(dir.to_owned(), notes);
        }
        if let Ok(mut asked) = self.permissions_asked.lock() {
            asked.remove(dir);
        }
        if let Ok(mut relisted) = self.relisted.lock() {
            relisted.push(dir.to_owned());
        }
        (self.wake)();
    }

    /// What's known of the folders holding `paths` let go (they changed on the server: a
    /// file uploaded, its permissions set): read again when the tree shows them.
    pub(crate) fn forget_permissions<'a>(&self, paths: impl IntoIterator<Item = &'a str>) {
        let Ok(mut known) = self.permissions.lock() else { return };
        let mut forgotten = Vec::new();
        for path in paths {
            let dir = parent(path);
            if known.remove(dir).is_some() {
                forgotten.push(dir.to_owned());
            }
        }
        drop(known);
        if !forgotten.is_empty()
            && let Ok(mut relisted) = self.relisted.lock()
        {
            relisted.extend(forgotten);
        }
    }

    /// Every folder's permissions let go (a change applied recursively): read again as the
    /// tree shows them.
    pub(crate) fn forget_all_permissions(&self) {
        let dirs: Vec<String> = match self.permissions.lock() {
            Ok(mut known) => known.drain().map(|(dir, _)| dir).collect(),
            Err(_) => return,
        };
        if let Ok(mut relisted) = self.relisted.lock() {
            relisted.extend(dirs);
        }
        (self.wake)();
    }

    /// A file just uploaded (`relative`): its permissions forgotten, and — the setting on —
    /// looked at once the uploads end, with the others uploaded meanwhile (one request, a
    /// connection already open): a warning once for one the web server can't read.
    pub(crate) fn uploaded_check(&self, relative: &str) {
        use std::sync::atomic::Ordering::{AcqRel, Relaxed};
        self.forget_permissions([relative]);
        if !crate::settings::warn_unreadable() {
            return;
        }
        if let Ok(mut to_check) = self.to_check.lock() {
            to_check.push(relative.to_owned());
        }
        if self.checking.swap(true, AcqRel) {
            return;
        }
        let Some(state) = super::session(self.project) else {
            self.checking.store(false, Relaxed);
            return;
        };
        kernel::work::run("remote-permissions", move || {
            loop {
                // The uploads first: the look takes a connection they let go.
                while state.busy.load(Relaxed) > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                let batch: Vec<String> =
                    state.to_check.lock().map(|mut c| std::mem::take(&mut *c)).unwrap_or_default();
                if batch.is_empty() {
                    break;
                }
                state.warn_unreadable(&batch);
            }
            state.checking.store(false, Relaxed);
            // One more came as the look ended: another look for it.
            let late = state.to_check.lock().ok().and_then(|mut c| c.pop());
            if let Some(late) = late {
                state.uploaded_check(&late);
            }
        });
    }

    /// The files of `batch` the web server can't read: a warning each (once).
    fn warn_unreadable(&self, batch: &[String]) {
        let Ok(read) = self.connector.connect().and_then(|mut fs| fs.properties(batch)) else {
            return;
        };
        for props in read.into_iter().flatten() {
            if web_readable(&props) {
                continue;
            }
            let first = self.warned.lock().is_ok_and(|mut w| w.insert(props.path.clone()));
            if !first {
                continue;
            }
            let mode = props.mode.map(remote::properties::Mode::octal).unwrap_or_default();
            let path = props.path.as_str();
            let text = i18n::tr!("remote.unreadable", path = path, mode = mode.as_str());
            self.tell(Notice::Message(workbench_model::Notification::warning(text)));
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/session/permissions.rs"]
mod tests;
