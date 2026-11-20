//! Refactoring module: renames the symbol at the cursor — a class, an interface, a method,
//! a function, a property, a constant, a variable or a parameter, a CSS class / id /
//! custom property — everywhere it's used, in PHP, JS, CSS and HTML.
//!
//! The command asks the workbench port what is at the cursor
//! ([`WorkbenchHost::rename_at_cursor`]: the language's rules, through the `refactor`
//! domain's ports) and opens the preview: every place found, file by file, the sure ones
//! chosen, the unsure ones shown for the user to choose; the new name is typed there and
//! the preview follows it as it's typed. Applying goes back through the port, and the
//! whole refactoring can be undone at once.

mod move_view;
mod name_field;
mod page;
mod param_fields;
mod plan_list;
mod rename_view;
mod signature_view;
mod usages_view;

use std::path::Path;
use std::sync::Arc;

use command_model::{CommandId, CommandSpec, MenuAt};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use i18n::{t, tr};
use ui_components::{Viewer, ViewerKind, ViewerProvider};
use workbench_model::menus::sections;
use workbench_model::{
    CommandContribution, CommandHandler, ContextMenuItem, FileRefactoring, LanguageFeature,
    MenuPlace, MenuSection, MenuWhen, WorkbenchHost,
};

pub use move_view::{MoveData, MoveView};
pub use rename_view::{EXTENSION, RenameData, RenameView};
pub use signature_view::{SignatureData, SignatureView};
pub use usages_view::{UsagesData, UsagesView};

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("refactoring"),
    name: "Refactoring",
    description: "Rename symbols everywhere they're used (PHP, JS, CSS, HTML), change signatures, move classes and files, with a preview and undo",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "wand", ..extensions::ModuleInfo::BUILT_IN },
};

pub const RENAME: CommandId = workbench_model::RENAME_SYMBOL;
pub const UNDO: CommandId = CommandId("refactor.undo");
pub const CHANGE_SIGNATURE: CommandId = CommandId("refactor.change_signature");
pub const FIND_USAGES: CommandId = workbench_model::FIND_USAGES;
pub const MOVE: CommandId = CommandId("refactor.move");

#[derive(Debug, Default)]
pub struct RefactoringModule;

impl Module for RefactoringModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        // Its threads: their CPU counts on its row in the Task Manager.
        ctx.contribute(workbench_model::ModuleThreads(&["refactor-", "find-usages"]));
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(RenameProvider));
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(SignatureProvider));
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(UsagesProvider));
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(MoveProvider));
        // Renaming a class's file renames the class; moving files moves their namespaces.
        ctx.contribute::<Arc<dyn FileRefactoring>>(Arc::new(Files));
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: FIND_USAGES,
                label: "Find Usages",
                menu: Some(MenuAt::new(sections::FIND_PROJECT, 20)),
                shortcuts: &["Alt+F7"],
            },
            icon: Some("search"),
            handler: workbench_model::Requiring::new(
                &[command_model::Requires::TextFile, command_model::Requires::FileOnDisk],
                Arc::new(FindUsages),
            ),
        });
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: RENAME,
                label: "Rename…",
                menu: Some(MenuAt::new(sections::REFACTOR_CHANGE, 10)),
                shortcuts: &["Shift+F6"],
            },
            icon: Some("file-pen"),
            handler: workbench_model::Requiring::new(
                &[command_model::Requires::TextFile, command_model::Requires::FileOnDisk],
                Arc::new(Rename),
            ),
        });
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: CHANGE_SIGNATURE,
                label: "Change Signature…",
                menu: Some(MenuAt::new(sections::REFACTOR_CHANGE, 20)),
                shortcuts: &["CmdOrCtrl+F6"],
            },
            icon: Some("braces"),
            handler: workbench_model::Requiring::new(
                &[command_model::Requires::TextFile, command_model::Requires::FileOnDisk],
                Arc::new(ChangeSignature),
            ),
        });
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: MOVE,
                label: "Move…",
                menu: Some(MenuAt::new(sections::REFACTOR_CHANGE, 30)),
                shortcuts: &["F6"],
            },
            icon: Some("folder-input"),
            handler: workbench_model::Requiring::new(
                &[command_model::Requires::TextFile, command_model::Requires::FileOnDisk],
                Arc::new(Move),
            ),
        });
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: UNDO,
                label: "Undo Last Refactoring",
                menu: Some(MenuAt::new(sections::REFACTOR_UNDO, 10)),
                shortcuts: &[],
            },
            icon: Some("rotate-ccw"),
            handler: Arc::new(Undo),
        });
        // In the right-click menus.
        // In the editor's, where the file's language offers each (as its intelligence says).
        const EDITOR: &[MenuPlace] = &[MenuPlace::Editor];
        let offers = |feature| MenuWhen::Provides(feature);
        for item in [
            ContextMenuItem::new(RENAME, EDITOR, MenuSection::Refactor, 10)
                .when(offers(LanguageFeature::Rename)),
            ContextMenuItem::new(CHANGE_SIGNATURE, EDITOR, MenuSection::Refactor, 20)
                .when(offers(LanguageFeature::ChangeSignature)),
            ContextMenuItem::new(MOVE, EDITOR, MenuSection::Refactor, 30)
                .when(offers(LanguageFeature::Move)),
            ContextMenuItem::new(FIND_USAGES, EDITOR, MenuSection::Navigate, 20)
                .when(offers(LanguageFeature::Navigation)),
            // In the tree's, with its clipboard: where an entry goes.
            ContextMenuItem::new(
                MOVE,
                &[MenuPlace::TreeFile, MenuPlace::TreeFolder],
                MenuSection::Clipboard,
                40,
            ),
        ] {
            ctx.contribute(item);
        }
        Ok(())
    }
}

