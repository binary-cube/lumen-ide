use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};

use refactor::NameStyle;
use refactor::application::{ConflictCheck, RenameTarget, Resolution, Resolver, Scope, Sources};

use super::*;

struct Files(HashMap<PathBuf, String>);

impl Sources for Files {
    fn candidates(&self) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = self.0.keys().cloned().collect();
        paths.sort();
        paths
    }
    fn read(&self, path: &Path) -> Option<String> {
        self.0.get(path).cloned()
    }
}

/// Everything after `->` is the target; anything else unknown.
struct Arrow;

impl Resolver for Arrow {
    fn resolve(&self, _: &Path, text: &str, range: Range<usize>) -> Resolution {
        if text[..range.start].ends_with("->") || text[..range.start].ends_with("function ") {
            Resolution::Target
        } else {
            Resolution::Unknown
        }
    }
}

/// `taken` is already declared.
struct Taken;

impl ConflictCheck for Taken {
    fn conflicts(&self, new_name: &str) -> Vec<Conflict> {
        (new_name == "taken")
            .then(|| Conflict {
                path: "/p/a.php".into(),
                line: 0,
                line_text: "function taken()".into(),
            })
            .into_iter()
            .collect()
    }
}

fn view(name: &str, files: &[(&str, &str)]) -> RenameView {
    let sources = Files(files.iter().map(|(p, t)| (PathBuf::from(p), (*t).to_owned())).collect());
    let target = RenameTarget {
        name: name.into(),
        style: NameStyle::CODE.with_sigil(Some('$')),
        scope: Scope::Project,
        file: None,
    };
    let context = RenameContext {
        target: target.clone(),
        sources: Arc::new(sources),
        resolver: Arc::new(Arrow),
        conflicts: Arc::new(Taken),
    };
    let mut view = RenameView::default();
    // The plan computed here, not on the background thread.
    let plan = plan_rename(&target, context.sources.as_ref(), context.resolver.as_ref(), &|| false);
    view.set_data(&RenameData { context, name: None });
    view.set_plan(plan);
    view
}

#[test]
fn the_preview_follows_the_typed_name() {
    let mut view = view("save", &[("/p/a.php", "<?php function save() {} $r->save(); save();")]);
    // The old name is selected: typing replaces it.
    assert!(!view.can_apply(), "the same name can't be applied");
    view.type_name("store");
    assert_eq!(view.new_name(), "store");
    let plan = view.plan().unwrap();
    assert!(plan.files[0].occurrences.iter().all(|o| o.replacement == "store"));
    assert_eq!(plan.chosen(), (2, 1), "the unknown call isn't chosen");
    assert!(view.can_apply());
    let Some(ViewerAction::ApplyRefactoring(plan)) = view.apply() else { panic!("no action") };
    assert_eq!(plan.edits()[0].edits.len(), 2);
}

#[test]
fn checkboxes_choose_places_and_files() {
    let mut view = view("save", &[("/p/a.php", "<?php function save() {} $r->save(); save();")]);
    view.type_name("store");
    view.toggle(Row::Place(0, 2));
    assert_eq!(view.plan().unwrap().chosen(), (3, 1));
    view.toggle(Row::File(0));
    assert_eq!(view.plan().unwrap().chosen(), (0, 0));
    assert!(!view.can_apply());
    view.toggle(Row::File(0));
    assert_eq!(view.plan().unwrap().chosen(), (3, 1));
}

#[test]
fn invalid_names_cant_be_applied_and_php_variables_keep_their_dollar() {
    let mut view = view("save", &[("/p/a.php", "<?php function save() {}")]);
    view.type_name("2x");
    assert!(!view.name_ok());
    let mut var = view_var();
    var.type_name("sum");
    assert_eq!(var.new_name(), "$sum");
    assert!(var.name_ok());
}

fn view_var() -> RenameView {
    view("$total", &[("/p/a.php", "<?php $total = 1;")])
}

#[test]
fn a_taken_name_shows_its_conflict() {
    let mut view = view("save", &[("/p/a.php", "<?php function save() {}")]);
    view.type_name("taken");
    assert_eq!(view.conflicts().len(), 1);
    assert!(view.can_apply(), "still possible, on purpose");
    view.type_name("2");
    assert!(view.conflicts().is_empty(), "takenx2: free");
}

/// With a waker (the app), a typed name's clashes are found on a worker: none on the key
/// itself, there by a poll once found.
#[test]
fn a_name_s_conflicts_are_found_on_a_worker() {
    let mut view = view("save", &[("/p/a.php", "<?php function save() {}")]);
    view.waker = Some(Arc::new(|| {}));
    view.type_name("taken");
    assert!(view.conflicts().is_empty(), "not on the key");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while view.checking.is_some() && std::time::Instant::now() < deadline {
        view.poll();
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(view.conflicts().len(), 1, "there by a poll");
}
