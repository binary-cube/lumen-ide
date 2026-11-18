//! A console's profiles: made by its runner on `db-console` (the driver's profile, then
//! what's worked out of it: [`database::profile::derive`], then its analysis: [`analysis`],
//! 39h3), kept here ([`store::Profiles`]) while the project is open — never written anywhere
//! unless exported (their conditions may hold personal data) — and seen (39h4) in a tab of
//! the Services window or an editor tab of its own ([`view::ProfileView`], `view-profile`):
//! the tree with the time's heat, the costliest branch and the problems (and, 39h5, the same
//! as a diagram and as a flame graph; 39h6, its tables with the server's statistics of them,
//! and two profiles compared: [`tables`], [`compare`]), the statement's summary, the findings
//! and the details. Its actions are commands working in that view
//! (F2 / ⇧F2, H…), remappable.

pub mod analysis;
pub mod compare;
pub mod details;
pub mod export;
pub mod figures;
pub mod findings;
pub mod flame;
pub mod graph;
pub mod layout;
#[cfg(any(test, feature = "snapshot"))]
pub mod sample;
pub mod store;
pub mod summary;
pub mod tables;
pub mod tree;
pub mod view;

use std::path::Path;
use std::sync::Arc;

use command_model::{CommandId, CommandSpec};
use ui_components::{Viewer, ViewerKind, ViewerProvider};
use workbench_model::{CommandContribution, ViewAction, WorkbenchHost};

pub use analysis::Analysis;
pub use store::{Profiles, StoredProfile};
pub use view::{ProfileTabData, ProfileView};

/// The profile's view (its tab's extension, where its actions' keys work).
pub const VIEW: &str = "view-profile";

/// The next / previous problem (F2 / ⇧F2), the hottest step (H), the tree opened or folded,
/// the profile copied as text, Export ▾, Open in Tab, the tree's, the diagram's, the flame's
/// and the tables' views, Compare ▾.
pub const NEXT_PROBLEM: CommandId = CommandId("database.profile.next_problem");
pub const PREV_PROBLEM: CommandId = CommandId("database.profile.prev_problem");
pub const HOTTEST: CommandId = CommandId("database.profile.hottest");
pub const EXPAND_ALL: CommandId = CommandId("database.profile.expand_all");
pub const COLLAPSE_ALL: CommandId = CommandId("database.profile.collapse_all");
pub const COPY_TEXT: CommandId = CommandId("database.profile.copy_text");
pub const EXPORT: CommandId = CommandId("database.profile.export");
pub const OPEN_IN_TAB: CommandId = CommandId("database.profile.open_in_tab");
pub const VIEW_TREE: CommandId = CommandId("database.profile.view_tree");
pub const VIEW_GRAPH: CommandId = CommandId("database.profile.view_graph");
pub const VIEW_FLAME: CommandId = CommandId("database.profile.view_flame");
pub const VIEW_TABLES: CommandId = CommandId("database.profile.view_tables");
/// Compare ▾: the console's other profiles to compare with.
pub const COMPARE: CommandId = CommandId("database.profile.compare");

/// The view's actions: the command, its label, its keys, what the view is sent.
pub const ACTIONS: [(CommandId, &str, &[&str], &str); 13] = [
    (NEXT_PROBLEM, "Next Problem (Profile)", &["F2"], "next_problem"),
    (PREV_PROBLEM, "Previous Problem (Profile)", &["Shift+F2"], "prev_problem"),
    (HOTTEST, "Hottest Step (Profile)", &["h"], "hottest"),
    (EXPAND_ALL, "Expand All (Profile)", &[], "expand_all"),
    (COLLAPSE_ALL, "Collapse All (Profile)", &[], "collapse_all"),
    (COPY_TEXT, "Copy Profile as Text", &[], "copy_text"),
    (EXPORT, "Export Profile…", &[], "export"),
    (OPEN_IN_TAB, "Open Profile in Tab", &[], "open_in_tab"),
    (VIEW_TREE, "Profile as a Tree", &[], "view_tree"),
    (VIEW_GRAPH, "Profile as a Diagram", &[], "view_graph"),
    (VIEW_FLAME, "Profile as a Flame Graph", &[], "view_flame"),
    (VIEW_TABLES, "Profile's Tables", &[], "view_tables"),
    (COMPARE, "Compare Profile with…", &[], "compare"),
];

/// Its commands: the view's actions, and the internal ones.
pub fn commands() -> Vec<CommandContribution> {
    let out: Vec<CommandContribution> = ACTIONS
        .into_iter()
        .map(|(id, label, shortcuts, action)| CommandContribution {
            spec: CommandSpec { id, label, menu: None, shortcuts },
            icon: None,
            handler: Arc::new(ViewAction { view: VIEW, action }),
        })
        .collect();
    out
}

/// `sql` appended to console `file` (after a blank line), selected; nothing run. `false`:
/// the console couldn't be shown.
pub fn insert_into(host: &mut dyn WorkbenchHost, file: &Path, sql: &str) -> bool {
    if !host.open_file(file) {
        return false;
    }
    let text = host.file_text(file).unwrap_or_default();
    let end = text.len();
    let lead = if text.is_empty() || text.ends_with("\n\n") {
        ""
    } else if text.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    let sql = sql.trim_end();
    let body = if sql.ends_with(';') { sql.to_owned() } else { format!("{sql};") };
    let insert = format!("{lead}{body}\n");
    if !host.edit_active(end..end, &insert) {
        return false;
    }
    let start = end + lead.len();
    host.select_active(start..start + body.len());
    true
}

/// The profile's editor tab (`view-profile`): its profile given by [`OPEN`].
#[derive(Debug, Default)]
pub struct ProfileViewProvider;

impl ViewerProvider for ProfileViewProvider {
    fn id(&self) -> &'static str {
        VIEW
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        (path.extension().and_then(|e| e.to_str()) == Some(VIEW)).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(ProfileView::empty()))
    }
}
