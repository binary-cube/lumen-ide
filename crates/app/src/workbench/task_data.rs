//! What the Task Manager reads: the process's memory and GPU memory, the app's own parts
//! (open files, rendering, the index, syntax, git, the terminal…) with what they hold and
//! the threads doing their work, the modules and their state — refreshed with the status
//! bar's sample — and what the modules' reporters say, asked when it's read. Each window
//! has its own (its project); every window's are listed (weakly) for the view of all the
//! open projects.

use std::sync::{Mutex, Weak};
use std::time::{Duration, Instant};

use workbench_model::{
    ActOutcome, ActionTarget, OpenProject, PartRef, ResourceReporter, ResourceSnapshot,
    ResourceSource, ResourceUse,
};

/// Every window's picture (held weakly: a window closed goes from the list by itself).
static WINDOWS: Mutex<Vec<Weak<AppResources>>> = Mutex::new(Vec::new());

/// When a Task Manager last asked for every window's picture: meanwhile (and a little
/// after), every window keeps its own fresh, not only the one it's open in.
static ALL_ASKED: Mutex<Option<Instant>> = Mutex::new(None);

/// How long every window keeps its picture fresh after the view of all was asked.
const ALL_FOR: Duration = Duration::from_secs(3);

use super::*;

/// The app's side of the Task Manager's picture.
#[derive(Debug)]
pub(crate) struct AppResources {
    board: Mutex<ResourceSnapshot>,
    reporters: Vec<Arc<dyn ResourceReporter>>,
    /// The modules (one's data released from its row).
    modules: Option<Arc<crate::module_manager::ModuleManager>>,
}

/// The Task Manager's action releasing a module's data (on its row).
pub(crate) const RELEASE: &str = "release";

impl AppResources {
    /// This window's picture, listed among every window's.
    pub(super) fn new(
        reporters: Vec<Arc<dyn ResourceReporter>>,
        modules: Option<Arc<crate::module_manager::ModuleManager>>,
    ) -> Arc<Self> {
        let resources = Arc::new(Self { board: Mutex::default(), reporters, modules });
        if let Ok(mut windows) = WINDOWS.lock() {
            windows.retain(|w| w.strong_count() > 0);
            windows.push(Arc::downgrade(&resources));
        }
        resources
    }

    /// Someone reads it (the Task Manager is open here, or the view of every project is
    /// open anywhere): worth filling in the parts.
    pub(super) fn watched(self: &Arc<Self>) -> bool {
        let all = ALL_ASKED.lock().ok().and_then(|at| *at).is_some_and(|at| at.elapsed() < ALL_FOR);
        Arc::strong_count(self) > 1 || all
    }

    fn board(&self) -> ResourceSnapshot {
        self.board.lock().map(|b| b.clone()).unwrap_or_default()
    }

    /// The modules' parts (every project's).
    fn reported(&self) -> impl Iterator<Item = ResourceUse> + '_ {
        self.reporters.iter().flat_map(|r| r.report())
    }

    pub(super) fn set(&self, snapshot: ResourceSnapshot) {
        if let Ok(mut board) = self.board.lock() {
            *board = snapshot;
        }
    }
}

impl AppResources {
    /// This window's project's parts (by its identity) and what's every project's, with
    /// the modules' parts `reported`.
    fn own(&self, reported: &[ResourceUse]) -> ResourceSnapshot {
        let mut snapshot = self.board();
        let this = snapshot.projects.first().map(|p| p.id);
        let reported = reported.iter().filter(|p| p.project.is_none() || p.project == this);
        snapshot.parts.extend(reported.cloned());
        // The other windows' projects, to name their programs listed here (a terminal's).
        if snapshot.parts.iter().any(|p| !p.process_projects.is_empty()) {
            let windows: Vec<Arc<AppResources>> = WINDOWS
                .lock()
                .map(|w| w.iter().filter_map(Weak::upgrade).collect())
                .unwrap_or_default();
            for window in windows.iter().filter(|w| !std::ptr::eq(w.as_ref(), self)) {
                let board = window.board();
                snapshot
                    .elsewhere
                    .extend(board.projects.into_iter().filter(|p| Some(p.id) != this));
            }
        }
        snapshot
    }

