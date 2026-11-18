//! The consoles in the Services window: a data source › its consoles' sessions › their
//! results. A session's tabs are its Output (the log, never closed), a tab
//! per result (`database.schema.table`, `Result 2`) and one per profile (`Profile ·
//! orders`): an unpinned one is replaced by the next run (a plan by the next plan too), a
//! pinned one stays; closed, its rows go at once. A run ending shows its last result
//! (or the Output, when it failed or brought none).
//!
//! Nothing is made before the window is first shown for the project ([`ConsoleServices`] is
//! its window's); the nodes are made again only when a console's state or results change
//! (their generations), the views when they're shown.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use database::DataSourceId;
use database::console::{ConsoleId, SourceBrief};
use ui_components::{
    LogView, ProjectServices, ServiceId, ServiceNode, ServiceProvider, ServiceState, ServiceTab,
    ServicesContext, Viewer,
};

use crate::consoles::Consoles;
use crate::result_source::ResultSource;
use crate::result_view::ResultView;
use crate::runner::{Runner, millis};

/// The contributor's id.
pub const ID: &str = "database-console";

/// The consoles' part of the Services window.
#[derive(Debug, Default)]
pub struct ConsoleServicesProvider;

impl ServiceProvider for ConsoleServicesProvider {
    fn id(&self) -> &'static str {
        ID
    }

    fn order(&self) -> u16 {
        10
    }

    fn icon(&self) -> &'static str {
        "database"
    }

    fn title(&self) -> String {
        i18n::t("console.services_group")
    }

    fn open(&self, ctx: &ServicesContext) -> Option<Box<dyn ProjectServices>> {
        let consoles = crate::consoles::consoles(ctx.project?)?;
        Some(Box::new(ConsoleServices::new(consoles, Arc::clone(&ctx.wake))))
    }
}

/// A node or tab's identity, kept for what it shows (the same node every time).
#[derive(Debug, Default)]
struct Ids {
    sources: HashMap<DataSourceId, ServiceId>,
    consoles: HashMap<ConsoleId, ServiceId>,
    outputs: HashMap<ConsoleId, ServiceId>,
}

impl Ids {
    fn source(&mut self, id: DataSourceId) -> ServiceId {
        *self.sources.entry(id).or_default()
    }

    fn console(&mut self, id: ConsoleId) -> ServiceId {
        *self.consoles.entry(id).or_default()
    }

    fn output(&mut self, id: ConsoleId) -> ServiceId {
        *self.outputs.entry(id).or_default()
    }
}

/// What a node is (its tabs, its views).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Source,
    Console(ConsoleId),
    /// A result of a console.
    Result(ConsoleId),
}

/// A project's consoles in its Services window.
pub struct ConsoleServices {
    consoles: Arc<Consoles>,
    wake: Arc<dyn Fn() + Send + Sync>,
    ids: Ids,
    nodes: Vec<ServiceNode>,
    kinds: HashMap<ServiceId, Kind>,
    generation: u64,
    /// What the nodes were made from (the runners' and their results' generations).
    seen: Option<Vec<(ConsoleId, u64, u64)>>,
    reveal: Option<(ServiceId, Option<ServiceId>)>,
    /// What a node's menu asked of the app.
    action: Option<ui_components::ViewerAction>,
}

impl std::fmt::Debug for ConsoleServices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConsoleServices").field("nodes", &self.nodes.len()).finish_non_exhaustive()
    }
}

impl ConsoleServices {
    pub fn new(consoles: Arc<Consoles>, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let mut services = Self {
            consoles,
            wake,
            ids: Ids::default(),
            nodes: Vec::new(),
            kinds: HashMap::new(),
            generation: 0,
            seen: None,
            reveal: None,
            action: None,
        };
        services.refresh();
        services
    }

    /// The node of console `id` (`None`: not shown).
    pub fn console_node(&self, id: ConsoleId) -> Option<ServiceId> {
        self.ids.consoles.get(&id).copied()
    }

    /// The Output tab of console `id`.
    pub fn output_tab(&self, id: ConsoleId) -> Option<ServiceId> {
        self.ids.outputs.get(&id).copied()
    }

    /// The runners shown: the consoles of this session (opened, or that ran).
    fn runners(&self) -> Vec<Runner> {
        self.consoles.runners()
    }

    /// The nodes made again when a console changed. `true` if they were.
    fn refresh(&mut self) -> bool {
        let runners = self.runners();
        for runner in &runners {
            runner.add_wake("services", Arc::clone(&self.wake));
        }
        let seen: Vec<(ConsoleId, u64, u64)> =
            runners.iter().map(|r| (r.console(), r.generation(), r.results_generation())).collect();
        let mut revealed = None;
        for runner in &runners {
            if let Some(tab) = runner.take_reveal() {
                revealed = Some((runner.console(), tab));
            }
        }
        let changed = self.seen.as_ref() != Some(&seen);
        if changed {
            self.seen = Some(seen);
            self.build(&runners);
            self.generation += 1;
        }
        if let Some((console, tab)) = revealed
            && let Some(node) = self.console_node(console)
        {
            let tab = tab.or_else(|| self.output_tab(console));
            self.reveal = Some((node, tab));
        }
        changed || revealed.is_some()
    }

