//! The "Data Sources and Drivers" modal: every configured source (the project's and every
//! project's) on the left, the one selected on the right — its name and comment, then its
//! driver's fields by tab (General, Options, SSH/SSL, Schemas, Advanced), its URL kept in
//! step with them both ways, its password and where it's kept ([`crate::credentials`]).
//! "Test Connection" connects in the background (cancelable): the server's version and how
//! long it took, or why not. Nothing reaches the catalog until Apply or OK; the passwords
//! are kept, moved and removed in the background then. A module's modal
//! (`host.open_modal`): closed only by ×, Esc or its buttons, never by a click outside —
//! and with unsaved changes, only once the user says to discard them.
//!
//! Opened by its command ([`DATA_SOURCES`]), from the "Database" window (`+` › a server's
//! driver: a new source of it; a source's "Properties…"; a source not configured yet),
//! which gives it its request through `ViewerAction::OpenView`.

pub mod form;
mod modal;
mod paint;

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use command_model::CommandId;
use database::{DataSource, DataSourceId, Driver, SecretPolicy, TableKind};
use kernel::Secret;
use ui_components::{Viewer, ViewerKind, ViewerProvider};
use workbench_model::{CommandHandler, WorkbenchHost};

pub use form::{
    DriverInfo, Editor, Entry, Field, Item, ListRow, Place, Plan, Problem, SecretOp, Tab,
};
pub use modal::{Choice, SourcesView};

use crate::credentials::{Credentials, effective};
use crate::runtime::Runtime;
use crate::sessions::Sessions;

/// The modal's virtual extension (its view's kind).
pub const EXTENSION: &str = "view-data-sources";

/// Opens the modal (the request waiting, or the current project's sources).
pub const DATA_SOURCES: CommandId = CommandId("database.data_sources");

/// What the modal is asked to show.
#[derive(Clone)]
pub struct SourcesRequest {
    pub sessions: Arc<Sessions>,
    /// The source selected first.
    pub select: Option<DataSourceId>,
    /// Or the folder selected first.
    pub select_folder: Option<database::FolderId>,
    /// A new source of this driver, selected.
    pub add: Option<String>,
    /// The field with the keys first (its password, for a source that asks for one).
    pub focus: Option<Field>,
    pub wake: Arc<dyn Fn() + Send + Sync>,
}

impl std::fmt::Debug for SourcesRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourcesRequest").field("select", &self.select).finish_non_exhaustive()
    }
}

impl SourcesRequest {
    pub fn new(sessions: Arc<Sessions>) -> Self {
        Self {
            sessions,
            select: None,
            select_folder: None,
            add: None,
            focus: None,
            wake: Arc::new(|| {}),
        }
    }
}

/// Opens the modal: the request waiting, else the current project's sources.
#[derive(Debug, Default)]
pub struct SourcesCommand;

impl CommandHandler for SourcesCommand {
    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some(sessions) = host.project_id().and_then(crate::sessions) else { return false };
        let request = SourcesRequest::new(sessions);
        host.open_modal(&i18n::t("datasources.modal_title"), EXTENSION, &request)
    }
}

/// The modal's provider (its virtual extension).
#[derive(Debug, Default)]
pub struct SourcesProvider;

impl ViewerProvider for SourcesProvider {
    fn id(&self) -> &'static str {
        EXTENSION
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        (path.extension().and_then(|e| e.to_str()) == Some(EXTENSION))
            .then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(SourcesView::default()))
    }
}

/// The drivers registered, as the modal shows them.
pub fn drivers(sessions: &Sessions) -> Vec<DriverInfo> {
    let mut all: Vec<DriverInfo> = database::drivers::all()
        .into_iter()
        .filter_map(|f| {
            let driver = sessions.driver(&f.id)?;
            let mut info = info(&f.id, &f.name, driver.as_ref());
            info.external = f.external;
            Some(info)
        })
        .collect();
    all.sort_by_key(|d| d.name.to_lowercase());
    all
}

