//! The relations diagram, from the explorer's side: the source's structure read for it
//! through the source's worker ([`SourceSchema`], the domain's
//! [`database::diagram::SchemaAccess`]), a table's data opened where the explorer opens it,
//! and the command opening the diagram's view (another module's: only the domain's request
//! is shared, [`database::diagram::DiagramRequest`]).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::Duration;

use database::diagram::{DiagramRequest, Opening, SchemaAccess, VIEW_EXTENSION};
use database::relations::RelationNotes;
use database::{
    Cancel, DataSource, DataSourceId, DbError, Scope, SecretPolicy, Structure, TableInfo, TableRef,
};
use kernel::ProjectId;

use crate::sessions::{FileLease, Sessions};
use crate::worker::{Request, Response, SessionWorker, Ticket};

/// The diagram's tab (the view is another module's: told if it's turned off).
pub fn view(title: String, request: DiagramRequest) -> ui_components::OpenView {
    ui_components::OpenView::new(title, VIEW_EXTENSION, request)
        .or_tell(i18n::t("relations.unavailable"))
}

/// A source's structure, as the diagram reads it: through its worker (one request at a
/// time on the pool `db`, as any), its tables' data opened by the explorer.
pub struct SourceSchema {
    id: DataSourceId,
    /// Its name (a file's: its name, following its renames).
    name: Arc<Mutex<String>>,
    project: Option<ProjectId>,
    worker: SessionWorker,
    /// The schemas shown (none: all).
    schemas: Vec<String>,
    /// A file-based source's file (absolute; where a rename or move took it): its own view
    /// opens its tables.
    file: Arc<Mutex<Option<PathBuf>>>,
    /// Follows the file's renames (a file's diagram).
    _moves: Option<kernel::Subscription>,
    /// Keeps the file's session while the diagram is open.
    _lease: Option<FileLease>,
    sessions: Weak<Sessions>,
    /// The decisions of a file not kept as a source (only which relations are shown: one
    /// with a key accepted or set aside becomes a project's source).
    memory: Mutex<RelationNotes>,
    /// Its color, as last worked out (the diagram's tab carries it).
    color: crate::catalog::ColorCache,
}

impl std::fmt::Debug for SourceSchema {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceSchema").field("id", &self.id).finish_non_exhaustive()
    }
}

impl SourceSchema {
    /// Source `id` of `sessions` (named `name`), read through `worker`; a file's (`file`,
    /// with its lease) opens its tables in its own view.
    pub fn new(
        sessions: &Arc<Sessions>,
        id: DataSourceId,
        name: String,
        worker: SessionWorker,
        file: Option<(PathBuf, Option<FileLease>)>,
    ) -> Self {
        // The decisions about its relations are kept with the configured sources: read now
        // (in the background), ready before the first one is made.
        sessions.catalog().ensure_loaded();
        let schemas = sessions.catalog().get(id).map(|s| s.schemas).unwrap_or_default();
        let (file, lease) = match file {
            Some((path, lease)) => (Some(path), lease),
            None => (None, None),
        };
        let followed = file.is_some();
        let (file, name) = (Arc::new(Mutex::new(file)), Arc::new(Mutex::new(name)));
        // A file's diagram follows it renamed or moved (its name, when it was the file's).
        let moves = followed.then(|| {
            let (file, name) = (Arc::clone(&file), Arc::clone(&name));
            sessions.events().subscribe(move |e: &workspace::events::FileRenamed| {
                let mut file = file.lock().unwrap_or_else(PoisonError::into_inner);
                let Some(old) = file.clone() else { return };
                let Some(new) = workspace::clipboard::moved_path(&old, &e.from, &e.to) else {
                    return;
                };
                let name_of = |p: &std::path::Path| {
                    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
                };
                let mut name = name.lock().unwrap_or_else(PoisonError::into_inner);
                if *name == name_of(&old) {
                    *name = name_of(&new);
                }
                *file = Some(new);
            })
        });
        Self {
            id,
            name,
            project: Some(sessions.project()),
            worker,
            schemas,
            file,
            _moves: moves,
            _lease: lease,
            sessions: Arc::downgrade(sessions),
            memory: Mutex::default(),
            color: crate::catalog::ColorCache::default(),
        }
    }