/// The rename preview, in a tab.
#[derive(Debug)]
struct RenameProvider;

impl ViewerProvider for RenameProvider {
    fn id(&self) -> &'static str {
        "refactor.rename"
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(RenameView::default()))
    }
}

/// The Change Signature preview, in a tab.
#[derive(Debug)]
struct SignatureProvider;

impl ViewerProvider for SignatureProvider {
    fn id(&self) -> &'static str {
        "refactor.signature"
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension()
            .is_some_and(|e| e == signature_view::EXTENSION)
            .then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(SignatureView::default()))
    }
}

/// The usages list, in a tab.
#[derive(Debug)]
struct UsagesProvider;

impl ViewerProvider for UsagesProvider {
    fn id(&self) -> &'static str {
        "refactor.usages"
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == usages_view::EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(UsagesView::default()))
    }
}

/// The Move preview, in a tab.
#[derive(Debug)]
struct MoveProvider;

impl ViewerProvider for MoveProvider {
    fn id(&self) -> &'static str {
        "refactor.move"
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == move_view::EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(MoveView::default()))
    }
}

/// Renaming and moving project files as refactorings: the type named after a renamed file
/// (PSR-4) renamed with it, in the preview with the new name typed; a moved file or
/// folder's classes' namespaces following it, in the Move preview.
#[derive(Debug)]
struct Files;

impl FileRefactoring for Files {
    fn rename(&self, host: &mut dyn WorkbenchHost, path: &Path, name: &str) -> bool {
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or_default();
        let (Some(old), Some(new)) = (
            path.file_stem().and_then(|s| s.to_str()),
            name.strip_suffix(extension).and_then(|n| n.strip_suffix('.')),
        ) else {
            return false;
        };
        if extension.is_empty()
            || !refactor::valid_name(new, refactor::NameStyle::CODE)
            || new == old
        {
            return false;
        }
        let Some(context) = host.rename_type_of_file(path) else { return false };
        let title = tr!("refactor.rename_tab", name = old);
        host.open_view_below(&title, EXTENSION, &RenameData { context, name: Some(new.to_owned()) })
    }

    fn moved(&self, host: &mut dyn WorkbenchHost, from: &Path, into: &Path) -> bool {
        match host.move_context_to(from, into) {
            Ok(context) => open_move(host, context),
            Err(why) => {
                host.notify(workbench_model::Notification::info(why));
                true
            }
        }
    }
}

/// The Move preview for `context`.
fn open_move(host: &mut dyn WorkbenchHost, context: refactor::moving::MoveContext) -> bool {
    let name = context.target.name().to_string_lossy().into_owned();
    let title = tr!("refactor.move_tab", name = name.as_str());
    host.open_view_below(&title, move_view::EXTENSION, &MoveData { context })
}

/// Move: the file / folder the command is for (the tree's, or the active file), with its
/// classes' namespaces.
struct Move;

impl CommandHandler for Move {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        let _ = host;
        true
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        match host.move_context() {
            Ok(context) => open_move(host, context),
            Err(why) => {
                host.notify(workbench_model::Notification::info(why));
                true
            }
        }
    }
}

/// Find Usages: the symbol at the cursor, its places found like a rename's — a language
/// server's references where one answers them, joined with the built-in engine's.
struct FindUsages;

impl CommandHandler for FindUsages {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.active_file().is_some()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        match host.usages_at_cursor() {
            Ok(context) => {
                let title = tr!("refactor.usages_tab", name = context.target.name.as_str());
                host.open_view_below(&title, usages_view::EXTENSION, &UsagesData { context })
            }
            Err(why) => {
                host.notify(workbench_model::Notification::info(why));
                true
            }
        }
    }
}

struct ChangeSignature;

impl CommandHandler for ChangeSignature {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.active_file().is_some()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        match host.change_signature_at_cursor() {
            Ok(context) => {
                let title = tr!("refactor.signature_tab", name = context.target.name.as_str());
                host.open_view_below(&title, signature_view::EXTENSION, &SignatureData { context })
            }
            Err(why) => {
                host.notify(workbench_model::Notification::info(why));
                true
            }
        }
    }
}

struct Rename;

impl CommandHandler for Rename {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.active_file().is_some()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        match host.rename_at_cursor() {
            Ok(context) => {
                let title = tr!("refactor.rename_tab", name = context.target.name.as_str());
                host.open_view_below(&title, EXTENSION, &RenameData { context, name: None })
            }
            Err(why) => {
                host.notify(workbench_model::Notification::info(why));
                true
            }
        }
    }
}

struct Undo;

impl CommandHandler for Undo {
    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let message = match host.undo_refactoring() {
            None => workbench_model::Notification::info(t("refactor.nothing_to_undo")),
            Some(0) => workbench_model::Notification::success(t("refactor.undone")),
            Some(skipped) => workbench_model::Notification::warning(t("refactor.undone"))
                .with_detail(tr!("refactor.skipped", count = skipped)),
        };
        host.notify(message);
        true
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