/// What the modal knows of `driver`.
pub fn info(id: &str, name: &str, driver: &dyn Driver) -> DriverInfo {
    let capabilities = driver.capabilities();
    let (icon, tone) = crate::engine_icon::of(id);
    DriverInfo {
        id: id.to_owned(),
        name: name.to_owned(),
        icon,
        tone,
        spec: driver.connection(),
        file_based: capabilities.file_based,
        schemas: capabilities.schemas,
        databases: !capabilities.file_based && capabilities.console.is_some_and(|c| c.databases),
        external: false,
    }
}

// ─── Test Connection ───

/// How the last test went.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Probe {
    #[default]
    Idle,
    Running,
    /// Connected: the server's name and version, how long it took, its schemas (of the
    /// database it opened), its server's databases (those the user may connect to; none
    /// for a source without).
    Connected {
        server: String,
        millis: u64,
        schemas: Vec<String>,
        databases: Vec<String>,
    },
    /// Not: why (localized).
    Failed(String),
    Cancelled,
}

/// A test of a source's connection, in the background: the latest one asked wins (one
/// cancelled, or asked again, is left to end on its own and forgotten).
#[derive(Clone, Default)]
pub struct Tester {
    state: Arc<Mutex<(u64, Probe)>>,
    generation: Arc<AtomicU64>,
}

impl std::fmt::Debug for Tester {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tester").field("probe", &self.probe()).finish()
    }
}

impl Tester {
    pub fn probe(&self) -> Probe {
        self.state.lock().unwrap_or_else(PoisonError::into_inner).1.clone()
    }

    /// The result shown now (a snapshot's, a test's).
    pub fn set(&self, probe: Probe) {
        let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        *self.state.lock().unwrap_or_else(PoisonError::into_inner) = (generation, probe);
    }

    /// Tests `source` (its file, if any, absolute) with `driver` on `runtime`: its password
    /// typed (`typed`), else as kept (read there, never here); `wake` called once it's
    /// known.
    pub fn start(
        &self,
        runtime: &Runtime,
        driver: Arc<dyn Driver>,
        source: DataSource,
        typed: Option<Secret>,
        credentials: Arc<Credentials>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) {
        let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        *self.state.lock().unwrap_or_else(PoisonError::into_inner) = (generation, Probe::Running);
        let mut target = source.target.clone();
        if let Some(file) = &source.file {
            target.database = file.to_string_lossy().into_owned();
        }
        if source.read_only {
            target.options.insert(database::Target::READ_ONLY.to_owned(), "true".to_owned());
        }
        let state = Arc::clone(&self.state);
        let needs = driver.connection().has_secret();
        let file_based = driver.capabilities().file_based;
        runtime.spawn(move || {
            let secret = match typed {
                Some(secret) => secret,
                None if needs => credentials.peek(&source).unwrap_or_default(),
                None => Secret::default(),
            };
            let started = Instant::now();
            let probe = match driver.connect(&target, &secret) {
                Ok(mut session) => {
                    let millis = started.elapsed().as_millis() as u64;
                    let schemas = schemas(session.as_mut());
                    let databases = if file_based {
                        Vec::new()
                    } else {
                        session.databases().unwrap_or_default()
                    };
                    Probe::Connected { server: session.server(), millis, schemas, databases }
                }
                Err(e) => Probe::Failed(probe_error(&e)),
            };
            drop(secret);
            {
                let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
                // A newer test (or a cancel) came: this one's forgotten.
                if state.0 != generation {
                    return;
                }
                state.1 = probe;
            }
            wake();
        });
    }

    /// The test running is forgotten (its connection ends on its own).
    pub fn cancel(&self) {
        if self.probe() == Probe::Running {
            self.set(Probe::Cancelled);
        }
    }
}

/// Why a test failed, in words (the server's own message kept).
pub fn probe_error(e: &database::DbError) -> String {
    use database::DbError as E;
    match e {
        E::Connect(why) => i18n::tr!("datasources.test_unreachable", why = why.as_str()),
        E::Auth(why) => i18n::tr!("datasources.test_refused", why = why.as_str()),
        other => crate::grid_source::message(other),
    }
}