    /// Source `id`'s access: a file-based one through its file's session (kept while the
    /// diagram is open), another through its own worker.
    pub fn open(
        sessions: &Arc<Sessions>,
        id: DataSourceId,
        name: &str,
        driver: &str,
        file: Option<&std::path::Path>,
    ) -> Result<Self, DbError> {
        if let Some(path) = file {
            let (worker, lease) = sessions.open_file(path, driver, &[])?;
            return Ok(Self::new(
                sessions,
                id,
                name.to_owned(),
                worker,
                Some((path.to_path_buf(), Some(lease))),
            ));
        }
        let worker = match sessions.get(id) {
            Some(worker) => worker,
            None => sessions.source_worker(id)?,
        };
        Ok(Self::new(sessions, id, name.to_owned(), worker, None))
    }
}

impl SourceSchema {
    /// Its file, where it is now (a file-based source's).
    fn file(&self) -> Option<PathBuf> {
        self.file.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

impl SchemaAccess for SourceSchema {
    fn source(&self) -> DataSourceId {
        self.id
    }

    fn color(&self) -> Option<database::SourceColor> {
        let sessions = self.sessions.upgrade()?;
        self.color.get(sessions.catalog(), |_| {
            let source = configured(&sessions, self.id, self.file().as_deref())?;
            sessions.catalog().color_of(source.id)
        })
    }

    fn name(&self) -> String {
        self.name.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn read(
        &self,
        schema: Option<&str>,
        cancel: &Cancel,
        progress: &(dyn Fn(usize, usize) + Sync),
    ) -> Result<Vec<(TableInfo, Structure)>, DbError> {
        // Woken by each reply (on the pool's thread), the replies taken here.
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let tx = Mutex::new(tx);
        let client = self.worker.client(Arc::new(move || {
            let _ = tx.lock().unwrap_or_else(PoisonError::into_inner).send(());
        }));
        let wait = |tickets: &[Ticket]| -> Result<(), DbError> {
            if cancel.is_cancelled() {
                for &ticket in tickets {
                    client.cancel(ticket);
                }
                return Err(DbError::Cancelled);
            }
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(()) | Err(RecvTimeoutError::Timeout) => Ok(()),
                Err(RecvTimeoutError::Disconnected) => Err(DbError::Cancelled),
            }
        };
        let listed = client.send(Request::Tables(Vec::new()));
        let tables = loop {
            let reply = client.poll().into_iter().find(|r| r.ticket == listed);
            match reply.map(|r| r.result) {
                Some(Ok(Response::Tables(tables))) => break tables,
                Some(Ok(_)) => return Err(DbError::Query("unexpected reply".to_owned())),
                Some(Err(e)) => return Err(e),
                None => wait(&[listed])?,
            }
        };
        let tables: Vec<TableInfo> = tables
            .into_iter()
            .filter(|t| match (schema, &t.table.schema) {
                (Some(wanted), Some(own)) => wanted == own,
                (Some(_), None) => false,
                (None, own) => {
                    self.schemas.is_empty() || own.as_ref().is_none_or(|s| self.schemas.contains(s))
                }
            })
            .collect();
        let total = tables.len();
        progress(0, total);
        // Every structure asked at once (the worker runs them one by one), taken as they come.
        let mut asked: HashMap<Ticket, usize> = tables
            .iter()
            .enumerate()
            .map(|(i, t)| (client.send(Request::Structure(t.table.clone())), i))
            .collect();
        let mut structures: Vec<Option<Structure>> = vec![None; total];
        while !asked.is_empty() {
            let replies = client.poll();
            if replies.is_empty() {
                let waiting: Vec<Ticket> = asked.keys().copied().collect();
                wait(&waiting)?;
                continue;
            }
            for reply in replies {
                let Some(i) = asked.remove(&reply.ticket) else { continue };
                structures[i] = match reply.result {
                    Ok(Response::Structure(s)) => Some(s),
                    // A table whose structure can't be read is shown without its columns.
                    Ok(_) | Err(_) => None,
                };
            }
            progress(total - asked.len(), total);
        }
        Ok(tables
            .into_iter()
            .zip(structures)
            .map(|(info, structure)| {
                let structure = structure.unwrap_or_else(|| Structure {
                    table: info.table.clone(),
                    ..Structure::default()
                });
                (info, structure)
            })
            .collect())
    }

    fn notes(&self) -> RelationNotes {
        let kept =
            self.sessions.upgrade().and_then(|s| configured(&s, self.id, self.file().as_deref()));
        match kept {
            Some(source) => source.relations,
            None => self.memory.lock().unwrap_or_else(PoisonError::into_inner).clone(),
        }
    }

    fn set_notes(&self, notes: RelationNotes) {
        (*self.memory.lock().unwrap_or_else(PoisonError::into_inner)).clone_from(&notes);
        let Some(sessions) = self.sessions.upgrade() else { return };
        let catalog = sessions.catalog();
        match configured(&sessions, self.id, self.file().as_deref()) {
            Some(mut source) => {
                if source.relations != notes {
                    source.relations = notes;
                    if let Err(why) = catalog.update(source) {
                        tracing::warn!(error = %why, "the relations decided weren't kept");
                    }
                }
            }
            // A file shown by itself: kept as the project's source (its session's id) once
            // a key is accepted or set aside.
            None if !notes.virtual_keys.is_empty() || !notes.ignored.is_empty() => {
                let Some(file) = self.file() else { return };
                let file = &file;
                let Some((_, _, driver)) = sessions
                    .open_files()
                    .into_iter()
                    .find(|(path, ..)| sessions.identities().same(path, file))
                else {
                    return;
                };
                let names: Vec<String> = catalog.all().into_iter().map(|s| s.name).collect();
                let name = database::unique_name(&self.name(), names.iter().map(String::as_str));
                let mut source = DataSource::new(name, driver, Scope::Project);
                source.id = self.id;
                source.file = Some(database::source::stored_path(file, sessions.root()));
                source.secrets = SecretPolicy::Never;
                source.relations = notes;
                if let Err(why) = catalog.add(source) {
                    tracing::warn!(error = %why, "the file wasn't kept as a data source");
                }
            }
            None => {}
        }
    }

    fn open(&self, table: &TableRef) -> Option<Opening> {
        if let Some(file) = self.file() {
            return Some(Opening::File(file, crate::view::rows_of(table.clone(), None)));
        }
        let open = crate::view::OpenSource {
            project: self.project?,
            source: self.id,
            name: self.name(),
            table: Some(table.clone()),
            structure: false,
            column: None,
            filter: None,
        };
        Some(open.into_opening())
    }
}

/// The configured source a session of `id` is (a view's, a diagram's): by its id, else by
/// its file `file` (a file shown by itself, kept as a source since).
pub fn configured(
    sessions: &Sessions,
    id: DataSourceId,
    file: Option<&std::path::Path>,
) -> Option<DataSource> {
    let catalog = sessions.catalog();
    catalog.get(id).or_else(|| {
        let file = file?;
        catalog.source_of_file(sessions.root(), file)
    })
}

/// The request for the diagram of `access`'s source (its `schema`, or around `focus`),
/// with the tab's title.
pub fn request(
    sessions: &Arc<Sessions>,
    access: SourceSchema,
    schema: Option<String>,
    focus: Option<TableRef>,
) -> (String, DiagramRequest) {
    let title = match (&focus, &schema) {
        (Some(table), _) => i18n::tr!("relations.tab", name = table.name.clone()),
        (None, Some(schema)) => i18n::tr!("relations.tab", name = schema.clone()),
        (None, None) => i18n::tr!("relations.tab", name = access.name()),
    };
    let request = DiagramRequest {
        access: Arc::new(access),
        events: Some(sessions.events().clone()),
        schema,
        focus,
    };
    (title, request)
}

#[cfg(test)]
#[path = "../tests/unit/diagram.rs"]
mod tests;
