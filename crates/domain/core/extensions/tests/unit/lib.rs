use super::*;

#[derive(Debug, PartialEq)]
struct Grammar(&'static str);

struct Lang(ModuleManifest, &'static str);
impl Module for Lang {
    fn manifest(&self) -> &ModuleManifest {
        &self.0
    }
    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute(Grammar(self.1));
        ctx.contribute(42u32);
        Ok(())
    }
}

struct Broken(ModuleManifest);
impl Module for Broken {
    fn manifest(&self) -> &ModuleManifest {
        &self.0
    }
    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute(Grammar("partial"));
        panic!("boom");
    }
}

fn manifest(id: &'static str, deps: &'static [&'static str]) -> ModuleManifest {
    ModuleManifest {
        id: ModuleId::new_static(id),
        name: id,
        description: "",
        depends_on: deps,
        info: crate::ModuleInfo::BUILT_IN,
    }
}

fn registry() -> ModuleRegistry {
    let mut r = ModuleRegistry::default();
    r.register(Box::new(Lang(manifest("lang.html", &[]), "html")));
    r.register(Box::new(Lang(manifest("lang.php", &["lang.html"]), "php")));
    r.register(Box::new(Broken(manifest("broken", &[]))));
    r
}

#[test]
fn contributions_are_typed_and_failures_isolated() {
    let (mut c, reports) = registry().activate(&ModuleSelection::default());
    assert_eq!(c.take::<Grammar>(), vec![Grammar("html"), Grammar("php")]);
    assert_eq!(c.count::<u32>(), 2);
    assert_eq!(reports[2].state, ModuleState::Failed(ModuleError::Panicked));
}

#[test]
fn selection_and_dependencies() {
    let selection = ModuleSelection { disabled: vec!["lang.html".into()], ..Default::default() };
    let (mut c, reports) = registry().activate(&selection);
    assert_eq!(reports[0].state, ModuleState::Disabled);
    assert_eq!(
        reports[1].state,
        ModuleState::Failed(ModuleError::MissingDependency("lang.html".into()))
    );
    assert!(c.take::<Grammar>().is_empty());

    let explicit =
        ModuleSelection { explicit: true, enabled: vec!["lang.html".into()], ..Default::default() };
    let (mut c, _) = registry().activate(&explicit);
    assert_eq!(c.take::<Grammar>(), vec![Grammar("html")]);
}

/// A module whose dependency is selected but failed to start doesn't start on top of it.
#[test]
fn a_failed_dependency_is_missing() {
    let mut r = ModuleRegistry::default();
    r.register(Box::new(Broken(manifest("broken", &[]))));
    r.register(Box::new(Lang(manifest("needs.broken", &["broken"]), "x")));
    let (mut c, reports) = r.activate(&ModuleSelection::default());
    assert_eq!(reports[0].state, ModuleState::Failed(ModuleError::Panicked));
    assert_eq!(
        reports[1].state,
        ModuleState::Failed(ModuleError::MissingDependency("broken".into()))
    );
    assert!(c.take::<Grammar>().is_empty());
}

/// One module's data let go on its own (the Task Manager's "Release its data"), whatever its
/// age; the others keep theirs.
#[test]
fn one_modules_data_is_released() {
    use std::sync::atomic::{AtomicBool, Ordering};
    struct Holding(ModuleManifest, AtomicBool);
    impl Module for Holding {
        fn manifest(&self) -> &ModuleManifest {
            &self.0
        }
        fn activate(&self, _: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
            Ok(())
        }
        fn release_idle(&self, _idle: std::time::Duration) -> bool {
            self.1.swap(false, Ordering::Relaxed)
        }
        fn loaded(&self) -> bool {
            self.1.load(Ordering::Relaxed)
        }
    }
    let manifest = |id: &'static str| ModuleManifest {
        id: ModuleId::new_static(id),
        name: id,
        description: "",
        depends_on: &[],
        info: crate::ModuleInfo::BUILT_IN,
    };
    let mut registry = ModuleRegistry::default();
    registry.register(Box::new(Holding(manifest("a"), AtomicBool::new(true))));
    registry.register(Box::new(Holding(manifest("b"), AtomicBool::new(true))));
    assert!(registry.release("a"));
    assert!(!registry.loaded("a") && registry.loaded("b"), "only its own");
    assert!(!registry.release("a"), "nothing left to release");
    assert!(!registry.release("nobody"));
}

/// A hidden module still starts (what it serves others stays); the app drops the kinds of
/// contributions it decides not to show, only that module's.
#[test]
fn a_hidden_module_runs_and_only_the_dropped_kinds_of_its_contributions_go() {
    let selection = ModuleSelection { hidden: vec!["lang.php".into()], ..Default::default() };
    let (mut c, reports) = registry().activate(&selection);
    assert_eq!(reports[1].state, ModuleState::Active);
    assert!(reports[1].hidden && !reports[0].hidden && !reports[2].hidden);
    let hidden = |id: &ModuleId| reports.iter().any(|r| r.hidden && r.id == *id);
    assert_eq!(c.drop_from::<Grammar>(hidden), 1);
    assert_eq!(c.take::<Grammar>(), vec![Grammar("html")]);
    assert_eq!(c.count::<u32>(), 2, "the kinds not dropped stay");
    assert_eq!(c.drop_from::<String>(hidden), 0, "none of that kind");
    // A module off isn't "hidden": it doesn't run at all.
    let both = ModuleSelection {
        disabled: vec!["lang.html".into()],
        hidden: vec!["lang.html".into()],
        ..Default::default()
    };
    let (_, reports) = registry().activate(&both);
    assert_eq!(reports[0].state, ModuleState::Disabled);
    assert!(!reports[0].hidden);
}
