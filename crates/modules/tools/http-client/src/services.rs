//! The runs in the Services window: a group "HTTP Requests", a node
//! per run, the newest first (`#3 GET users`, its status and time after it, a spinner while
//! it runs, an error's mark), its Response, Headers, Timeline and Raw as tabs. A run just started is shown (its tab
//! chosen); a node chosen (↵, a double click) opens its request in its file.

use std::sync::Arc;

use i18n::t;
use ui_components::services::{
    ProjectServices, ServiceId, ServiceNode, ServiceProvider, ServiceState, ServiceTab,
    ServicesContext,
};
use ui_components::{ViewMenuEntry, Viewer, ViewerAction};

use crate::response_view::Part;
use crate::runs::{RunId, RunState, Runs};

/// The icon of the group and of each run.
pub const ICON: &str = "globe";

#[derive(Debug, Default)]
pub struct HttpServices;

impl ServiceProvider for HttpServices {
    fn id(&self) -> &'static str {
        "http-requests"
    }

    fn order(&self) -> u16 {
        20
    }

    fn icon(&self) -> &'static str {
        ICON
    }

    fn title(&self) -> String {
        i18n::t("http.services_group")
    }

    fn open(&self, ctx: &ServicesContext) -> Option<Box<dyn ProjectServices>> {
        let runs = Runs::of(ctx.project, None);
        runs.follow(&ctx.wake);
        Some(Box::new(ProjectRuns {
            runs,
            generation: u64::MAX,
            nodes: Vec::new(),
            action: None,
            wake: Arc::clone(&ctx.wake),
        }))
    }
}

struct ProjectRuns {
    runs: Arc<Runs>,
    generation: u64,
    nodes: Vec<ServiceNode>,
    action: Option<ViewerAction>,
    /// Kept alive: the runs wake the window through it.
    #[allow(dead_code)]
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl ProjectRuns {
    /// The nodes made again when the runs changed.
    fn refresh(&mut self) -> bool {
        let generation = self.runs.generation();
        if generation == self.generation {
            return false;
        }
        self.generation = generation;
        self.nodes = self
            .runs
            .runs()
            .iter()
            .rev()
            .map(|run| {
                let mut node = ServiceNode::new(
                    ServiceId(run.id),
                    ICON,
                    format!("#{} {}", run.number, run.label),
                );
                match &run.state {
                    RunState::Running => node.state = ServiceState::Running,
                    RunState::Failed(_) => node.state = ServiceState::Error,
                    RunState::Done(outcome) => {
                        let r = &outcome.response;
                        node.detail = i18n::tr!(
                            "http.node_detail",
                            status = i64::from(r.status),
                            ms = r.timings.total.as_millis() as i64
                        );
                        // An error, or a test of its script that failed.
                        let failed = outcome.script.as_ref().is_some_and(|s| !s.passed());
                        if r.status >= 400 || failed {
                            node.state = ServiceState::Error;
                        }
                    }
                }
                node
            })
            .collect();
        true
    }

    /// The run and the part tab `tab` shows.
    fn of_tab(&self, tab: ServiceId) -> Option<(RunId, Part)> {
        self.nodes.iter().find_map(|n| {
            Part::ALL
                .into_iter()
                .chain([Part::Tests, Part::Preview])
                .find(|part| tab_of(n.id, *part) == tab)
                .map(|part| (n.id.0, part))
        })
    }
}

impl ProjectServices for ProjectRuns {
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
        // The Tests tab only for a run whose request has scripts, Preview for an HTML one.
        let Some(run) = self.runs.run(node.0) else { return Vec::new() };
        Part::of(&run.state)
            .into_iter()
            .map(|part| ServiceTab::new(tab_of(node, part), ICON, part.title()).fixed())
            .collect()
    }

    fn open_tab(&mut self, tab: ServiceId) -> Option<Box<dyn Viewer>> {
        let (run, part) = self.of_tab(tab)?;
        Some(Box::new(crate::response_view::ResponseView::new(Arc::clone(&self.runs), run, part)))
    }

    fn activate(&mut self, node: ServiceId) -> bool {
        let Some(run) = self.runs.run(node.0) else { return false };
        self.action = Some(ViewerAction::OpenAt { path: run.file, line: run.line_number });
        true
    }

    fn menu(&self, node: ServiceId) -> Vec<ViewMenuEntry> {
        if self.runs.run(node.0).is_none() {
            return Vec::new();
        }
        vec![
            ViewMenuEntry::item("open", t("http.open_request")).with_icon("file"),
            ViewMenuEntry::item("delete", t("http.delete_run")).with_icon("trash"),
            ViewMenuEntry::item("delete-all", t("http.delete_all_runs")).with_icon("trash"),
        ]
    }

    fn command(&mut self, node: ServiceId, id: &str) -> bool {
        match id {
            "open" => self.activate(node),
            "delete" => self.runs.remove(node.0),
            "delete-all" => self.runs.clear_finished() > 0,
            _ => false,
        }
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        // An OAuth sign-in's page, asked for by a run.
        if let Some(asked) = self.runs.take_asked() {
            return Some(asked);
        }
        self.action.take()
    }

    // The OAuth tokens can be had again: let go with the rebuildable caches.
    fn purge(&mut self) {
        self.runs.tokens.clear();
    }

    fn take_reveal(&mut self) -> Option<(ServiceId, Option<ServiceId>)> {
        self.refresh();
        let run = self.runs.take_reveal()?;
        let node = ServiceId(run);
        Some((node, Some(tab_of(node, Part::Body))))
    }
}

/// A run's tab of `part`: its identity derived from its node's (the same every time).
fn tab_of(node: ServiceId, part: Part) -> ServiceId {
    ServiceId(kernel::id::derived_uuid(node.0, part.key()))
}