    fn build(&mut self, runners: &[Runner]) {
        let access = self.consoles.access();
        let mut nodes = Vec::new();
        let mut kinds = HashMap::new();
        // The sources in the order their consoles were first opened.
        let mut sources: Vec<DataSourceId> = Vec::new();
        for runner in runners {
            if !sources.contains(&runner.source()) {
                sources.push(runner.source());
            }
        }
        for source in sources {
            let brief: Option<SourceBrief> = access.as_ref().and_then(|a| a.source(source));
            let id = self.ids.source(source);
            let name =
                brief.as_ref().map_or_else(|| i18n::t("console.source_gone"), |b| b.name.clone());
            let mut node =
                ServiceNode::new(id, brief.as_ref().map_or("database", |b| b.icon), name);
            if let Some(brief) = &brief {
                let driver = database::drivers::find(&brief.driver)
                    .map_or_else(|| brief.driver.clone(), |d| d.name.into_owned());
                node = node.with_detail(driver);
                if let Some(color) = brief.color {
                    node = node.with_color(color.name());
                }
            }
            nodes.push(node);
            kinds.insert(id, Kind::Source);
            for runner in runners.iter().filter(|r| r.source() == source) {
                let console = runner.console();
                let node_id = self.ids.console(console);
                self.ids.output(console);
                let spec = self.consoles.spec(console);
                // An attached file of the project: a file's icon, not a console's.
                let icon = if spec.as_ref().is_some_and(|s| s.is_attached()) {
                    "file-code"
                } else {
                    "console"
                };
                let name = spec.as_ref().map_or_else(|| "console".to_owned(), |s| s.name.clone());
                let snapshot = runner.snapshot();
                let state = if snapshot.running.is_some() {
                    ServiceState::Running
                } else if snapshot.in_transaction {
                    ServiceState::TxOpen
                } else if snapshot.last.as_ref().is_some_and(|l| !l.ok) {
                    ServiceState::Error
                } else {
                    ServiceState::Idle
                };
                let detail = snapshot
                    .last
                    .as_ref()
                    .map(|l| millis(l.elapsed))
                    .or_else(|| {
                        spec.and_then(|s| s.last_run_ms).map(|ms| millis(Duration::from_millis(ms)))
                    })
                    .unwrap_or_default();
                nodes.push(
                    ServiceNode::new(node_id, icon, name)
                        .under(id)
                        .with_detail(detail)
                        .with_state(state),
                );
                kinds.insert(node_id, Kind::Console(console));
                for tab in runner.result_tabs() {
                    let state =
                        if tab.loading { ServiceState::Running } else { ServiceState::Idle };
                    let icon = if tab.plan { "plan" } else { "table" };
                    nodes.push(
                        ServiceNode::new(tab.id, icon, tab.title).under(node_id).with_state(state),
                    );
                    kinds.insert(tab.id, Kind::Result(console));
                }
            }
        }
        self.nodes = nodes;
        self.kinds = kinds;
    }

    fn runner_of(&self, console: ConsoleId) -> Option<Runner> {
        self.consoles.runner_of(console)
    }

    /// The console a node or tab is of.
    fn console_of(&self, id: ServiceId) -> Option<ConsoleId> {
        match self.kinds.get(&id) {
            Some(Kind::Console(c) | Kind::Result(c)) => Some(*c),
            _ => self.ids.outputs.iter().find(|(_, o)| **o == id).map(|(c, _)| *c),
        }
    }
}

impl ProjectServices for ConsoleServices {
    fn nodes(&self) -> &[ServiceNode] {
        &self.nodes
    }

    fn generation(&self) -> u64 {
        self.generation
    }

    fn poll(&mut self) -> bool {
        self.refresh()
    }

    fn tabs(&self, node: ServiceId) -> Vec<ServiceTab> {
        let Some(console) = self.console_of(node) else { return Vec::new() };
        let Some(runner) = self.runner_of(console) else { return Vec::new() };
        let mut tabs = Vec::new();
        if let Some(output) = self.output_tab(console) {
            tabs.push(ServiceTab::new(output, "file-text", i18n::t("console.output")).fixed());
        }
        for tab in runner.result_tabs() {
            let icon = if tab.plan { "plan" } else { "table" };
            tabs.push(ServiceTab::new(tab.id, icon, tab.title).pinned(tab.pinned));
        }
        tabs
    }

