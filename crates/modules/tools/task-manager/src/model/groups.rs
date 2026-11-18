//! How the parts are grouped: by kind (the app's core, indexing, terminals, external
//! programs, what an earlier run left running, connections, language servers, modules), by project (every open project's view:
//! one group each by its identity, then what they share, then the modules), or as one list.

use i18n::t;
use workbench_model::resources::{
    APP, INDEXING, LANGUAGE_SERVERS, LEFT_BEHIND, MODULES, PROGRAMS, SERVICES, TERMINALS,
};

use crate::collector::{Frame, Line};

/// The kinds, in their order.
pub const KINDS: [&str; 8] =
    [APP, INDEXING, TERMINALS, PROGRAMS, LEFT_BEHIND, SERVICES, LANGUAGE_SERVERS, MODULES];

/// How the parts are grouped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Grouping {
    /// By what they are ([`KINDS`]).
    #[default]
    Kind,
    /// By project (only in the view of every open project; else by kind).
    Project,
    /// One list, no groups.
    List,
}

impl Grouping {
    /// What it is on `frame` (by project only in the view of every project).
    pub fn on(self, frame: &Frame) -> Self {
        match self {
            Grouping::Project if !frame.all => Grouping::Kind,
            other => other,
        }
    }
}

/// A group of the list: a kind — or an open project (by its identity, never its name: two
/// may share one), and what the projects share; the operations, above the parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GroupKey {
    Kind(&'static str),
    Project(kernel::ProjectId),
    Shared,
    /// The operations (running, and the last ended), above the parts.
    Operations,
}

/// A kind's name, in the user's language.
pub fn group_name(kind: &str) -> String {
    t(&format!("tasks.group_{kind}"))
}

/// The groups of `frame`, in their order, with their names (`grouping` as it is on the
/// frame: [`Grouping::on`]; none for a list).
pub fn groups(frame: &Frame, grouping: Grouping) -> Vec<(GroupKey, String)> {
    match grouping.on(frame) {
        Grouping::List => Vec::new(),
        Grouping::Kind => KINDS.into_iter().map(|g| (GroupKey::Kind(g), group_name(g))).collect(),
        Grouping::Project => {
            let projects = frame.projects.iter().map(|p| (GroupKey::Project(p.id), p.name.clone()));
            projects
                .chain([
                    (GroupKey::Shared, t("tasks.group_shared")),
                    (GroupKey::Kind(MODULES), group_name(MODULES)),
                ])
                .collect()
        }
    }
}

/// Whether `line` is in group `key` of `frame`.
pub fn belongs(frame: &Frame, grouping: Grouping, line: &Line, key: GroupKey) -> bool {
    let by_project = grouping.on(frame) == Grouping::Project;
    match key {
        GroupKey::Kind(kind) if by_project => line.project.is_none() && line.group == kind,
        GroupKey::Kind(kind) => line.group == kind,
        GroupKey::Project(id) => line.project == Some(id),
        // Its rows are the operations, not parts.
        GroupKey::Operations => false,
        // Every project's: not a project's, not a module's row.
        GroupKey::Shared => {
            (line.project.is_none() || !frame.projects.iter().any(|p| Some(p.id) == line.project))
                && line.group != MODULES
        }
    }
}