    /// Every window's, with the modules' parts `reported`.
    fn every(&self, reported: Vec<ResourceUse>) -> ResourceSnapshot {
        if let Ok(mut asked) = ALL_ASKED.lock() {
            *asked = Some(Instant::now());
        }
        let windows: Vec<Arc<AppResources>> = WINDOWS
            .lock()
            .map(|w| w.iter().filter_map(Weak::upgrade).collect())
            .unwrap_or_default();
        let mut snapshot = self.board();
        snapshot.parts.clear();
        snapshot.projects.clear();
        let mut gpu = GpuOfAll::default();
        let render = i18n::t("tasks.render");
        for window in &windows {
            let board = window.board();
            gpu.add(board.gpu, board.gpu_shared);
            for part in board.parts {
                let listed = snapshot.parts.iter_mut().find(|p| {
                    part.project.is_none()
                        && p.project.is_none()
                        && p.owner == part.owner
                        && p.name == part.name
                });
                match listed {
                    // The rendering (every window's): each window's own surfaces add up,
                    // what they share once.
                    Some(listed) if part.owner == "app" && part.name == render => {
                        let own = part.gpu.map(|g| g.saturating_sub(board.gpu_shared.unwrap_or(0)));
                        listed.gpu = match (listed.gpu, own) {
                            (Some(a), Some(b)) => Some(a + b),
                            (a, b) => a.or(b),
                        };
                    }
                    Some(_) => {}
                    None => snapshot.parts.push(part),
                }
            }
            for project in board.projects {
                if !snapshot.projects.iter().any(|p| p.id == project.id) {
                    snapshot.projects.push(project);
                }
            }
        }
        snapshot.gpu = gpu.total().or(snapshot.gpu);
        snapshot.parts.extend(reported);
        snapshot
    }
}

/// The GPU memory of every window: each one's own adds up, what they share (one GPU device
/// per process: the glyph atlas, the images) counts once.
#[derive(Debug, Default)]
struct GpuOfAll {
    own: Option<u64>,
    shared: u64,
}

impl GpuOfAll {
    /// A window's: all it draws with (`gpu`), of which `shared` is every window's.
    fn add(&mut self, gpu: Option<u64>, shared: Option<u64>) {
        let Some(gpu) = gpu else { return };
        let shared = shared.unwrap_or(0).min(gpu);
        self.own = Some(self.own.unwrap_or(0) + gpu - shared);
        // Every window reads the same (they may have sampled it a moment apart).
        self.shared = self.shared.max(shared);
    }

    fn total(&self) -> Option<u64> {
        self.own.map(|own| own + self.shared)
    }
}

impl ResourceSource for AppResources {
    /// This window's project's parts (by its identity) and what's every project's.
    fn snapshot(&self) -> ResourceSnapshot {
        let reported: Vec<ResourceUse> = self.reported().collect();
        self.own(&reported)
    }

    /// Every window's: each project's parts, the app's own (the same in every window: the
    /// interface, the modules…) once, every open project listed.
    fn snapshot_all(&self) -> ResourceSnapshot {
        self.every(self.reported().collect())
    }

    /// Both, the modules asked once.
    fn snapshot_both(&self) -> (ResourceSnapshot, ResourceSnapshot) {
        let reported: Vec<ResourceUse> = self.reported().collect();
        (self.own(&reported), self.every(reported))
    }

    /// A part's action, done by whoever reported it: a module's row (the app's: its data
    /// released), else the reporter whose part it is (by its owner and name).
    fn act(&self, part: &PartRef, action: &str, target: ActionTarget) -> ActOutcome {
        if action == RELEASE && target == ActionTarget::Part {
            let module = self
                .board()
                .parts
                .into_iter()
                .any(|p| p.group == workbench_model::resources::MODULES && part.is(&p));
            if let (true, Some(modules)) = (module, &self.modules) {
                return if modules.release(&part.owner) {
                    ActOutcome::Done
                } else {
                    ActOutcome::Failed(i18n::t("tasks.nothing_released"))
                };
            }
        }
        self.reporters
            .iter()
            .find(|r| r.report().iter().any(|p| part.is(p)))
            .map_or(ActOutcome::Unsupported, |r| r.act(part, action, target))
    }
}