/// The schemas (or databases) a session shows at its top; none for one without.
fn schemas(session: &mut dyn database::Session) -> Vec<String> {
    let mut out: Vec<String> = match session.children(&[]) {
        Ok(nodes) => nodes
            .into_iter()
            .filter(|n| matches!(n.kind, "schema" | "database"))
            .map(|n| n.name)
            .collect(),
        Err(_) => Vec::new(),
    };
    if out.is_empty()
        && let Ok(tables) = session.tables(&[])
    {
        for t in tables.iter().filter(|t| t.kind == TableKind::Table || t.kind == TableKind::View) {
            if let Some(s) = &t.table.schema
                && !out.contains(s)
            {
                out.push(s.clone());
            }
        }
    }
    out
}

// ─── Apply ───

/// What a password's move says (shown after Apply): a source moved to every project with
/// its password in the project's vault has it in the keychain from now on.
pub fn move_notes(plan: &Plan) -> Vec<String> {
    plan.secrets
        .iter()
        .filter_map(|op| match op {
            SecretOp::Move { from, to } if effective(from) != effective(to) => {
                let key = match effective(to) {
                    SecretPolicy::Keychain => "datasources.moved_keychain",
                    SecretPolicy::ProjectVault => "datasources.moved_project",
                    SecretPolicy::Session => "datasources.moved_session",
                    SecretPolicy::Never => "datasources.moved_never",
                };
                Some(i18n::tr!(key, name = to.name.as_str()))
            }
            _ => None,
        })
        .collect()
}

/// `plan` made: the catalog changed now, its folders first (said on the bus, written in
/// the background), the sessions of the sources removed or whose connection changed closed
/// (the next use connects as they're set; one only moved or colored keeps its own), the
/// passwords kept, moved and removed on `runtime` (the keychain may make it
/// wait). Why the catalog refused, if it did; the passwords' outcome goes to `done`.
pub fn apply(
    sessions: &Sessions,
    plan: &Plan,
    done: Arc<dyn Fn(Result<(), String>) + Send + Sync>,
) -> Result<(), String> {
    let catalog = sessions.catalog();
    for (scope, folders) in &plan.folders {
        catalog.set_folders(*scope, folders.clone())?;
    }
    for source in &plan.remove {
        catalog.remove(source.id)?;
        sessions.close(source.id);
        sessions.events().publish(&database::events::SchemaInvalidated { source: source.id });
    }
    for source in &plan.add {
        catalog.add(source.clone())?;
    }
    for source in &plan.update {
        let before = catalog.get(source.id);
        catalog.update(source.clone())?;
        // Moved to a folder, colored: its session (and what's open on it) stays.
        if before.is_none_or(|b| !b.same_connection(source)) {
            sessions.close(source.id);
            // Another connection: another schema, maybe (a console's completion reads it again).
            sessions.events().publish(&database::events::SchemaInvalidated { source: source.id });
        }
    }
    let credentials = sessions.credentials();
    let ops = plan.secrets.clone();
    if ops.is_empty() {
        done(Ok(()));
        return Ok(());
    }
    sessions.runtime().spawn(move || {
        let mut failed = None;
        for op in ops {
            let outcome = match &op {
                SecretOp::Set(source, secret) => credentials.set(source, secret),
                SecretOp::Move { from, to } => credentials.moved(from, to).map(|_| ()),
                SecretOp::Copy { from, to } => match credentials.peek(from) {
                    Some(secret) => credentials.set(to, &secret),
                    None => Ok(()),
                },
                SecretOp::Remove(source) => credentials.remove(source),
            };
            if let Err(why) = outcome {
                // Never the secret: only where it was going.
                tracing::warn!("a data source's password wasn't kept: {why}");
                failed.get_or_insert(why);
            }
        }
        done(failed.map_or(Ok(()), Err));
    });
    Ok(())
}

#[cfg(test)]
#[path = "../../tests/unit/sources/mod.rs"]
mod tests;
