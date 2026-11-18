//! The extractors offered, and the one chosen: the built-in ones ([`builtin`]: SQL, the CSV
//! formats configured, the other formats), those the modules contribute ([`Catalog::register`])
//! and the user's templates of each project ([`Catalog::set_scripted`]); the choice is a
//! project's (⌘C, "Copy As" and the export follow it), [`DEFAULT_ID`] until one is made.
//! [`catalog`] is the app's; a test makes its own ([`Catalog::new`]).

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, PoisonError, RwLock};

use kernel::ProjectId;

use crate::delimited::{CsvFormat, Delimited};
use crate::extractor::{Extractor, Group, Options};
use crate::formats::{Html, Json, Markdown, OneRow, Pretty, Xml};
use crate::sql_writers::{SqlInserts, SqlUpdates, WhereClause};

/// The extractor used until a project chooses one: tab-separated values without the header
/// (pasted into a spreadsheet as cells).
pub const DEFAULT_ID: &str = "tsv";

/// The built-in extractors, `csv` the separated formats offered, in the popover's order.
pub fn builtin(csv: &[CsvFormat]) -> Vec<Arc<dyn Extractor>> {
    let mut all: Vec<Arc<dyn Extractor>> = vec![
        Arc::new(SqlInserts { multirow: false }),
        Arc::new(SqlInserts { multirow: true }),
        Arc::new(SqlUpdates),
        Arc::new(WhereClause),
    ];
    all.extend(csv.iter().map(|f| Arc::new(Delimited(f.clone())) as Arc<dyn Extractor>));
    all.extend([
        Arc::new(Json) as Arc<dyn Extractor>,
        Arc::new(Markdown),
        Arc::new(Html),
        Arc::new(Xml),
        Arc::new(OneRow),
        Arc::new(Pretty),
    ]);
    all
}

#[derive(Default)]
struct State {
    contributed: Vec<Arc<dyn Extractor>>,
    /// The separated formats configured (`None`: the defaults).
    csv: Option<Vec<CsvFormat>>,
    options: Options,
    scripted: HashMap<ProjectId, Vec<Arc<dyn Extractor>>>,
    chosen: HashMap<Option<ProjectId>, String>,
    /// The built-in and contributed ones, made once per change.
    fixed: Option<Vec<Arc<dyn Extractor>>>,
    generation: u64,
}

impl State {
    fn fixed(&mut self) -> Vec<Arc<dyn Extractor>> {
        if self.fixed.is_none() {
            let csv = self.csv.clone().unwrap_or_else(CsvFormat::defaults);
            let mut all = builtin(&csv);
            all.extend(self.contributed.iter().cloned());
            self.fixed = Some(all);
        }
        self.fixed.clone().unwrap_or_default()
    }

    fn changed(&mut self) {
        self.fixed = None;
        self.generation += 1;
    }
}

/// See the module's doc.
#[derive(Default)]
pub struct Catalog {
    state: RwLock<State>,
}

impl std::fmt::Debug for Catalog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Catalog").field("generation", &self.generation()).finish_non_exhaustive()
    }
}

impl Catalog {
    pub fn new() -> Self {
        Self::default()
    }

    fn write<R>(&self, change: impl FnOnce(&mut State) -> R) -> R {
        change(&mut self.state.write().unwrap_or_else(PoisonError::into_inner))
    }

    /// An extractor a module brings (listed in its group, after the built-in ones).
    pub fn register(&self, extractor: Arc<dyn Extractor>) {
        self.write(|s| {
            s.contributed.retain(|e| e.id() != extractor.id());
            s.contributed.push(extractor);
            s.changed();
        });
    }

    /// The separated formats offered (`None`: the defaults).
    pub fn set_csv_formats(&self, formats: Option<Vec<CsvFormat>>) {
        self.write(|s| {
            if s.csv != formats {
                s.csv = formats;
                s.changed();
            }
        });
    }

    pub fn csv_formats(&self) -> Vec<CsvFormat> {
        self.write(|s| s.csv.clone().unwrap_or_else(CsvFormat::defaults))
    }

    pub fn set_options(&self, options: Options) {
        self.write(|s| {
            if s.options != options {
                s.options = options;
                s.generation += 1;
            }
        });
    }

    pub fn options(&self) -> Options {
        self.write(|s| s.options)
    }

    /// Project `project`'s templates (read again: they replace the ones it had).
    pub fn set_scripted(&self, project: ProjectId, extractors: Vec<Arc<dyn Extractor>>) {
        self.write(|s| {
            s.scripted.insert(project, extractors);
            s.generation += 1;
        });
    }

    pub fn scripted(&self, project: ProjectId) -> Vec<Arc<dyn Extractor>> {
        self.write(|s| s.scripted.get(&project).cloned().unwrap_or_default())
    }

    /// Project `project` closed: its templates and its choice let go.
    pub fn forget(&self, project: ProjectId) {
        self.write(|s| {
            s.scripted.remove(&project);
            s.chosen.remove(&Some(project));
            s.generation += 1;
        });
    }

    /// The extractor chosen for `project` (`None`: a grid outside any project).
    pub fn choose(&self, project: Option<ProjectId>, id: &str) {
        self.write(|s| {
            if s.chosen.get(&project).map(String::as_str) != Some(id) {
                s.chosen.insert(project, id.to_owned());
                s.generation += 1;
            }
        });
    }

    /// The id `project` chose (as chosen: it may not be offered any more).
    pub fn chosen_id(&self, project: Option<ProjectId>) -> Option<String> {
        self.write(|s| s.chosen.get(&project).cloned())
    }

    /// Every extractor offered to `project`, in the popover's order (by group).
    pub fn all(&self, project: Option<ProjectId>) -> Vec<Arc<dyn Extractor>> {
        let mut all = self.write(|s| {
            let mut all = s.fixed();
            if let Some(project) = project
                && let Some(scripted) = s.scripted.get(&project)
            {
                all.extend(scripted.iter().cloned());
            }
            all
        });
        // Stable: the order within a group is kept.
        all.sort_by_key(|e| e.group());
        all
    }

    /// Extractor `id` of those offered to `project`.
    pub fn find(&self, project: Option<ProjectId>, id: &str) -> Option<Arc<dyn Extractor>> {
        self.all(project).into_iter().find(|e| e.id() == id)
    }

    /// The extractor `project` uses (its choice, if it's still offered and works; else the
    /// default).
    pub fn chosen(&self, project: Option<ProjectId>) -> Arc<dyn Extractor> {
        let all = self.all(project);
        let id = self.chosen_id(project);
        let usable = |id: &str| all.iter().find(|e| e.id() == id && e.problem().is_none()).cloned();
        id.and_then(|id| usable(&id))
            .or_else(|| usable(DEFAULT_ID))
            .or_else(|| all.iter().find(|e| e.group() == Group::Delimited).cloned())
            .unwrap_or_else(|| Arc::new(Json))
    }

    /// Changes with every change of what's offered, chosen or skipped.
    pub fn generation(&self) -> u64 {
        self.state.read().unwrap_or_else(PoisonError::into_inner).generation
    }
}

static CATALOG: LazyLock<Catalog> = LazyLock::new(Catalog::new);

/// The app's catalog.
pub fn catalog() -> &'static Catalog {
    &CATALOG
}

#[cfg(test)]
#[path = "../tests/unit/catalog.rs"]
mod tests;
