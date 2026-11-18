use super::*;
use refactor::application::Sources;
use refactor::moving::{MoveTarget, NamespaceRoots, Types};

struct NoSources;

impl Sources for NoSources {
    fn candidates(&self) -> Vec<PathBuf> {
        Vec::new()
    }
    fn read(&self, _path: &Path) -> Option<String> {
        None
    }
}

struct NoTypes;

impl Types for NoTypes {
    fn is_type(&self, _fqn: &str) -> bool {
        false
    }
}

/// A project that isn't on disk (nothing "exists" there).
const ROOT: &str = "/nonexistent-project";

fn view(from: &str, into: &str) -> MoveView {
    let root = PathBuf::from(ROOT);
    let target = MoveTarget::new(
        root.join(from),
        false,
        root.join(into),
        Vec::new(),
        &NamespaceRoots::default(),
    );
    let context = MoveContext {
        target,
        roots: NamespaceRoots::default(),
        root,
        sources: Arc::new(NoSources),
        rules: None,
        types: Arc::new(NoTypes),
    };
    let mut view = MoveView::default();
    view.set_data(&MoveData { context });
    view
}

#[test]
fn a_plain_file_moves_to_the_folder_typed() {
    let mut view = view("notes/todo.txt", "docs");
    assert_eq!(view.destination(), Some(PathBuf::from(ROOT).join("docs/todo.txt")));
    let Some(ViewerAction::ApplyRefactoring(plan)) = view.apply() else { panic!("can be applied") };
    assert_eq!(plan.renames.len(), 1);
    assert!(plan.files.is_empty());
    // Typed back to where it is: nothing to do.
    view.command("select_all");
    view.type_text("notes");
    assert_eq!(view.problem(), Some(Problem::Same));
    assert!(view.apply().is_none());
}
