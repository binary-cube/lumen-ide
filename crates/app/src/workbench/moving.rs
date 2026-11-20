//! Moving files and folders (the tree's drag & drop, Refactor › Move…): what is moved, the
//! classes in it (from the index: a PHP file's class named after it), the project's
//! namespace roots (PSR-4) and the ports the preview plans with — see `refactor::moving`.

use super::*;
use refactor::moving::{ClassFile, MoveContext, MoveTarget, NamespaceRules, Types};

/// The index answers which classes exist.
struct IndexTypes(Arc<std::sync::RwLock<code_intelligence::SymbolIndex>>);

impl Types for IndexTypes {
    fn is_type(&self, fqn: &str) -> bool {
        self.0.read().is_ok_and(|index| index.type_by_fqn(fqn).is_some())
    }
}

impl Workbench {
    /// What moving `from` into folder `into` changes.
    pub(super) fn move_context_for(
        &mut self,
        from: &Path,
        into: &Path,
    ) -> Result<MoveContext, String> {
        let root = self.project.as_ref().map(|p| p.root_path().to_path_buf());
        let root = root.ok_or_else(|| i18n::t("refactor.nothing_to_move"))?;
        if !refactor::moving::movable(&root, from) {
            return Err(i18n::t("refactor.nothing_to_move"));
        }
        self.refresh_symbols_now();
        let is_dir = self.is_dir(from);
        let rules: Option<Arc<dyn NamespaceRules>> = self.support.namespaces.first().cloned();
        let classes = rules
            .as_ref()
            .map_or_else(Vec::new, |rules| self.classes_in(from, is_dir, rules.as_ref()));
        let roots = match &rules {
            Some(rules) => rules.roots(&root, &|p| read_text(&*self.ports.disk, p)),
            None => refactor::moving::NamespaceRoots::default(),
        };
        let target =
            MoveTarget::new(from.to_path_buf(), is_dir, into.to_path_buf(), classes, &roots);
        let extensions = rules.as_ref().map_or(&[][..], |r| r.extensions());
        let sources = self.project_sources(from, extensions);
        Ok(MoveContext {
            target,
            roots,
            root,
            sources: Arc::new(sources),
            rules,
            types: Arc::new(IndexTypes(Arc::clone(&self.data.symbols))),
        })
    }

    /// The classes in `from` (a file, or every file under a folder) named after their file.
    fn classes_in(&self, from: &Path, is_dir: bool, rules: &dyn NamespaceRules) -> Vec<ClassFile> {
        let Ok(index) = self.data.symbols.read() else { return Vec::new() };
        refactor::moving::classes_in(index.files(), from, is_dir, rules.extensions())
    }

    /// Moves `from` into `into` (the tree's drop): as a refactoring when a module takes it
    /// over (the Move preview, the namespaces following), else just moved.
    pub fn move_entry(&mut self, from: &Path, into: &Path) -> bool {
        let into = if self.is_dir(into) { into } else { into.parent().unwrap_or(into) };
        if !refactor::moving::moves_somewhere(from, into) {
            return false;
        }
        let support = Arc::clone(&self.support);
        if support.file_refactorings.iter().any(|f| f.moved(self, from, into)) {
            return true;
        }
        let Some(name) = from.file_name() else { return false };
        if let Err(e) = self.move_file(from, &into.join(name)) {
            workbench_model::WorkbenchHost::notify(
                self,
                workbench_model::Notification::error(e.to_string()),
            );
        }
        true
    }

    /// A press is held whose moves the mouse must follow: a side panel's (a row dragged into
    /// a folder), the projects list's, or a window over the editor's (each says).
    pub fn drag_pending(&mut self) -> bool {
        self.tools.sides.iter().any(|s| s.dragging)
            || self.picker_pressed()
            || Overlay::ALL.into_iter().any(|o| o.slot().held(self))
    }
}
