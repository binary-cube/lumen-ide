//! Changing a source's structure from its data view: tables created, renamed and dropped;
//! columns added, changed, moved and dropped; indexes and foreign keys made and dropped —
//! each in a modal of the module's own (`host.open_modal`: closed only by its buttons, ×
//! or Esc, never by a click outside), offered as far as the driver can
//! ([`database::SchemaChanges`]).
//!
//! The modal's form ([`form`]) becomes [`database::SchemaChange`]s; the source's worker
//! plans them ([`crate::Request::PlanAlter`]: the domain's [`database::Script`], what runs,
//! shown as it is — a source without SQL, a CSV file, sums them up instead), and what's
//! destructive is listed and ticked first: the data dropped, the views, triggers and
//! indexes a rebuild loses ([`database::danger`]). A rename shows what SQLite rewrites by
//! itself. Submitting runs exactly what was shown ([`crate::Request::AlterAsShown`]: planned
//! again and refused if it differs), in one transaction; nothing changes when it fails.
//! The view that asked is told through a shared slot.
//!
//! The view opens it through its action (`ViewerAction::OpenView`).

pub mod form;
mod modal;
mod paint;

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use database::{Applied, SchemaChange, Value};
use i18n::{t, tr};
use ui_components::{Viewer, ViewerKind, ViewerProvider};

pub use form::{Context, Form, Kind, Text};
pub use modal::{Answer, Choice, Popup, SchemaView, Tick, take_answer};

use crate::worker::SessionWorker;

/// The modal's virtual extension (its view's kind).
pub const EXTENSION: &str = "view-data-schema";

/// A structure change made: what, and how it went.
#[derive(Debug, Clone, PartialEq)]
pub struct Done {
    pub changes: Vec<SchemaChange>,
    pub applied: Applied,
}

/// What the modal is asked: its form, the source it changes, where its outcome goes.
#[derive(Clone)]
pub struct SchemaRequest {
    pub kind: Kind,
    pub ctx: Context,
    pub worker: SessionWorker,
    /// Set once the changes are made (a modal closed otherwise leaves it empty).
    pub answer: Arc<Mutex<Option<Done>>>,
    /// Wakes the view that asked.
    pub wake: Arc<dyn Fn() + Send + Sync>,
}

impl std::fmt::Debug for SchemaRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SchemaRequest").field("kind", &self.kind).finish_non_exhaustive()
    }
}

impl SchemaRequest {
    fn done(&self, done: Done) {
        *self.answer.lock().unwrap_or_else(PoisonError::into_inner) = Some(done);
        (self.wake)();
    }
}

/// The modal's provider (its virtual extension).
#[derive(Debug, Default)]
pub struct SchemaProvider;

impl ViewerProvider for SchemaProvider {
    fn id(&self) -> &'static str {
        EXTENSION
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        (path.extension().and_then(|e| e.to_str()) == Some(EXTENSION))
            .then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(SchemaView::default()))
    }
}

/// A change in words, for a source that writes no SQL (a CSV file): what its file becomes.
pub fn summary(changes: &[SchemaChange]) -> Vec<String> {
    changes
        .iter()
        .map(|change| match change {
            SchemaChange::AddColumn { column, .. } => match &column.default {
                Some(database::DefaultValue::Value(v)) if *v != Value::Null => tr!(
                    "schema.summary_add_filled",
                    column = column.name.as_str(),
                    value = v.display()
                ),
                _ => tr!("schema.summary_add", column = column.name.as_str()),
            },
            SchemaChange::RenameColumn { from, to, .. } => {
                tr!("schema.summary_rename", from = from.as_str(), to = to.as_str())
            }
            SchemaChange::AlterColumn { column, to, .. } => {
                tr!("schema.summary_rename", from = column.as_str(), to = to.name.as_str())
            }
            SchemaChange::DropColumn { column, .. } => {
                tr!("schema.summary_drop", column = column.as_str())
            }
            SchemaChange::MoveColumn { column, to, .. } => {
                tr!("schema.summary_move", column = column.as_str(), place = to + 1)
            }
            other => tr!("schema.summary_other", what = format!("{other:?}")),
        })
        .collect()
}

/// A note under the summary: how a file is written.
pub fn file_note() -> String {
    t("database.preview_file_note")
}

#[cfg(test)]
#[path = "../../tests/unit/schema/mod.rs"]
mod tests;