    fn open_tab(&mut self, tab: ServiceId) -> Option<Box<dyn Viewer>> {
        let console = self.console_of(tab)?;
        let runner = self.runner_of(console)?;
        if self.output_tab(console) == Some(tab) {
            return Some(Box::new(LogView::new(runner.log().clone())));
        }
        runner.result_generation(tab)?;
        if runner.plan(tab).is_some() {
            return Some(Box::new(crate::profile::ProfileView::new(runner, tab)));
        }
        let folder = self
            .consoles
            .spec(console)
            .and_then(|s| s.path(self.consoles.root()).parent().map(Path::to_path_buf));
        let source = ResultSource::new(runner.clone(), tab, runner.runtime());
        Some(Box::new(ResultView::new(source, Some(crate::settings::fetch_limit()), folder)))
    }

    fn close_tab(&mut self, tab: ServiceId) {
        if let Some(runner) = self.console_of(tab).and_then(|c| self.runner_of(c)) {
            runner.close_result(tab);
        }
        self.refresh();
    }

    fn pin_tab(&mut self, tab: ServiceId, pinned: bool) {
        if let Some(runner) = self.console_of(tab).and_then(|c| self.runner_of(c)) {
            runner.pin(tab, pinned);
        }
        self.refresh();
    }

    fn take_reveal(&mut self) -> Option<(ServiceId, Option<ServiceId>)> {
        self.reveal.take()
    }

    /// A source: its consoles ("Database Consoles…": listed, renamed, deleted); a console's
    /// session: its editor, renamed, deleted; an attached file's: its editor, detached.
    fn menu(&self, node: ServiceId) -> Vec<ui_components::ViewMenuEntry> {
        use ui_components::ViewMenuEntry;
        match self.kinds.get(&node) {
            Some(Kind::Source) => vec![
                ViewMenuEntry::item("consoles", i18n::t("console.menu_consoles"))
                    .with_icon("console"),
            ],
            Some(Kind::Console(c)) if self.consoles.spec(*c).is_some_and(|s| s.is_attached()) => {
                vec![
                    ViewMenuEntry::item("open", i18n::t("console.menu_open_file"))
                        .with_icon("file-code"),
                    ViewMenuEntry::Separator,
                    ViewMenuEntry::item("detach", i18n::t("console.detach")).with_icon("x"),
                ]
            }
            Some(Kind::Console(_)) => vec![
                ViewMenuEntry::item("open", i18n::t("console.menu_open")).with_icon("console"),
                ViewMenuEntry::Separator,
                ViewMenuEntry::item("rename", i18n::t("console.rename")).with_icon("file-pen"),
                ViewMenuEntry::item("delete", i18n::t("console.delete")).with_icon("trash"),
            ],
            _ => Vec::new(),
        }
    }

    fn command(&mut self, node: ServiceId, id: &str) -> bool {
        use ui_components::{ViewerAction, WithHost};
        let run = |id: &str| Some(ViewerAction::RunCommand(id.to_owned()));
        let project = self.consoles.project();
        self.action = match (self.kinds.get(&node).copied(), id) {
            (Some(Kind::Source), "consoles") => {
                let Some(source) =
                    self.ids.sources.iter().find(|(_, n)| **n == node).map(|(s, _)| *s)
                else {
                    return false;
                };
                database::console::ask_jump(database::console::JumpRequest {
                    project,
                    source,
                    schema: None,
                    table: None,
                    new: false,
                    list: true,
                    database: None,
                });
                run(crate::JUMP.0)
            }
            (Some(Kind::Console(console)), "open") => {
                let Some(spec) = self.consoles.spec(console) else { return false };
                Some(ViewerAction::OpenFile {
                    path: spec.path(self.consoles.root()),
                    line: None,
                    column: None,
                })
            }
            (Some(Kind::Console(console)), "detach") => {
                let Some(spec) = self.consoles.spec(console).filter(|s| s.is_attached()) else {
                    return false;
                };
                if self.runner_of(console).is_some_and(|r| r.in_transaction()) {
                    self.consoles.notify(workbench_model::Notification::warning(i18n::t(
                        "console.tx_open_attach",
                    )));
                    return false;
                }
                self.consoles.detach(&spec.path(self.consoles.root()));
                self.refresh();
                None
            }
            (Some(Kind::Console(console)), "rename") => {
                Some(ViewerAction::WithHost(WithHost::once(move |host| {
                    crate::rename::ask_name(host, console);
                })))
            }
            (Some(Kind::Console(console)), "delete") => {
                Some(ViewerAction::WithHost(WithHost::once(move |host| {
                    crate::rename::confirm_delete(host, console);
                })))
            }
            _ => return false,
        };
        true
    }

    fn take_action(&mut self) -> Option<ui_components::ViewerAction> {
        self.action.take()
    }

    fn purge(&mut self) {
        for runner in self.runners() {
            runner.purge();
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/services.rs"]
mod tests;
