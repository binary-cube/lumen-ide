//! A pane over a tabular result (plan 94): what a module offers to show about rows another
//! module holds — a chart of a query's result, of a table's rows — without either knowing
//! the other.
//!
//! - **A contribution, registered by the app** ([`register`]): a [`ResultPaneProvider`]
//!   (its id, icon, title and how a pane is made). Whoever shows rows (the database console's
//!   result, the explorer's table) lists [`providers`] and offers a button for each; with
//!   none, it offers nothing (`presence`).
//! - **The rows lent, never copied** ([`Lent`]): a snapshot of what the host holds (its
//!   chunks shared), readable on a worker; a new one with a new epoch when the rows change.
//! - **The pane is a part of the host's view** ([`ResultPane`]: a [`Control`] that paints):
//!   the host lays it out (a splitter under its table), hands it the input and asks it
//!   what it wants ([`PaneRequest`]: all the rows, up to the pane's limit). What it is the host never
//!   knows: its state comes back as text ([`ResultPane::saved`]) the host keeps with the
//!   query or the table and gives back ([`ResultPane::restore`]).

use std::fmt::Debug;
use std::sync::{Arc, PoisonError, RwLock};

use charts_model::Tabular;

mod host;

pub use host::{PANE_MENU, PaneHost, Spawn, key_of};

use crate::control::Control;
use crate::viewer::ViewerPaint;

/// Rows lent to a pane: readable from any thread, the host's memory shared.
pub type Lent = Arc<dyn Tabular + Send + Sync>;

/// What a pane asks of its host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneRequest {
    /// Where to save a file (`name`: offered): the host asks the system's "Save As" and gives
    /// the path chosen back to the pane as the command `picked-save:<path>` ([`Control::command`]).
    SaveAs { name: String },
    /// Something to tell the user (how a save went): `ok` or not.
    Tell { text: String, ok: bool },
    /// Every row of the result, up to `limit` (the pane's setting): the host fetches them and
    /// lends them anew as they come.
    AllRows { limit: usize },
    /// The pane drawn from all the rows again, not the selection it was drawn from
    /// ([`PaneHost::pick`]): the host does it itself and never passes it on.
    WholeRows,
}

/// How many of the result's rows the host holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Held {
    pub rows: usize,
    /// More can be fetched (the result goes on).
    pub more: bool,
    /// Fetching now.
    pub fetching: bool,
    /// The rows are the grid's selection ([`PaneHost::pick`]), not the result's.
    pub picked: bool,
}

/// A pane over rows, shown by the host under its table.
pub trait ResultPane: Control + Debug + Send {
    /// The rows as of `epoch` (a new epoch: they changed), and how many the host holds.
    fn set_rows(&mut self, rows: Lent, epoch: u64, held: Held);

    /// Draws it in `paint.bounds`.
    fn paint(&mut self, paint: ViewerPaint<'_>);

    /// Work done meanwhile taken (a chart prepared, rasterized): `true` to draw again.
    fn poll(&mut self) -> bool {
        false
    }

    /// It's waiting for work of its own (a frame will come when it's done).
    fn busy(&self) -> bool {
        false
    }

    /// What it asks of the host, once.
    fn take_request(&mut self) -> Option<PaneRequest> {
        None
    }

    /// Its state as text, for the host to keep with what it shows (`None`: nothing to keep).
    fn saved(&self) -> Option<String> {
        None
    }

    /// Its state as kept (a text it gave before; one it can't read is left).
    fn restore(&mut self, _saved: &str) {}

    /// What it holds let go while it isn't shown.
    fn hidden(&mut self) {}
}

/// A pane's kind, contributed by a module.
pub trait ResultPaneProvider: Send + Sync + Debug {
    /// Its id (ours, static: `chart`), the key its state is kept under.
    fn id(&self) -> &'static str;

    /// Its button's icon (the app's set).
    fn icon(&self) -> &'static str;

    /// Its button's tooltip and the pane's title.
    fn title(&self) -> String;

    /// A new pane.
    fn make(&self) -> Box<dyn ResultPane>;
}

static PROVIDERS: RwLock<Vec<Arc<dyn ResultPaneProvider>>> = RwLock::new(Vec::new());

/// Registers a provider (the app, at startup, from the modules' contributions); one with the
/// same id is replaced.
pub fn register(provider: Arc<dyn ResultPaneProvider>) {
    let mut all = PROVIDERS.write().unwrap_or_else(PoisonError::into_inner);
    all.retain(|p| p.id() != provider.id());
    all.push(provider);
}

/// Removes `id`'s provider (tests, a module unloaded).
pub fn unregister(id: &str) {
    PROVIDERS.write().unwrap_or_else(PoisonError::into_inner).retain(|p| p.id() != id);
}

/// The providers registered, in the order they came.
pub fn providers() -> Vec<Arc<dyn ResultPaneProvider>> {
    PROVIDERS.read().unwrap_or_else(PoisonError::into_inner).clone()
}

/// The provider `id`.
pub fn provider(id: &str) -> Option<Arc<dyn ResultPaneProvider>> {
    PROVIDERS.read().unwrap_or_else(PoisonError::into_inner).iter().find(|p| p.id() == id).cloned()
}

#[cfg(test)]
#[path = "../../tests/unit/result_pane.rs"]
mod tests;
