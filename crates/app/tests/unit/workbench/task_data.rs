//! The window's resources do a part's action through whoever reported it: a module's row
//! (its data released by the app), else the reporter whose part it is — by its owner and
//! name, another's never; nobody's: unsupported.

use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use workbench_model::{ActionKind, PartAction};

/// A module's reporter answering for its one part, counting what it's asked.
#[derive(Debug, Default)]
struct Reporter {
    asked: AtomicUsize,
}

impl ResourceReporter for Reporter {
    fn report(&self) -> Vec<ResourceUse> {
        let mut part = ResourceUse::new("Formatters", "quality");
        part.actions = vec![
            PartAction::new("stop-program", ActionKind::Stop, ActionTarget::Process(9)).confirmed(),
        ];
        vec![part]
    }

    fn act(&self, _part: &PartRef, action: &str, target: ActionTarget) -> ActOutcome {
        self.asked.fetch_add(1, Ordering::Relaxed);
        if action == "stop-program" && target == ActionTarget::Process(9) {
            ActOutcome::Done
        } else {
            ActOutcome::Unsupported
        }
    }
}

/// A module holding data until released.
struct Holding(extensions::ModuleManifest, AtomicBool);

impl extensions::Module for Holding {
    fn manifest(&self) -> &extensions::ModuleManifest {
        &self.0
    }
    fn activate(
        &self,
        _: &mut extensions::ModuleContext<'_>,
    ) -> Result<(), extensions::ModuleError> {
        Ok(())
    }
    fn release_idle(&self, _idle: Duration) -> bool {
        self.1.swap(false, Ordering::Relaxed)
    }
    fn loaded(&self) -> bool {
        self.1.load(Ordering::Relaxed)
    }
}

#[test]
fn an_action_goes_to_whoever_reported_the_part() {
    let reporter = Arc::new(Reporter::default());
    let mut registry = extensions::ModuleRegistry::default();
    let manifest = extensions::ModuleManifest {
        id: extensions::ModuleId::new_static("lang.test"),
        name: "Test",
        description: "",
        depends_on: &[],
        info: extensions::ModuleInfo::BUILT_IN,
    };
    registry.register(Box::new(Holding(manifest, AtomicBool::new(true))));
    let modules = Arc::new(crate::module_manager::ModuleManager::new(
        registry,
        Vec::new(),
        Vec::new(),
        Arc::new(Default::default()),
    ));
    let resources = AppResources::new(vec![reporter.clone()], Some(Arc::clone(&modules)));
    // The module's row, as the app lists it.
    let mut row = ResourceUse::new("Test", "lang.test");
    row.group = workbench_model::resources::MODULES;
    resources.set(ResourceSnapshot { parts: vec![row], ..ResourceSnapshot::default() });
    let named =
        |owner: &str, name: &str| PartRef { owner: owner.into(), name: name.into(), project: None };
    // Its owner, by owner and name.
    let formatters = named("quality", "Formatters");
    assert_eq!(
        resources.act(&formatters, "stop-program", ActionTarget::Process(9)),
        ActOutcome::Done
    );
    assert_eq!(reporter.asked.load(Ordering::Relaxed), 1);
    // Nobody's part: nobody asked.
    assert_eq!(
        resources.act(&named("quality", "Other"), "stop-program", ActionTarget::Process(9)),
        ActOutcome::Unsupported
    );
    assert_eq!(reporter.asked.load(Ordering::Relaxed), 1, "another's part never reaches it");
    // A module's row: its data released by the app; again, nothing left.
    assert!(modules.loaded("lang.test"));
    assert_eq!(
        resources.act(&named("lang.test", "Test"), RELEASE, ActionTarget::Part),
        ActOutcome::Done
    );
    assert!(!modules.loaded("lang.test"));
    assert!(matches!(
        resources.act(&named("lang.test", "Test"), RELEASE, ActionTarget::Part),
        ActOutcome::Failed(_)
    ));
}

#[test]
fn every_window_s_gpu_counts_what_they_share_once() {
    const MB: u64 = 1024 * 1024;
    let mut gpu = GpuOfAll::default();
    assert_eq!(gpu.total(), None, "no window measured: unknown");
    // Three windows: 21 MB of surfaces each, the same 20 MB atlas (one read a moment later,
    // after it grew).
    gpu.add(Some(41 * MB), Some(20 * MB));
    gpu.add(Some(41 * MB), Some(20 * MB));
    gpu.add(Some(45 * MB), Some(24 * MB));
    assert_eq!(gpu.total(), Some(3 * 21 * MB + 24 * MB));
    // A window that can't estimate it adds nothing; one that doesn't split counts all.
    gpu.add(None, None);
    gpu.add(Some(5 * MB), None);
    assert_eq!(gpu.total(), Some(3 * 21 * MB + 24 * MB + 5 * MB));
}

/// A reporter of every project's programs (the terminals'), each with its project.
#[derive(Debug)]
struct Shells(kernel::ProjectId);

impl ResourceReporter for Shells {
    fn report(&self) -> Vec<ResourceUse> {
        let mut part = ResourceUse::new("Terminal", "terminal");
        part.processes = vec![7];
        part.process_projects = vec![(7, self.0)];
        vec![part]
    }
}

/// A window's picture names the other windows' projects (its terminals list their programs
/// too), never its own among them; without such programs, nobody else's is looked at.
#[test]
fn a_window_names_the_projects_of_the_programs_it_lists() {
    let open = |name: &str| workbench_model::OpenProject {
        id: kernel::ProjectId::new(),
        name: name.into(),
        root: std::path::PathBuf::from("/w").join(name),
    };
    let (here, there) = (open("php-editor"), open("api"));
    let window = |project: &workbench_model::OpenProject, reporters| {
        let resources = AppResources::new(reporters, None);
        resources.set(ResourceSnapshot {
            projects: vec![project.clone()],
            ..ResourceSnapshot::default()
        });
        resources
    };
    let a = window(&here, vec![Arc::new(Shells(there.id))]);
    let _b = window(&there, Vec::new());
    let snapshot = a.snapshot();
    assert!(snapshot.elsewhere.iter().any(|p| p.id == there.id));
    assert!(snapshot.elsewhere.iter().all(|p| p.id != here.id));
    let alone = window(&here, Vec::new());
    assert!(alone.snapshot().elsewhere.is_empty());
}