impl Workbench {
    /// The app's parts, as they are now (for the Task Manager).
    pub(super) fn app_parts(&self) -> Vec<ResourceUse> {
        let part = |key: &str, threads: Vec<&'static str>| {
            let mut part = ResourceUse::new(i18n::t(key), "app");
            part.threads = threads;
            part
        };
        let mut parts = Vec::new();
        // The interface: the window's thread (unnamed or `main`) and its helpers.
        parts.push(part(
            "tasks.ui",
            vec![
                "main",
                "",
                "font-loader",
                render_wgpu::PREPARE_THREAD,
                crate::MODULE_LOADER,
                "watchdog",
                "resources",
                "instance",
                super::session::SESSION_THREAD,
                super::recent_writes::RECENT_FILES_THREAD,
                crate::recent::RECORD_THREAD,
                crate::config::RELOAD_THREAD,
            ],
        ));
        // Open files: their text and undo history.
        let (mut files, mut text, mut undo) = (0, 0, 0);
        for tab in all_tabs(&self.tabs, &self.editors.groups).filter(|t| t.holds_text()) {
            let (t, u) = tab.doc.memory();
            files += 1;
            text += t;
            undo += u;
        }
        let mut open = part("tasks.files", Vec::new());
        open.memory = Some((text + undo) as u64);
        // Its saves: the project's.
        if self.project.is_some() {
            open.project = Some(self.data.id);
            self.data.disk.report(&mut open);
        }
        open.detail =
            i18n::tr!("tasks.files_detail", count = files, undo = i18n::bytes(undo as u64));
        parts.push(open);
        // Rendering: what the GPU holds (glyphs, images, the window's surfaces).
        let mut render = part("tasks.render", Vec::new());
        render.gpu =
            self.resources.as_ref().and_then(|(_, gpu)| gpu.map(render_api::GpuMemory::total));
        render.detail = i18n::t("tasks.render_detail");
        parts.push(render);
        let mut index = part(
            "tasks.index",
            // The project's index (`indexer`, `index-queue`, `file-index`) and the semantic
            // model (`embed`, its download) are the indexing module's part.
            vec!["symbols", "tree-watch"],
        );
        index.memory = self.index_memory();
        index.group = workbench_model::resources::INDEXING;
        parts.push(index);
        parts.push(part(
            "tasks.syntax",
            vec!["syntax", "level-detect", "module-preload", "module-release"],
        ));
        // (Git's reads for the editor's decorations are the git module's part.)
        // Every module's `host.spawn` (pastes, deletions, git's sync): shared, the app's.
        parts.push(part("tasks.file_ops", vec!["module-job"]));
        // The project's frameworks: their models made in the background, their answers on
        // the UI thread over budget counted.
        let frameworks = self.data.frameworks.active();
        if !frameworks.is_empty() {
            let mut row = part("tasks.frameworks", vec![crate::frameworks::FRAMEWORKS_THREAD]);
            row.project = self.project.as_ref().map(|_| self.data.id);
            row.detail = frameworks
                .iter()
                .map(|f| {
                    let named = self.support.frameworks.iter().find(|n| n.id() == f.id);
                    let name = named.map_or(f.id, |n| n.name());
                    i18n::tr!("tasks.frameworks_slow", name = name, count = f.slow())
                })
                .collect::<Vec<_>>()
                .join(" · ");
            parts.push(row);
        }
        // The views open (tabs, the bottom's windows): what each module's hold.
        let mut views: std::collections::HashMap<&str, ui_workbench::ViewMemory> =
            std::collections::HashMap::new();
        let tab_views = all_tabs(&self.tabs, &self.editors.groups)
            .filter_map(|t| t.viewer.as_ref().map(|v| (v.provider, v.viewer.memory())));
        for (provider, memory) in tab_views.chain(self.panel_views_memory()) {
            let module = self
                .support
                .viewer_modules
                .iter()
                .find(|(p, _)| *p == provider)
                .map_or("", |(_, m)| m.as_str());
            let total = views.entry(module).or_default();
            total.ram += memory.ram;
            total.gpu += memory.gpu;
        }
        // The modules: whether they run and whether their data is in memory; the threads
        // they name (their CPU), what their data and their views hold.
        for module in &self.support.modules {
            let id = module.id.as_str();
            let state = match &module.state {
                extensions::ModuleState::Active if self.support.manager.loaded(id) => {
                    i18n::t("tasks.module_loaded")
                }
                extensions::ModuleState::Active => i18n::t("tasks.module_idle"),
                extensions::ModuleState::Disabled => i18n::t("tasks.module_off"),
                extensions::ModuleState::Failed(_) => i18n::t("tasks.module_failed"),
            };
            let mut row =
                ResourceUse::new(super::frameworks::localized(id, "_name", module.name), id);
            row.detail = state;
            // Its data in memory: released from here (read again when needed; not asked).
            if matches!(module.state, extensions::ModuleState::Active)
                && self.support.manager.loaded(id)
            {
                row.actions.push(workbench_model::PartAction::new(
                    RELEASE,
                    workbench_model::ActionKind::Release,
                    ActionTarget::Part,
                ));
            }
            row.group = workbench_model::resources::MODULES;
            row.threads = self
                .support
                .module_threads
                .iter()
                .filter(|(m, _)| m == id)
                .flat_map(|(_, threads)| threads.iter().copied())
                .collect();
            let mut view = views.get(id).copied().unwrap_or_default();
            // The theme's decorations this window draws, if they're this module's.
            if self.support.decorations_modules.first().is_some_and(|m| m == id) {
                let (ram, gpu) = self.decorations_memory();
                view.ram += ram as u64;
                view.gpu += gpu;
            }
            let data = self.support.manager.memory(id);
            if data.is_some() || view.ram > 0 {
                row.memory = Some(data.unwrap_or(0) + view.ram);
            }
            row.gpu = (view.gpu > 0).then_some(view.gpu);
            for (_, counters) in self.support.module_counters.iter().filter(|(m, _)| m == id) {
                if let Some(disk) = counters.disk {
                    disk.report(&mut row);
                }
                if let Some(traffic) = counters.traffic {
                    traffic.report(&mut row);
                }
            }
            parts.push(row);
        }
        parts
    }

    /// The index's memory: measured again every few seconds (walking it isn't free), never
    /// waiting for the indexer (the last figure meanwhile).
    fn index_memory(&self) -> Option<u64> {
        const EVERY: std::time::Duration = std::time::Duration::from_secs(5);
        let last = self.index_memory.get();
        if last.is_some_and(|(at, _)| at.elapsed() < EVERY) {
            return last.map(|(_, bytes)| bytes);
        }
        match self.data.symbols.try_read() {
            Ok(symbols) => {
                let bytes = symbols.memory();
                self.index_memory.set(Some((std::time::Instant::now(), bytes)));
                Some(bytes)
            }
            Err(_) => last.map(|(_, bytes)| bytes),
        }
    }

    /// While the Task Manager is open: its picture refreshed, at most once a second (its
    /// collector wakes the app as often).
    pub(crate) fn refresh_task_data_if_watched(&mut self) {
        if !self.task_data.watched() {
            return;
        }
        let due = self
            .task_refreshed
            .is_none_or(|at| at.elapsed() >= std::time::Duration::from_millis(900));
        if due {
            self.task_refreshed = Some(std::time::Instant::now());
            self.refresh_task_data();
        }
    }

    /// The Task Manager's picture, refreshed (only while it's open).
    pub(super) fn refresh_task_data(&self) {
        // The process's memory and GPU from the status bar's sample (none yet: unknown).
        let (memory, gpu) =
            self.resources.as_ref().map_or((0, None), |(sample, gpu)| (sample.ram_bytes, *gpu));
        let (gpu, gpu_shared) = (gpu.map(render_api::GpuMemory::total), gpu.map(|g| g.shared));
        let parts = if self.task_data.watched() { self.app_parts() } else { Vec::new() };
        // This window's project (by its identity; its name and root only shown).
        let projects = self
            .project
            .as_ref()
            .map(|p| {
                let root = p.root_path().to_path_buf();
                let name = root.file_name().map_or_else(
                    || root.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                );
                OpenProject { id: self.data.id, name, root }
            })
            .into_iter()
            .collect();
        self.task_data.set(ResourceSnapshot {
            memory,
            gpu,
            gpu_shared,
            heap: kernel::memory::live_heap().map(|bytes| bytes as u64),
            parts,
            projects,
            ..ResourceSnapshot::default()
        });
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/task_data.rs"]
mod tests;
