//! Marks on the project's folders and files (the tree's "Indexing ›"): what the user decides is
//! left out of the index, a library, generated, or normal whatever the rules say. Saved beside
//! the project's data, for version control; told on the project's bus, so what indexes looks
//! again. Without this module nothing is marked and everything works as before.

mod page;

use std::path::Path;
use std::sync::Arc;

use command_model::{CommandId, CommandSpec};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use workbench_model::{
    CommandContribution, CommandHandler, Completion, ContextMenuItem, MenuPlace, MenuSection,
    Notification, ProjectContext, ProjectService, SettingsContribution, WorkbenchHost,
};
use workspace::marks::{FILE_NAME, PathMark, PathMarks, ProjectMarks};

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("project.marks"),
    name: "Path Marks",
    description: "Decide what's indexed: excluded, library, generated or normal folders",
    depends_on: &[],
};

const EXCLUDED: CommandId = CommandId("marks.excluded");
const LIBRARY: CommandId = CommandId("marks.library");
const GENERATED: CommandId = CommandId("marks.generated");
const NORMAL: CommandId = CommandId("marks.normal");
const CLEAR: CommandId = CommandId("marks.clear");

/// The folder's own `.gitignore` keeps everything else out of version control.
/// Its line in the project folder's `.gitignore` (the marks are the project's: committed).
const LET_THROUGH: &str = "!path-marks";

#[derive(Debug, Default)]
pub struct PathMarksModule;

impl Module for PathMarksModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute(workbench_model::ModuleThreads(&["path-marks"]));
        ctx.contribute::<Arc<dyn ProjectService>>(Arc::new(Service));
        ctx.contribute::<Arc<dyn SettingsContribution>>(Arc::new(page::IndexedPage));
        let rows: [(CommandId, &'static str, Option<PathMark>, &'static str, u16); 5] = [
            (EXCLUDED, "Exclude from Index", Some(PathMark::Excluded), "folder-x", 10),
            (LIBRARY, "Index as Library", Some(PathMark::Library), "folder-input", 20),
            (GENERATED, "Index, Skip Text Search", Some(PathMark::Generated), "gear", 30),
            (NORMAL, "Index as Project Code", Some(PathMark::Normal), "folder", 40),
            (CLEAR, "Clear Indexing Choice", None, "x", 50),
        ];
        const ENTRY: &[MenuPlace] = &[MenuPlace::TreeFile, MenuPlace::TreeFolder];
        for (id, label, mark, icon, order) in rows {
            ctx.contribute(CommandContribution {
                spec: CommandSpec { id, label, menu: None, shortcuts: &[] },
                icon: Some(icon),
                handler: Arc::new(MarkAs(mark)),
            });
            ctx.contribute(ContextMenuItem::new(id, ENTRY, MenuSection::Marks, order));
        }
        Ok(())
    }
}

/// The paths a menu's command acts on, relative to `root`, with `/`.
fn targets(host: &dyn WorkbenchHost, root: &Path) -> Vec<String> {
    let context = host.context();
    let relative = |p: &std::path::PathBuf| {
        p.strip_prefix(root).ok().map(|r| r.to_string_lossy().replace('\\', "/"))
    };
    context.paths().iter().filter_map(relative).filter(|r| !r.is_empty()).collect()
}

#[derive(Debug)]
struct Service;

impl ProjectService for Service {
    fn attach(&self, project: &ProjectContext<'_>) -> Vec<kernel::Subscription> {
        page::attach(project)
    }
}

/// The folders marked as libraries told to what ranks and renames by paths.
fn tell_libraries(root: &Path, marks: &PathMarks) {
    let libraries = marks
        .iter()
        .filter(|(_, mark)| *mark == PathMark::Library)
        .map(|(path, _)| root.join(path))
        .collect();
    kernel::paths::set_libraries(root, libraries);
}

/// `targets` marked `mark`: in force at once, saved in the background.
fn apply(
    root: &Path,
    marks: &ProjectMarks,
    events: &kernel::EventBus,
    targets: &[String],
    mark: Option<PathMark>,
    failed: impl FnOnce(std::io::Error) + Send + 'static,
) {
    let new = marked(&marks.get(), targets, mark);
    let text = new.to_text();
    marks.replace(new);
    events.publish(&workspace::events::PathMarksChanged);
    let root = root.to_path_buf();
    kernel::work::run("path-marks", move || {
        if let Err(e) = save(&root, &text) {
            failed(e);
        }
    });
}

/// `targets` marked `mark` in `marks`.
fn marked(marks: &PathMarks, targets: &[String], mark: Option<PathMark>) -> PathMarks {
    let mut marks = marks.clone();
    for target in targets {
        marks.set(target, mark);
    }
    marks
}

/// `text` kept in the project's own folder, which lets it through to version control.
fn save(root: &Path, text: &str) -> std::io::Result<()> {
    let dir = root.join(kernel::brand::PROJECT_DIR);
    std::fs::create_dir_all(&dir)?;
    // Its own line added (another module's kept).
    ::files::ignore::let_through(&dir, &[LET_THROUGH])?;
    ::files::write_atomic(&dir.join(FILE_NAME), text.as_bytes())
}

#[derive(Debug)]
struct MarkAs(Option<PathMark>);

impl CommandHandler for MarkAs {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.project_root().is_some_and(|root| !targets(host, &root).is_empty())
            && host.path_marks().is_some()
    }

    /// The one the first target has, as its menu row's switch.
    fn switched(&self, host: &dyn WorkbenchHost) -> Option<bool> {
        self.0?;
        let (root, marks) = (host.project_root()?, host.path_marks()?);
        let first = targets(host, &root).into_iter().next()?;
        Some(marks.get().own(&first) == self.0)
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let (Some(root), Some(marks)) = (host.project_root(), host.path_marks()) else {
            return false;
        };
        let chosen = targets(host, &root);
        if chosen.is_empty() {
            return false;
        }
        let new = marked(&marks.get(), &chosen, self.0);
        let text = new.to_text();
        marks.replace(new);
        host.events().publish(&workspace::events::PathMarksChanged);
        host.spawn(Box::new(move || {
            let saved = save(&root, &text);
            Box::new(move |host: &mut dyn WorkbenchHost| {
                if let Err(e) = saved {
                    tracing::warn!(error = %e, "the path marks weren't saved");
                    host.notify(Notification::error(i18n::t("marks.not_saved")));
                }
            }) as Completion
        }));
        true
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
